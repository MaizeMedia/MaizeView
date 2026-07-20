//! 4Play (M2) — four videos, one per quadrant, inside a single window.
//!
//! Four plain Win32 child windows ("static" class) tile the client area 2x2
//! as geometry anchors. FOUR libmpv instances embed into the TOP-LEVEL window
//! (`initialOptions.wid` = the parent HWND) and each instance's render window
//! is positioned over its quadrant with SetWindowPos. Embedding into a pane
//! instead makes mpv's render window a GRANDCHILD of the Tauri window, and
//! grandchildren never paint: decode runs, `time-pos` advances, the window
//! tree looks perfect, but nothing reaches the screen — the user sees
//! straight through the client area. Reparenting the same mpv window to the
//! top-level Tauri window made it paint immediately (verified live
//! 2026-07-19, tools/quad-reparent-test.py). So: `wid` must be the top-level
//! window, and quadrant placement is done by us, post-init.
//!
//! Per-instance identification: all four render windows are "mpv"-class
//! children of the same top-level window, indistinguishable from each other.
//! The frontend initializes instances SEQUENTIALLY (init, claim, init,
//! claim, …); after each init, exactly one new "mpv" child exists — the one
//! not already registered (quad_claim_mpv diffs and assigns it a quadrant).
//!
//! The flip side of `wid` = top-level window: mpv hooks the parent's window
//! proc and snaps its render window back to the FULL client area on every
//! move/resize. To keep the videos in their quadrants even mid-drag, we
//! subclass the window AFTER mpv installed its hook (quad_subclass_proc):
//! our proc runs first, lets the rest of the chain (mpv's hook included) run,
//! then re-fits every registered render window — all within the same message
//! dispatch, so no full-frame flash ever reaches the screen.
//!
//! Known leftovers: entries in INSTANCES / QuadState for closed windows are
//! never removed (dead HWNDs never match a live tree; harmless for a spike),
//! and RemoveWindowSubclass is skipped (the subclass dies with the window).

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tauri::{AppHandle, Manager, State};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, EnumChildWindows, GetClassNameW, GetClientRect, GetParent, GetWindowRect,
    SetWindowPos, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOSENDCHANGING, SWP_NOZORDER, WM_MOVE,
    WM_MOVING, WM_SIZE, WM_SIZING, WM_WINDOWPOSCHANGED, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
};

/// Child HWNDs of every 4Play window, keyed by window label.
///
/// Stored as `isize`, not `HWND`: in windows-sys 0.61 `HWND` is
/// `*mut c_void`, which is neither Send nor Sync, so it can't live inside
/// Tauri-managed state. The cast through isize preserves the bits (HWNDs are
/// opaque handles that fit in a pointer-sized int).
#[derive(Default)]
pub struct QuadState {
    panes: Mutex<HashMap<String, Vec<isize>>>,
    /// Window labels whose top-level HWND already carries our re-fit subclass.
    subclassed: Mutex<HashSet<String>>,
}

/// mpv render-window HWNDs per 4Play window, keyed by the top-level HWND
/// (one slot per quadrant; 0 = not claimed yet). Read by the subclass proc,
/// which can't reach Tauri-managed state. Entries for closed windows are
/// never removed — dead HWNDs never match a live window tree anyway.
static INSTANCES: OnceLock<Mutex<HashMap<isize, [isize; 4]>>> = OnceLock::new();

fn instances() -> &'static Mutex<HashMap<isize, [isize; 4]>> {
    INSTANCES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Resolve the OS HWND of a Tauri window — same approach as the libmpv
/// plugin's utils.rs: `window_handle()` → `as_raw()` → Win32 handle.
fn window_hwnd(app: &AppHandle, label: &str) -> Result<HWND, String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("window '{label}' not found"))?;
    let handle = window.window_handle().map_err(|e| e.to_string())?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Ok(h.hwnd.get() as HWND),
        _ => Err("quad panes are only supported on Windows".to_string()),
    }
}

/// Client-area quadrant rects: [top-left, top-right, bottom-left, bottom-right]
/// as (x, y, w, h). Odd sizes put the remainder in the right/bottom quadrants.
fn quadrants(parent: HWND) -> Result<[(i32, i32, i32, i32); 4], String> {
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if unsafe { GetClientRect(parent, &mut rc) } == 0 {
        return Err(format!("GetClientRect failed: {}", unsafe {
            GetLastError()
        }));
    }
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    let hw = w / 2;
    let hh = h / 2;
    Ok([
        (0, 0, hw, hh),
        (hw, 0, w - hw, hh),
        (0, hh, hw, h - hh),
        (hw, hh, w - hw, h - hh),
    ])
}

