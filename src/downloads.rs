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
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    verify_file(&mut file, item, cancel)
}
fn verify_file(file: &mut fs::File, item: &Item, cancel: &AtomicBool) -> io::Result<()> {
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() != item.file.size {
        return Err(io::Error::other("Downloaded size mismatch"));
    }
    if let Some(expected) = &item.file.sha256 {
        file.seek(SeekFrom::Start(0))?;
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
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "SHA-256 mismatch; resume to restart the partial file",
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
    transfer_from(
        item,
        cancel,
        progress,
        huggingface::resolve(&item.file)?,
        |path| fs2::available_space(path),
    )
}
fn transfer_from(
    item: &Item,
    cancel: &AtomicBool,
    progress: &AtomicU64,
    source: url::Url,
    available_space: impl Fn(&Path) -> io::Result<u64>,
) -> io::Result<()> {
    cancelled(cancel)?;

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
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
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
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
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
    // A full corrupted partial cannot be repaired by appending. A subsequent explicit resume
    // starts over, while the original failed attempt retains the staging file.
    if offset == item.file.size && item.file.sha256.is_some() {
        match verify_file(&mut file, item, cancel) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                file.set_len(0)?;
                offset = 0;
            }
            Err(e) => return Err(e),
        }
    }
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
    if available_space(parent)? < item.file.size - offset {
        return Err(io::Error::other("Insufficient free disk space"));
    }
    if offset < item.file.size {
        let mut url = source.clone();
        let http = huggingface::agent();
        let mut response = None;
        for _ in 0..8 {
            cancelled(cancel)?;
            let mut req = huggingface::request(&http, &url).set("Accept-Encoding", "identity");
            if offset > 0 {
                req = req.set("Range", &format!("bytes={offset}-"));
            }
            let r = match req.call() {
                Ok(response) => response,
                Err(ureq::Error::Status(416, response)) if offset > 0 => {
                    // Only restart on an explicit total matching the pinned file metadata.
                    if response
                        .header("Content-Range")
                        .and_then(|v| v.strip_prefix("bytes */"))
                        .and_then(|v| v.parse::<u64>().ok())
                        != Some(item.file.size)
                    {
                        return Err(io::Error::other(
                            "Server range total differs from pinned model size",
                        ));
                    }
                    file.set_len(0)?;
                    offset = 0;
                    progress.store(0, Ordering::Relaxed);
                    if available_space(parent)? < item.file.size {
                        return Err(io::Error::other("Insufficient free disk space"));
                    }
                    url = source.clone();
                    continue;
                }
                Err(e) => return Err(huggingface::error(e)),
            };
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
                if available_space(parent)? < item.file.size {
                    return Err(io::Error::other("Insufficient free disk space"));
                }
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
    verify_file(&mut file, item, cancel)?;
    let current = fs::symlink_metadata(&partial)?;
    let opened = file.metadata()?;
    if !current.is_file() || current.ino() != opened.ino() || current.dev() != opened.dev() {
        return Err(io::Error::other("Partial file changed during transfer"));
    }
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
    retry(
        cancel,
        || transfer(item, cancel, progress),
        Duration::from_millis(100),
    )
}
fn retry(
    cancel: &AtomicBool,
    mut operation: impl FnMut() -> io::Result<()>,
    delay: Duration,
) -> io::Result<()> {
    for attempt in 0..3 {
        cancelled(cancel)?;
        match operation() {
            Err(e) if retryable(&e) && attempt < 2 => {
                for _ in 0..(10 * (1 << attempt)) {
                    cancelled(cancel)?;
                    std::thread::sleep(delay);
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
    pub fn shutdown(&mut self) -> io::Result<()> {
        self.pause();
        if let Some((index, job, _)) = self.active.take() {
            self.queue.items[index].state = match job.finish_result() {
                Ok(()) => State::Complete,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => State::Paused,
                Err(_) => State::Failed,
            };
            self.queue.save(&self.path)?;
        }
        Ok(())
    }
    pub fn tick(&mut self) {
        let Some((_, job, progress)) = &mut self.active else {
            return;
        };
        let bytes = progress.load(Ordering::Relaxed);
        if bytes != u64::MAX {
            self.rate.update(Instant::now(), bytes);
        }
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
    fn fixture(root: &Path) -> Item {
        Item {
            file: File {
                repo: "owner/model".into(),
                revision: "a".repeat(40),
                name: "model.gguf".into(),
                size: 6,
                sha256: Some(format!("{:x}", Sha256::digest(b"abcdef"))),
            },
            root: root.into(),
            state: State::Queued,
        }
    }
    // A finite local HTTP server drives the actual ureq transfer path. The public
    // entrypoint still pins the origin to HTTPS Hugging Face.
    fn server(
        responses: Vec<(&'static str, &'static str)>,
    ) -> (url::Url, std::thread::JoinHandle<()>) {
        use std::{
            io::{BufRead, BufReader},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url =
            url::Url::parse(&format!("http://{}/model", listener.local_addr().unwrap())).unwrap();
        let handle = std::thread::spawn(move || {
            for (range, response) in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e)
                            if e.kind() == io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        Err(e) => panic!("fixture accept: {e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    headers.push_str(&line.to_ascii_lowercase());
                }
                if range.is_empty() {
                    assert!(!headers.contains("range: bytes="));
                } else {
                    assert!(headers.contains(range), "{headers}");
                }
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (url, handle)
    }
    #[test]
    fn resumes_206_and_restarts_200_and_416() {
        const FULL: &str =
            "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabcdef";
        const REST:&str="HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nConnection: close\r\n\r\ndef";
        const RANGE:&str="HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nContent-Range: bytes */6\r\nConnection: close\r\n\r\n";
        for responses in [
            vec![("range: bytes=3-", REST)],
            vec![("range: bytes=3-", FULL)],
            vec![("range: bytes=3-", RANGE), ("", FULL)],
        ] {
            let root = tempfile::tempdir().unwrap();
            let item = fixture(root.path());
            let dest = destination(&item).unwrap();
            let partial = dest.with_file_name("model.gguf.part");
            fs::write(&partial, b"abc").unwrap();
            let (url, worker) = server(responses);
            transfer_from(
                &item,
                &AtomicBool::new(false),
                &AtomicU64::new(0),
                url,
                |_| Ok(100),
            )
            .unwrap();
            worker.join().unwrap();
            assert_eq!(fs::read(&dest).unwrap(), b"abcdef");
            assert!(!partial.exists());
        }
    }
    #[test]
    fn rejects_wrong_range_and_changed_object() {
        for response in [
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/7\r\nConnection: close\r\n\r\ndef",
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nConnection: close\r\n\r\nxyz",
        ] {
            let root=tempfile::tempdir().unwrap();let item=fixture(root.path());let dest=destination(&item).unwrap();let partial=dest.with_file_name("model.gguf.part");fs::write(&partial,b"abc").unwrap();
            let (url,worker)=server(vec![("range: bytes=3-",response)]);
            assert!(transfer_from(&item,&AtomicBool::new(false),&AtomicU64::new(0),url,|_|Ok(100)).is_err());
            worker.join().unwrap();assert!(!dest.exists());assert!(partial.exists());
        }
    }
    #[test]
    fn interrupted_response_can_resume_and_lock_prevents_concurrent_writer() {
        let root = tempfile::tempdir().unwrap();
        let item = fixture(root.path());
        let dest = destination(&item).unwrap();
        let partial = dest.with_file_name("model.gguf.part");
        let (url, worker) = server(vec![(
            "",
            "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabc",
        )]);
        assert!(transfer_from(
            &item,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
            url,
            |_| Ok(100)
        )
        .is_err());
        worker.join().unwrap();
        assert_eq!(fs::read(&partial).unwrap(), b"abc");
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&partial)
            .unwrap();
        FileExt::try_lock_exclusive(&lock).unwrap();
        let url = url::Url::parse("http://127.0.0.1:1/").unwrap();
        assert!(transfer_from(
            &item,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
            url.clone(),
            |_| Ok(100)
        )
        .unwrap_err()
        .to_string()
        .contains("in use"));
        drop(lock);
        assert!(transfer_from(
            &item,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
            url,
            |_| Ok(0)
        )
        .unwrap_err()
        .to_string()
        .contains("disk space"));
        let (url,worker)=server(vec![("range: bytes=3-","HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nConnection: close\r\n\r\ndef")]);
        transfer_from(
            &item,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
            url,
            |_| Ok(100),
        )
        .unwrap();
        worker.join().unwrap();
        assert_eq!(fs::read(dest).unwrap(), b"abcdef");
    }
    #[test]
    fn bounded_retry_exhaustion_and_cancellation() {
        let cancel = AtomicBool::new(false);
        let mut attempts = 0;
        assert!(retry(
            &cancel,
            || {
                attempts += 1;
                Err(io::ErrorKind::TimedOut.into())
            },
            Duration::ZERO
        )
        .is_err());
        assert_eq!(attempts, 3);
        attempts = 0;
        assert_eq!(
            retry(
                &cancel,
                || {
                    attempts += 1;
                    cancel.store(true, Ordering::Release);
                    Err(io::ErrorKind::TimedOut.into())
                },
                Duration::ZERO
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
        assert_eq!(attempts, 1);
    }
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
    fn checksums_and_existing_files_are_not_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        let item = Item {
            file: File {
                repo: "owner/repo".into(),
                revision: "a".repeat(40),
                name: "test.gguf".into(),
                size: 3,
                sha256: Some(format!("{:x}", Sha256::digest(b"abc"))),
            },
            root: temp.path().into(),
            state: State::Queued,
        };
        let dest = destination(&item).unwrap();
        fs::write(&dest, b"bad").unwrap();
        assert_eq!(
            transfer(&item, &AtomicBool::new(false), &AtomicU64::new(0))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(fs::read(&dest).unwrap(), b"bad");
        fs::write(&dest, b"abc").unwrap();
        assert!(verify(&dest, &item, &AtomicBool::new(false)).is_ok());
        fs::remove_file(&dest).unwrap();
        std::os::unix::fs::symlink("/dev/null", &dest).unwrap();
        assert!(verify(&dest, &item, &AtomicBool::new(false)).is_err());
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
