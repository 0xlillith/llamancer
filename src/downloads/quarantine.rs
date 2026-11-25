//! Recoverable removal of verified, Llamancer-published GGUF files only.
//! Quarantine is a same-directory rename, never unlink or permanent deletion.
use super::{cancelled, same_inode, verify_file, Destination, Item, Receipt, State};
use std::{
    ffi::CString,
    fs::{self, File, OpenOptions},
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

fn quarantine_name(name: &str) -> String {
    format!("{name}.llamancer-quarantine")
}
pub(super) fn expected(item: &Item) -> io::Result<PathBuf> {
    crate::huggingface::validate(&item.file)?;
    Ok(item
        .root
        .join(&item.file.repo)
        .join(&item.file.revision)
        .join(&item.file.name))
}
fn path_state(path: &Path, receipt: &Receipt) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(m) if receipt.matches(&m) => Ok(true),
        Ok(_) => Err(io::Error::other("Managed file identity changed")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}
pub(super) fn reconcile(item: &mut Item) {
    let Some(receipt) = &item.receipt else { return };
    let Ok(path) = expected(item) else { return };
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return;
    };
    let trash = path.with_file_name(quarantine_name(name));
    // An unavailable volume or conflicting path is not evidence of a completed move.
    match (path_state(&path, receipt), path_state(&trash, receipt)) {
        (Ok(true), Ok(false)) => item.state = State::Complete,
        (Ok(false), Ok(true)) => item.state = State::Quarantined,
        _ => {}
    }
}
fn idle_endpoint(endpoint: &str) -> io::Result<Vec<TcpListener>> {
    let url = crate::llama::client::endpoint(endpoint)?;
    let port = url.port_or_known_default().unwrap_or(80);
    if url.scheme() != "http" {
        return Err(io::Error::other(
            "Quarantine requires a local HTTP endpoint",
        ));
    }
    let addresses = match url.host_str() {
        Some("127.0.0.1") => vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)],
        Some("[::1]") => vec![SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port)],
        Some("localhost") => vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port),
        ],
        _ => {
            return Err(io::Error::other(
                "Quarantine requires a local endpoint and stopped inference",
            ))
        }
    };
    if addresses.is_empty() {
        return Err(io::Error::other("No local endpoint addresses"));
    }
    addresses
        .into_iter()
        .map(|a| {
            TcpListener::bind(a).map_err(|_| {
                io::Error::other(
                    "Stop all servers using the configured endpoint before moving files",
                )
            })
        })
        .collect()
}
struct BreakSignal {
    flag: Arc<AtomicBool>,
    id: signal_hook::SigId,
}
impl BreakSignal {
    fn new() -> io::Result<Self> {
        let flag = Arc::new(AtomicBool::new(false));
        let id = signal_hook::flag::register(libc::SIGIO, flag.clone())?;
        Ok(Self { flag, id })
    }
}
impl Drop for BreakSignal {
    fn drop(&mut self) {
        signal_hook::low_level::unregister(self.id);
    }
}
struct Lease(File);
impl Lease {
    fn acquire(file: File) -> io::Result<Self> {
        // SAFETY: fcntl operates on a live owned descriptor; the command takes an int.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLEASE, libc::F_WRLCK) } < 0 {
            return Err(io::Error::other("Cannot obtain exclusive Linux file lease; file is in use or filesystem does not support leases"));
        }
        Ok(Self(file))
    }
    fn intact(&self) -> bool {
        // SAFETY: the descriptor is live, F_GETLEASE takes no additional argument.
        (unsafe { libc::fcntl(self.0.as_raw_fd(), libc::F_GETLEASE) }) == libc::F_WRLCK
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        // SAFETY: release the lease before closing its descriptor.
        unsafe { libc::fcntl(self.0.as_raw_fd(), libc::F_SETLEASE, libc::F_UNLCK) };
    }
}
struct Move {
    destination: Destination,
    source: String,
    target: String,
    lease: Lease,
}
fn rename_no_replace(directory: &File, source: &str, target: &str) -> io::Result<()> {
    let source = CString::new(source.as_bytes()).map_err(io::Error::other)?;
    let target = CString::new(target.as_bytes()).map_err(io::Error::other)?;
    let fd = directory.as_raw_fd();
    // SAFETY: directory descriptor is live and both names are valid C strings.
    if unsafe {
        libc::renameat2(
            fd,
            source.as_ptr(),
            fd,
            target.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
impl Move {
    fn check(&self, signal: &BreakSignal) -> io::Result<()> {
        self.destination.check_visible()?;
        if signal.flag.load(Ordering::Acquire) || !self.lease.intact() {
            return Err(io::Error::other(
                "Another process attempted to open a managed file",
            ));
        }
        let source = Destination::fd_path(&self.destination.parent).join(&self.source);
        let meta = fs::symlink_metadata(source)?;
        if !meta.is_file() || !same_inode(&meta, &self.lease.0.metadata()?) {
            return Err(io::Error::other("Managed pathname was replaced"));
        }
        Ok(())
    }
    fn apply(&self) -> io::Result<()> {
        rename_no_replace(&self.destination.parent, &self.source, &self.target)?;
        self.destination.parent.sync_all()
    }
}
pub(super) fn move_items(
    items: Vec<Item>,
    restore: bool,
    endpoint: &str,
    cancel: &AtomicBool,
) -> io::Result<()> {
    let _listeners = idle_endpoint(endpoint)?;
    let signal = BreakSignal::new()?;
    let mut prepared = Vec::new();
    for item in items {
        cancelled(cancel)?;
        let receipt = item
            .receipt
            .as_ref()
            .ok_or_else(|| io::Error::other("Missing ownership receipt"))?;
        if item.file.sha256.is_none() {
            return Err(io::Error::other("Missing authoritative checksum"));
        }
        let destination = Destination::open(&item)?;
        let dir = Destination::fd_path(&destination.parent);
        let original = destination.leaf.clone();
        let trash = quarantine_name(&original);
        let (source, target) = if restore {
            (trash, original)
        } else {
            (original, trash)
        };
        let source_exists = path_state(&dir.join(&source), receipt)?;
        let target_exists = path_state(&dir.join(&target), receipt)?;
        match (source_exists, target_exists) {
            (false, true) => continue, // Interrupted multi-shard operations are resumable.
            (true, false) => {}
            _ => return Err(io::Error::other("Missing or conflicting managed paths")),
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(dir.join(&source))?;
        let before = file.metadata()?;
        if !receipt.matches(&before) {
            return Err(io::Error::other("Managed file identity changed"));
        }
        // Hash before leasing so other readers are not blocked by a long checksum pass.
        verify_file(&mut file, &item, cancel)?;
        prepared.push((destination, source, target, file, before, receipt.clone()));
    }
    let mut moves = Vec::new();
    for (destination, source, target, file, before, receipt) in prepared {
        cancelled(cancel)?;
        let lease = Lease::acquire(file)?;
        let after = lease.0.metadata()?;
        if !receipt.matches(&after)
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
        {
            return Err(io::Error::other("Managed file changed during verification"));
        }
        moves.push(Move {
            destination,
            source,
            target,
            lease,
        });
    }
    for entry in &moves {
        entry.check(&signal)?;
    }
    // A failure/crash may leave some shards in quarantine. Every move is a
    // no-replace rename and the original receipt survives; reconcile on restart.
    for entry in &moves {
        cancelled(cancel)?;
        entry.check(&signal)?;
        entry.apply()?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::huggingface::File as HubFile;
    use sha2::{Digest, Sha256};
    fn fixture(root: &Path) -> (Item, PathBuf) {
        let mut item = Item {
            file: HubFile {
                repo: "owner/model".into(),
                revision: "a".repeat(40),
                name: "model.gguf".into(),
                size: 6,
                sha256: Some(format!("{:x}", Sha256::digest(b"abcdef"))),
            },
            root: root.into(),
            state: State::Complete,
            receipt: None,
        };
        let path = super::super::destination(&item).unwrap();
        fs::write(&path, b"abcdef").unwrap();
        item.receipt = Some(Receipt::from_metadata(&fs::metadata(&path).unwrap()));
        (item, path)
    }
    fn endpoint() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    }
    #[test]
    fn refuses_unowned_open_and_conflicting_files() {
        let root = tempfile::tempdir().unwrap();
        let (item, path) = fixture(root.path());
        let mut unowned = item.clone();
        unowned.receipt = None;
        assert!(move_items(vec![unowned], false, &endpoint(), &AtomicBool::new(false)).is_err());
        let open = File::open(&path).unwrap();
        assert!(move_items(
            vec![item.clone()],
            false,
            &endpoint(),
            &AtomicBool::new(false)
        )
        .is_err());
        drop(open);
        let trash = path.with_file_name(quarantine_name("model.gguf"));
        fs::write(&trash, b"external").unwrap();
        assert!(move_items(vec![item], false, &endpoint(), &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(path).unwrap(), b"abcdef");
        assert_eq!(fs::read(trash).unwrap(), b"external");
    }
    #[test]
    fn occupied_endpoint_and_modified_content_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let (item, path) = fixture(root.path());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        assert!(move_items(
            vec![item.clone()],
            false,
            &format!("http://{}", listener.local_addr().unwrap()),
            &AtomicBool::new(false)
        )
        .is_err());
        drop(listener);
        fs::write(&path, b"change").unwrap();
        assert!(move_items(vec![item], false, &endpoint(), &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(path).unwrap(), b"change");
    }
    #[test]
    fn quarantine_and_restore_preserve_inode_and_recover_after_restart() {
        let root = tempfile::tempdir().unwrap();
        let (mut item, path) = fixture(root.path());
        let trash = path.with_file_name(quarantine_name("model.gguf"));
        let cancel = AtomicBool::new(false);
        move_items(vec![item.clone()], false, &endpoint(), &cancel).unwrap();
        assert!(!path.exists());
        assert_eq!(fs::read(&trash).unwrap(), b"abcdef");
        // The queue may still say Complete after a crash; loading reconciles it.
        let queue_path = root.path().join("queue.toml");
        super::super::Queue {
            items: vec![item.clone()],
        }
        .save(&queue_path)
        .unwrap();
        item = super::super::Queue::load(&queue_path)
            .unwrap()
            .items
            .remove(0);
        assert_eq!(item.state, State::Quarantined);
        move_items(vec![item.clone()], true, &endpoint(), &cancel).unwrap();
        reconcile(&mut item);
        assert_eq!(item.state, State::Complete);
        assert!(!trash.exists());
        assert!(item.receipt.unwrap().matches(&fs::metadata(path).unwrap()));
    }
}