/// All DIRECT children of `parent` with window class "mpv" (one per libmpv
/// instance embedded with `wid` = parent).
fn find_mpv_children(parent: HWND) -> Vec<HWND> {
    unsafe extern "system" fn enum_proc(hwnd: HWND, lp: LPARAM) -> i32 {
        let ctx = &mut *(lp as *mut (HWND, Vec<HWND>));
        if GetParent(hwnd) == ctx.0 {
            let mut buf = [0u16; 64];
            let n = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            if n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == "mpv" {
                ctx.1.push(hwnd);
            }
        }
        1 // continue
    }
    let mut ctx: (HWND, Vec<HWND>) = (parent, Vec::new());
    unsafe {
        EnumChildWindows(parent, Some(enum_proc), &mut ctx as *mut _ as LPARAM);
    }
    ctx.1
}

/// Move `hwnd` over quadrant rect `quad` (in `parent` client coords).
/// Shared by quad_claim_mpv, quad_relayout, and the subclass proc.
///
/// Uses SWP_ASYNCWINDOWPOS | SWP_NOSENDCHANGING — the same discipline mpv's
/// own parent hook uses (resize_child_win in w32_common.c): never
/// synchronously SendMessage-rendezvous with the mpv VO threads from the UI
/// thread. A plain SetWindowPos from the subclass proc wedged the whole app
/// with 4 instances (UI thread suspended mid-rendezvous, busy cursor
/// everywhere, window unclosable). Skips no-op fits so the subclass proc
/// (which fires on every geometry message) doesn't churn messages.
fn fit_window(parent: HWND, hwnd: HWND, quad: (i32, i32, i32, i32)) -> Result<(), String> {
    let (x, y, w, h) = quad;
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let mut origin = POINT { x: 0, y: 0 };
    unsafe {
        GetWindowRect(hwnd, &mut rc);
        ClientToScreen(parent, &mut origin);
    }
    if rc.left - origin.x == x
        && rc.top - origin.y == y
        && rc.right - rc.left == w
        && rc.bottom - rc.top == h
    {
        return Ok(());
    }
    let ok = unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(), // keep z-order (SWP_NOZORDER)
            x,
            y,
            w,
            h,
            SWP_ASYNCWINDOWPOS | SWP_NOSENDCHANGING | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
    if ok == 0 {
        return Err(format!("SetWindowPos failed: {}", unsafe {
            GetLastError()
        }));
    }
    Ok(())
}

/// Re-fit every registered mpv render window of `parent` into its quadrant.
fn fit_instances(parent: HWND) -> Result<(), String> {
    let quads = quadrants(parent)?;
    let slots = {
        let guard = instances().lock().expect("quad instances mutex poisoned");
        guard.get(&(parent as isize)).cloned()
    };
    if let Some(slots) = slots {
        for (i, hwnd) in slots.iter().enumerate() {
            // Tolerate dead slots (a closed pane's render window is gone);
            // don't let one failure skip the others.
            if *hwnd != 0 {
                let _ = fit_window(parent, *hwnd as HWND, quads[i]);
            }
        }
    }
    Ok(())
}

/// See the module doc: re-fit all registered render windows after every
/// geometry message, AFTER the rest of the proc chain (mpv's own fit-to-full
/// hook) has run — same dispatch, so the full-frame snap never reaches the
/// screen.
///
/// Safe against recursion: fitting a CHILD sends WM_MOVE/WM_SIZE to the
/// child, not back to this window.
unsafe extern "system" fn quad_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uid_subclass: usize,
    _ref_data: usize,
) -> LRESULT {
    let result = DefSubclassProc(hwnd, msg, wparam, lparam);
    if matches!(
        msg,
        WM_MOVE | WM_SIZE | WM_WINDOWPOSCHANGED | WM_MOVING | WM_SIZING
    ) {
        let _ = fit_instances(hwnd);
    }
    result
}

/// Install quad_subclass_proc on `parent` once per window label. Runs the
/// SetWindowSubclass call on the UI thread (the window's owner). Must happen
/// AFTER mpv init so we wrap mpv's own parent hook (LIFO chain order puts us
/// ahead of it, and we re-fit after calling the rest of the chain).
fn install_subclass_once(
    app: &AppHandle,
    state: &State<'_, QuadState>,
    label: &str,
    parent: HWND,
) -> Result<(), String> {
    {
        let guard = state.subclassed.lock().expect("quad state mutex poisoned");
        if guard.contains(label) {
            return Ok(());
        }
    }
    let addr = parent as usize;
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    app.run_on_main_thread(move || {
        let ok = unsafe { SetWindowSubclass(addr as HWND, Some(quad_subclass_proc), 0, 0) };
        let _ = tx.send(ok != 0);
    })
    .map_err(|e| format!("run_on_main_thread failed: {e}"))?;
    let ok = rx
        .recv()
        .map_err(|e| format!("subclass install never replied: {e}"))?;
    if !ok {
        return Err("SetWindowSubclass failed".to_string());
    }
    state
        .subclassed
        .lock()
        .expect("quad state mutex poisoned")
        .insert(label.to_string());
    Ok(())
}

