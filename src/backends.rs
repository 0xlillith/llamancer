//! Versioned, checksum-pinned official llama.cpp distributions. No sudo or PATH edits.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{symlink, DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Duration,
};
use url::Url;

pub const RELEASE: &str = "b12721";
const MAX_EXPANDED: u64 = 8 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Cpu,
    Vulkan,
    Rocm,
}
impl Kind {
    pub const ALL: [Self; 3] = [Self::Cpu, Self::Vulkan, Self::Rocm];
    pub fn name(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Vulkan => "vulkan",
            Self::Rocm => "rocm",
        }
    }
    pub fn parse(name: &str) -> io::Result<Self> {
        Self::ALL
            .into_iter()
            .find(|k| k.name() == name)
            .ok_or_else(|| io::Error::other("Choose cpu, vulkan or rocm"))
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Asset {
    pub kind: Kind,
    pub suffix: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}
impl Asset {
    pub fn filename(self) -> String {
        format!("llama-{RELEASE}-bin-ubuntu-{}.tar.gz", self.suffix)
    }
    pub fn url(self) -> String {
        format!(
            "https://github.com/ggml-org/llama.cpp/releases/download/{RELEASE}/{}",
            self.filename()
        )
    }
    pub fn directory(self, root: &Path) -> PathBuf {
        root.join(format!("{RELEASE}-{}", self.suffix))
    }
    pub fn binary(self, root: &Path) -> PathBuf {
        self.directory(root)
            .join(format!("llama-{RELEASE}/llama-server"))
    }
    pub fn installed(self, root: &Path) -> bool {
        let dir = self.directory(root);
        fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir())
            && fs::symlink_metadata(self.binary(root))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            && fs::read_to_string(dir.join("sha256")).is_ok_and(|s| s.trim() == self.sha256)
    }
}
pub fn catalog(kind: Kind) -> Option<Asset> {
    let (suffix, size, sha256) = match (std::env::consts::ARCH, kind) {
        ("x86_64", Kind::Cpu) => (
            "x64",
            23388595,
            "ab56524ce899526e51c04e77f089a5792c59282b49736b239900970d3947be47",
        ),
        ("x86_64", Kind::Vulkan) => (
            "vulkan-x64",
            37095907,
            "ba2a338af1aa1d82d74d23f7e50e19ca5cb4da064d54bd86f6c01c80f77e7bda",
        ),
        ("x86_64", Kind::Rocm) => (
            "rocm-10.0-x64",
            277131895,
            "bfe159abce6a526ec6b5cc8ba44a9e91600d3c7632b78c3d48733efe224937bb",
        ),
        ("aarch64", Kind::Cpu) => (
            "arm64",
            18944130,
            "2c615cee75536574a30002ead85a3a259eac2ed3519d5e22546e61a5a9430c40",
        ),
        ("aarch64", Kind::Vulkan) => (
            "vulkan-arm64",
            30074713,
            "d5c0356952fa73344ed2baab6a7a2aceff4a4f23005341e053eb7d5d222b553e",
        ),
        _ => return None,
    };
    Some(Asset {
        kind,
        suffix,
        size,
        sha256,
    })
}
pub fn root() -> io::Result<PathBuf> {
    let absolute = |key: &str| {
        std::env::var_os(key)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    absolute("XDG_DATA_HOME")
        .or_else(|| absolute("HOME").map(|p| p.join(".local/share")))
        .map(|p| p.join("llamancer/backends"))
        .ok_or_else(|| io::Error::other("No XDG data directory"))
}
fn cancel(flag: &AtomicBool) -> io::Result<()> {
    if flag.load(Ordering::Acquire) {
        Err(io::ErrorKind::Interrupted.into())
    } else {
        Ok(())
    }
}
fn safe_origin(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some(
                "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
}
fn download(
    asset: Asset,
    out: &mut File,
    flag: &AtomicBool,
    progress: &AtomicU64,
) -> io::Result<()> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(15))
        .build();
    let mut url = Url::parse(&asset.url()).map_err(io::Error::other)?;
    let mut response = None;
    for _ in 0..5 {
        cancel(flag)?;
        if !safe_origin(&url) {
            return Err(io::Error::other("Untrusted release redirect"));
        }
        let res = agent
            .get(url.as_str())
            .set("Accept-Encoding", "identity")
            .call()
            .map_err(|_| io::Error::other("Official release download failed"))?;
        if matches!(res.status(), 301 | 302 | 303 | 307 | 308) {
            let next = res
                .header("Location")
                .ok_or_else(|| io::Error::other("Missing release redirect"))?;
            url = url.join(next).map_err(io::Error::other)?;
        } else {
            response = Some(res);
            break;
        }
    }
    let response = response.ok_or_else(|| io::Error::other("Too many release redirects"))?;
    if response.status() != 200 {
        return Err(io::Error::other("Unexpected release HTTP status"));
    }
    if response
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
        .is_some_and(|n| n != asset.size)
    {
        return Err(io::Error::other("Release size mismatch"));
    }
    let mut reader = response.into_reader();
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buf = [0u8; 65536];
    loop {
        cancel(flag)?;
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > asset.size {
            return Err(io::Error::other("Release exceeds pinned size"));
        }
        hash.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        progress.store(count, Ordering::Release);
    }
    if count != asset.size || format!("{:x}", hash.finalize()) != asset.sha256 {
        return Err(io::Error::other("Release checksum or size mismatch"));
    }
    out.sync_all()
}
fn normal(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))
}
fn unpack(reader: impl Read, dir: &Path, flag: &AtomicBool) -> io::Result<()> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(reader));
    let mut seen = BTreeSet::new();
    let mut links = Vec::new();
    let mut total = 0u64;
    let prefix = format!("llama-{RELEASE}");
    for entry in archive.entries()? {
        cancel(flag)?;
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        if !normal(&path)
            || !path.starts_with(&prefix)
            || !seen.insert(path.clone())
            || seen.len() > MAX_ENTRIES
        {
            return Err(io::Error::other("Invalid or duplicate archive path"));
        }
        let target = dir.join(&path);
        let ty = entry.header().entry_type();
        if ty.is_dir() {
            fs::create_dir_all(&target)?;
        } else if ty.is_file() {
            total = total
                .checked_add(entry.size())
                .filter(|n| *n <= MAX_EXPANDED)
                .ok_or_else(|| io::Error::other("Expanded archive limit exceeded"))?;
            fs::create_dir_all(
                target
                    .parent()
                    .ok_or_else(|| io::Error::other("Missing parent"))?,
            )?;
            let mode = if entry.header().mode()? & 0o111 != 0 {
                0o700
            } else {
                0o600
            };
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(mode)
                .open(&target)?;
            let mut buf = [0u8; 65536];
            loop {
                cancel(flag)?;
                let n = entry.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                file.write_all(&buf[..n])?;
            }
            file.sync_all()?;
        } else if ty.is_symlink() {
            let link = entry
                .link_name()?
                .ok_or_else(|| io::Error::other("Missing link target"))?
                .into_owned();
            if !normal(&link) || link.components().count() != 1 {
                return Err(io::Error::other("Unsafe archive symlink"));
            }
            links.push((target, link));
        } else {
            return Err(io::Error::other("Unsupported archive entry type"));
        }
    }
    // No symlinks are created until all data has been extracted. Links may only
    // refer to a sibling; check chains resolve to actual regular files in staging.
    for (target, link) in &links {
        symlink(link, target)?;
    }
    let canonical = dir.canonicalize()?;
    for (target, _) in &links {
        let real = target.canonicalize()?;
        if !real.starts_with(&canonical) || !real.is_file() {
            return Err(io::Error::other("Unresolved archive symlink"));
        }
    }
    Ok(())
}
fn private_root(root: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root)?;
    let m = fs::symlink_metadata(root)?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o022 != 0 {
        return Err(io::Error::other("Unsafe backend storage directory"));
    }
    Ok(())
}
pub fn install(
    root: &Path,
    asset: Asset,
    flag: &AtomicBool,
    progress: &AtomicU64,
) -> io::Result<PathBuf> {
    private_root(root)?;
    let _lock = crate::diagnostics::SessionLock::acquire(root)?;
    let dest = asset.directory(root);
    if asset.installed(root) {
        return Ok(asset.binary(root));
    }
    if fs::symlink_metadata(&dest).is_ok() {
        return Err(io::Error::other(
            "Backend destination already exists; refusing replacement",
        ));
    }
    if fs2::available_space(root)? < asset.size + MAX_EXPANDED {
        return Err(io::Error::other(
            "Need archive size plus 8 GiB free for staging",
        ));
    }
    let staging = tempfile::tempdir_in(root)?;
    let mut archive = tempfile::tempfile_in(root)?;
    download(asset, &mut archive, flag, progress)?;
    use std::io::{Seek, SeekFrom};
    archive.seek(SeekFrom::Start(0))?;
    unpack(archive, staging.path(), flag)?;
    let binary = staging.path().join(format!("llama-{RELEASE}/llama-server"));
    if !fs::symlink_metadata(&binary)?.is_file() {
        return Err(io::Error::other("Missing regular llama-server"));
    }
    cancel(flag)?;
    let version = crate::llama::process::command_output(
        binary
            .to_str()
            .ok_or_else(|| io::Error::other("Non-UTF8 backend path"))?,
        &["--version"],
    )?;
    if !version.contains(RELEASE.trim_start_matches('b')) {
        return Err(io::Error::other("Unexpected backend version"));
    }
    fs::write(staging.path().join("sha256"), asset.sha256)?;
    cancel(flag)?;
    fs::rename(staging.path(), &dest)?;
    File::open(root)?.sync_all()?;
    Ok(asset.binary(root))
}
pub fn probe(binary: &Path) -> io::Result<String> {
    let _lock = execution_lock(binary)?;
    crate::llama::process::command_output(
        binary
            .to_str()
            .ok_or_else(|| io::Error::other("Non-UTF8 backend path"))?,
        &["--list-devices"],
    )
}
/// Hold a shared storage lock during execution of a managed backend. Removal
/// takes the same lock exclusively. External binaries are not governed by it.
pub fn execution_lock(binary: &Path) -> io::Result<Option<File>> {
    let Ok(root) = root() else { return Ok(None) };
    let Ok(canonical_root) = root.canonicalize() else {
        return Ok(None);
    };
    let lexical = if binary.is_absolute() {
        binary.to_owned()
    } else {
        std::env::current_dir()?.join(binary)
    };
    let canonical = binary.canonicalize().unwrap_or(lexical.clone());
    if !lexical.starts_with(&root) && !canonical.starts_with(&canonical_root) {
        return Ok(None);
    }
    private_root(&root)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(root.join("tui.lock"))?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.nlink() != 1 || meta.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::other("Unsafe backend lock"));
    }
    fs2::FileExt::try_lock_shared(&file)
        .map_err(|_| io::Error::other("Backend storage is being changed"))?;
    if !binary.is_file() {
        return Err(io::Error::other("Managed backend no longer installed"));
    }
    Ok(Some(file))
}
/// The installed archive is not necessarily a usable GPU backend.
pub fn detected_device(kind: Kind, output: &str) -> io::Result<String> {
    if kind == Kind::Cpu {
        return Ok("none".into());
    }
    let prefix = if kind == Kind::Vulkan {
        "Vulkan"
    } else {
        "ROCm"
    };
    let devices: Vec<_> = output
        .lines()
        .filter_map(|line| {
            let id = line.trim().split(':').next()?;
            let index = id.strip_prefix(prefix)?;
            (!index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()))
                .then_some(id.to_owned())
        })
        .collect();
    devices.into_iter().next().ok_or_else(||io::Error::other(if kind==Kind::Rocm{
        "ROCm installed but no device detected. Check compatible HIP, hipBLAS and rocBLAS runtimes."
    }else{"Vulkan installed but no device detected. Check Vulkan loader and GPU driver."}))
}
/// A bounded benchmark of a deliberately small local model. Never fetch a model.
pub fn benchmark(root: &Path, asset: Asset, model: &Path, flag: &AtomicBool) -> io::Result<String> {
    use std::{
        io::{Seek, SeekFrom},
        os::unix::process::CommandExt,
        process::{Command, Stdio},
        time::Instant,
    };
    if !asset.installed(root) {
        return Err(io::Error::other("Install backend first"));
    }
    let model = model.canonicalize()?;
    if !model.is_file() || model.metadata()?.len() > 512 * 1024 * 1024 {
        return Err(io::Error::other(
            "Use a complete local GGUF up to 512 MiB for this quick benchmark",
        ));
    }
    let output = probe(&asset.binary(root))?;
    let device = detected_device(asset.kind, &output)?;
    let bench = asset.binary(root).with_file_name("llama-bench");
    let _lock = execution_lock(&bench)?;
    let mut file = tempfile::tempfile()?;
    let mut child = Command::new(bench)
        .process_group(0)
        .arg("--offline")
        .arg("-m")
        .arg(&model)
        .args([
            "-p",
            "128",
            "-n",
            "32",
            "-r",
            "2",
            "-t",
            "2",
            "-ngl",
            if asset.kind == Kind::Cpu { "0" } else { "8" },
            "-dev",
            &device,
            "-o",
            "json",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::from(file.try_clone()?))
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if flag.load(Ordering::Acquire) || Instant::now() >= deadline {
            // The benchmark is a newly spawned, unreaped process group.
            unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
            child.wait()?;
            return Err(io::Error::other("Benchmark cancelled or timed out"));
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if !status.success() {
        return Err(io::Error::other(
            "Benchmark failed; check model and backend",
        ));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    file.take(1024 * 1024).read_to_string(&mut text)?;
    let results: Vec<serde_json::Value> = serde_json::from_str(&text).map_err(io::Error::other)?;
    if results.len() != 2 {
        return Err(io::Error::other("Unexpected benchmark output"));
    }
    let mut lines = vec![format!(
        "{} | {} | 2 threads, 2 repeats",
        asset.kind.name(),
        device
    )];
    for row in results {
        let speed = row["avg_ts"]
            .as_f64()
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or_else(|| io::Error::other("Missing benchmark throughput"))?;
        lines.push(format!(
            "pp {} / tg {}: {:.2} tok/s",
            row["n_prompt"], row["n_gen"], speed
        ));
    }
    Ok(lines.join("\n"))
}
/// Remove only the catalog-owned version directory, never an external executable.
/// Runtime and service selection are separate; callers must stop inference first.
pub fn remove(root: &Path, asset: Asset, selected: &str) -> io::Result<()> {
    private_root(root)?;
    let _lock = crate::diagnostics::SessionLock::acquire(root)?;
    if !asset.installed(root) {
        return Err(io::Error::other("No verified installation to remove"));
    }
    let dir = asset.directory(root).canonicalize()?;
    if Path::new(selected)
        .canonicalize()
        .is_ok_and(|p| p.starts_with(&dir))
    {
        return Err(io::Error::other(
            "Select another backend or external executable first",
        ));
    }
    // Refuse any visible process executing the backend, including router workers.
    for process in fs::read_dir("/proc")?.flatten() {
        if !process
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        if fs::read_link(process.path().join("exe")).is_ok_and(|p| p.starts_with(&dir)) {
            return Err(io::Error::other(
                "Backend is in use; stop all inference first",
            ));
        }
    }
    // A rename makes the retired version unreachable before deletion. This is
    // intended for private managed installs, not arbitrary externally used paths.
    let retired = tempfile::tempdir_in(root)?;
    let target = retired.path().join("retired");
    fs::rename(&dir, &target)?;
    fs::remove_dir_all(target)?;
    File::open(root)?.sync_all()
}
pub fn hardware() -> String {
    let Ok(cards) = fs::read_dir("/sys/class/drm") else {
        return "GPU information unavailable".into();
    };
    let mut devices = Vec::new();
    for card in cards.flatten() {
        let path = card.path().join("device");
        let vendor = fs::read_to_string(path.join("vendor")).unwrap_or_default();
        if let Ok(bytes) = fs::read_to_string(path.join("mem_info_vram_total")) {
            if let Ok(n) = bytes.trim().parse::<u64>() {
                devices.push(format!(
                    "{} {} MiB",
                    if vendor.trim() == "0x1002" {
                        "AMD"
                    } else {
                        "GPU"
                    },
                    n / 1048576
                ));
            }
        }
    }
    devices.sort();
    devices.dedup();
    if devices.is_empty() {
        "GPU detection unavailable; probe installed backend".into()
    } else {
        devices.join(" | ")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requires_real_gpu_device() {
        assert_eq!(
            detected_device(Kind::Cpu, "Available devices:\n  (none)").unwrap(),
            "none"
        );
        assert_eq!(
            detected_device(Kind::Vulkan, "  Vulkan0: Radeon").unwrap(),
            "Vulkan0"
        );
        assert!(detected_device(Kind::Rocm, "Available devices:\n  (none)").is_err());
        assert!(detected_device(Kind::Vulkan, "  ROCm0: Radeon").is_err());
    }
    #[test]
    fn catalog_has_pinned_https_assets() {
        for kind in Kind::ALL {
            if let Some(a) = catalog(kind) {
                assert_eq!(a.sha256.len(), 64);
                assert!(safe_origin(&Url::parse(&a.url()).unwrap()));
                assert!(a.size > 0);
            }
        }
        assert!(!safe_origin(
            &Url::parse("https://github.com.evil.invalid/a").unwrap()
        ));
        assert!(!normal(Path::new("../bad")));
        assert!(!normal(Path::new("/tmp/bad")));
    }
    #[test]
    fn safe_extract_and_reject_special_entries() {
        fn fixture(ty: tar::EntryType, link: Option<&str>) -> Vec<u8> {
            let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
                Vec::new(),
                flate2::Compression::fast(),
            ));
            let mut h = tar::Header::new_gnu();
            h.set_mode(0o755);
            h.set_size(0);
            h.set_entry_type(ty);
            if let Some(link) = link {
                h.set_link_name(link).unwrap();
            }
            h.set_cksum();
            tar.append_data(&mut h, format!("llama-{RELEASE}/file"), io::empty())
                .unwrap();
            tar.into_inner().unwrap().finish().unwrap()
        }
        for (ty, link, ok) in [
            (tar::EntryType::Regular, None, true),
            (tar::EntryType::Symlink, Some("../outside"), false),
            (tar::EntryType::Fifo, None, false),
        ] {
            let root = tempfile::tempdir().unwrap();
            assert_eq!(
                unpack(
                    fixture(ty, link).as_slice(),
                    root.path(),
                    &AtomicBool::new(false)
                )
                .is_ok(),
                ok
            );
        }
    }
}
