//! Read-only model discovery.
pub mod gguf;
pub mod group;
use std::{
    collections::HashSet,
    fs, io,
    path::PathBuf,
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
}

pub fn scan(roots: Vec<PathBuf>, cancel: &AtomicBool) -> io::Result<Scan> {
    let mut result = Scan::default();
    let mut pending = roots;
    let mut seen = HashSet::new();
    while let Some(path) = pending.pop() {
        if cancel.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Scan cancelled"));
        }
        let Ok(path) = path.canonicalize() else {
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
                            Ok(e) => match e.file_type() {
                                Ok(t) if t.is_dir() || t.is_file() => pending.push(e.path()),
                                _ => {}
                            },
                            Err(_) => result.skipped += 1,
                        }
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => return Err(e),
                Err(_) => result.skipped += 1,
            }
        } else if meta.is_file()
            && path
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("gguf"))
        {
            match gguf::read(&path, cancel) {
                Ok(metadata) => result.candidates.push(Candidate {
                    path,
                    bytes: meta.len(),
                    version: metadata.version,
                    metadata,
                }),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => return Err(e),
                Err(_) => result.skipped += 1,
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
