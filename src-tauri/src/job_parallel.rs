//! Shared concurrency for background media jobs (previews, pHash, MD5).
//!
//! Goals:
//!   * Scale with CPU: roughly `cores - 4`, clamped so the UI stays responsive.
//!   * Prefer spreading work **across drives** so one slow volume doesn't
//!     serialize the whole batch (and so we don't hammer a single HDD with
//!     every worker).

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, LazyLock,
    },
};

use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

/// Slot in `AppState` for a cancellable background media job.
pub type JobCancelSlot = Arc<Mutex<Option<Arc<CancellationToken>>>>;

pub fn new_cancel_slot() -> JobCancelSlot {
    Arc::new(Mutex::new(None))
}

/// Install a fresh token; errors if a job is already registered.
pub async fn begin_cancellable_job(slot: &JobCancelSlot) -> Result<Arc<CancellationToken>, String> {
    let mut guard = slot.lock().await;
    if guard.is_some() {
        return Err("that job is already running".into());
    }
    let token = Arc::new(CancellationToken::new());
    *guard = Some(token.clone());
    Ok(token)
}

pub async fn end_cancellable_job(slot: &JobCancelSlot) {
    let mut guard = slot.lock().await;
    *guard = None;
}

/// Cancel and clear. Returns whether a job was running.
pub async fn cancel_cancellable_job(slot: &JobCancelSlot) -> bool {
    let mut guard = slot.lock().await;
    if let Some(token) = guard.take() {
        token.cancel();
        true
    } else {
        false
    }
}

/// Absolute max concurrent media workers (SQLite pool is 12; leave UI headroom).
pub const MEDIA_JOB_WORKERS_MAX: usize = 16;
/// Never run fewer than this when there is work to do (auto mode).
pub const MEDIA_JOB_WORKERS_MIN: usize = 2;
/// Cores reserved for UI / OS / a concurrent Convert job.
pub const MEDIA_JOB_CORE_RESERVE: usize = 4;
/// Cap concurrent ffmpeg/readers on **each** volume when the batch spans
/// multiple drives (keeps one slow HDD from taking every slot).
pub const MEDIA_JOB_PER_DRIVE_MAX: usize = 3;

/// User override from Settings. `0` = auto (`cores - reserve`, clamped).
static JOB_WORKERS_CAP: AtomicUsize = AtomicUsize::new(0);

/// Persist/apply Settings → max parallel workers (`0` = auto).
pub fn set_job_workers_cap(cap: usize) {
    let v = if cap == 0 {
        0
    } else {
        cap.clamp(1, MEDIA_JOB_WORKERS_MAX)
    };
    JOB_WORKERS_CAP.store(v, Ordering::Relaxed);
}

/// Raw setting value (`0` = auto).
pub fn job_workers_cap() -> usize {
    JOB_WORKERS_CAP.load(Ordering::Relaxed)
}

pub fn cpu_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8)
}

/// Effective worker count for scan indexing + preview/pHash/MD5 jobs.
pub fn media_job_workers() -> usize {
    let cores = cpu_count();
    let auto = media_job_workers_for(cores);
    let cap = job_workers_cap();
    if cap == 0 {
        auto
    } else {
        // Manual: honor the slider, but never above CPU count or absolute max.
        cap.min(MEDIA_JOB_WORKERS_MAX).min(cores.max(1)).max(1)
    }
}

pub fn media_job_workers_for(cores: usize) -> usize {
    cores
        .saturating_sub(MEDIA_JOB_CORE_RESERVE)
        .clamp(MEDIA_JOB_WORKERS_MIN, MEDIA_JOB_WORKERS_MAX)
}

/// Per-drive concurrency given total workers and how many distinct volumes
/// appear in the batch.
///
/// - **One volume:** use the full worker budget (NVMe can feed many ffmpeg;
///   the old hard cap of 3 left a 32-thread box stuck ~¼ busy).
/// - **Several volumes:** spread slots across drives, max
///   [`MEDIA_JOB_PER_DRIVE_MAX`] each, so work fans out instead of stacking
///   on the first path.
pub fn per_drive_workers(total_workers: usize, drive_count: usize) -> usize {
    if drive_count <= 1 {
        return total_workers.max(1);
    }
    (total_workers / drive_count)
        .max(1)
        .min(MEDIA_JOB_PER_DRIVE_MAX)
}

/// Stable key for scheduling: Windows drive (`G:`) or UNC share root, else
/// the first path component / `"local"`.
pub fn drive_key(path: &Path) -> String {
    let s = path.to_string_lossy();
    // Windows drive letter: "G:\foo" / "g:/foo"
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return format!("{}:", bytes[0].to_ascii_uppercase() as char);
    }
    // UNC: \\server\share\...
    if s.starts_with("\\\\") || s.starts_with("//") {
        let rest = s.trim_start_matches(['\\', '/']);
        let mut parts = rest.split(['\\', '/']).filter(|p| !p.is_empty());
        if let (Some(host), Some(share)) = (parts.next(), parts.next()) {
            return format!("\\\\{host}\\{share}");
        }
        return "\\\\unc".to_string();
    }
    path.components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "local".to_string())
}

pub fn distinct_drives<'a, I>(paths: I) -> usize
where
    I: IntoIterator<Item = &'a Path>,
{
    let set: HashSet<String> = paths.into_iter().map(drive_key).collect();
    set.len().max(1)
}

/// Global + per-drive semaphores.
///
/// Acquire order is **drive → global**. The reverse (global first) lets one
/// busy volume's waiters hold every global slot while blocked on that volume's
/// cap — other drives starve even when they have pending work.
pub struct DriveLimiter {
    global: Arc<Semaphore>,
    default_per_drive: usize,
    caps: HashMap<String, usize>,
    per_drive: Mutex<HashMap<String, Arc<Semaphore>>>,
}

