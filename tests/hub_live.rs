//! Opt-in network test of a small public GGUF, using an isolated temporary directory.
use llamancer::{
    downloads::{Controller, State},
    huggingface, models,
};
use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
#[test]
#[ignore = "requires access to Hugging Face"]
fn download_and_index_real_gguf() {
    let temp = tempfile::tempdir().unwrap();
    let files = huggingface::files("ggml-org/tiny-llamas").unwrap();
    let file = files
        .into_iter()
        .find(|f| f.name == "stories260K.gguf")
        .expect("Tiny test model available");
    assert!(file.size < 2_000_000);
    assert!(file.sha256.is_some());
    let mut downloads = Controller::new(temp.path().join("queue.toml")).unwrap();
    downloads
        .add(vec![file.clone()], temp.path().to_path_buf())
        .unwrap();
    downloads.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(100);
    while downloads.busy() {
        downloads.tick();
        assert!(Instant::now() < deadline, "Download timed out");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        downloads.queue.items[0].state,
        State::Complete,
        "{}",
        downloads.status
    );
    let scan = models::scan(vec![temp.path().to_path_buf()], &AtomicBool::new(false)).unwrap();
    assert_eq!(scan.candidates.len(), 1);
    assert_eq!(
        scan.candidates[0].metadata.architecture.as_deref(),
        Some("llama")
    );
    let model = &scan.candidates[0].path;
    assert!(downloads.queue.items[0].receipt.is_some());
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let endpoint = format!("http://127.0.0.1:{port}");
    downloads.quarantine(false, &endpoint).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while downloads.busy() {
        downloads.tick();
        assert!(Instant::now() < deadline, "Quarantine timed out");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        downloads.queue.items[0].state,
        State::Quarantined,
        "{}",
        downloads.status
    );
    assert!(!model.exists());
    let mut downloads = Controller::new(temp.path().join("queue.toml")).unwrap();
    assert_eq!(downloads.queue.items[0].state, State::Quarantined);
    downloads.quarantine(true, &endpoint).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while downloads.busy() {
        downloads.tick();
        assert!(Instant::now() < deadline, "Restore timed out");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        downloads.queue.items[0].state,
        State::Complete,
        "{}",
        downloads.status
    );
    assert!(model.exists());

    // Seed an interrupted transfer in a fresh isolated root, never mutate an
    // already published managed model to simulate a network failure.
    let resumed = tempfile::tempdir().unwrap();
    let dest = resumed
        .path()
        .join(&file.repo)
        .join(&file.revision)
        .join(&file.name);
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    let partial = dest.with_file_name(format!("{}.part", file.name));
    std::fs::copy(model, &partial).unwrap();
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(&partial)
        .unwrap();
    f.set_len(file.size / 2).unwrap();
    drop(f);
    let mut downloads = Controller::new(resumed.path().join("queue.toml")).unwrap();
    downloads.add(vec![file], resumed.path().into()).unwrap();
    downloads.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(100);
    while downloads.busy() {
        downloads.tick();
        assert!(Instant::now() < deadline, "Resume timed out");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        downloads.queue.items[0].state,
        State::Complete,
        "{}",
        downloads.status
    );
    assert!(downloads.queue.items[0].receipt.is_some());
    assert!(dest.exists() && !partial.exists());
}
