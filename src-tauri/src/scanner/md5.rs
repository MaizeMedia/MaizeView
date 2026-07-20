//! Full-file MD5 fingerprint (StashDB / Stash-box compatible).
//!
//! Computed lazily during identify — not at scan time — so library scans stay fast.

use std::{
    io::{Read, Write},
    path::Path,
};

use anyhow::{Context, Result};
use md5::{Digest, Md5};

const READ_BUF: usize = 1024 * 1024;

/// Lowercase hex without the `hex` crate (md-5 0.11's output type no
/// longer implements LowerHex).
fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// MD5 hex digest of the entire file (lowercase, no dashes).
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {} for md5", path.display()))?;
    let mut hasher = Md5::new();
    let mut buf = vec![0u8; READ_BUF];
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("reading {} for md5", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

/// Write progress to stderr for long-running CLI backfills.
pub fn hash_file_with_progress(path: &Path, out: &mut impl Write) -> Result<String> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {} for md5", path.display()))?;
    let total = file
        .metadata()
        .with_context(|| format!("statting {} for md5", path.display()))?
        .len();
    let mut hasher = Md5::new();
    let mut buf = vec![0u8; READ_BUF];
    let mut read: u64 = 0;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        read += n as u64;
        if total > 0 && read % (64 * 1024 * 1024) < READ_BUF as u64 {
            let pct = (read as f64 / total as f64 * 100.0) as u32;
            let _ = writeln!(out, "{}: md5 {}%", path.display(), pct);
        }
    }
    Ok(hex_lower(&hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn md5_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("empty.bin");
        std::fs::File::create(&p).unwrap();
        assert_eq!(hash_file(&p).unwrap(), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn md5_known_content() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("hello.txt");
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(b"hello").unwrap();
        assert_eq!(hash_file(&p).unwrap(), "5d41402abc4b2a76b9719d911017c592");
    }
}
