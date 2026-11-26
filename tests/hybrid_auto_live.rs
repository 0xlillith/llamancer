//! Opt-in actual hybrid automatic context and inference check.
use llamancer::{config::Config, llama::session::Controller, models};
use std::{
    net::TcpListener,
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
#[test]
#[ignore = "requires a verified small hybrid GGUF and installed backend"]
fn hybrid_auto_context_inference() {
    let binary = std::env::var("LLAMANCER_TEST_SERVER").unwrap();
    let path = PathBuf::from(std::env::var_os("LLAMANCER_TEST_HYBRID").unwrap());
    let groups = models::group::group(
        models::scan(vec![path], &AtomicBool::new(false))
            .unwrap()
            .candidates,
    );
    assert_eq!(groups.len(), 1);
    let g = &groups[0];
    assert!(g.complete && g.bytes() < 1024_u64.pow(3));
    assert!(models::estimate::estimate(g, &Default::default()).is_none());
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config = Config {
        server_binary: binary,
        device: std::env::var("LLAMANCER_TEST_DEVICE").unwrap_or_else(|_| "none".into()),
        endpoint: format!("http://127.0.0.1:{port}"),
        ..Default::default()
    };
    let mut c = Controller::default();
    c.start(&config, Vec::new(), Some(g.clone())).unwrap();
    let deadline = Instant::now() + Duration::from_secs(70);
    loop {
        c.tick(&config);
        if c.snapshot.as_ref().is_some_and(|s| !s.models.is_empty()) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{}\n{}",
            c.status,
            c.logs.join("\n")
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(c.active_context, Some(131072));
    let snapshot = c.snapshot.as_ref().unwrap();
    let model = &snapshot.models[0];
    let properties = snapshot.properties.get(&model.id).unwrap();
    assert_eq!(
        properties.context.or_else(|| model.context_limit()),
        Some(131072)
    );
    let output:serde_json::Value=ureq::post(&format!("{}/completion",config.endpoint)).timeout(Duration::from_secs(30)).send_json(serde_json::json!({"prompt":"The capital of France is","n_predict":8,"temperature":0.0})).unwrap().into_json().unwrap();
    assert!(output["content"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(config.profiles.is_empty());
    println!(
        "HYBRID_AUTO backend={} device={} context={} output_tokens={}",
        config.server_binary,
        config.device,
        c.active_context.unwrap(),
        output["tokens_predicted"]
    );
    c.shutdown().unwrap();
    assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
}
