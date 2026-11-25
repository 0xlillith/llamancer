//! Persistent, resumable, single-transfer queue. Partial files never use a GGUF suffix.
use crate::{
    huggingface::{self, File},
    jobs::Job,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::{
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum State {
    Queued,
    Paused,
    Complete,
    Failed,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub file: File,
    pub root: PathBuf,
    pub state: State,
}
#[derive(Default, Serialize, Deserialize)]
pub struct Queue {
    pub items: Vec<Item>,
}
impl Queue {
    pub fn load(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(s) => {
                let queue: Self = toml::from_str(&s).map_err(io::Error::other)?;
                for item in &queue.items {
                    huggingface::validate(&item.file)?;
                    if !item.root.is_absolute() {
                        return Err(io::Error::other("Queue root must be absolute"));
                    }
                }
                Ok(queue)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("Missing queue parent"))?;
        let mut f = tempfile::NamedTempFile::new_in(parent)?;
        f.write_all(toml::to_string(self).map_err(io::Error::other)?.as_bytes())?;
        f.as_file().sync_all()?;
        f.persist(path).map_err(|e| e.error)?;
        fs::File::open(parent)?.sync_all()
    }
}
fn destination(item: &Item) -> io::Result<PathBuf> {
    huggingface::validate(&item.file)?;
    let mut dir = item.root.canonicalize()?;
    if !dir.is_dir() {
        return Err(io::Error::other("Download root is not a directory"));
    }
    let mut parts: Vec<_> = item.file.repo.split('/').collect();
    parts.push(&item.file.revision);
    let mut name: Vec<_> = item.file.name.split('/').collect();
    let leaf = name
        .pop()
        .ok_or_else(|| io::Error::other("Missing filename"))?;
    parts.extend(name);
    for part in parts {
        dir.push(part);
        match fs::create_dir(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        let m = fs::symlink_metadata(&dir)?;
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err(io::Error::other("Unsafe destination directory"));
        }
    }
    Ok(dir.join(leaf))
}
fn cancelled(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Transfer paused",
        ))
    } else {
        Ok(())
    }
}
fn verify(path: &Path, item: &Item, cancel: &AtomicBool) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.len() != item.file.size {
        return Err(io::Error::other("Downloaded size mismatch"));
    }
    if let Some(expected) = &item.file.sha256 {
        let mut file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            cancelled(cancel)?;
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        if format!("{:x}", hash.finalize()) != expected.to_ascii_lowercase() {
            return Err(io::Error::other(
                "SHA-256 mismatch; partial retained for inspection",
            ));
        }
    }
    Ok(())
}
fn content_range(value: &str, offset: u64, total: u64) -> bool {
    let Some((range, length)) = value.strip_prefix("bytes ").and_then(|s| s.split_once('/')) else {
        return false;
    };
    let Some((start, end)) = range.split_once('-') else {
        return false;
    };
    start.parse::<u64>().ok() == Some(offset)
        && length.parse::<u64>().ok() == Some(total)
        && end.parse::<u64>().ok() == total.checked_sub(1)
}
fn transfer(item: &Item, cancel: &AtomicBool, progress: &AtomicU64) -> io::Result<()> {
    let dest = destination(item)?;
    if dest.exists() {
        return verify(&dest, item, cancel);
    }
    let partial = dest.with_file_name(format!(
        "{}.part",
        dest.file_name().unwrap_or_default().to_string_lossy()
    ));
    let mut file = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&partial)
    {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            if !fs::symlink_metadata(&partial)?.is_file() {
                return Err(io::Error::other("Unsafe partial file"));
            }
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&partial)?
        }
        Err(e) => return Err(e),
    };
    FileExt::try_lock_exclusive(&file)
        .map_err(|_| io::Error::other("Partial file is in use by another downloader"))?;
    if !file.metadata()?.is_file() || file.metadata()?.nlink() != 1 {
        return Err(io::Error::other("Unsafe partial file link count"));
    }
    let mut offset = file.metadata()?.len();
    // Without an authoritative checksum, restart instead of trusting an old prefix.
    if item.file.sha256.is_none() && offset > 0 {
        file.set_len(0)?;
        offset = 0;
    }
    if offset > item.file.size {
        return Err(io::Error::other("Partial file exceeds expected size"));
    }
    progress.store(offset, Ordering::Relaxed);
    let parent = dest
        .parent()
        .ok_or_else(|| io::Error::other("Missing destination parent"))?;
    if fs2::available_space(parent)? < item.file.size - offset {
        return Err(io::Error::other("Insufficient free disk space"));
    }
    if offset < item.file.size {
        let mut url = huggingface::resolve(&item.file)?;
        let http = huggingface::agent();
        let mut response = None;
        for _ in 0..8 {
            cancelled(cancel)?;
            let mut req = huggingface::request(&http, &url).set("Accept-Encoding", "identity");
            if offset > 0 {
                req = req.set("Range", &format!("bytes={offset}-"));
            }
            let r = req.call().map_err(huggingface::error)?;
            if matches!(r.status(), 301 | 302 | 303 | 307 | 308) {
                let location = r
                    .header("Location")
                    .ok_or_else(|| io::Error::other("Redirect without location"))?;
                url = url
                    .join(location)
                    .map_err(|_| io::Error::other("Invalid download redirect"))?;
                if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some()
                {
                    return Err(io::Error::other("Unsafe download redirect"));
                }
                continue;
            }
            response = Some(r);
            break;
        }
        let r = response.ok_or_else(|| io::Error::other("Too many download redirects"))?;
        match r.status() {
            206 if content_range(
                r.header("Content-Range").unwrap_or(""),
                offset,
                item.file.size,
            ) => {}
            200 => {
                file.set_len(0)?;
                offset = 0;
                progress.store(0, Ordering::Relaxed);
            }
            _ => return Err(io::Error::other("Unexpected download range response")),
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut reader = r.into_reader();
        let mut buffer = [0u8; 65536];
        loop {
            cancelled(cancel)?;
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            if offset
                .checked_add(n as u64)
                .is_none_or(|v| v > item.file.size)
            {
                return Err(io::Error::other("Download exceeds expected size"));
            }
            file.write_all(&buffer[..n])?;
            offset += n as u64;
            progress.store(offset, Ordering::Relaxed);
        }
    }
    file.sync_all()?;
    if file.metadata()?.len() != item.file.size {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Incomplete transfer",
        ));
    }
    verify(&partial, item, cancel)?;
    cancelled(cancel)?;
    // Atomic publication without overwriting an existing model.
    fs::hard_link(&partial, &dest)?;
    fs::remove_file(&partial)?;
    drop(file);
    if let Some(parent) = dest.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::TimedOut
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::WouldBlock
    )
}
fn transfer_with_retry(item: &Item, cancel: &AtomicBool, progress: &AtomicU64) -> io::Result<()> {
    for attempt in 0..3 {
        cancelled(cancel)?;
        match transfer(item, cancel, progress) {
            Err(e) if retryable(&e) && attempt < 2 => {
                for _ in 0..(10 * (1 << attempt)) {
                    cancelled(cancel)?;
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            result => return result,
        }
    }
    unreachable!("bounded retry loop always returns")
}
#[derive(Default)]
struct Rate {
    sample: Option<(Instant, u64)>,
    bytes_per_second: f64,
}
impl Rate {
    fn update(&mut self, now: Instant, bytes: u64) {
        if let Some((last, old)) = self.sample {
            if bytes < old {
                self.sample = Some((now, bytes));
                self.bytes_per_second = 0.0;
                return;
            }
            let seconds = now.duration_since(last).as_secs_f64();
            if seconds < 0.5 {
                return;
            }
            let rate = (bytes - old) as f64 / seconds;
            self.bytes_per_second = if self.bytes_per_second == 0.0 {
                rate
            } else {
                0.25 * rate + 0.75 * self.bytes_per_second
            };
        }
        self.sample = Some((now, bytes));
    }
}
pub struct Controller {
    pub queue: Queue,
    pub cursor: usize,
    pub status: String,
    pub path: PathBuf,
    active: Option<(usize, Job<()>, Arc<AtomicU64>)>,
    run_queue: bool,
    rate: Rate,
}
impl Controller {
    pub fn new(path: PathBuf) -> io::Result<Self> {
        Ok(Self {
            queue: Queue::load(&path)?,
            cursor: 0,
            status: String::new(),
            path,
            active: None,
            run_queue: false,
            rate: Rate::default(),
        })
    }
    pub fn busy(&self) -> bool {
        self.active.is_some()
    }
    pub fn progress(&self, index: usize) -> Option<u64> {
        self.active
            .as_ref()
            .filter(|(i, _, _)| *i == index)
            .map(|(_, _, p)| p.load(Ordering::Relaxed))
            .filter(|&n| n != u64::MAX)
    }
    pub fn speed_eta(&self, index: usize) -> Option<(f64, u64)> {
        let bytes = self.progress(index)?;
        let speed = self.rate.bytes_per_second;
        let remaining = self.queue.items.get(index)?.file.size.saturating_sub(bytes);
        (speed >= 1.0 && remaining > 0).then(|| (speed, (remaining as f64 / speed).ceil() as u64))
    }
    pub fn add(&mut self, files: Vec<File>, root: PathBuf) -> io::Result<()> {
        for file in &files {
            huggingface::validate(file)?;
        }
        let old = self.queue.items.len();
        for file in files {
            if !self
                .queue
                .items
                .iter()
                .any(|i| i.file == file && i.root == root)
            {
                self.queue.items.push(Item {
                    file,
                    root: root.clone(),
                    state: State::Queued,
                });
            }
        }
        if let Err(e) = self.queue.save(&self.path) {
            self.queue.items.truncate(old);
            return Err(e);
        }
        Ok(())
    }
    pub fn start(&mut self) -> io::Result<()> {
        if self.busy() {
            return Err(io::Error::other("A download is already active"));
        }
        let index = self.cursor;
        let item = self
            .queue
            .items
            .get(index)
            .ok_or_else(|| io::Error::other("Select a queued file"))?
            .clone();
        if item.state == State::Complete {
            return Err(io::Error::other("File already complete"));
        }
        let progress = Arc::new(AtomicU64::new(u64::MAX));
        let p = Arc::clone(&progress);
        let job = Job::start(move |cancel| transfer_with_retry(&item, &cancel, &p))?;
        self.rate = Rate::default();
        self.active = Some((index, job, progress));
        self.status = "Downloading…".into();
        Ok(())
    }
    pub fn start_queue(&mut self) -> io::Result<()> {
        if self.busy() {
            return Err(io::Error::other("A download is already active"));
        }
        self.cursor = self
            .queue
            .items
            .iter()
            .position(|i| i.state == State::Queued)
            .ok_or_else(|| {
                io::Error::other("No queued files; resume paused/failed items individually")
            })?;
        self.start()?;
        self.run_queue = true;
        Ok(())
    }
    pub fn remove_selected(&mut self) -> io::Result<()> {
        if self.busy() {
            return Err(io::Error::other(
                "Pause active download before removing a queue entry",
            ));
        }
        if self.cursor >= self.queue.items.len() {
            return Err(io::Error::other("Select a queue entry"));
        }
        let item = self.queue.items.remove(self.cursor);
        if let Err(e) = self.queue.save(&self.path) {
            self.queue.items.insert(self.cursor, item);
            return Err(e);
        }
        self.cursor = self.cursor.min(self.queue.items.len().saturating_sub(1));
        self.status = "Queue entry removed; files retained".into();
        Ok(())
    }
    pub fn pause(&mut self) {
        self.run_queue = false;
        if let Some((_, job, _)) = &self.active {
            job.cancel();
            self.status = "Pausing…".into();
        }
    }
    pub fn tick(&mut self) {
        let Some((_, job, progress)) = &mut self.active else {
            return;
        };
        self.rate
            .update(Instant::now(), progress.load(Ordering::Relaxed));
        if let Some(result) = job.poll() {
            if let Some((index, job, _)) = self.active.take() {
                job.finish();
                let (state, status) = match result {
                    Ok(()) => (State::Complete, "Download complete; rescan Library".into()),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                        (State::Paused, "Download paused".into())
                    }
                    Err(e) => (State::Failed, e.to_string()),
                };
                let completed = state == State::Complete;
                self.queue.items[index].state = state;
                self.status = status;
                if self.queue.save(&self.path).is_err() {
                    self.status.push_str("; queue save failed");
                    self.run_queue = false;
                }
                if self.run_queue && completed {
                    if self.start_queue().is_err() {
                        self.run_queue = false;
                        self.status = "Queue complete; rescan Library".into();
                    }
                } else {
                    self.run_queue = false;
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rate_ignores_initial_resumed_bytes() {
        let now = Instant::now();
        let mut rate = Rate::default();
        rate.update(now, 10_000);
        assert_eq!(rate.bytes_per_second, 0.0);
        rate.update(now + Duration::from_secs(1), 11_000);
        assert_eq!(rate.bytes_per_second, 1000.0);
        rate.update(now + Duration::from_secs(2), 10);
        assert_eq!(rate.bytes_per_second, 0.0);
        assert!(retryable(&io::Error::from(io::ErrorKind::TimedOut)));
        assert!(!retryable(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
    }
    #[test]
    fn range_validation() {
        assert!(content_range("bytes 20-99/100", 20, 100));
        assert!(!content_range("bytes 0-99/100", 20, 100));
        assert!(!content_range("bytes 20-99/200", 20, 100));
    }
    #[test]
    fn queue_roundtrip_and_non_destructive_removal() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("queue.toml");
        let file = File {
            repo: "owner/repo".into(),
            revision: "a".repeat(40),
            name: "model.gguf".into(),
            size: 3,
            sha256: None,
        };
        let mut controller = Controller::new(path.clone()).unwrap();
        controller
            .add(vec![file.clone()], temp.path().into())
            .unwrap();
        let dest = destination(&controller.queue.items[0]).unwrap();
        fs::write(&dest, b"abc").unwrap();
        assert_eq!(Queue::load(&path).unwrap().items.len(), 1);
        controller.remove_selected().unwrap();
        assert!(dest.exists());
        assert!(Queue::load(&path).unwrap().items.is_empty());
        let mut bad = file.clone();
        bad.name = "../escape.gguf".into();
        assert!(controller.add(vec![file, bad], temp.path().into()).is_err());
        assert!(controller.queue.items.is_empty());
    }
}
