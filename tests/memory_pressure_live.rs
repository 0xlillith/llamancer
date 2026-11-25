//! Opt-in memory allocation failure/recovery without exhausting host RAM or GPU VRAM.
use llamancer::{
    config::Config,
    llama::{client::Client, session::Controller},
    models::{self, library, profile::Profile},
};
use std::{
    fs,
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
fn wait(c: &mut Controller, cfg: &Config, condition: impl Fn(&Controller) -> bool) {
    let until = Instant::now() + Duration::from_secs(95);
    loop {
        c.tick(cfg);
        if condition(c) {
            return;
        }
        assert!(
            Instant::now() < until,
            "{}\n{}",
            c.status,
            c.logs.join("\n")
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
#[test]
#[ignore = "requires prlimit, real CPU backend and a local >2 GiB GGUF fixture"]
fn bounded_memory_failure_cancel_and_recover() {
    let binary = std::env::var("LLAMANCER_TEST_SERVER").expect("Set CPU server");
    let model = PathBuf::from(std::env::var_os("LLAMANCER_TEST_MODEL").expect("Set model"))
        .canonicalize()
        .unwrap();
    let scan = models::scan(vec![model.clone()], &AtomicBool::new(false)).unwrap();
    let group = models::group::group(scan.candidates)
        .into_iter()
        .find(|g| g.files[0].path == model)
        .unwrap();
    assert!(group.complete && group.bytes() > 2 * 1024 * 1024 * 1024);
    let tmp = tempfile::tempdir().unwrap();
    // Probe the genuine executable normally, then restrict only the child
    // inference process's virtual address space to 1 GiB.
    let wrapper = tmp.path().join("limited-server");
    fs::write(&wrapper,"#!/bin/sh\ncase \"$1\" in --help|--version) exec \"$LLAMANCER_TEST_SERVER\" \"$@\";; esac\nexec /usr/bin/prlimit --as=1073741824 -- \"$LLAMANCER_TEST_SERVER\" \"$@\"\n").unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut config = Config {
        server_binary: wrapper.to_string_lossy().into_owned(),
        device: "none".into(),
        endpoint: format!("http://127.0.0.1:{port}"),
        ..Config::default()
    };
    let mut profile = Profile::default();
    profile.engine.context_size = Some(2048);
    profile.engine.gpu_layers = Some(0);
    profile.engine.threads = Some(2);
    config.profiles.insert(library::identity(&group), profile);
    let mut c = Controller::default();
    c.start(&config, vec![group.clone()], Some(group.clone()))
        .unwrap();
    wait(&mut c, &config, |c| {
        !c.busy() && !c.owns_server() && c.status.contains("exited")
    });
    assert!(c.snapshot.is_none());
    let log = c.logs.join("\n");
    eprintln!("LIMITED_EXIT {}\n{}", c.status, log);
    assert!(
        log.to_ascii_lowercase().contains("memory")
            || log.to_ascii_lowercase().contains("mmap")
            || log.to_ascii_lowercase().contains("alloc"),
        "Expected actual memory allocation failure"
    );
    c.shutdown().unwrap();
    assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    config.server_binary = binary;
    // Cancel while probing/starting, and verify no owned listener remains.
    c.start(&config, vec![group.clone()], Some(group.clone()))
        .unwrap();
    c.shutdown().unwrap();
    assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    c.start(&config, vec![group.clone()], Some(group)).unwrap();
    wait(&mut c, &config, |c| !c.busy() && c.snapshot.is_some());
    assert!(!Client::new(&config.endpoint)
        .unwrap()
        .snapshot()
        .unwrap()
        .models
        .is_empty());
    let response: serde_json::Value = ureq::post(&format!("{}/completion", config.endpoint))
        .timeout(Duration::from_secs(45))
        .send_json(serde_json::json!({"prompt":"Once upon a time","n_predict":8,"temperature":0.0}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(response["content"].as_str().is_some_and(|s| !s.is_empty()));
    c.shutdown().unwrap();
    assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    println!("MEMORY_EVIDENCE bounded 1 GiB address-space failure, startup cancellation, port cleanup and real CPU inference recovery passed");
}
