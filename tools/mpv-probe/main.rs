// Isolate which single option causes the wrapper to hang.
// Previous finding: {"vo":"null","vid":"no"} works; {"vo":"direct3d",...} hung.
// New finding: {"vo":"direct3d","hwdec":"no"} works.
// => The hang is tied to a specific option combo. Bisect it.
use libloading::{Library, Symbol};
use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::process::Command;
use std::time::{Duration, Instant};

type WrapperCreate = unsafe extern "C" fn(
    *const c_char, *const c_char,
    Option<extern "C" fn(*const c_char, *mut c_void)>, *mut c_void,
) -> *mut c_void;
type WrapperDestroy = unsafe extern "C" fn(*mut c_void);
extern "C" fn event_cb(_: *const c_char, _: *mut c_void) {}

const TESTS: &[(&str, &str)] = &[
    ("direct3d only",                r#"{"vo":"direct3d"}"#),
    ("direct3d + hwdec=auto-safe",   r#"{"vo":"direct3d","hwdec":"auto-safe"}"#),
    ("direct3d + hwdec=auto",        r#"{"vo":"direct3d","hwdec":"auto"}"#),
    ("direct3d + hwdec=auto-copy",   r#"{"vo":"direct3d","hwdec":"auto-copy"}"#),
    ("direct3d + keep-open",         r#"{"vo":"direct3d","keep-open":"yes"}"#),
    ("direct3d + osc=no",            r#"{"vo":"direct3d","osc":"no"}"#),
    ("direct3d + osd_level=0",       r#"{"vo":"direct3d","osd_level":"0"}"#),
    ("direct3d + all 4 (orig)",      r#"{"vo":"direct3d","hwdec":"auto-safe","keep-open":"yes","osc":"no","osd_level":"0"}"#),
    ("gpu + hwdec=auto-safe (orig)", r#"{"vo":"gpu","hwdec":"auto-safe","keep-open":"yes","osc":"no","osd_level":"0"}"#),
    ("gpu + hwdec=auto-safe only",   r#"{"vo":"gpu","hwdec":"auto-safe"}"#),
];

fn run_one(name: &str, opts: &str) {
    let started = Instant::now();
    let lib = unsafe { Library::new("libmpv-wrapper.dll").unwrap() };
    unsafe {
        let create: Symbol<WrapperCreate> = lib.get(b"mpv_wrapper_create\0").unwrap();
        let destroy: Symbol<WrapperDestroy> = lib.get(b"mpv_wrapper_destroy\0").unwrap();
        let opts_c = CString::new(opts).unwrap();
        let obs_c = CString::new("{}").unwrap();
        let h = create(opts_c.as_ptr(), obs_c.as_ptr(), Some(event_cb), std::ptr::null_mut());
        if h.is_null() { println!("    NULL at {:.2?}", started.elapsed()); }
        else { println!("    OK {:p} at {:.2?}", h, started.elapsed()); destroy(h); }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 2 {
        let idx: usize = args[1].parse().unwrap();
        let (name, opts) = TESTS[idx];
        let no = name.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(6));
            eprintln!("    *** 6s HANG *** [{}]", no);
            std::process::exit(99);
        });
        run_one(name, opts);
        std::process::exit(0);
    }
    let exe = std::env::current_exe().unwrap();
    let cwd = std::env::current_dir().unwrap();
    for (i, (name, _)) in TESTS.iter().enumerate() {
        print!("{:<34} ", name);
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let started = Instant::now();
        let out = Command::new(&exe).arg(i.to_string()).current_dir(&cwd).output();
        match out {
            Ok(o) => {
                let code = o.status.code().unwrap_or(-1);
                if code == 99 { println!("HUNG (>6s)"); }
                else { println!("ok ({:.2?})", started.elapsed()); }
                if !o.stderr.is_empty() { print!("{}", String::from_utf8_lossy(&o.stderr)); }
            }
            Err(e) => println!("err: {}", e),
        }
    }
}
