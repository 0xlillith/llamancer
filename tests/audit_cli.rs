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

#[test]
fn verify_exact_target_audits_all_shards_without_touching_other_models() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("models");
    fs::create_dir(&root).unwrap();
    let cfg = tmp.path().join("config/llamancer");
    fs::create_dir_all(&cfg).unwrap();
    let queue = cfg.join("downloads.toml");
    let files: Vec<_> = [
        "test-00001-of-00002.gguf",
        "test-00002-of-00002.gguf",
        "other.gguf",
    ]
    .into_iter()
    .map(|name| File {
        repo: "owner/model".into(),
        revision: "a".repeat(40),
        name: name.into(),
        size: 6,
        sha256: Some(format!("{:x}", Sha256::digest(b"abcdef"))),
    })
    .collect();
    let paths: Vec<_> = files
        .iter()
        .map(|f| root.join(&f.repo).join(&f.revision).join(&f.name))
        .collect();
    fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
    for path in &paths {
        fs::write(path, b"badbad").unwrap();
    }
    let mut d = Controller::new(queue.clone()).unwrap();
    d.add(files, root).unwrap();
    for item in &mut d.queue.items {
        item.state = State::Complete;
    }
    d.queue.save(&queue).unwrap();
    let run = |path: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_llamancer"))
            .args(["verify", "--strict", "--model"])
            .arg(path)
            .env("XDG_CONFIG_HOME", cfg.parent().unwrap())
            .output()
            .unwrap()
    };
    assert!(!run(std::path::Path::new("other.gguf")).status.success());
    assert!(!run(&tmp.path().join("unknown.gguf")).status.success());
    let out = run(&paths[1]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("2 checked, 2 failed, 0 skipped"),
        "{:?}",
        out
    );
    let q = Queue::load(&queue).unwrap();
    assert!(q.items[0].integrity_error.is_some());
    assert!(q.items[1].integrity_error.is_some());
    assert!(q.items[2].integrity_error.is_none());
    for path in &paths {
        assert_eq!(fs::read(path).unwrap(), b"badbad");
    }
}

#[test]
fn recover_refuses_bad_bundle_without_publishing_verified_sibling() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("models");
    fs::create_dir(&root).unwrap();
    let cfg = tmp.path().join("config/llamancer");
    fs::create_dir_all(&cfg).unwrap();
    let queue = cfg.join("downloads.toml");
    let good = vec![17u8; 4096];
    let files: Vec<_> = (1..=2)
        .map(|i| File {
            repo: "owner/model".into(),
            revision: "a".repeat(40),
            name: format!("test-{i:05}-of-00002.gguf"),
            size: good.len() as u64,
            sha256: Some(format!("{:x}", Sha256::digest(&good))),
        })
        .collect();
    let paths: Vec<_> = files
        .iter()
        .map(|f| root.join(&f.repo).join(&f.revision).join(&f.name))
        .collect();
    let parts: Vec<_> = paths
        .iter()
        .map(|p| p.with_file_name(format!("{}.part", p.file_name().unwrap().to_str().unwrap())))
        .collect();
    fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
    fs::write(&parts[0], &good).unwrap();
    fs::write(&parts[1], vec![19u8; 4096]).unwrap();
    let mut d = Controller::new(queue.clone()).unwrap();
    d.add(files, root).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_llamancer"))
            .args(["recover", "--model"])
            .arg(&paths[0])
            .env("XDG_CONFIG_HOME", cfg.parent().unwrap())
            .output()
            .unwrap()
    };
    let out = run();
    assert!(!out.status.success());
    assert!(paths.iter().all(|p| !p.exists()));
    assert_eq!(fs::read(&parts[0]).unwrap(), good);
    assert_eq!(fs::read(&parts[1]).unwrap(), vec![19u8; 4096]);
    fs::write(&parts[1], &good).unwrap();
    let mut saved: toml::Value = toml::from_str(&fs::read_to_string(&queue).unwrap()).unwrap();
    let items = saved["items"].as_array_mut().unwrap();
    for item in items {
        item["root_identity"]["device"] = toml::Value::String("0".into());
    }
    fs::write(&queue, toml::to_string(&saved).unwrap()).unwrap();
    let denied = run();
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("root was replaced"));
    assert!(paths.iter().all(|p| !p.exists()));
    let run_rebind = || {
        Command::new(env!("CARGO_BIN_EXE_llamancer"))
            .args(["recover", "--model"])
            .arg(&paths[0])
            .arg("--rebind-device")
            .env("XDG_CONFIG_HOME", cfg.parent().unwrap())
            .output()
            .unwrap()
    };
    let mut wrong = saved.clone();
    wrong["items"].as_array_mut().unwrap()[0]["root_identity"]["inode"] =
        toml::Value::String("0".into());
    fs::write(&queue, toml::to_string(&wrong).unwrap()).unwrap();
    assert!(!run_rebind().status.success());
    assert!(paths.iter().all(|p| !p.exists()));
    fs::write(&queue, toml::to_string(&saved).unwrap()).unwrap();
    let out = run_rebind();
    // Strict O_DIRECT is intentionally not available on every filesystem.
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("Invalid argument") || err.contains("not supported"),
            "{out:?}"
        );
        assert!(paths.iter().all(|p| !p.exists()));
        return;
    }
    assert!(String::from_utf8_lossy(&out.stdout).contains("2 staged parts"));
    let q = Queue::load(&queue).unwrap();
    assert!(q
        .items
        .iter()
        .all(|i| i.state == State::Complete && i.receipt.is_some() && i.integrity_error.is_none()));
    for p in &paths {
        assert_eq!(fs::read(p).unwrap(), good);
    }
    assert!(parts.iter().all(|p| !p.exists()));
}
