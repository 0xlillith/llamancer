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
        .add(vec![file], temp.path().to_path_buf())
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
    let partial = model.with_file_name(format!(
        "{}.part",
        model.file_name().unwrap().to_string_lossy()
    ));
    std::fs::rename(model, &partial).unwrap();
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(&partial)
        .unwrap();
    f.set_len(scan.candidates[0].bytes / 2).unwrap();
    drop(f);
    downloads.queue.items[0].state = State::Paused;
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
    assert!(model.exists() && !partial.exists());
}
