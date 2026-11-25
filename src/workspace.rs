//! Explicit workspace selection; never run code while discovering projects.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const MAX_PROJECTS: usize = 512;

pub fn projects(root: &Path) -> io::Result<Vec<PathBuf>> {
    let root = root.canonicalize()?;
    let mut result = Vec::new();
    // Bound both retained results and inspected entries; large workspaces should be split.
    for entry in fs::read_dir(&root)?.take(4096) {
        let entry = entry?;
        if entry.file_type()?.is_dir() && !entry.file_name().to_string_lossy().starts_with('.') {
            result.push(entry.path());
            if result.len() >= MAX_PROJECTS {
                break;
            }
        }
    }
    result.sort();
    Ok(result)
}

pub fn select(root: &Path, candidate: &Path) -> io::Result<PathBuf> {
    let root = root.canonicalize()?;
    if !fs::symlink_metadata(candidate)?.is_dir() {
        return Err(io::Error::other("Project is no longer a regular directory"));
    }
    let path = candidate.canonicalize()?;
    if !path.is_dir() || path.parent() != Some(root.as_path()) {
        return Err(io::Error::other(
            "Choose an existing direct child of the workspace root",
        ));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_visible_projects_only() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["b", "a", ".hidden"] {
            fs::create_dir(dir.path().join(name)).unwrap();
        }
        fs::write(dir.path().join("file"), "").unwrap();
        std::os::unix::fs::symlink("/tmp", dir.path().join("external")).unwrap();
        let found = projects(dir.path()).unwrap();
        assert_eq!(found, vec![dir.path().join("a"), dir.path().join("b")]);
        assert!(select(dir.path(), &dir.path().join("a")).is_ok());
        assert!(select(dir.path(), &dir.path().join("external")).is_err());
        assert!(select(dir.path(), dir.path()).is_err());
    }
}
