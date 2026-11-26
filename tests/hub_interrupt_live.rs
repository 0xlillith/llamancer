//! Real pinned 532 MB HTTP Hub transfer: pause an active child, reload queue,
//! resume its cache, and reject a complete but incorrect staged file.
use llamancer::{
    downloads::{Controller, State, Transport},
    huggingface::File,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
fn wait(q: &mut Controller, seconds: u64) {
    let until = Instant::now() + Duration::from_secs(seconds);
    while q.busy() {
        q.tick();
        if Instant::now() > until {
            q.shutdown().unwrap();
            panic!("Timeout: {}", q.status);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn partial_bytes(path: &Path) -> u64 {
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| {
            let p = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                partial_bytes(&p)
            } else if p.extension().is_some_and(|v| v == "incomplete") {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            } else {
                0
            }
        })
        .sum()
}
#[test]
#[ignore = "requires hf CLI and a fresh isolated HF_HUB_CACHE with HF_HUB_DISABLE_XET=1"]
fn interrupted_hub_transfer_and_full_size_mismatch() {
    let cache = PathBuf::from(std::env::var_os("HF_HUB_CACHE").unwrap());
    assert!(cache.is_absolute() && !cache.exists());
    assert_eq!(std::env::var("HF_HUB_DISABLE_XET").as_deref(), Ok("1"));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("models");
    fs::create_dir(&root).unwrap();
    let file = File {
        repo: "unsloth/Qwen3.5-0.8B-GGUF".into(),
        revision: "6ab461498e2023f6e3c1baea90a8f0fe38ab64d0".into(),
        name: "Qwen3.5-0.8B-Q4_K_M.gguf".into(),
        size: 532517120,
        sha256: Some("bd258782e35f7f458f8aced1adc053e6e92e89bc735ba3be89d38a06121dc517".into()),
    };
    let dest = root.join(&file.repo).join(&file.revision).join(&file.name);
    let part = dest.with_file_name(format!("{}.part", file.name));
    let queue = tmp.path().join("queue.toml");
    let mut q = Controller::new(queue.clone()).unwrap();
    q.transport = Transport::Hub;
    q.add(vec![file.clone()], root.clone()).unwrap();
    q.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut observed = 0;
    while q.busy() {
        q.tick();
        observed = partial_bytes(&cache);
        if observed >= 1024 * 1024 && observed < file.size {
            q.pause();
            break;
        }
        if Instant::now() > deadline {
            q.shutdown().unwrap();
            panic!("No observable active HTTP cache: {}", q.status);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    wait(&mut q, 30);
    assert_eq!(q.queue.items[0].state, State::Paused, "{}", q.status);
    let retained = partial_bytes(&cache);
    assert!(observed > 0 && retained >= observed && retained < file.size && !dest.exists());
    drop(q);
    let mut q = Controller::new(queue).unwrap();
    q.transport = Transport::Hub;
    q.start().unwrap();
    wait(&mut q, 240);
    assert_eq!(q.queue.items[0].state, State::Complete, "{}", q.status);
    assert!(q.queue.items[0].receipt.is_some() && dest.exists() && !part.exists());
    q.verify_all().unwrap();
    wait(&mut q, 90);
    assert_eq!(q.last_audit.unwrap().failed, 0);
    println!(
        "HUB_HTTP_RESUME_OK retained_bytes={retained} bytes={}",
        file.size
    );
    // A sparse, full-size incorrect staging file is deliberately retained.
    let badroot = tmp.path().join("bad");
    fs::create_dir(&badroot).unwrap();
    let bad = badroot
        .join(&file.repo)
        .join(&file.revision)
        .join(&file.name);
    fs::create_dir_all(bad.parent().unwrap()).unwrap();
    let badpart = bad.with_file_name(format!("{}.part", file.name));
    fs::File::create(&badpart)
        .unwrap()
        .set_len(file.size)
        .unwrap();
    let mut q = Controller::new(tmp.path().join("bad.toml")).unwrap();
    q.transport = Transport::Hub;
    q.add(vec![file.clone()], badroot).unwrap();
    q.start().unwrap();
    wait(&mut q, 90);
    assert_eq!(q.queue.items[0].state, State::Failed, "{}", q.status);
    assert!(
        q.queue.items[0].receipt.is_none()
            && !bad.exists()
            && fs::metadata(&badpart).unwrap().len() == file.size
    );
    assert!(
        q.queue.items[0]
            .error
            .as_deref()
            .is_some_and(|s| s.contains("SHA-256")
                || s.contains("Checksum")
                || s.contains("checksum")),
        "{}",
        q.status
    );
    println!("FULL_SIZE_MISMATCH_RETAINED bytes={}", file.size);
}
