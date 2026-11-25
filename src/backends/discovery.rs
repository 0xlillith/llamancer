//! Read-only discovery. Upstream metadata is never authority to install an
//! executable: installation still requires a reviewed, checksum-pinned catalog.
use super::{Kind, RELEASES};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    io::{self, Read},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

const ENDPOINT: &str = "https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=8";
const MAX_BODY: u64 = 4 * 1024 * 1024;
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<UpstreamAsset>,
}
#[derive(Deserialize)]
struct UpstreamAsset {
    name: String,
    size: u64,
    digest: Option<String>,
}
fn version(tag: &str) -> Option<u32> {
    let n = tag.strip_prefix('b')?;
    if n.is_empty() || n.len() > 9 || !n.bytes().all(|c| c.is_ascii_digit()) || n.starts_with('0') {
        return None;
    }
    n.parse().ok()
}
fn suffix(kind: Kind, arch: &str) -> Option<&'static str> {
    match (arch, kind) {
        ("x86_64", Kind::Cpu) => Some("x64"),
        ("x86_64", Kind::Vulkan) => Some("vulkan-x64"),
        ("x86_64", Kind::Rocm) => Some("rocm-10.0-x64"),
        ("x86_64", Kind::Cuda) => Some("cuda-12.8-x64"),
        ("aarch64", Kind::Cuda) => Some("cuda-13.5-arm64"),
        ("aarch64", Kind::Cpu) => Some("arm64"),
        ("aarch64", Kind::Vulkan) => Some("vulkan-arm64"),
        _ => None,
    }
}
fn parse(bytes: &[u8], arch: &str) -> io::Result<String> {
    let releases: Vec<Release> = serde_json::from_slice(bytes)
        .map_err(|_| io::Error::other("Invalid GitHub release response"))?;
    let reviewed = RELEASES
        .iter()
        .filter_map(|tag| version(tag))
        .max()
        .unwrap_or(0);
    let mut newer = BTreeMap::new();
    for release in releases.into_iter().take(8) {
        let Some(n) = version(&release.tag_name) else {
            continue;
        };
        if release.draft || n <= reviewed {
            continue;
        }
        let mut kinds = Vec::new();
        for kind in Kind::ALL {
            let Some(suffix) = suffix(kind, arch) else {
                continue;
            };
            let filename = format!("llama-{}-bin-ubuntu-{suffix}.tar.gz", release.tag_name);
            let mut matches = release.assets.iter().filter(|a| a.name == filename);
            let Some(asset) = matches.next() else {
                continue;
            };
            if matches.next().is_some() || asset.size == 0 || asset.size > 8 * 1024 * 1024 * 1024 {
                continue;
            }
            if !asset
                .digest
                .as_deref()
                .and_then(|d| d.strip_prefix("sha256:"))
                .is_some_and(|d| d.len() == 64 && d.bytes().all(|c| c.is_ascii_hexdigit()))
            {
                continue;
            }
            if kind == Kind::Cuda {
                let companion = format!("cudart-{filename}");
                let mut found = release.assets.iter().filter(|a| a.name == companion);
                let Some(runtime) = found.next() else {
                    continue;
                };
                if found.next().is_some()
                    || runtime.size == 0
                    || runtime.size > 8 * 1024 * 1024 * 1024
                    || !runtime
                        .digest
                        .as_deref()
                        .and_then(|d| d.strip_prefix("sha256:"))
                        .is_some_and(|d| d.len() == 64 && d.bytes().all(|c| c.is_ascii_hexdigit()))
                {
                    continue;
                }
            }
            kinds.push(kind.name());
        }
        if !kinds.is_empty() {
            newer.insert(
                n,
                format!(
                    "{}{}: {}",
                    release.tag_name,
                    if release.prerelease {
                        " (prerelease)"
                    } else {
                        ""
                    },
                    kinds.join(", ")
                ),
            );
        }
    }
    if newer.is_empty() {
        return Ok(format!("No newer compatible checksummed builds in the eight recent releases. Latest reviewed: b{reviewed}."));
    }
    let mut lines = vec![format!(
        "Newer upstream builds (not reviewed or installable yet; latest reviewed b{reviewed}):"
    )];
    lines.extend(newer.into_values().rev().take(3));
    lines.push("Installation requires a catalog update. Nothing downloaded or changed.".into());
    Ok(lines.join("\n"))
}
pub fn check(cancel: &AtomicBool) -> io::Result<String> {
    let check_cancel = || {
        if cancel.load(Ordering::Acquire) {
            Err(io::Error::from(io::ErrorKind::Interrupted))
        } else {
            Ok(())
        }
    };
    check_cancel()?;
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(20))
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(10))
        .build();
    let response = agent
        .get(ENDPOINT)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "Llamancer-backend-check")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => {
                io::Error::other(format!("GitHub release check HTTP {code}"))
            }
            _ => io::Error::other("GitHub release check unavailable"),
        })?;
    if response.status() != 200 {
        return Err(io::Error::other("Unexpected release response"));
    }
    let mut reader = response.into_reader().take(MAX_BODY + 1);
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 16384];
    loop {
        check_cancel()?;
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
    if bytes.len() as u64 > MAX_BODY {
        return Err(io::Error::other("GitHub release metadata exceeded limit"));
    }
    parse(&bytes, std::env::consts::ARCH)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn newer_release_requires_exact_name_and_digest() {
        let good = json!({"name":"llama-b99999-bin-ubuntu-x64.tar.gz","size":100,"digest":format!("sha256:{}","a".repeat(64))});
        let bad =
            json!({"name":"llama-b99999-bin-ubuntu-vulkan-x64.tar.gz","size":100,"digest":null});
        let fixture = json!([
            {"tag_name":"b99999","draft":false,"prerelease":true,"assets":[good,bad]},
            {"tag_name":"b99998","draft":true,"prerelease":false,"assets":[]},
            {"tag_name":"../../evil","draft":false,"prerelease":false,"assets":[]},
            {"tag_name":"b12721","draft":false,"prerelease":false,"assets":[]}
        ]);
        let bytes = serde_json::to_vec(&fixture).unwrap();
        let result = parse(&bytes, "x86_64").unwrap();
        assert!(result.contains("b99999 (prerelease): cpu"));
        assert!(
            !result.contains("vulkan") && !result.contains("evil") && !result.contains("b99998")
        );
        assert!(parse(&bytes, "aarch64")
            .unwrap()
            .contains("No newer compatible"));
        assert!(parse(b"invalid", "x86_64").is_err());
    }
    #[test]
    fn cancelled_check_never_contacts_network() {
        assert_eq!(
            check(&AtomicBool::new(true)).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
    }
}