/// Result of [`quad_create_panes`]: the quadrant pane HWNDs plus the
/// top-level window's own HWND (which is what libmpv must embed into — see
/// the module doc for why panes can't host mpv directly).
#[derive(serde::Serialize)]
pub struct QuadPanes {
    pub panes: [i64; 4],
    pub parent: i64,
}

/// Create the 4 quadrant child windows for `label`. The returned `parent`
/// HWND is what the frontend passes to libmpv as `wid`.
///
/// The CreateWindowExW calls run on the app's UI thread: a Win32 window is
/// owned by its creating thread, and parenting video output under a window
/// owned by a dead Tauri command worker thread (no message loop) is asking
/// for exactly the kind of silent paint failure this spike already hit once.
#[tauri::command]
pub fn quad_create_panes(
    app: AppHandle,
    state: State<'_, QuadState>,
    label: String,
) -> Result<QuadPanes, String> {
    let parent = window_hwnd(&app, &label)?;
    // HWND is *mut c_void (not Send); ferry it across as usize.
    let parent_addr = parent as usize;
    let (tx, rx) = std::sync::mpsc::channel::<Result<[i64; 4], String>>();
    app.run_on_main_thread(move || {
        let _ = tx.send(create_panes(parent_addr as HWND));
    })
    .map_err(|e| format!("run_on_main_thread failed: {e}"))?;
    let hwnds = rx
        .recv()
        .map_err(|e| format!("UI-thread pane creation never replied: {e}"))??;

    state
        .panes
        .lock()
        .expect("quad state mutex poisoned")
        .insert(label, hwnds.iter().map(|&h| h as isize).collect());
    Ok(QuadPanes {
        panes: hwnds,
        parent: parent as i64,
    })
}

/// The actual CreateWindowExW loop — must run on the UI thread (see above).
fn create_panes(parent: HWND) -> Result<[i64; 4], String> {
    let quads = quadrants(parent)?;
    let class: Vec<u16> = "static\0".encode_utf16().collect();

    let mut hwnds = [0i64; 4];
    for (i, &(x, y, w, h)) in quads.iter().enumerate() {
        let hwnd = unsafe {
            CreateWindowExW(
                0, // no extended styles
                class.as_ptr(),
                std::ptr::null(), // no window name
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                x,
                y,
                w,
                h,
                parent,
                std::ptr::null_mut(), // no menu
                std::ptr::null_mut(), // instance (not needed for system classes)
                std::ptr::null(),     // no create-param
            )
        };
        if hwnd.is_null() {
            return Err(format!("CreateWindowExW (pane {i}) failed: {}", unsafe {
                GetLastError()
            }));
        }
        hwnds[i] = hwnd as i64;
    }
    Ok(hwnds)
}

/// Claim the render window of the mpv instance just initialized for quadrant
/// `index` and fit it into place. Also installs the re-fit subclass on first
/// claim.
///
/// Identification by diff (see the module doc): the new instance's render
/// window is the only "mpv"-class child of the top-level window that isn't
/// registered yet. Returns the claimed HWND (diagnostics only). The frontend
/// retries this a few times — mpv creates the window asynchronously on init.
#[tauri::command]
pub fn quad_claim_mpv(
    app: AppHandle,
    state: State<'_, QuadState>,
    label: String,
    index: usize,
) -> Result<i64, String> {
    let parent = window_hwnd(&app, &label)?;
    install_subclass_once(&app, &state, &label, parent)?;
    let index = index.min(3);
    let current = find_mpv_children(parent);
    let mut guard = instances().lock().expect("quad instances mutex poisoned");
    let slots = guard.entry(parent as isize).or_insert([0; 4]);
    let new = current
        .into_iter()
        .find(|h| !slots.contains(&(*h as isize)))
        .ok_or("no unclaimed mpv render window — was a new instance initialized?")?;
    slots[index] = new as isize;
    drop(guard);
    let quads = quadrants(parent)?;
    fit_window(parent, new, quads[index])?;
    Ok(new as i64)
}

/// Re-tile the quadrant panes of `label` after the window is resized, and
/// re-fit every claimed render window over its quadrant (best-effort: slots
/// may be unclaimed if a resize fires before init).
#[tauri::command]
pub fn quad_relayout(
    app: AppHandle,
    state: State<'_, QuadState>,
    label: String,
) -> Result<(), String> {
    let parent = window_hwnd(&app, &label)?;
    let quads = quadrants(parent)?;
    let panes = {
        let guard = state.panes.lock().expect("quad state mutex poisoned");
        guard
            .get(&label)
            .cloned()
            .ok_or_else(|| format!("no quad panes for window '{label}'"))?
    };
    for (hwnd, &quad) in panes.iter().zip(quads.iter()) {
        fit_window(parent, *hwnd as HWND, quad)?;
    }
    fit_instances(parent)
}
