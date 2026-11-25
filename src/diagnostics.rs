//! Local, bounded diagnostics. Only fixed event codes and numeric counters are persisted.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
const MAX_BYTES: u64 = 256 * 1024;
pub struct SessionLock(File);
impl SessionLock {
    pub fn acquire(root: &Path) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(root.join("tui.lock"))?;
        if !file.metadata()?.is_file() || file.metadata()?.nlink() != 1 {
            return Err(io::Error::other("Unsafe session lock file"));
        }
        fs2::FileExt::try_lock_exclusive(&file).map_err(|_| {
            io::Error::other("Another Llamancer TUI is using this configuration directory")
        })?;
        Ok(Self(file))
    }
}
impl Drop for SessionLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}
#[derive(Debug, Clone, Copy)]
pub enum Event {
    Startup,
    Shutdown,
    ScanStarted,
    ScanCompleted,
    ScanCancelled,
    ScanFailed,
    SettingsSaved,
    InputFailed,
}
impl Event {
    fn name(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Shutdown => "shutdown",
            Self::ScanStarted => "scan_started",
            Self::ScanCompleted => "scan_completed",
            Self::ScanCancelled => "scan_cancelled",
            Self::ScanFailed => "scan_failed",
            Self::SettingsSaved => "settings_saved",
            Self::InputFailed => "input_failed",
        }
    }
}
pub struct Diagnostics {
    path: PathBuf,
}
impl Diagnostics {
    pub fn new(root: &Path) -> Self {
        Self {
            path: root.join("diagnostics.jsonl"),
        }
    }
    pub fn record(&self, event: Event, count: Option<usize>) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.path)?;
        if !file.metadata()?.is_file() || file.metadata()?.nlink() != 1 {
            return Err(io::Error::other("Unsafe diagnostics file"));
        }
        // A single TUI owns this log; reset at the size cap, never retain prompt or path text.
        if file.metadata()?.len() >= MAX_BYTES {
            file.set_len(0)?;
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        writeln!(
            file,
            "{}",
            serde_json::json!({"timestamp":timestamp,"event":event.name(),"count":count})
        )?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exclusive_lock_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let first = SessionLock::acquire(dir.path()).unwrap();
        assert!(SessionLock::acquire(dir.path()).is_err());
        drop(first);
        assert!(SessionLock::acquire(dir.path()).is_ok());
    }
    #[test]
    fn diagnostics_bounded_and_structured() {
        let dir = tempfile::tempdir().unwrap();
        let log = Diagnostics::new(dir.path());
        fs::write(&log.path, vec![b'x'; MAX_BYTES as usize]).unwrap();
        log.record(Event::ScanCompleted, Some(2)).unwrap();
        let text = fs::read_to_string(&log.path).unwrap();
        let event: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(event["event"], "scan_completed");
        assert_eq!(event["count"], 2);
        assert!(text.len() < 256);
    }
}
