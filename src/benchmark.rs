//! Comparable, bounded local benchmark records. Model contents and paths are never stored.
use serde::{Deserialize, Serialize};
use std::io;
pub const LIMIT: usize = 64;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub kind: String,
    pub release: String,
    pub device: String,
    pub model_sha256: String,
    pub model_bytes: u64,
    pub pp: f64,
    pub tg: f64,
    pub gpu_layers: u32,
}
impl Record {
    pub fn validate(&self) -> bool {
        crate::backends::Kind::parse(&self.kind).is_ok()
            && crate::backends::RELEASES.contains(&self.release.as_str())
            && !self.device.is_empty()
            && self.device.len() <= 64
            && self
                .device
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            && self.model_sha256.len() == 64
            && self.model_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            && self.model_bytes > 0
            && self.model_bytes <= 512 * 1024 * 1024
            && [self.pp, self.tg]
                .into_iter()
                .all(|v| v.is_finite() && v > 0.0)
            && self.gpu_layers == if self.kind == "cpu" { 0 } else { 8 }
    }
    pub fn summary(&self) -> String {
        format!("{} {} | {} | 2 threads, 2 repeats | {} GPU layers\npp 128 / tg 0: {:.2} tok/s\npp 0 / tg 32: {:.2} tok/s\nh compare saved runs",self.kind,self.release,self.device,self.gpu_layers,self.pp,self.tg)
    }
}
pub fn record(history: &mut Vec<Record>, value: Record) {
    history.retain(|old| {
        !(old.kind == value.kind
            && old.release == value.release
            && old.device == value.device
            && old.model_sha256 == value.model_sha256)
    });
    history.push(value);
    if history.len() > LIMIT {
        history.drain(..history.len() - LIMIT);
    }
}
pub fn validate(history: &[Record]) -> io::Result<()> {
    if history.len() > LIMIT || history.iter().any(|r| !r.validate()) {
        return Err(io::Error::other("Invalid benchmark history"));
    }
    Ok(())
}
pub fn comparison(history: &[Record]) -> String {
    let Some(latest) = history.last() else {
        return "No saved benchmark runs. Select a small GGUF in Library, then use b in Backend Manager.".into();
    };
    let mut lines = vec![
        format!(
            "Latest model SHA-256: {}… | {:.1} MiB",
            &latest.model_sha256[..12],
            latest.model_bytes as f64 / 1048576.0
        ),
        "Same model only | PP128 / TG32 | 2 threads | 2 repeats".into(),
        "Backend     Version    Device       Layers      PP tok/s      TG tok/s".into(),
    ];
    for kind in crate::backends::Kind::ALL {
        let result = history
            .iter()
            .rev()
            .find(|r| r.model_sha256 == latest.model_sha256 && r.kind == kind.name());
        if let Some(r) = result {
            lines.push(format!(
                "{:<11} {:<10} {:<12} {:>6} {:>13.2} {:>13.2}",
                r.kind, r.release, r.device, r.gpu_layers, r.pp, r.tg
            ));
        } else {
            lines.push(format!("{:<11} — not measured for this model", kind.name()));
        }
    }
    lines.push(
        "Latest run per backend; release/device differences are shown. CPU uses 0 layers, GPU 8."
            .into(),
    );
    lines.push(
        "Local results only; not a full-offload benchmark or hardware compatibility guarantee."
            .into(),
    );
    lines.join("\n")
}
#[cfg(test)]
mod tests {
    use super::*;
    fn example(kind: &str, hash: char) -> Record {
        Record {
            kind: kind.into(),
            release: "b12726".into(),
            device: if kind == "cpu" {
                "none".into()
            } else {
                "Vulkan0".into()
            },
            model_sha256: hash.to_string().repeat(64),
            model_bytes: 100,
            pp: 12.0,
            tg: 8.0,
            gpu_layers: if kind == "cpu" { 0 } else { 8 },
        }
    }
    #[test]
    fn comparison_uses_only_identical_model_and_latest_backend() {
        let mut rows = vec![];
        record(&mut rows, example("cpu", 'a'));
        record(&mut rows, example("vulkan", 'b'));
        let report = comparison(&rows);
        assert!(report.contains("cpu         — not measured"));
        record(&mut rows, example("cpu", 'b'));
        assert!(comparison(&rows).contains("12.00"));
        assert!(validate(&rows).is_ok());
        record(&mut rows, example("cpu", 'b'));
        assert_eq!(rows.len(), 3);
    }
}
