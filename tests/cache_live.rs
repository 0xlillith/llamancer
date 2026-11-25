//! Opt-in read-only scan of an existing Hub cache, without loading model weights.
use llamancer::models::{self, group};
use std::{path::PathBuf, sync::atomic::AtomicBool};

#[test]
#[ignore = "requires explicit existing cache directory"]
fn real_cache_snapshot() {
    let root = PathBuf::from(std::env::var_os("LLAMANCER_TEST_CACHE").expect("Set cache root"));
    let scan = models::scan(vec![root], &AtomicBool::new(false)).unwrap();
    assert!(!scan.candidates.is_empty(), "No real model files indexed");
    let files = scan.candidates.len();
    let groups = group::group(scan.candidates);
    for group in &groups {
        eprintln!(
            "architecture={:?} complete={} projector={} shards={}/{} payload_bounds_checked={}",
            group.files[0].metadata.architecture,
            group.complete,
            group.projector,
            group.files.len(),
            group.expected,
            group
                .files
                .iter()
                .all(|f| f.metadata.payload_bounds_checked)
        );
    }
    assert!(groups.iter().any(|g| g.complete && !g.projector));
    eprintln!(
        "Indexed {files} files in {} groups; {} skipped",
        groups.len(),
        scan.skipped
    );
}
