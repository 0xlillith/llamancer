//! Read-only model discovery.
pub mod estimate;
pub mod gguf;
pub mod group;
pub mod library;
pub mod profile;
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub path: PathBuf,
    pub bytes: u64,
    pub version: u32,
    pub metadata: gguf::Metadata,
}
#[derive(Debug, Default)]
pub struct Scan {
    pub candidates: Vec<Candidate>,
    pub skipped: usize,
    pub invalid: Vec<(PathBuf, io::ErrorKind)>,
    pub invalid_total: usize,
}

/// Preserve the logical filename of a file reference (including a Hub snapshot
/// symlink to a content-addressed blob). Canonicalize only its parent.
pub fn file_reference(path: &Path) -> io::Result<PathBuf> {
    if !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Expected a regular file",
        ));
    }
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("Missing filename"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(parent.canonicalize()?.join(name))
}
fn gguf_name(path: &Path) -> bool {
    path.extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("gguf"))
}
fn directory_entry(entry: &fs::DirEntry) -> bool {
    entry.file_type().is_ok_and(|t| {
        t.is_dir()
            || t.is_file()
            || (t.is_symlink() && gguf_name(&entry.path()) && entry.path().is_file())
    })
}
pub fn scan(roots: Vec<PathBuf>, cancel: &AtomicBool) -> io::Result<Scan> {
    let mut result = Scan::default();
    let mut pending = Vec::new();
    for root in roots {
        // For explicitly imported shards, include matching siblings only.
        let canonical = if root.is_file() {
            file_reference(&root).unwrap_or(root)
        } else {
            root.canonicalize().unwrap_or(root)
        };
        if canonical.is_file() {
            if let Some((base, _, total)) = group::shard(&canonical) {
                if let Some(parent) = canonical.parent() {
                    if let Ok(entries) = fs::read_dir(parent) {
                        for entry in entries.flatten() {
                            if cancel.load(Ordering::Acquire) {
                                return Err(io::Error::new(
                                    io::ErrorKind::Interrupted,
                                    "Scan cancelled",
                                ));
                            }
                            let path = entry.path();
                            if directory_entry(&entry)
                                && path.is_file()
                                && gguf_name(&path)
                                && group::shard(&path)
                                    .is_some_and(|(b, _, t)| b == base && t == total)
                            {
                                pending.push(path);
                            }
                        }
                    }
                }
            }
        }
        pending.push(canonical);
    }
    let mut seen = HashSet::new();
    while let Some(path) = pending.pop() {
        if cancel.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Scan cancelled"));
        }
        let normalized = if path.is_file() {
            file_reference(&path)
        } else {
            path.canonicalize()
        };
        let Ok(path) = normalized else {
            result.skipped += 1;
            continue;
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        let Ok(meta) = fs::metadata(&path) else {
            result.skipped += 1;
            continue;
        };
        if meta.is_dir() {
            match fs::read_dir(&path) {
                Ok(entries) => {
                    for entry in entries {
                        if cancel.load(Ordering::Acquire) {
                            return Err(io::Error::new(
                                io::ErrorKind::Interrupted,
                                "Scan cancelled",
                            ));
                        }
                        match entry {
                            Ok(e) if directory_entry(&e) => pending.push(e.path()),
                            Ok(_) => {}
                            Err(_) => result.skipped += 1,
                        }
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => return Err(e),
                Err(_) => result.skipped += 1,
            }
        } else if meta.is_file() && gguf_name(&path) {
            match gguf::read(&path, cancel) {
                Ok(metadata) => result.candidates.push(Candidate {
                    path,
                    bytes: meta.len(),
                    version: metadata.version,
                    metadata,
                }),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => return Err(e),
                Err(e) => {
                    result.skipped += 1;
                    result.invalid_total += 1;
                    if result.invalid.len() < 1000 {
                        result.invalid.push((path, e.kind()));
                    }
                }
            }
        }
    }
    result.candidates.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scans_unique_candidates_and_skips_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("model.gguf");
        let mut bytes = [0u8; 24];
        bytes[..4].copy_from_slice(b"GGUF");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        fs::write(&p, bytes).unwrap();
        fs::write(dir.path().join("bad.gguf"), b"broken").unwrap();
        fs::write(dir.path().join("partial.gguf.part"), bytes).unwrap();
        let result = scan(vec![dir.path().into(), p], &AtomicBool::new(false)).unwrap();
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.skipped, 1);
        assert_eq!(result.invalid.len(), 1);
    }
    #[test]
    fn explicit_shard_import_does_not_scan_unrelated_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = [0u8; 24];
        bytes[..4].copy_from_slice(b"GGUF");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        let a = dir.path().join("model-00001-of-00002.gguf");
        fs::write(&a, bytes).unwrap();
        fs::write(dir.path().join("model-00002-of-00002.gguf"), bytes).unwrap();
        fs::write(dir.path().join("unrelated.gguf"), bytes).unwrap();
        let result = scan(vec![a], &AtomicBool::new(false)).unwrap();
        assert_eq!(result.candidates.len(), 2);
    }
    #[test]
    fn preserves_hub_snapshot_filenames_and_does_not_follow_directory_links() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let blobs = root.path().join("blobs");
        let snapshot = root.path().join("snapshots/revision");
        fs::create_dir_all(&blobs).unwrap();
        fs::create_dir_all(&snapshot).unwrap();
        let mut header = [0u8; 24];
        header[..4].copy_from_slice(b"GGUF");
        header[4..8].copy_from_slice(&3u32.to_le_bytes());
        for index in 1..=2 {
            let blob = blobs.join(format!("hash{index}"));
            fs::write(&blob, header).unwrap();
            symlink(
                blob,
                snapshot.join(format!("model-{index:05}-of-00002.gguf")),
            )
            .unwrap();
        }
        let first = snapshot.join("model-00001-of-00002.gguf");
        symlink(root.path(), snapshot.join("loop")).unwrap();
        symlink(root.path().join("missing"), snapshot.join("missing.gguf")).unwrap();
        let explicit = scan(vec![first.clone()], &AtomicBool::new(false)).unwrap();
        assert_eq!(explicit.candidates.len(), 2);
        assert_eq!(explicit.candidates[0].path, first);
        let all = scan(vec![root.path().into(), first], &AtomicBool::new(false)).unwrap();
        assert_eq!(all.candidates.len(), 2);
        assert!(all.candidates.iter().all(|c| c.path.starts_with(&snapshot)));
    }
    #[test]
    fn scans_read_only_root_without_mutating_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("read-only.gguf");
        let mut bytes = [0u8; 24];
        bytes[..4].copy_from_slice(b"GGUF");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        fs::write(&path, bytes).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let result = scan(vec![dir.path().into()], &AtomicBool::new(false));
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(result.unwrap().candidates.len(), 1);
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    #[test]
    fn cancels_before_traversal() {
        assert_eq!(
            scan(vec![".".into()], &AtomicBool::new(true))
                .unwrap_err()
                .kind(),
            io::ErrorKind::Interrupted
        );
    }
}

pub mod fit;
