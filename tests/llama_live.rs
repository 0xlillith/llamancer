//! Opt-in real CPU server lifecycle check with a caller-provided fixture.
use llamancer::{
    config::Config,
    llama::{client::Client, session::Controller},
    models,
};
use std::{
    net::TcpListener,
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
fn wait(c: &mut Controller, config: &Config, condition: impl Fn(&Controller) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        c.tick(config);
        if condition(c) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{}\n{}",
            c.status,
            c.logs.join("\n")
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
#[test]
#[ignore = "requires an installed llama-server and a tiny GGUF fixture"]
fn real_cpu_router_lifecycle() {
    let binary = std::env::var("LLAMANCER_TEST_SERVER").expect("Set LLAMANCER_TEST_SERVER");
    let root = PathBuf::from(
        std::env::var_os("LLAMANCER_TEST_MODELS").expect("Set LLAMANCER_TEST_MODELS"),
    );
    let groups = models::group::group(
        models::scan(vec![root], &AtomicBool::new(false))
            .unwrap()
            .candidates,
    );
    assert_eq!(groups.len(), 1);
    assert!(groups[0].bytes() < 10_000_000);
    let id = llamancer::llama::router::id(&groups[0]);
    let second_root = tempfile::tempdir().unwrap();
    std::fs::copy(
        &groups[0].files[0].path,
        second_root.path().join("second.gguf"),
    )
    .unwrap();
    let second = models::group::group(
        models::scan(vec![second_root.path().into()], &AtomicBool::new(false))
            .unwrap()
            .candidates,
    )
    .remove(0);
    let second_id = llamancer::llama::router::id(&second);
    let catalog = vec![groups[0].clone(), second];
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config = Config {
        server_binary: binary,
        endpoint: format!("http://127.0.0.1:{port}"),
        device: "none".into(),
        ..Config::default()
    };
    let mut key = tempfile::NamedTempFile::new().unwrap();
    use std::io::Write;
    writeln!(key, "llamancer-test-key").unwrap();
    let config = Config {
        api_key_file: Some(key.path().to_path_buf()),
        ..config
    };
    let mut controller = Controller::default();
    controller.start(&config, catalog.clone(), None).unwrap();
    wait(&mut controller, &config, |c| {
        !c.busy()
            && c.snapshot
                .as_ref()
                .is_some_and(|s| s.router && !s.models.is_empty())
    });
    assert!(Client::new(&config.endpoint).unwrap().snapshot().is_err());
    controller.select_id(&id).unwrap();
    controller.set_loaded(&config, true).unwrap();
    wait(&mut controller, &config, |c| {
        !c.busy() && c.selected().is_some_and(|m| m.state() == "loaded")
    });
    assert!(
        Client::from_config(&config)
            .unwrap()
            .snapshot()
            .unwrap()
            .router
    );
    let response:serde_json::Value=ureq::post(&format!("{}/completion",config.endpoint))
        .set("Authorization","Bearer llamancer-test-key")
        .timeout(Duration::from_secs(15))
        .send_json(serde_json::json!({"model":id,"prompt":"Once upon a time","n_predict":4,"temperature":0.0})).unwrap().into_json().unwrap();
    assert!(response["content"].as_str().is_some_and(|s| !s.is_empty()));
    let mut external = Controller::default();
    external.refresh(&config).unwrap();
    wait(&mut external, &config, |c| {
        !c.busy() && c.snapshot.is_some()
    });
    assert!(external.stop().is_err());
    assert!(external.restart(&config, catalog.clone()).is_err());
    assert!(Client::from_config(&config).unwrap().snapshot().is_ok());
    external.disconnect().unwrap();
    assert!(!external.polling());
    controller.set_loaded(&config, false).unwrap();
    wait(&mut controller, &config, |c| {
        !c.busy() && c.selected().is_some_and(|m| m.state() == "unloaded")
    });
    controller.select_id(&second_id).unwrap();
    controller.set_loaded(&config, true).unwrap();
    wait(&mut controller, &config, |c| {
        !c.busy()
            && c.selected()
                .is_some_and(|m| m.id == second_id && m.state() == "loaded")
    });
    controller.restart(&config, catalog.clone()).unwrap();
    wait(&mut controller, &config, |c| {
        !c.busy()
            && c.snapshot.as_ref().is_some_and(|s| {
                s.router && s.models.len() == 2 && s.models.iter().all(|m| m.state() == "unloaded")
            })
    });
    controller.stop().unwrap();
    assert!(!controller.owns_server()); // Reuse the same endpoint for a directly managed single model.
    controller
        .start(&config, Vec::new(), Some(groups[0].clone()))
        .unwrap();
    wait(&mut controller, &config, |c| {
        !c.busy()
            && c.snapshot
                .as_ref()
                .is_some_and(|s| !s.router && !s.models.is_empty())
    });
    let response: serde_json::Value = ureq::post(&format!("{}/completion", config.endpoint))
        .set("Authorization", "Bearer llamancer-test-key")
        .timeout(Duration::from_secs(15))
        .send_json(serde_json::json!({"prompt":"Once upon a time","n_predict":4,"temperature":0.0}))
        .unwrap()
        .into_json()
        .unwrap();
    assert!(response["content"].as_str().is_some_and(|s| !s.is_empty()));
    controller.restart(&config, Vec::new()).unwrap();
    wait(&mut controller, &config, |c| {
        !c.busy()
            && c.snapshot
                .as_ref()
                .is_some_and(|s| !s.router && !s.models.is_empty())
    });
    controller.shutdown().unwrap();
    // Verify the foreground service supervisor independently of systemd.
    let root = groups[0].files[0].path.parent().unwrap().to_path_buf();
    let config = Config {
        model_roots: vec![root],
        ..config
    };
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let token = stop.clone();
    let settings = config.clone();
    let supervisor = std::thread::spawn(move || llamancer::service::run(settings, token));
    let deadline = Instant::now() + Duration::from_secs(30);
    let client = Client::from_config(&config).unwrap();
    while client.snapshot().is_err() {
        if Instant::now() >= deadline {
            stop.store(true, std::sync::atomic::Ordering::Release);
            let _ = supervisor.join();
            panic!("Service readiness timeout");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    stop.store(true, std::sync::atomic::Ordering::Release);
    supervisor.join().unwrap().unwrap();
    assert!(TcpListener::bind(config.endpoint.strip_prefix("http://").unwrap()).is_ok());
}
