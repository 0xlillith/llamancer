//! Opt-in backend acceptance using the production controller and an explicit GGUF fixture.
use llamancer::{
    config::Config,
    llama::{client::Client, process, router, session::Controller},
    models::{self, estimate, library, profile::Profile},
};
use std::{
    collections::BTreeMap,
    net::TcpListener,
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

fn wait(controller: &mut Controller, config: &Config, condition: impl Fn(&Controller) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        controller.tick(config);
        if condition(controller) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Backend timeout: {}\n{}",
            controller.status,
            controller.logs.join("\n")
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

// AMD amdgpu exposes global VRAM usage without elevated privileges. This is an
// observation, not a per-process measurement or proof of offload.
fn vram() -> BTreeMap<String, u64> {
    let Ok(cards) = std::fs::read_dir("/sys/class/drm") else {
        return BTreeMap::new();
    };
    cards
        .filter_map(Result::ok)
        .filter_map(|card| {
            let name = card.file_name().into_string().ok()?;
            if !name
                .strip_prefix("card")?
                .bytes()
                .all(|b| b.is_ascii_digit())
            {
                return None;
            }
            let used =
                std::fs::read_to_string(card.path().join("device/mem_info_vram_used")).ok()?;
            Some((name, used.trim().parse().ok()?))
        })
        .collect()
}

#[test]
#[ignore = "requires explicitly selected backend, device and real GGUF"]
fn desktop_backend() {
    let binary = std::env::var("LLAMANCER_TEST_SERVER").expect("Set LLAMANCER_TEST_SERVER");
    let path =
        PathBuf::from(std::env::var_os("LLAMANCER_TEST_MODEL").expect("Set LLAMANCER_TEST_MODEL"))
            .canonicalize()
            .unwrap();
    let device = std::env::var("LLAMANCER_TEST_DEVICE")
        .expect("Set LLAMANCER_TEST_DEVICE to an exact ID from --list-devices, or none for CPU");
    let layers: u32 = std::env::var("LLAMANCER_TEST_GPU_LAYERS")
        .expect("Set LLAMANCER_TEST_GPU_LAYERS explicitly")
        .parse()
        .unwrap();
    assert!(
        (device == "none" && layers == 0) || (device != "none" && device != "auto" && layers > 0)
    );
    let context: u32 = std::env::var("LLAMANCER_TEST_CONTEXT")
        .unwrap_or_else(|_| "2048".into())
        .parse()
        .unwrap();
    assert!(
        (128..=32768).contains(&context),
        "Use a bounded initial context"
    );
    let scan = models::scan(vec![path.clone()], &AtomicBool::new(false)).unwrap();
    let group = models::group::group(scan.candidates)
        .into_iter()
        .find(|g| g.files.iter().any(|f| f.path == path))
        .expect("GGUF fixture indexed");
    assert!(group.complete && !group.projector);
    assert!(
        group
            .files
            .iter()
            .all(|f| f.metadata.payload_bounds_checked),
        "Unverified tensor type in fixture"
    );
    let mut profile = Profile::default();
    profile.engine.context_size = Some(context);
    profile.engine.gpu_layers = Some(layers);
    profile.engine.threads = Some(2);
    let estimate = estimate::estimate(&group, &profile);
    let mut key = tempfile::NamedTempFile::new().unwrap();
    use std::io::Write;
    writeln!(key, "desktop-test-only-token").unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut config = Config {
        server_binary: binary,
        device,
        endpoint: format!("http://127.0.0.1:{port}"),
        api_key_file: Some(key.path().into()),
        ..Config::default()
    };
    config.profiles.insert(library::identity(&group), profile);
    let version = process::command_output(&config.server_binary, &["--version"]).unwrap();
    let devices = process::command_output(&config.server_binary, &["--list-devices"]).unwrap();
    eprintln!("Backend version: {version}\nDevices: {devices}");
    eprintln!("Requested device: {} | layers: {layers} | context: {context} | architecture: {:?} | estimate: {estimate:?}",config.device,group.files[0].metadata.architecture);
    if config.device != "none" {
        assert!(
            devices.contains(&config.device),
            "Requested device not advertised"
        );
    }
    for use_router in [false, true] {
        let baseline = vram();
        let mut controller = Controller::default();
        let start = Instant::now();
        controller
            .start(
                &config,
                vec![group.clone()],
                (!use_router).then(|| group.clone()),
            )
            .unwrap();
        wait(&mut controller, &config, |c| {
            !c.busy()
                && c.snapshot
                    .as_ref()
                    .is_some_and(|s| s.router == use_router && !s.models.is_empty())
        });
        assert!(
            Client::new(&config.endpoint).unwrap().snapshot().is_err(),
            "Unauthenticated control unexpectedly succeeded"
        );
        if use_router {
            controller.select_id(&router::id(&group)).unwrap();
            controller.set_loaded(&config, true).unwrap();
            wait(&mut controller, &config, |c| {
                !c.busy() && c.selected().is_some_and(|m| m.state() == "loaded")
            });
        }
        let model = controller.selected().unwrap().id.clone();
        let loaded = vram();
        let load_ms = start.elapsed().as_millis();
        let response: serde_json::Value = ureq::post(&format!("{}/completion",config.endpoint))
            .set("Authorization","Bearer desktop-test-only-token")
            .timeout(Duration::from_secs(90))
            .send_json(serde_json::json!({"model":model,"prompt":"Once upon a time","n_predict":32,"temperature":0.0,"seed":42}))
            .unwrap().into_json().unwrap();
        assert!(response["content"].as_str().is_some_and(|s| !s.is_empty()));
        eprintln!(
            "DESKTOP_EVIDENCE {}",
            serde_json::json!({
                "router":use_router,"device":config.device,"gpu_layers_requested":layers,
                "context":context,"load_ms":load_ms,"timings":response["timings"],
                "vram_before_bytes":baseline,"vram_loaded_bytes":loaded,"vram_after_generation_bytes":vram(),
                "estimate_weights_bytes":estimate.map(|e|e.weights),"estimate_kv_bytes":estimate.map(|e|e.kv)
            })
        );
        if use_router {
            controller.set_loaded(&config, false).unwrap();
            wait(&mut controller, &config, |c| {
                !c.busy() && c.selected().is_some_and(|m| m.state() == "unloaded")
            });
        }
        controller.shutdown().unwrap();
        assert!(TcpListener::bind(config.endpoint.strip_prefix("http://").unwrap()).is_ok());
        eprintln!("VRAM after shutdown: {:?}", vram());
    }
}
