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

/// Run an optional graphical picker without blocking the terminal event loop.
pub fn browse(
    initial: &Path,
    cancel: &std::sync::atomic::AtomicBool,
) -> io::Result<Option<PathBuf>> {
    use std::process::Command;
    let path = std::env::var_os("PATH");
    let mut command = if crate::doctor::executable("kdialog", path.as_deref()) {
        let mut cmd = Command::new("kdialog");
        cmd.arg("--getexistingdirectory").arg(initial);
        cmd
    } else if crate::doctor::executable("zenity", path.as_deref()) {
        let mut cmd = Command::new("zenity");
        let mut arg = std::ffi::OsString::from("--filename=");
        arg.push(initial);
        arg.push("/");
        cmd.args(["--file-selection", "--directory"]).arg(arg);
        cmd
    } else {
        return Err(io::Error::other(
            "No graphical folder picker; use the path editor",
        ));
    };
    run_picker(&mut command, cancel)
}
fn run_picker(
    command: &mut std::process::Command,
    cancel: &std::sync::atomic::AtomicBool,
) -> io::Result<Option<PathBuf>> {
    use std::{io::Read, process::Stdio, sync::atomic::Ordering, time::Duration};
    if cancel.load(Ordering::Acquire) {
        return Ok(None);
    }
    let mut output = tempfile::tempfile()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(Stdio::null())
        .spawn()?;
    // No shell interpolation. The only accepted output is a bounded absolute
    // directory path, validated again before saving by the runtime.
    let status = loop {
        if cancel.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        if output.metadata().map_or(true, |m| m.len() > 4096) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("Folder picker output too large"));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        }
    };
    if !status.success() || cancel.load(Ordering::Acquire) {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    use std::io::{Seek, SeekFrom};
    output.seek(SeekFrom::Start(0))?;
    output.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(io::Error::other("Folder picker output too large"));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(io::Error::other)?
        .trim_end_matches(['\r', '\n']);
    if text.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(text);
    if !path.is_absolute() || !path.is_dir() {
        return Err(io::Error::other(
            "Folder picker did not return an existing absolute directory",
        ));
    }
    Ok(Some(path.canonicalize()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picker_validates_output_and_cancels_child() {
        use std::{
            process::Command,
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc,
            },
            time::{Duration, Instant},
        };
        let dir = tempfile::tempdir().unwrap();
        let mut cmd = Command::new("/usr/bin/printf");
        cmd.arg("%s\\n").arg(dir.path());
        assert_eq!(
            run_picker(&mut cmd, &AtomicBool::new(false)).unwrap(),
            Some(dir.path().canonicalize().unwrap())
        );
        let mut cmd = Command::new("/usr/bin/printf");
        cmd.args(["%s\\n", "relative"]);
        assert!(run_picker(&mut cmd, &AtomicBool::new(false)).is_err());
        let mut cmd = Command::new("/usr/bin/false");
        assert_eq!(run_picker(&mut cmd, &AtomicBool::new(false)).unwrap(), None);
        let flag = Arc::new(AtomicBool::new(false));
        let token = flag.clone();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            token.store(true, Ordering::Release);
        });
        let mut cmd = Command::new("/usr/bin/sleep");
        cmd.arg("30");
        let now = Instant::now();
        assert_eq!(run_picker(&mut cmd, &flag).unwrap(), None);
        worker.join().unwrap();
        assert!(now.elapsed() < Duration::from_secs(3));
    }
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
