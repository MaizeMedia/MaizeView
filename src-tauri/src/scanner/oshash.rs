//! oshash — a fast partial-file hash for video identity.
//!
//! Matches the OpenSubtitles / Stash convention: read the file header and
//! footer plus the file size, treat bytes as little-endian u64 chunks and sum.
//! Cheap to compute even on multi-GB files (only reads ~2 × 64 KB) and stable
//! across renames. Reference: https://trac.opensubtitles.org/projects/opensubtitles/wiki/HashSourceCodes

use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use anyhow::{Context, Result};

/// Chunk size: OpenSubtitles hashes the first and last 64 KiB.
const CHUNK_BYTES: u64 = 64 * 1024;

/// Compute the OpenSubtitles / Stash oshash for a file.
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("opening {} for oshash", path.display()))?;
    let size = file
        .metadata()
        .with_context(|| format!("statting {} for oshash", path.display()))?
        .len();

    // Files smaller than one chunk: hash whatever exists.
    if size < CHUNK_BYTES {
        let mut buf = vec![0u8; size as usize];
        file.read_exact(&mut buf)?;
        return Ok(format!("{:016x}", sum_u64_le(&buf) + size));
    }

    let mut head = vec![0u8; CHUNK_BYTES as usize];
    file.read_exact(&mut head)?;

    let mut tail = vec![0u8; CHUNK_BYTES as usize];
    file.seek(SeekFrom::End(-(CHUNK_BYTES as i64)))?;
    file.read_exact(&mut tail)?;

    let combined = sum_u64_le(&head)
        .wrapping_add(sum_u64_le(&tail))
        .wrapping_add(size);
    Ok(format!("{:016x}", combined))
}

/// Sum bytes as little-endian u64 words. Mirrors the canonical algorithm.
fn sum_u64_le(buf: &[u8]) -> u64 {
    let mut sum: u64 = 0;
    for chunk in buf.chunks_exact(8) {
        let mut b = [0u8; 8];
        b.copy_from_slice(chunk);
        sum = sum.wrapping_add(u64::from_le_bytes(b));
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_for_same_content() {
        // Write identical bytes to two temp files and confirm identical hashes.
        let dir = std::env::temp_dir();
        let a = dir.join("mv_oshash_a.bin");
        let b = dir.join("mv_oshash_b.bin");
        let payload = vec![0xABu8; 200_000]; // > CHUNK_BYTES so both head+tail are read
        std::fs::write(&a, &payload).unwrap();
        std::fs::write(&b, &payload).unwrap();
        assert_eq!(hash_file(&a).unwrap(), hash_file(&b).unwrap());

        // Different content → different hash.
        let mut other = payload.clone();
        other[0] = 0xCD;
        let c = dir.join("mv_oshash_c.bin");
        std::fs::write(&c, &other).unwrap();
        assert_ne!(hash_file(&a).unwrap(), hash_file(&c).unwrap());

        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
        let _ = std::fs::remove_file(&c);
    }
}
