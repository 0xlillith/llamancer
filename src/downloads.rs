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

/// Planning is read-only; the actual transfer rechecks free space and path identity.
pub fn bundle_bytes(files: &[File]) -> io::Result<u64> {
    files.iter().try_fold(0u64, |sum, file| {
        sum.checked_add(file.size)
            .ok_or_else(|| io::Error::other("Bundle size overflow"))
    })
}

pub fn destination_space(root: &Path) -> io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    if !root.is_absolute() || !root.is_dir() || root.canonicalize()? != root {
        return Err(io::Error::other("Configured root is missing or changed"));
    }
    let name = std::ffi::CString::new(root.as_os_str().as_bytes()).map_err(io::Error::other)?;
    // SAFETY: valid NUL-terminated path and valid output storage for statvfs.
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(name.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let stat = unsafe { stat.assume_init() };
    if stat.f_flag & libc::ST_RDONLY != 0
        || unsafe {
            libc::faccessat(
                libc::AT_FDCWD,
                name.as_ptr(),
                libc::W_OK | libc::X_OK,
                libc::AT_EACCESS,
            )
        } != 0
    {
        return Err(io::Error::other("Download root is not writable"));
    }
    fs2::available_space(root)
}

pub fn check_destination(root: &Path, required: u64) -> io::Result<()> {
    if destination_space(root)? < required {
        return Err(io::Error::other(
            "Insufficient free space for complete bundle",
        ));
    }
    Ok(())
}

