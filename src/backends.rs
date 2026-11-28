//! Versioned, checksum-pinned official llama.cpp distributions. No sudo or PATH edits.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{symlink, DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Duration,
};
use url::Url;

mod catalog;
pub mod discovery;
pub use catalog::{catalog, catalog_version, Asset, Kind, RELEASE, RELEASES};
const MAX_EXPANDED: u64 = 8 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 8192;

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
    archive: &catalog::Archive,
    release: &str,
    out: &mut File,
    flag: &AtomicBool,
    progress: &AtomicU64,
) -> io::Result<()> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(600))
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(15))
        .build();
    let mut url = Url::parse(&archive.url(release)).map_err(io::Error::other)?;
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
        .is_some_and(|n| n != archive.size)
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
        if count > archive.size {
            return Err(io::Error::other("Release exceeds pinned size"));
        }
        hash.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        progress.store(count, Ordering::Release);
    }
    if count != archive.size || format!("{:x}", hash.finalize()) != archive.sha256 {
        return Err(io::Error::other("Release checksum or size mismatch"));
    }
    out.sync_all()
}
fn normal(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))
}
fn unpack(reader: impl Read, dir: &Path, release: &str, flag: &AtomicBool) -> io::Result<()> {
    let prefix = format!("llama-{release}");
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(reader));
    let mut seen = BTreeSet::new();
    let mut links = Vec::new();
    let mut total = 0u64;
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
fn storage_lock(root: &Path, exclusive: bool) -> io::Result<File> {
    private_root(root)?;
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
    if exclusive {
        fs2::FileExt::try_lock_exclusive(&file)
    } else {
        fs2::FileExt::try_lock_shared(&file)
    }
    .map_err(|_| {
        io::Error::other(
            "Backend storage is in use; stop inference or wait for the current operation",
        )
    })?;
    Ok(file)
}
pub fn install(
    root: &Path,
    asset: Asset,
    flag: &AtomicBool,
    progress: &AtomicU64,
) -> io::Result<PathBuf> {
    if !asset.valid() {
        return Err(io::Error::other("Asset is not in the reviewed catalog"));
    }
    private_root(root)?;
    let _lock = storage_lock(root, true)?;
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
            "Insufficient free space for verified archive staging",
        ));
    }
    let staging = tempfile::tempdir_in(root)?;
    let mut archive = tempfile::tempfile_in(root)?;
    download(
        &asset.primary(),
        asset.release,
        &mut archive,
        flag,
        progress,
    )?;
    use std::io::{Seek, SeekFrom};
    archive.seek(SeekFrom::Start(0))?;
    unpack(archive, staging.path(), asset.release, flag)?;
    let binary = staging
        .path()
        .join(format!("llama-{}/llama-server", asset.release));
    if !fs::symlink_metadata(&binary)?.is_file() {
        return Err(io::Error::other("Missing regular llama-server"));
    }
    cancel(flag)?;
    let version = crate::llama::process::command_output_unlocked(
        binary
            .to_str()
            .ok_or_else(|| io::Error::other("Non-UTF8 backend path"))?,
        &["--version"],
    )?;
    if !version.contains(asset.release.trim_start_matches('b')) {
        return Err(io::Error::other("Unexpected backend version"));
    }
    fs::write(staging.path().join("sha256"), asset.sha256)?;
    cancel(flag)?;
    fs::rename(staging.path(), &dest)?;
    File::open(root)?.sync_all()?;
    Ok(asset.binary(root))
}
pub fn probe(binary: &Path) -> io::Result<String> {
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
    let file = storage_lock(&root, false)?;
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
    let prefix = match kind {
        Kind::Vulkan => "Vulkan",
        Kind::Rocm => "ROCm",
        Kind::Cpu => unreachable!(),
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
pub fn benchmark(
    root: &Path,
    asset: Asset,
    model: &Path,
    flag: &AtomicBool,
) -> io::Result<crate::benchmark::Record> {
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
    let mut model_file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&model)?;
    let before = model_file.metadata()?;
    if !before.is_file() || before.len() == 0 || before.len() > 512 * 1024 * 1024 {
        return Err(io::Error::other("Invalid benchmark model size"));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut count = 0u64;
    loop {
        cancel(flag)?;
        let n = model_file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > before.len() {
            return Err(io::Error::other("Benchmark model changed"));
        }
        hash.update(&buffer[..n]);
    }
    if count != before.len() {
        return Err(io::Error::other("Benchmark model changed"));
    }
    let model_sha256 = format!("{:x}", hash.finalize());
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
    let after = fs::metadata(&model)?;
    if (
        before.dev(),
        before.ino(),
        before.len(),
        before.mtime(),
        before.mtime_nsec(),
    ) != (
        after.dev(),
        after.ino(),
        after.len(),
        after.mtime(),
        after.mtime_nsec(),
    ) {
        return Err(io::Error::other("Benchmark model changed during run"));
    }
    let speed = |prompt: u64, generated: u64| -> io::Result<f64> {
        results
            .iter()
            .find(|row| {
                row["n_prompt"].as_u64() == Some(prompt) && row["n_gen"].as_u64() == Some(generated)
            })
            .and_then(|row| row["avg_ts"].as_f64())
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or_else(|| io::Error::other("Missing matching benchmark throughput"))
    };
    Ok(crate::benchmark::Record {
        kind: asset.kind.name().into(),
        release: asset.release.into(),
        device,
        model_sha256,
        model_bytes: before.len(),
        pp: speed(128, 0)?,
        tg: speed(0, 32)?,
        gpu_layers: if asset.kind == Kind::Cpu { 0 } else { 8 },
    })
}
/// Remove only the catalog-owned version directory, never an external executable.
/// Runtime and service selection are separate; callers must stop inference first.
pub fn remove(root: &Path, asset: Asset, selected: &str) -> io::Result<()> {
    if !asset.valid() {
        return Err(io::Error::other("Asset is not in the reviewed catalog"));
    }
    private_root(root)?;
    let _lock = storage_lock(root, true)?;
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
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn malformed_receipt_and_fabricated_catalog_entry_are_rejected() {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let root = tempfile::tempdir().unwrap();
        let asset = catalog(Kind::Cpu).unwrap();
        let binary = asset.binary(root.path());
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::write(&binary, b"fixture").unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
        let receipt = asset.directory(root.path()).join("sha256");
        let name = CString::new(receipt.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(!asset.installed(root.path()));
        let forged = Asset {
            suffix: "../outside",
            ..asset
        };
        assert!(install(
            root.path(),
            forged,
            &AtomicBool::new(false),
            &AtomicU64::new(0)
        )
        .is_err());
    }
    #[test]
    fn active_storage_readers_refuse_mutation() {
        let root = tempfile::tempdir().unwrap();
        let a = storage_lock(root.path(), false).unwrap();
        let b = storage_lock(root.path(), false).unwrap();
        assert!(storage_lock(root.path(), true).is_err());
        drop(a);
        drop(b);
        let writer = storage_lock(root.path(), true).unwrap();
        assert!(storage_lock(root.path(), false).is_err());
        drop(writer);
        assert!(storage_lock(root.path(), true).is_ok());
    }
    #[test]
    fn versions_are_distinct_and_only_pinned_tags_are_accepted() {
        let root = Path::new("/tmp/backend-fixture");
        let a = catalog_version(Kind::Cpu, RELEASES[0]).unwrap();
        let b = catalog_version(Kind::Cpu, RELEASES[1]).unwrap();
        assert_ne!(a.binary(root), b.binary(root));
        assert_ne!(a.sha256, b.sha256);
        assert!(b.url().contains("/b12726/llama-b12726-"));
        assert!(catalog_version(Kind::Cpu, "../bad").is_none());
    }
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
    fn managed_backend_choices_exclude_cuda() {
        assert!(Kind::parse("cuda").is_err());
        assert_eq!(Kind::ALL.map(Kind::name), ["cpu", "vulkan", "rocm"]);
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
                    RELEASE,
                    &AtomicBool::new(false)
                )
                .is_ok(),
                ok
            );
        }
    }
}
