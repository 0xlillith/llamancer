//! Actual managed server flags and /props, using a pinned verified tiny reviewed model.
use llamancer::{
    config::Config,
    llama::session::Controller,
    models::{
        self,
        library::identity,
        profile::{Profile, Thinking},
    },
};
use std::{
    net::TcpListener,
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
#[test]
#[ignore = "requires pinned Qwen3.5-0.8B GGUF in managed layout and CPU server"]
fn reviewed_profile_matches_effective_server() {
    let path = PathBuf::from(std::env::var_os("LLAMANCER_TEST_HYBRID").unwrap());
    let groups = models::group::group(
        models::scan(vec![path], &AtomicBool::new(false))
            .unwrap()
            .candidates,
    );
    assert_eq!(groups.len(), 1);
    let g = &groups[0];
    assert!(models::profile::recommendation_match(g)
        .unwrap()
        .exact_review());
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut config = Config {
        server_binary: std::env::var("LLAMANCER_TEST_SERVER").unwrap(),
        device: "none".into(),
        endpoint: format!("http://127.0.0.1:{port}"),
        default_context: Some(4096),
        ..Default::default()
    };
    for (mode, temp, top_p, presence) in [
        ("recommended", 1.0, 1.0, 2.0),
        ("custom", 0.42, 1.0, 2.0),
        ("reset", 1.0, 1.0, 2.0),
    ] {
        if mode == "custom" {
            let mut p = Profile::default();
            p.generation.temperature = Some(temp);
            p.agent_thinking = Some(Thinking::Off);
            config.profiles.insert(identity(g), p);
        } else {
            config.profiles.remove(&identity(g));
        }
        let before = config.clone();
        let mut c = Controller::default();
        c.start(&config, Vec::new(), Some(g.clone())).unwrap();
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            c.tick(&config);
            if c.snapshot
                .as_ref()
                .is_some_and(|s| !s.properties.is_empty())
            {
                break;
            }
            if Instant::now() > deadline {
                let logs = c.logs.join("\n");
                c.shutdown().unwrap();
                panic!("Server timeout {logs}");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let s = c.snapshot.as_ref().unwrap();
        let props = &s.properties[&s.models[0].id];
        let actual = props.generation.clone();
        let ctx = props.context;
        c.shutdown().unwrap();
        assert_eq!(ctx, Some(4096));
        for (got, want) in [
            (actual.temperature, temp),
            (actual.top_p, top_p),
            (actual.presence_penalty, presence),
            (actual.min_p, 0.0),
            (actual.repeat_penalty, 1.0),
        ] {
            assert!(
                got.is_some_and(|v| (v - want).abs() < 0.0001),
                "{mode}: got {got:?}, expected {want}"
            );
        }
        assert_eq!(actual.top_k, Some(20));
        assert_eq!(config, before);
        println!("PROFILE_PROPS_OK {mode} ctx=4096 temp={temp} top_p={top_p} presence={presence}");
    }
}