pub struct DrivePermit {
    _global: OwnedSemaphorePermit,
    _drive: OwnedSemaphorePermit,
}

/// Global semaphore shared by ALL media jobs (previews, MD5, pHash). The
/// three jobs run concurrently post-scan; with per-job globals they'd
/// oversubscribe the disk 3× (thirty ffmpeg/readers fighting one HDD).
/// Rebuilt when the worker budget changes (Settings slider).
static SHARED_GLOBAL: LazyLock<std::sync::Mutex<(usize, Arc<Semaphore>)>> =
    LazyLock::new(|| std::sync::Mutex::new((0, Arc::new(Semaphore::new(1)))));

pub fn shared_global_semaphore(workers: usize) -> Arc<Semaphore> {
    let w = workers.max(1);
    let mut g = SHARED_GLOBAL
        .lock()
        .expect("shared media semaphore poisoned");
    if g.0 != w {
        *g = (w, Arc::new(Semaphore::new(w)));
    }
    g.1.clone()
}

/// Drive letters whose backing disk is rotational (`E:`, `G:` style keys,
/// matching `drive_key`). Probed via PowerShell (one call per job start);
/// failure → empty set (callers pick the fallback behavior).
pub fn rotational_drives() -> HashSet<String> {
    #[cfg(not(windows))]
    {
        HashSet::new()
    }
    #[cfg(windows)]
    {
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-Partition | Where-Object DriveLetter | ForEach-Object { $d = Get-PhysicalDisk | Where-Object DeviceId -eq $_.DiskNumber | Select-Object -First 1; if ($d) { \"$($_.DriveLetter):$($d.MediaType)\" } }",
            ])
            .output();
        let Ok(out) = out else {
            return HashSet::new();
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let (letter, media) = line.trim().split_once(':')?;
                // HDD or Unspecified (cheap USB enclosures often don't
                // report) count as rotational; SSD alone opts out.
                (media.trim() != "SSD").then(|| format!("{}:", letter.trim().to_ascii_uppercase()))
            })
            .collect()
    }
}

impl DriveLimiter {
    /// All media jobs share one global budget (see `shared_global_semaphore`).
    pub fn new(total_workers: usize, per_drive: usize) -> Self {
        Self::with_caps(total_workers, per_drive, HashMap::new())
    }

    /// Like `new`, but lets specific drives override the per-drive cap
    /// (e.g. rotational disks capped to 1 reader for the MD5 job).
    pub fn with_caps(total_workers: usize, per_drive: usize, caps: HashMap<String, usize>) -> Self {
        Self {
            global: shared_global_semaphore(total_workers),
            default_per_drive: per_drive.max(1),
            caps,
            per_drive: Mutex::new(HashMap::new()),
        }
    }

    pub async fn acquire(&self, drive: &str) -> DrivePermit {
        let cap = self
            .caps
            .get(drive)
            .copied()
            .unwrap_or(self.default_per_drive)
            .max(1);
        let drive_sem = {
            let mut map = self.per_drive.lock().await;
            map.entry(drive.to_string())
                .or_insert_with(|| Arc::new(Semaphore::new(cap)))
                .clone()
        };
        // Drive first: excess work for a saturated volume waits here without
        // consuming global capacity other volumes need.
        let drive = drive_sem
            .acquire_owned()
            .await
            .expect("media job per-drive semaphore closed");
        let global = self
            .global
            .clone()
            .acquire_owned()
            .await
            .expect("media job global semaphore closed");
        DrivePermit {
            _global: global,
            _drive: drive,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn workers_leave_core_headroom() {
        assert_eq!(media_job_workers_for(32), 16); // clamped to MAX
        assert_eq!(media_job_workers_for(20), 16);
        assert_eq!(media_job_workers_for(16), 12);
        assert_eq!(media_job_workers_for(8), 4);
        assert_eq!(media_job_workers_for(6), 2);
        assert_eq!(media_job_workers_for(4), 2); // floor
        assert_eq!(media_job_workers_for(2), 2);
    }

    #[test]
    fn manual_cap_overrides_auto() {
        set_job_workers_cap(0);
        let auto = media_job_workers_for(cpu_count());
        assert_eq!(media_job_workers(), auto);
        set_job_workers_cap(3);
        assert_eq!(media_job_workers(), 3.min(cpu_count()).max(1));
        set_job_workers_cap(0); // restore for other tests
    }

    #[test]
    fn per_drive_spreads_and_caps() {
        assert_eq!(per_drive_workers(16, 8), 2);
        assert_eq!(per_drive_workers(8, 2), 3); // 4→cap 3
        assert_eq!(per_drive_workers(16, 1), 16); // single volume: full budget
        assert_eq!(per_drive_workers(4, 1), 4);
        assert_eq!(per_drive_workers(4, 3), 1);
    }

    #[test]
    fn drive_key_windows_and_unc() {
        assert_eq!(drive_key(Path::new(r"G:\Media\880.mp4")), "G:");
        assert_eq!(drive_key(Path::new(r"e:/foo/bar.mp4")), "E:");
        assert_eq!(
            drive_key(Path::new(r"\\nas\media\clip.mp4")),
            r"\\nas\media"
        );
    }

    #[test]
    fn distinct_drives_counts_volumes() {
        let paths = [
            PathBuf::from(r"G:\a.mp4"),
            PathBuf::from(r"G:\b.mp4"),
            PathBuf::from(r"E:\c.mp4"),
        ];
        assert_eq!(distinct_drives(paths.iter().map(|p| p.as_path())), 2);
    }
}
