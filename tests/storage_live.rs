//! Opt-in real Hub acceptance with separate model roots and a large hybrid fixture.
use llamancer::{
    config::Config,
    downloads::{self, Controller, State},
    huggingface, models,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
fn wait(q: &mut Controller, limit: u64) {
    let until = Instant::now() + Duration::from_secs(limit);
    while q.busy() {
        q.tick();
        assert!(Instant::now() < until, "{}", q.status);
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn dest(root: &Path, f: &huggingface::File) -> PathBuf {
    root.join(&f.repo).join(&f.revision).join(&f.name)
}
#[test]
#[ignore = "downloads a bounded public hybrid GGUF; set LLAMANCER_TEST_STORAGE_DIR to a fresh isolated directory"]
fn two_roots_resume_and_index() {
    use std::os::unix::fs::PermissionsExt;
    let base = PathBuf::from(
        std::env::var_os("LLAMANCER_TEST_STORAGE_DIR").expect("Set fresh storage directory"),
    )
    .canonicalize()
    .unwrap();
    assert!(fs::read_dir(&base).unwrap().next().is_none());
    let primary = base.join("models");
    fs::create_dir(&primary).unwrap();
    let second = tempfile::tempdir_in("/tmp").unwrap();
    let secondary = second.path().canonicalize().unwrap();
    let missing = base.join("missing");
    assert!(downloads::destination_space(&missing).is_err());
    let read_only = base.join("read-only");
    fs::create_dir(&read_only).unwrap();
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o500)).unwrap();
    let rejected = downloads::destination_space(&read_only).is_err();
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(rejected, "run as regular user");
    assert!(downloads::check_destination(&primary, u64::MAX).is_err());
    let large = huggingface::files("unsloth/Qwen3.5-0.8B-GGUF")
        .unwrap()
        .into_iter()
        .find(|f| f.name == "Qwen3.5-0.8B-Q4_K_M.gguf")
        .unwrap();
    let tiny = huggingface::files("ggml-org/tiny-llamas")
        .unwrap()
        .into_iter()
        .find(|f| f.name == "stories260K.gguf")
        .unwrap();
    assert!(large.size > 400_000_000 && large.size < 650_000_000 && tiny.size < 2_000_000);
    assert!(large.sha256.is_some() && tiny.sha256.is_some());
    let choice = downloads::DestinationChoice::new(
        vec![large.clone()],
        vec![primary.clone(), secondary.clone()],
        Some(&primary),
    )
    .unwrap();
    assert_eq!(choice.cursor, 0);
    assert_eq!(choice.required, large.size);
    let queue = base.join("queue.toml");
    let mut q = Controller::new(queue.clone()).unwrap();
    q.add(vec![large.clone()], primary.clone()).unwrap();
    q.add(vec![tiny.clone()], secondary.clone()).unwrap();
    q.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(100);
    while q.busy() {
        q.tick();
        if q.progress(0)
            .is_some_and(|n| n >= 4 * 1024 * 1024 && n < large.size)
        {
            q.pause();
            break;
        }
        assert!(Instant::now() < deadline, "{}", q.status);
        std::thread::sleep(Duration::from_millis(10));
    }
    wait(&mut q, 30);
    assert_eq!(q.queue.items[0].state, State::Paused, "{}", q.status);
    let target = dest(&primary, &large);
    let partial = target.with_file_name(format!("{}.part", large.name));
    let prefix = fs::metadata(&partial).unwrap().len();
    assert!(prefix > 0 && prefix < large.size && !target.exists());
    // Changing the default and removing the original root from the current settings
    // must not move a previously queued partial.
    let config = Config {
        model_roots: vec![secondary.clone()],
        download_root: Some(secondary.clone()),
        ..Config::default()
    };
    config.save(&base.join("config.toml")).unwrap();
    drop(q);
    let mut q = Controller::new(queue).unwrap();
    assert_eq!(q.queue.items[0].root, primary);
    assert_eq!(q.queue.items[1].root, secondary);
    q.start().unwrap();
    wait(&mut q, 240);
    assert_eq!(q.queue.items[0].state, State::Complete, "{}", q.status);
    assert!(q.queue.items[0].receipt.is_some() && target.exists() && !partial.exists());
    q.cursor = 1;
    q.start().unwrap();
    wait(&mut q, 100);
    assert_eq!(q.queue.items[1].state, State::Complete, "{}", q.status);
    assert!(q.queue.items[1].receipt.is_some() && dest(&secondary, &tiny).exists());
    assert!(!dest(&secondary, &large).exists() && !dest(&primary, &tiny).exists());
    let scan = models::scan(
        vec![primary.clone(), secondary.clone()],
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(scan.candidates.len(), 2);
    let group = models::group::group(scan.candidates)
        .into_iter()
        .find(|g| g.files[0].path == target)
        .unwrap();
    assert!(group.complete);
    assert!(
        models::estimate::estimate(&group, &models::profile::Profile::default()).is_none(),
        "Hybrid estimate must remain unavailable"
    );
    println!(
        "STORAGE_EVIDENCE bytes={} paused_prefix={} tiny_bytes={} architecture={:?} model={}",
        large.size,
        prefix,
        tiny.size,
        group.files[0].metadata.architecture,
        target.display()
    );
}
