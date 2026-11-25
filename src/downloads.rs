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
    Quarantined,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub file: File,
    pub root: PathBuf,
    pub state: State,
    #[serde(default)]
    pub receipt: Option<Receipt>,
}
/// Ownership evidence is recorded only after publishing a newly downloaded file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Receipt {
    device: String,
    inode: String,
    bytes: u64,
    mtime: i64,
    mtime_nsec: i64,
}
impl Receipt {
    fn from_metadata(m: &fs::Metadata) -> Self {
        Self {
            device: format!("{:x}", m.dev()),
            inode: format!("{:x}", m.ino()),
            bytes: m.len(),
            mtime: m.mtime(),
            mtime_nsec: m.mtime_nsec(),
        }
    }
    fn matches(&self, m: &fs::Metadata) -> bool {
        m.is_file() && m.nlink() == 1 && *self == Self::from_metadata(m)
    }
}
mod quarantine;
#[derive(Default, Serialize, Deserialize)]
pub struct Queue {
    pub items: Vec<Item>,
}
impl Queue {
    pub fn load(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(s) => {
                let mut queue: Self = toml::from_str(&s).map_err(io::Error::other)?;
                for item in &mut queue.items {
                    huggingface::validate(&item.file)?;
                    if !item.root.is_absolute() {
                        return Err(io::Error::other("Queue root must be absolute"));
                    }
                    quarantine::reconcile(item);
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
/// Hold directory descriptors across transfer, so a symlink or rename cannot redirect
/// writes to another tree. The Linux /proc/self/fd paths address the opened directory.
struct Destination {
    parent: fs::File,
    visible: PathBuf,
    leaf: String,
}
impl Destination {
    fn open(item: &Item) -> io::Result<Self> {
        huggingface::validate(&item.file)?;
        let mut visible = item.root.canonicalize()?;
        let mut parent = Self::open_directory(&visible)?;
        let mut parts: Vec<_> = item.file.repo.split('/').collect();
        parts.push(&item.file.revision);
        let mut name: Vec<_> = item.file.name.split('/').collect();
        let leaf = name
            .pop()
            .ok_or_else(|| io::Error::other("Missing filename"))?
            .to_owned();
        parts.extend(name);
        for part in parts {
            let next = Self::fd_path(&parent).join(part);
            match fs::create_dir(&next) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
            parent = Self::open_directory(&next)?;
            visible.push(part);
        }
        let result = Self {
            parent,
            visible,
            leaf,
        };
        result.check_visible()?;
        Ok(result)
    }
    fn fd_path(file: &fs::File) -> PathBuf {
        use std::os::fd::AsRawFd;
        PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
    }
    fn open_directory(path: &Path) -> io::Result<fs::File> {
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
    }
    fn check_visible(&self) -> io::Result<()> {
        let current = fs::symlink_metadata(&self.visible)?;
        let opened = self.parent.metadata()?;
        if !current.is_dir()
            || !same_inode(&current, &opened)
            || self.visible.canonicalize()? != self.visible
        {
            return Err(io::Error::other("Download destination moved or replaced"));
        }
        Ok(())
    }
    fn path(&self) -> PathBuf {
        Self::fd_path(&self.parent).join(&self.leaf)
    }
    fn partial(&self) -> PathBuf {
        Self::fd_path(&self.parent).join(format!("{}.part", self.leaf))
    }
    fn publish(&self, file: &fs::File) -> io::Result<()> {
        use std::{
            ffi::CString,
            os::{fd::AsRawFd, unix::ffi::OsStrExt},
        };
        self.check_visible()?;
        let partial = self.partial();
        let current = fs::symlink_metadata(&partial)?;
        let opened = file.metadata()?;
        if !current.is_file() || !same_inode(&current, &opened) || opened.nlink() != 1 {
            return Err(io::Error::other("Partial file changed during transfer"));
        }
        // Link the verified open inode, not a possibly replaced staging pathname.
        let from =
            CString::new(Self::fd_path(file).as_os_str().as_bytes()).map_err(io::Error::other)?;
        let to = CString::new(self.leaf.as_bytes()).map_err(io::Error::other)?;
        // SAFETY: both C strings live through the call; parent is an open directory.
        let result = unsafe {
            libc::linkat(
                libc::AT_FDCWD,
                from.as_ptr(),
                self.parent.as_raw_fd(),
                to.as_ptr(),
                libc::AT_SYMLINK_FOLLOW,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        self.parent.sync_all()?;
        // A competing writer's replacement is never deliberately unlinked.
        if same_inode(&fs::symlink_metadata(&partial)?, &opened) {
            fs::remove_file(&partial)?;
            self.parent.sync_all()?;
        } else {
            return Err(io::Error::other(
                "Verified model published; staging path changed",
            ));
        }
        self.check_visible()
    }
}
fn same_inode(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.ino() == b.ino() && a.dev() == b.dev()
}
#[cfg(test)]
fn destination(item: &Item) -> io::Result<PathBuf> {
    let destination = Destination::open(item)?;
    Ok(destination.visible.join(destination.leaf))
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
fn transfer(item: &Item, cancel: &AtomicBool, progress: &AtomicU64) -> io::Result<Option<Receipt>> {
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
) -> io::Result<Option<Receipt>> {
    cancelled(cancel)?;

    let destination = Destination::open(item)?;
    let dest = destination.path();
    match fs::symlink_metadata(&dest) {
        Ok(_) => {
            verify(&dest, item, cancel)?;
            destination.check_visible()?;
            return Ok(None);
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let partial = destination.partial();
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
    let parent_path = Destination::fd_path(&destination.parent);
    let parent = parent_path.as_path();
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
    cancelled(cancel)?;
    destination.publish(&file)?;
    Ok(if item.file.sha256.is_some() {
        Some(Receipt::from_metadata(&file.metadata()?))
    } else {
        None
    })
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
fn transfer_with_retry(
    item: &Item,
    cancel: &AtomicBool,
    progress: &AtomicU64,
) -> io::Result<Option<Receipt>> {
    retry(
        cancel,
        || transfer(item, cancel, progress),
        Duration::from_millis(100),
    )
}
fn retry<T>(
    cancel: &AtomicBool,
    mut operation: impl FnMut() -> io::Result<T>,
    delay: Duration,
) -> io::Result<T> {
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
    active: Option<(usize, Job<Option<Receipt>>, Arc<AtomicU64>)>,
    mutation: Option<Job<()>>,
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
            mutation: None,
            run_queue: false,
            rate: Rate::default(),
        })
    }
    pub fn busy(&self) -> bool {
        self.active.is_some() || self.mutation.is_some()
    }
    pub fn mutating(&self) -> bool {
        self.mutation.is_some()
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
                    receipt: None,
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
        if matches!(item.state, State::Complete | State::Quarantined) {
            return Err(io::Error::other(
                "File is complete or quarantined; restore it instead",
            ));
        }
        if item.receipt.is_some() {
            return Err(io::Error::other(
                "Managed file requires quarantine recovery; do not redownload over it",
            ));
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
        if self.queue.items[self.cursor].receipt.is_some() {
            return Err(io::Error::other(
                "Keep ownership receipts for recoverable model removal",
            ));
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
    fn reconcile_mutation(&mut self) -> io::Result<()> {
        for item in &mut self.queue.items {
            quarantine::reconcile(item);
        }
        self.queue.save(&self.path)
    }
    pub fn quarantine_targets(&self) -> io::Result<Vec<usize>> {
        let selected = self
            .queue
            .items
            .get(self.cursor)
            .ok_or_else(|| io::Error::other("Select a downloaded model"))?;
        let same: Vec<_> = self
            .queue
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.root == selected.root
                    && item.file.repo == selected.file.repo
                    && item.file.revision == selected.file.revision
            })
            .collect();
        let files: Vec<_> = same.iter().map(|(_, item)| item.file.clone()).collect();
        let index = same
            .iter()
            .position(|(i, _)| *i == self.cursor)
            .ok_or_else(|| io::Error::other("Missing queue entry"))?;
        let bundle = huggingface::bundle(&files, index);
        if bundle.is_empty() {
            return Err(io::Error::other(
                "All model shards must be present in the queue",
            ));
        }
        let targets: Vec<_> = same
            .into_iter()
            .filter(|(_, item)| bundle.contains(&item.file))
            .map(|(i, _)| i)
            .collect();
        if targets.len() != bundle.len()
            || targets.iter().any(|&i| {
                self.queue.items[i].receipt.is_none() || self.queue.items[i].file.sha256.is_none()
            })
        {
            return Err(io::Error::other(
                "Quarantine requires a verified Llamancer ownership receipt for every shard",
            ));
        }
        Ok(targets)
    }
    pub fn select_managed_group(&mut self, group: &crate::models::group::Group) -> io::Result<()> {
        if !group.complete || group.files.is_empty() {
            return Err(io::Error::other("Select a complete managed model"));
        }
        let paths: std::collections::BTreeSet<_> =
            group.files.iter().map(|f| f.path.clone()).collect();
        let selected = self
            .queue
            .items
            .iter()
            .position(|item| {
                item.receipt.is_some()
                    && quarantine::expected(item)
                        .ok()
                        .and_then(|p| p.canonicalize().ok())
                        .is_some_and(|p| paths.contains(&p))
            })
            .ok_or_else(|| {
                io::Error::other(
                    "Imported or legacy models have no ownership receipt; use hide instead",
                )
            })?;
        let old = self.cursor;
        self.cursor = selected;
        let result = self.quarantine_targets().and_then(|indices| {
            let expected: std::collections::BTreeSet<_> = indices
                .iter()
                .map(|&i| quarantine::expected(&self.queue.items[i])?.canonicalize())
                .collect::<io::Result<_>>()?;
            if expected != paths {
                return Err(io::Error::other(
                    "Queue receipt bundle does not match the indexed model",
                ));
            }
            Ok(())
        });
        if result.is_err() {
            self.cursor = old;
        }
        result
    }
    pub fn quarantine(&mut self, restore: bool, endpoint: &str) -> io::Result<()> {
        if self.busy() {
            return Err(io::Error::other("Wait for the active download or mutation"));
        }
        let items = self
            .quarantine_targets()?
            .into_iter()
            .map(|i| self.queue.items[i].clone())
            .collect();
        let endpoint = endpoint.to_owned();
        self.mutation = Some(Job::start(move |cancel| {
            quarantine::move_items(items, restore, &endpoint, &cancel)
        })?);
        self.status = if restore {
            "Restoring verified files…"
        } else {
            "Verifying and quarantining owned files…"
        }
        .into();
        self.run_queue = false;
        Ok(())
    }
    pub fn pause(&mut self) {
        self.run_queue = false;
        if let Some(job) = &self.mutation {
            job.cancel();
        }
        if let Some((_, job, _)) = &self.active {
            job.cancel();
            self.status = "Pausing…".into();
        }
    }
    pub fn shutdown(&mut self) -> io::Result<()> {
        self.pause();
        if let Some(job) = self.mutation.take() {
            job.finish();
            self.reconcile_mutation()?;
        }
        if let Some((index, job, _)) = self.active.take() {
            self.queue.items[index].state = match job.finish_result() {
                Ok(receipt) => {
                    self.queue.items[index].receipt = receipt;
                    State::Complete
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => State::Paused,
                Err(_) => State::Failed,
            };
            self.queue.save(&self.path)?;
        }
        Ok(())
    }
    pub fn tick(&mut self) {
        if let Some(job) = self.mutation.as_mut() {
            if let Some(result) = job.poll() {
                if let Some(job) = self.mutation.take() {
                    job.finish();
                }
                self.status = match result {
                    Ok(()) => "Quarantine operation complete; rescan Library".into(),
                    Err(e) => format!("Quarantine: {e}; inspect queue before retrying"),
                };
                if self.reconcile_mutation().is_err() {
                    self.status.push_str("; queue save failed");
                }
            }
            return;
        }
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
                    Ok(receipt) => {
                        self.queue.items[index].receipt = receipt;
                        (State::Complete, "Download complete; rescan Library".into())
                    }
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
            receipt: None,
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
    fn changed_directory_never_redirects_publication() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let item = fixture(root.path());
        let dest = destination(&item).unwrap();
        let parent = dest.parent().unwrap();
        let moved = parent.with_extension("moved");
        let changed = AtomicBool::new(false);
        let (url, worker) = server(vec![(
            "",
            "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabcdef",
        )]);
        let result = transfer_from(
            &item,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
            url,
            |_| {
                if !changed.swap(true, Ordering::AcqRel) {
                    fs::rename(parent, &moved).unwrap();
                    symlink(outside.path(), parent).unwrap();
                }
                Ok(100)
            },
        );
        worker.join().unwrap();
        assert!(result.is_err());
        assert!(!outside.path().join("model.gguf").exists());
        assert_eq!(fs::read(moved.join("model.gguf.part")).unwrap(), b"abcdef");
    }
    #[test]
    fn changed_partial_is_not_published_or_removed() {
        let root = tempfile::tempdir().unwrap();
        let item = fixture(root.path());
        let dest = destination(&item).unwrap();
        let partial = dest.with_file_name("model.gguf.part");
        let saved = dest.with_file_name("original.part");
        let changed = AtomicBool::new(false);
        let (url, worker) = server(vec![(
            "",
            "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabcdef",
        )]);
        let result = transfer_from(
            &item,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
            url,
            |_| {
                if !changed.swap(true, Ordering::AcqRel) {
                    fs::rename(&partial, &saved).unwrap();
                    fs::write(&partial, b"external").unwrap();
                }
                Ok(100)
            },
        );
        worker.join().unwrap();
        assert!(result.is_err());
        assert!(!dest.exists());
        assert_eq!(fs::read(partial).unwrap(), b"external");
        assert_eq!(fs::read(saved).unwrap(), b"abcdef");
    }
    #[test]
    fn multipart_download_is_indexed_only_when_all_shards_exist() {
        // Minimal, structurally valid GGUF headers with split metadata; no weights.
        fn shard(index: u32) -> Vec<u8> {
            let mut b = b"GGUF".to_vec();
            b.extend(3u32.to_le_bytes());
            b.extend(0u64.to_le_bytes());
            b.extend(2u64.to_le_bytes());
            for (key, value) in [("split.no", index), ("split.count", 2)] {
                b.extend((key.len() as u64).to_le_bytes());
                b.extend(key.as_bytes());
                b.extend(4u32.to_le_bytes());
                b.extend(value.to_le_bytes());
            }
            b.resize((b.len() + 31) & !31, 0);
            b
        }
        let root = tempfile::tempdir().unwrap();
        for index in 0..2 {
            let payload = shard(index);
            let mut item = fixture(root.path());
            item.file.name = format!("model-{:05}-of-00002.gguf", index + 1);
            item.file.size = payload.len() as u64;
            item.file.sha256 = Some(format!("{:x}", Sha256::digest(&payload)));
            // The fixture server accepts text only; serve the binary GGUF from a one-shot socket.
            use std::net::TcpListener;
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = url::Url::parse(&format!("http://{}/shard", listener.local_addr().unwrap()))
                .unwrap();
            let worker = std::thread::spawn(move || {
                use std::io::{BufRead, BufReader};
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                }
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                )
                .unwrap();
                stream.write_all(&payload).unwrap();
            });
            transfer_from(
                &item,
                &AtomicBool::new(false),
                &AtomicU64::new(0),
                url,
                |_| Ok(1024),
            )
            .unwrap();
            worker.join().unwrap();
            let groups = crate::models::group::group(
                crate::models::scan(vec![root.path().into()], &AtomicBool::new(false))
                    .unwrap()
                    .candidates,
            );
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0].complete, index == 1);
        }
    }
    #[test]
    fn bounded_retry_exhaustion_and_cancellation() {
        let cancel = AtomicBool::new(false);
        let mut attempts = 0;
        assert!(retry(
            &cancel,
            || {
                attempts += 1;
                Err::<(), _>(io::ErrorKind::TimedOut.into())
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
                    Err::<(), _>(io::ErrorKind::TimedOut.into())
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
            receipt: None,
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
