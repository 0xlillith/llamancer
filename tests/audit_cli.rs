//! Offline CLI integrity acceptance using a disposable queue and model file.
use llamancer::{
    downloads::{Controller, Queue, State},
    huggingface::File,
};
use sha2::{Digest, Sha256};
use std::{fs, process::Command};
#[test]
fn verify_cli_failure_revalidation_and_exclusive_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("models");
    fs::create_dir(&root).unwrap();
    let cfg = tmp.path().join("config/llamancer");
    fs::create_dir_all(&cfg).unwrap();
    let queue = cfg.join("downloads.toml");
    let f = File {
        repo: "owner/model".into(),
        revision: "a".repeat(40),
        name: "test.gguf".into(),
        size: 6,
        sha256: Some(format!("{:x}", Sha256::digest(b"abcdef"))),
    };
    let path = root.join(&f.repo).join(&f.revision).join(&f.name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"badbad").unwrap();
    let mut d = Controller::new(queue.clone()).unwrap();
    d.add(vec![f], root).unwrap();
    d.queue.items[0].state = State::Complete;
    d.queue.save(&queue).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_llamancer"))
            .arg("verify")
            .env("XDG_CONFIG_HOME", cfg.parent().unwrap())
            .output()
            .unwrap()
    };
    let lock = llamancer::diagnostics::SessionLock::acquire(&cfg).unwrap();
    assert!(!run().status.success());
    drop(lock);
    let out = run();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("1 failed"));
    let q = Queue::load(&queue).unwrap();
    assert!(q.items[0].integrity_error.is_some());
    assert_eq!(fs::read(&path).unwrap(), b"badbad");
    fs::write(&path, b"abcdef").unwrap();
    let out = run();
    assert!(out.status.success(), "{:?}", out);
    assert!(String::from_utf8_lossy(&out.stdout).contains("0 failed"));
    assert!(Queue::load(&queue).unwrap().items[0]
        .integrity_error
        .is_none());
    assert_eq!(fs::read(&path).unwrap(), b"abcdef");
}