pub struct DestinationChoice {
    pub files: Vec<File>,
    pub roots: Vec<PathBuf>,
    pub available: Vec<Option<u64>>,
    pub required: u64,
    pub cursor: usize,
}
impl DestinationChoice {
    pub fn new(
        files: Vec<File>,
        roots: Vec<PathBuf>,
        preferred: Option<&Path>,
    ) -> io::Result<Self> {
        let required = bundle_bytes(&files)?;
        let cursor = roots
            .iter()
            .position(|p| Some(p.as_path()) == preferred)
            .unwrap_or(0);
        let available = roots.iter().map(|p| destination_space(p).ok()).collect();
        Ok(Self {
            files,
            roots,
            available,
            required,
            cursor,
        })
    }
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    root_identity: Option<RootIdentity>,
    pub state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_bytes: Option<u64>,
    #[serde(default)]
    pub receipt: Option<Receipt>,
}
// Directory identity, not mtime: adding files must not invalidate a queued root.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RootIdentity {
    device: String,
    inode: String,
}
impl RootIdentity {
    fn from_metadata(m: &fs::Metadata) -> Self {
        Self {
            device: format!("{:x}", m.dev()),
            inode: format!("{:x}", m.ino()),
        }
    }
}
fn open_root(root: &Path, expected: Option<&RootIdentity>) -> io::Result<fs::File> {
    if !root.is_absolute() || root.canonicalize()? != root {
        return Err(io::Error::other("Queued download root moved or redirected"));
    }
    let opened = Destination::open_directory(root)?;
    let metadata = opened.metadata()?;
    if !same_inode(&metadata, &fs::symlink_metadata(root)?)
        || expected.is_some_and(|id| *id != RootIdentity::from_metadata(&metadata))
    {
        return Err(io::Error::other(
            "Queued download root was replaced; restore the original directory",
        ));
    }
    Ok(opened)
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
        match crate::config::read_state(path, crate::config::MAX_STATE_BYTES) {
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
        let contents = toml::to_string(self).map_err(io::Error::other)?;
        crate::config::check_state_size(&contents)?;
        f.write_all(contents.as_bytes())?;
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
        let mut visible = item.root.clone();
        let mut parent = open_root(&visible, item.root_identity.as_ref())?;
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
        let actual = format!("{:x}", hash.finalize());
        if actual != expected.to_ascii_lowercase() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("SHA-256 mismatch; partial retained. Expected {expected}, got {actual}. Use Retry fresh to preserve the .part file and start a new transfer."),
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
// A lock must end with the transfer even if a concurrent fork briefly
// inherits its open-file description before exec closes the descriptor.
struct LockedPartial(fs::File);
impl LockedPartial {
    fn new(file: fs::File) -> io::Result<Self> {
        FileExt::try_lock_exclusive(&file)
            .map_err(|_| io::Error::other("Partial file is in use by another downloader"))?;
        Ok(Self(file))
    }
}
impl std::ops::Deref for LockedPartial {
    type Target = fs::File;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for LockedPartial {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for LockedPartial {
    fn drop(&mut self) {
        // The transfer is finished; no further file operations follow.
        let _ = FileExt::unlock(&self.0);
    }
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
    let file = match fs::OpenOptions::new()
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
    let mut file = LockedPartial::new(file)?;
    if !file.metadata()?.is_file() || file.metadata()?.nlink() != 1 {
        return Err(io::Error::other("Unsafe partial file link count"));
    }
    let mut offset = file.metadata()?.len();
    // Never truncate a fully downloaded, checksum-invalid file on resume.
    // Re-verify it and retain evidence. A correct complete partial can publish
    // immediately, without reading its entire contents twice.
    if offset == item.file.size && item.file.sha256.is_some() {
        progress.store(offset, Ordering::Relaxed);
        file.sync_all()?;
        verify_file(&mut file, item, cancel)?;
        cancelled(cancel)?;
        destination.publish(&file)?;
        return Ok(Some(Receipt::from_metadata(&file.metadata()?)));
    }
    // Without an authoritative digest, a resumed prefix cannot be trusted.
    if item.file.sha256.is_none() && offset > 0 {
        return Err(io::Error::other(
            "No SHA-256 for resumed prefix; partial retained. Choose Retry fresh.",
        ));
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
        let mut url = source;
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
                Err(ureq::Error::Status(416, _)) if offset > 0 => {
                    return Err(io::Error::other("Server rejected the resume range (HTTP 416); partial retained. Choose Retry fresh."));
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
        if r.header("Content-Encoding")
            .is_some_and(|v| !v.eq_ignore_ascii_case("identity"))
        {
            return Err(io::Error::other(
                "Unexpected transfer encoding; partial retained",
            ));
        }
        match r.status() {
            206 if content_range(
                r.header("Content-Range").unwrap_or(""),
                offset,
                item.file.size,
            ) => {}
            200 if offset == 0 => {}
            200 => return Err(io::Error::other(
                "Server ignored the resume range (HTTP 200); partial retained. Choose Retry fresh.",
            )),
            _ => return Err(io::Error::other("Unexpected download range response")),
        }
        if let Some(length) = r.header("Content-Length") {
            if length.parse::<u64>().ok() != Some(item.file.size - offset) {
                return Err(io::Error::other(
                    "HTTP response length differs from pinned model size; partial retained",
                ));
            }
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
fn preserve_failed_partial(item: &Item, cancel: &AtomicBool) -> io::Result<()> {
    use std::{
        ffi::CString,
        os::fd::AsRawFd,
        time::{SystemTime, UNIX_EPOCH},
    };
    if item.state != State::Failed || item.receipt.is_some() {
        return Err(io::Error::other(
            "Only a failed partial without ownership receipt can be retried fresh",
        ));
    }
    cancelled(cancel)?;
    let destination = Destination::open(item)?;
    match fs::symlink_metadata(destination.path()) {
        Ok(_) => {
            return Err(io::Error::other(
                "Published model exists; verify it instead",
            ))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let file = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(destination.partial())
    {
        Ok(file) => LockedPartial::new(file)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.nlink() != 1
        || !same_inode(&meta, &fs::symlink_metadata(destination.partial())?)
    {
        return Err(io::Error::other("Unsafe or changed partial file"));
    }
    if fs2::available_space(Destination::fd_path(&destination.parent))? < item.file.size {
        return Err(io::Error::other(
            "Not enough free space to keep the old partial and retry",
        ));
    }
    file.sync_all()?;
    destination.check_visible()?;
    cancelled(cancel)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let old = CString::new(format!("{}.part", destination.leaf)).map_err(io::Error::other)?;
    let saved = format!("{}.part.saved-{stamp}", destination.leaf);
    let new = CString::new(saved.as_bytes()).map_err(io::Error::other)?;
    let fd = destination.parent.as_raw_fd();
    // SAFETY: both leaf names and the directory descriptor remain valid.
    // Never replace another file; the old partial remains recoverable.
    if unsafe { libc::renameat2(fd, old.as_ptr(), fd, new.as_ptr(), libc::RENAME_NOREPLACE) } != 0 {
        return Err(io::Error::last_os_error());
    }
    destination.parent.sync_all()?;
    destination.check_visible()?;
    if !same_inode(
        &meta,
        &fs::symlink_metadata(Destination::fd_path(&destination.parent).join(saved))?,
    ) {
        return Err(io::Error::other(
            "Partial changed during preservation; inspect retained files",
        ));
    }
    Ok(())
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
    history: std::collections::VecDeque<f64>,
}
impl Rate {
    fn update(&mut self, now: Instant, bytes: u64) {
        if let Some((last, old)) = self.sample {
            if bytes < old {
                self.sample = Some((now, bytes));
                self.bytes_per_second = 0.0;
                self.history.clear();
                return;
            }
            let seconds = now.duration_since(last).as_secs_f64();
            if seconds < 0.5 {
                return;
            }
            let rate = (bytes - old) as f64 / seconds;
            if self.history.len() == 60 {
                self.history.pop_front();
            }
            self.history.push_back(rate);
            self.bytes_per_second = if self.bytes_per_second == 0.0 {
                rate
            } else {
                0.25 * rate + 0.75 * self.bytes_per_second
            };
        }
        self.sample = Some((now, bytes));
    }
}
/// A view of existing persistent file records, not a second queue.
pub struct BundleView {
    pub name: String,
    pub indices: Vec<usize>,
    pub expected: u64,
    pub bytes: u64,
    pub received: u64,
    pub complete: usize,
    pub active: Option<usize>,
    pub state: &'static str,
}
impl BundleView {
    pub fn label(&self) -> String {
        format!("{} ({}/{})", self.state, self.complete, self.expected)
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
    resume_parts: std::collections::VecDeque<(usize, bool)>,
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
            resume_parts: Default::default(),
            rate: Rate::default(),
        })
    }
    pub fn bundles(&self) -> Vec<BundleView> {
        use std::collections::BTreeMap;
        let mut by_key = BTreeMap::new();
        let mut groups: Vec<BundleView> = Vec::new();
        for (i, item) in self.queue.items.iter().enumerate() {
            let path = Path::new(&item.file.name);
            let (name, expected) = crate::models::group::shard(path)
                .map(|(base, _, total)| {
                    (
                        path.with_file_name(format!("{base}.gguf"))
                            .to_string_lossy()
                            .into_owned(),
                        total,
                    )
                })
                .unwrap_or_else(|| (item.file.name.clone(), 1));
            let key = (
                &item.root,
                &item.file.repo,
                &item.file.revision,
                name.clone(),
                expected,
            );
            let n = *by_key.entry(key).or_insert_with(|| {
                let n = groups.len();
                groups.push(BundleView {
                    name,
                    indices: vec![],
                    expected,
                    bytes: 0,
                    received: 0,
                    complete: 0,
                    active: None,
                    state: "Queued",
                });
                n
            });
            let g = &mut groups[n];
            g.indices.push(i);
            g.bytes = g.bytes.saturating_add(item.file.size);
            g.received = g
                .received
                .saturating_add(self.known_bytes(i).unwrap_or(0).min(item.file.size));
            if item.state == State::Complete {
                g.complete += 1;
            }
            if self.active.as_ref().is_some_and(|(j, _, _)| *j == i) {
                g.active = Some(i);
            }
        }
        for g in &mut groups {
            let has = |state: State| {
                g.indices
                    .iter()
                    .any(|&i| self.queue.items[i].state == state)
            };
            g.state = if let Some(i) = g.active {
                if self.is_verifying(i) {
                    "Verifying"
                } else {
                    "Downloading"
                }
            } else if g
                .indices
                .iter()
                .all(|&i| self.queue.items[i].state == State::Quarantined)
            {
                "Quarantined"
            } else if g.complete as u64 == g.expected {
                "Complete"
            } else if has(State::Failed) {
                "Failed"
            } else if has(State::Paused) {
                "Paused"
            } else if has(State::Quarantined) {
                "Mixed"
            } else if g.indices.len() as u64 != g.expected {
                "Incomplete"
            } else {
                "Queued"
            };
        }
        groups
    }
    pub fn bundle_cursor(&self, groups: &[BundleView]) -> usize {
        groups
            .iter()
            .position(|g| g.indices.contains(&self.cursor))
            .unwrap_or(0)
    }
    pub fn select_bundle(&mut self, index: usize) {
        if let Some(g) = self.bundles().get(index) {
            self.cursor = g.indices[0];
        }
    }
    pub fn move_bundle(&mut self, down: bool) {
        let groups = self.bundles();
        let i = self.bundle_cursor(&groups);
        self.select_bundle(if down {
            (i + 1).min(groups.len().saturating_sub(1))
        } else {
            i.saturating_sub(1)
        });
    }
    pub fn resume_bundle(&mut self) -> io::Result<()> {
        self.start_bundle(false)
    }
    pub fn retry_fresh(&mut self) -> io::Result<()> {
        self.start_bundle(true)
    }
    fn start_bundle(&mut self, fresh: bool) -> io::Result<()> {
        if self.busy() {
            return Err(io::Error::other("Pause active transfer first"));
        }
        let groups = self.bundles();
        let g = groups
            .get(self.bundle_cursor(&groups))
            .ok_or_else(|| io::Error::other("Select a model"))?;
        if g.indices.len() as u64 != g.expected {
            return Err(io::Error::other(
                "Missing shards; select the complete variant in Discover",
            ));
        }
        if g.indices
            .iter()
            .any(|&i| self.queue.items[i].state == State::Quarantined)
        {
            return Err(io::Error::other("Restore quarantined model first"));
        }
        if fresh
            && !g
                .indices
                .iter()
                .any(|&i| self.queue.items[i].state == State::Failed)
        {
            return Err(io::Error::other("Choose a failed model to retry fresh"));
        }
        let pending: std::collections::VecDeque<_> = g
            .indices
            .iter()
            .copied()
            .filter(|&i| self.queue.items[i].state != State::Complete)
            .map(|i| (i, fresh && self.queue.items[i].state == State::Failed))
            .collect();
        if pending.is_empty() {
            return Err(io::Error::other("Model already complete"));
        }
        self.resume_parts = pending;
        self.run_queue = false;
        self.start_next_part()
    }
    fn start_next_part(&mut self) -> io::Result<()> {
        let Some((i, fresh)) = self.resume_parts.pop_front() else {
            return Ok(());
        };
        self.cursor = i;
        if let Err(e) = self.start_mode(fresh) {
            self.resume_parts.clear();
            return Err(e);
        }
        Ok(())
    }
    pub fn remove_bundle(&mut self) -> io::Result<()> {
        if self.busy() {
            return Err(io::Error::other("Pause active transfer first"));
        }
        let groups = self.bundles();
        let g = groups
            .get(self.bundle_cursor(&groups))
            .ok_or_else(|| io::Error::other("Select a model"))?;
        if g.indices
            .iter()
            .any(|&i| self.queue.items[i].receipt.is_some())
        {
            return Err(io::Error::other(
                "Keep ownership receipts; quarantine the model instead",
            ));
        }
        let old = self.queue.items.clone();
        self.queue.items = old
            .iter()
            .enumerate()
            .filter(|(i, _)| !g.indices.contains(i))
            .map(|(_, item)| item.clone())
            .collect();
        if let Err(e) = self.queue.save(&self.path) {
            self.queue.items = old;
            return Err(e);
        }
        self.cursor = self.cursor.min(self.queue.items.len().saturating_sub(1));
        self.status = "Model queue removed; files retained".into();
        Ok(())
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
    pub fn known_bytes(&self, index: usize) -> Option<u64> {
        let item = self.queue.items.get(index)?;
        self.progress(index).or({
            if matches!(item.state, State::Complete | State::Quarantined) {
                Some(item.file.size)
            } else {
                item.last_bytes
            }
        })
    }
    pub fn speed_samples(&self, index: usize) -> Vec<u64> {
        if self.active.as_ref().is_none_or(|(i, _, _)| *i != index) {
            return Vec::new();
        }
        self.rate.history.iter().map(|v| *v as u64).collect()
    }
    pub fn speed_history(&self, index: usize) -> Option<(String, f64)> {
        if self.active.as_ref().is_none_or(|(i, _, _)| *i != index) || self.rate.history.is_empty()
        {
            return None;
        }
        let peak = self.rate.history.iter().copied().fold(0.0, f64::max);
        let bars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        let graph = self
            .rate
            .history
            .iter()
            .rev()
            .take(28)
            .rev()
            .map(|v| {
                if peak == 0.0 {
                    '▁'
                } else {
                    bars[((v / peak) * 7.0).round().clamp(0.0, 7.0) as usize]
                }
            })
            .collect();
        Some((graph, peak))
    }
    pub fn is_verifying(&self, index: usize) -> bool {
        self.progress(index)
            .zip(self.queue.items.get(index))
            .is_some_and(|(n, item)| n >= item.file.size)
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
        let root_identity = RootIdentity::from_metadata(&open_root(&root, None)?.metadata()?);
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
                    root_identity: Some(root_identity.clone()),
                    state: State::Queued,
                    error: None,
                    last_bytes: None,
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
    /// User explicitly added a new bundle. Do not resume failed/paused files,
    /// and do not auto-start an old queue merely because the application opened.
    pub fn enqueue_and_start(&mut self, files: Vec<File>, root: PathBuf) -> io::Result<()> {
        let old = self.queue.items.len();
        self.add(files, root)?;
        if self.queue.items.len() == old {
            self.status = "Already queued; select resume if paused or failed".into();
            return Ok(());
        }
        if let Some((_, job, _)) = &self.active {
            if !job.is_cancelled() && self.resume_parts.is_empty() {
                self.run_queue = true;
            }
            return Ok(());
        }
        if !self.busy() {
            self.start_queue()?;
        }
        Ok(())
    }
    pub fn start(&mut self) -> io::Result<()> {
        self.start_mode(false)
    }
    fn start_mode(&mut self, fresh: bool) -> io::Result<()> {
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
        let job = Job::start(move |cancel| {
            if fresh {
                preserve_failed_partial(&item, &cancel)?;
                p.store(0, Ordering::Relaxed);
            }
            transfer_with_retry(&item, &cancel, &p)
        })?;
        self.queue.items[index].error = None;
        self.rate = Rate::default();
        self.active = Some((index, job, progress));
        self.status = if fresh {
            "Preserving failed partial, then retrying…"
        } else {
            "Downloading…"
        }
        .into();
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
        self.resume_parts.clear();
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
        if let Some((index, job, progress)) = self.active.take() {
            self.queue.items[index].state = match job.finish_result() {
                Ok(receipt) => {
                    self.queue.items[index].receipt = receipt;
                    State::Complete
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => State::Paused,
                Err(e) => {
                    self.queue.items[index].error = Some(e.to_string());
                    State::Failed
                }
            };
            let bytes = progress.load(Ordering::Relaxed);
            if bytes != u64::MAX {
                self.queue.items[index].last_bytes = Some(bytes);
            }
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
            if let Some((index, job, progress)) = self.active.take() {
                job.finish();
                let bytes = progress.load(Ordering::Relaxed);
                if bytes != u64::MAX {
                    self.queue.items[index].last_bytes = Some(bytes);
                }
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
                self.queue.items[index].error = (state == State::Failed).then(|| status.clone());
                self.queue.items[index].state = state;
                self.status = status;
                if self.queue.save(&self.path).is_err() {
                    self.status.push_str("; queue save failed");
                    self.run_queue = false;
                    self.resume_parts.clear();
                }
                if completed && !self.resume_parts.is_empty() {
                    if let Err(e) = self.start_next_part() {
                        self.status = e.to_string();
                    }
                } else if self.run_queue && completed {
                    if self.start_queue().is_err() {
                        self.run_queue = false;
                        self.status = "Queue complete; rescan Library".into();
                    }
                } else {
                    self.run_queue = false;
                    self.resume_parts.clear();
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
            root_identity: None,
            state: State::Queued,
            error: None,
            last_bytes: None,
            receipt: None,
        }
    }
    #[test]
    fn multipart_view_and_resume_all_missing_parts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let mut q = Controller::new(root.join("queue.toml")).unwrap();
        let mut a = fixture(&root);
        a.file.name = "Q4-00001-of-00002.gguf".into();
        let mut b = a.clone();
        b.file.name = "Q4-00002-of-00002.gguf".into();
        let mut other = a.clone();
        other.file.name = "Q8-00001-of-00002.gguf".into();
        for item in [&a, &b] {
            let dest = destination(item).unwrap();
            fs::write(
                dest.with_file_name(format!("{}.part", item.file.name)),
                b"abcdef",
            )
            .unwrap();
        }
        q.add(vec![a.file, b.file, other.file], root).unwrap();
        q.queue.items[0].state = State::Failed;
        q.queue.items[1].state = State::Paused;
        let views = q.bundles();
        assert_eq!(views.len(), 2);
        assert_eq!(views[0].bytes, 12);
        assert_eq!(views[0].state, "Failed");
        q.resume_bundle().unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        while q.busy() {
            q.tick();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        }
        let views = q.bundles();
        assert_eq!(views[0].state, "Complete");
        assert_eq!(views[0].received, 12);
        assert_eq!(views[1].state, "Incomplete");
        assert_eq!(q.queue.items[2].state, State::Queued);
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
    fn resumes_206_and_preserves_partial_when_range_is_ignored_or_rejected() {
        const FULL: &str =
            "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabcdef";
        const REST: &str = "HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nConnection: close\r\n\r\ndef";
        const RANGE: &str = "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nContent-Range: bytes */6\r\nConnection: close\r\n\r\n";
        for (response, ok) in [(REST, true), (FULL, false), (RANGE, false)] {
            let root = tempfile::tempdir().unwrap();
            let item = fixture(root.path());
            let dest = destination(&item).unwrap();
            let partial = dest.with_file_name("model.gguf.part");
            fs::write(&partial, b"abc").unwrap();
            let (url, worker) = server(vec![("range: bytes=3-", response)]);
            let result = transfer_from(
                &item,
                &AtomicBool::new(false),
                &AtomicU64::new(0),
                url,
                |_| Ok(100),
            );
            worker.join().unwrap();
            assert_eq!(result.is_ok(), ok);
            if ok {
                assert_eq!(fs::read(&dest).unwrap(), b"abcdef");
                assert!(!partial.exists());
            } else {
                assert_eq!(fs::read(&partial).unwrap(), b"abc");
                assert!(!dest.exists());
            }
        }
    }
    #[test]
    fn explicit_retry_archives_failed_partial_without_touching_completed_file() {
        let root = tempfile::tempdir().unwrap();
        let mut item = fixture(root.path());
        item.state = State::Failed;
        let dest = destination(&item).unwrap();
        let partial = dest.with_file_name("model.gguf.part");
        fs::write(&partial, b"wrong!").unwrap();
        preserve_failed_partial(&item, &AtomicBool::new(false)).unwrap();
        assert!(!partial.exists());
        let archives: Vec<_> = fs::read_dir(dest.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("model.gguf.part.saved-")
            })
            .collect();
        assert_eq!(archives.len(), 1);
        assert_eq!(fs::read(archives[0].path()).unwrap(), b"wrong!");
        fs::write(&dest, b"abcdef").unwrap();
        fs::write(&partial, b"wrong!").unwrap();
        assert!(preserve_failed_partial(&item, &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(dest).unwrap(), b"abcdef");
        assert_eq!(fs::read(partial).unwrap(), b"wrong!");
    }
    #[test]
    fn auto_queue_starts_only_queued_entries_and_keeps_failed_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let mut q = Controller::new(root.join("queue.toml")).unwrap();
        let mut old = fixture(&root).file;
        old.name = "old.gguf".into();
        q.add(vec![old], root.clone()).unwrap();
        q.queue.items[0].state = State::Failed;
        let item = fixture(&root);
        let dest = destination(&item).unwrap();
        fs::write(dest.with_file_name("model.gguf.part"), b"abcdef").unwrap();
        q.enqueue_and_start(vec![item.file], root).unwrap();
        assert!(q.busy());
        assert_eq!(q.cursor, 1);
        let until = Instant::now() + Duration::from_secs(3);
        while q.busy() {
            q.tick();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(q.queue.items[0].state, State::Failed);
        assert_eq!(q.queue.items[1].state, State::Complete);
        assert_eq!(q.known_bytes(1), Some(6));
    }
    #[test]
    fn full_invalid_partial_is_preserved_on_repeated_resume() {
        let root = tempfile::tempdir().unwrap();
        let item = fixture(root.path());
        let dest = destination(&item).unwrap();
        let partial = dest.with_file_name("model.gguf.part");
        fs::write(&partial, b"xxxxxx").unwrap();
        for _ in 0..2 {
            let err = transfer_from(
                &item,
                &AtomicBool::new(false),
                &AtomicU64::new(0),
                url::Url::parse("http://127.0.0.1:1/").unwrap(),
                |_| Ok(100),
            )
            .unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidData);
            assert!(err.to_string().contains("partial retained"));
            assert_eq!(fs::read(&partial).unwrap(), b"xxxxxx");
            assert!(!dest.exists());
        }
        fs::write(&partial, b"abcdef").unwrap();
        assert!(transfer_from(
            &item,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
            url::Url::parse("http://127.0.0.1:1/").unwrap(),
            |_| Ok(100)
        )
        .unwrap()
        .is_some());
        assert_eq!(fs::read(dest).unwrap(), b"abcdef");
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
    fn transfer_lock_releases_even_with_inherited_description() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(temp.path())
            .unwrap();
        let locked = LockedPartial::new(file).unwrap();
        let inherited = locked.try_clone().unwrap();
        let other = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(temp.path())
            .unwrap();
        assert!(FileExt::try_lock_exclusive(&other).is_err());
        drop(locked);
        // The duplicate remains alive but the transfer has released its lock.
        FileExt::try_lock_exclusive(&other).unwrap();
        FileExt::unlock(&other).unwrap();
        drop(inherited);
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
        // Explicitly unlock: a concurrent fork in another test may briefly retain the fd.
        FileExt::unlock(&lock).unwrap();
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
            root_identity: None,
            state: State::Queued,
            error: None,
            last_bytes: None,
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
    fn destination_planning_is_read_only_and_checks_space() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let file = fixture(&root).file;
        let choice = DestinationChoice::new(
            vec![file.clone()],
            vec![root.clone(), root.join("missing")],
            Some(&root),
        )
        .unwrap();
        assert_eq!(choice.required, file.size);
        assert!(choice.available[0].is_some());
        assert_eq!(choice.available[1], None);
        assert!(check_destination(&root, u64::MAX).is_err());
        assert!(check_destination(&root, 0).is_ok());
        assert_eq!(fs::read_dir(root).unwrap().count(), 0);
    }
    #[test]
    fn queued_root_replacement_is_rejected_after_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("models");
        let moved = tmp.path().join("original");
        fs::create_dir(&root).unwrap();
        let path = tmp.path().join("queue.toml");
        let mut q = Controller::new(path.clone()).unwrap();
        q.add(vec![fixture(&root).file], root.clone()).unwrap();
        drop(q);
        fs::rename(&root, &moved).unwrap();
        let outside = tmp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, &root).unwrap();
        let q = Controller::new(path).unwrap();
        let item = &q.queue.items[0];
        assert!(Destination::open(item).is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        fs::remove_file(&root).unwrap();
        fs::create_dir(&root).unwrap();
        assert!(Destination::open(item).is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir(&root).unwrap();
        fs::rename(&moved, &root).unwrap();
        assert!(Destination::open(item).is_ok());
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
