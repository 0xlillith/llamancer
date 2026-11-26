//! Bounded opt-in live multipart acceptance through both transports.
use llamancer::{
    downloads::{Controller, State, Transport},
    huggingface, models,
};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
fn wait(q: &mut Controller) {
    let deadline = Instant::now() + Duration::from_secs(180);
    while q.busy() {
        q.tick();
        if Instant::now() >= deadline {
            let _ = q.shutdown();
            panic!("Timeout: {}", q.status);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn assert_complete(q: &mut Controller, root: &Path) {
    assert_eq!(q.queue.items.len(), 3);
    assert!(
        q.queue
            .items
            .iter()
            .all(|i| i.state == State::Complete && i.receipt.is_some()),
        "{}",
        q.status
    );
    let scan = models::scan(vec![root.to_path_buf()], &AtomicBool::new(false)).unwrap();
    let groups = models::group::group(scan.candidates);
    assert!(groups.iter().any(|g| g.complete && g.files.len() == 3));
    q.verify_all().unwrap();
    wait(q);
    println!("Audit: {}", q.status);
    assert!(q.status.contains("0 failed"), "{}", q.status);
}
#[test]
#[ignore = "requires public Hub access, optional hf CLI, and an isolated HF_HUB_CACHE"]
fn multipart_native_hub_and_cached_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let cache = std::env::var_os("HF_HUB_CACHE").expect("Set isolated HF_HUB_CACHE");
    assert!(
        Path::new(&cache).is_absolute() && !Path::new(&cache).exists(),
        "Use fresh cache"
    );
    // Pinned public fixtures avoid dependence on the moving repository listing.
    let files: Vec<_> = [
        (
            1,
            82420224,
            "b7f65a865470f8023edd038d0b787d8db9268abaa8a0a4e21448150772ceea33",
        ),
        (
            2,
            8301440,
            "44af9035d52becbe1dfe47fe73109eb6d806287cd7d1e25980dbd62ea18edd1e",
        ),
        (
            3,
            7636576,
            "34a6543e31bddf578870b1958b0a2937db1506314deaf0261c6d1e1f8db0ef21",
        ),
    ]
    .into_iter()
    .map(|(part, size, digest)| huggingface::File {
        repo: "ggml-org/models".into(),
        revision: "499bc8821c6b12b4e53c5bffcb21ec206f212d81".into(),
        name: format!("tinyllamas/split/stories15M-{part:05}-of-00003.gguf"),
        size,
        sha256: Some(digest.into()),
    })
    .collect();
    for (name, transport) in [
        ("native", Transport::Native),
        ("hub", Transport::Hub),
        ("cached-resume", Transport::Hub),
    ] {
        let root = temp.path().join(name);
        fs::create_dir(&root).unwrap();
        let mut q = Controller::new(temp.path().join(format!("{name}.toml"))).unwrap();
        q.transport = transport;
        q.add(files.clone(), root.clone()).unwrap();
        if name == "cached-resume" {
            let f = &files[0];
            let rel = Path::new(&f.repo).join(&f.revision).join(&f.name);
            let dest = root.join(&rel);
            fs::create_dir_all(dest.parent().unwrap()).unwrap();
            let partial = dest.with_file_name(format!(
                "{}.part",
                dest.file_name().unwrap().to_str().unwrap()
            ));
            let mut out = fs::File::create(&partial).unwrap();
            let n = std::io::copy(
                &mut fs::File::open(temp.path().join("native").join(rel))
                    .unwrap()
                    .take(4 * 1024 * 1024),
                &mut out,
            )
            .unwrap();
            out.flush().unwrap();
            assert_eq!(n, 4 * 1024 * 1024);
        }
        q.resume_bundle().unwrap();
        wait(&mut q);
        assert_complete(&mut q, &root);
        println!("MULTIPART_OK transport={name}");
    }
}
