//! Explicit model fit advisory. Device capacity is only reported from an actual
//! backend probe; weight placement and total process memory are not inferred.
use super::{estimate, group::Group, profile::Profile};
use crate::config::Config;

#[derive(Debug, PartialEq, Eq)]
pub struct DeviceMemory {
    pub id: String,
    pub total: u64,
    pub free: u64,
}

pub fn parse_devices(text: &str) -> Vec<DeviceMemory> {
    text.lines()
        .filter_map(|line| {
            let (id, info) = line.trim().split_once(':')?;
            if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return None;
            }
            let (_, tail) = info.rsplit_once('(')?;
            let tail = tail.strip_suffix(')')?;
            let (total, free) = tail.split_once(" MiB, ")?;
            let total = total.parse::<u64>().ok()?.checked_mul(1048576)?;
            let free = free
                .strip_suffix(" MiB free")?
                .parse::<u64>()
                .ok()?
                .checked_mul(1048576)?;
            (free <= total).then(|| DeviceMemory {
                id: id.into(),
                total,
                free,
            })
        })
        .take(32)
        .collect()
}

pub fn available_ram() -> Option<u64> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open("/proc/meminfo")
        .ok()?
        .take(16384)
        .read_to_string(&mut text)
        .ok()?;
    let value = text
        .lines()
        .find_map(|l| l.strip_prefix("MemAvailable:"))?
        .trim();
    value
        .strip_suffix(" kB")?
        .trim()
        .parse::<u64>()
        .ok()?
        .checked_mul(1024)
}

pub const CODING_CONTEXT_TARGET: u32 = 131_072;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextPlan {
    pub tokens: Option<u32>,
    pub source: &'static str,
}
/// Estimate host fit conservatively. Do not sum host and device memory:
/// integrated devices can share RAM, and offload placement is backend-specific.
/// Explicit user settings always win, even if the estimate warns.
pub fn context_plan(
    group: &Group,
    profile: &Profile,
    device: &str,
    devices: &str,
    ram: Option<u64>,
) -> ContextPlan {
    if let Some(n) = profile.engine.context_size {
        return ContextPlan {
            tokens: Some(n),
            source: "Saved preference",
        };
    }
    let backend = ContextPlan {
        tokens: None,
        source: "Backend auto-fit",
    };
    let Some(m) = group.files.first().map(|f| &f.metadata) else {
        return backend;
    };
    let Some(max) = m.context.filter(|n| *n > 0) else {
        return backend;
    };
    // Full explicit offload uses the matching device's last reported capacity.
    // Otherwise budget for the whole model on the host; this is conservative
    // for discrete GPU offload, and avoids counting shared RAM twice.
    let full_gpu = device != "none"
        && m.layers
            .is_some_and(|n| profile.engine.gpu_layers.is_some_and(|g| u64::from(g) > n));
    let free = if full_gpu {
        parse_devices(devices)
            .into_iter()
            .find(|d| d.id == device)
            .map(|d| d.free)
    } else {
        ram
    };
    let Some(free) = free else { return backend };
    const GIB: u64 = 1024 * 1024 * 1024;
    let reserve = (free / 5).max(if full_gpu { GIB } else { 4 * GIB });
    let budget = free.saturating_sub(reserve);
    let goal = max.min(u64::from(CODING_CONTEXT_TARGET)) as u32;
    let mut trial = profile.clone();
    for n in [goal, 65536, 32768, 16384, 8192, 4096, 2048, 1024] {
        if n > goal {
            continue;
        }
        trial.engine.context_size = Some(n);
        let Some(e) = estimate::estimate(group, &trial) else {
            return backend;
        };
        if e.weights.saturating_add(e.kv) <= budget {
            return ContextPlan {
                tokens: Some(n),
                source: "Estimated auto-fit",
            };
        }
    }
    backend
}

fn host_fit_bytes(output: &str) -> Option<u64> {
    let mut host = None;
    for line in output.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.first() == Some(&"Host") && fields.len() == 4 {
            if host.is_some() {
                return None;
            }
            let total = fields[1..]
                .iter()
                .try_fold(0u64, |sum, n| sum.checked_add(n.parse::<u64>().ok()?))?;
            host = Some(total.checked_mul(1048576)?);
        }
    }
    host
}
/// Use the installed backend's own metadata-only memory estimator for
/// architectures not supported by the conventional KV formula. Budget a
/// CPU-only measurement (no double-counted shared GPU RAM), then allow
/// llama-server to select offload automatically. Never parse generated CLI
/// flags or run a remote model script. A bounded failed measurement falls
/// back to backend auto-fit.
pub fn native_context_plan(
    group: &Group,
    profile: &Profile,
    binary: &str,
    cancel: &std::sync::atomic::AtomicBool,
    ram: Option<u64>,
) -> Option<u32> {
    if profile.engine.context_size.is_some()
        || profile.engine.projector.is_some()
        || profile.engine.gpu_layers.is_some_and(|n| n > 0)
        || !group.runnable()
    {
        return None;
    }
    let ram = ram?;
    let file = group.files.first()?;
    let limit = file.metadata.context?.min(u64::from(CODING_CONTEXT_TARGET)) as u32;
    if limit == 0 {
        return None;
    }
    let binary = std::path::Path::new(binary);
    if !binary.is_absolute() || binary.file_name()?.to_str()? != "llama-server" {
        return None;
    }
    let helper = binary.with_file_name("llama-fit-params");
    let helper = helper.to_str()?;
    if !crate::doctor::executable(helper, None) {
        return None;
    }
    let budget = ram.saturating_sub((ram / 5).max(4 * 1024_u64.pow(3)));
    // At most three bounded metadata probes. Unsupported versions or
    // missing measurements return None rather than guessing.
    let mut candidates = vec![limit, 65536u32.min(limit), 32768u32.min(limit)];
    candidates.dedup();
    for n in candidates {
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return None;
        }
        let mut args = vec![
            "--model".into(),
            file.path.to_str()?.to_owned(),
            "--ctx-size".into(),
            n.to_string(),
            "--device".into(),
            "none".into(),
            "--n-gpu-layers".into(),
            "0".into(),
            "--fit-print".into(),
            "on".into(),
            "--log-verbosity".into(),
            "2".into(),
        ];
        let e = &profile.engine;
        for (flag, value) in [
            ("--batch-size", e.batch_size.map(|v| v.to_string())),
            ("--ubatch-size", e.ubatch_size.map(|v| v.to_string())),
            ("--cache-type-k", e.cache_type_k.clone()),
            ("--cache-type-v", e.cache_type_v.clone()),
            (
                "--flash-attn",
                e.flash_attention
                    .map(|v| if v { "on".into() } else { "off".into() }),
            ),
        ] {
            if let Some(value) = value {
                args.extend([flag.to_owned(), value]);
            }
        }
        let refs: Vec<_> = args.iter().map(String::as_str).collect();
        let output = crate::llama::process::command_output(helper, &refs).ok()?;
        let required = host_fit_bytes(&output)?;
        if required > 0 && required <= budget {
            return Some(n);
        }
    }
    None
}

pub fn report(
    group: &Group,
    profile: &Profile,
    config: &Config,
    devices: &str,
    ram: Option<u64>,
) -> String {
    let gib = |n: u64| format!("{:.2} GiB", n as f64 / 1073741824.0);
    let mut lines = vec![
        format!("Model: {}", crate::ui::safe(&group.name)),
        format!("Device preference: {}", crate::ui::safe(&config.device)),
        format!(
            "GPU layers: {}",
            profile
                .engine
                .gpu_layers
                .map_or_else(|| "auto (backend default)".into(), |n| n.to_string())
        ),
        format!(
            "System RAM available: {}",
            ram.map(gib).unwrap_or_else(|| "unknown".into())
        ),
    ];
    let devices = parse_devices(devices);
    if devices.is_empty() {
        lines.push(
            "Device memory unavailable: use i in Server to probe the configured backend.".into(),
        );
    } else {
        lines
            .push("Last backend probe (may include shared memory; not current allocation):".into());
        for d in &devices {
            lines.push(format!(
                "  {}: {} free / {} total",
                d.id,
                gib(d.free),
                gib(d.total)
            ));
        }
    }
    lines.push(String::new());
    let Some(e) = estimate::estimate(group, profile) else {
        lines.push(format!("GGUF disk size: {}", gib(group.bytes())));
        lines.push("KV estimate unavailable for this architecture or metadata.".into());
        lines.push("No fit conclusion; test a reduced context in a controlled session.".into());
        return lines.join("\n");
    };
    lines.push(format!(
        "Weights on disk: {} | KV estimate: {} at {} tokens",
        gib(e.weights),
        gib(e.kv),
        e.context
    ));
    let Some(sum) = e.weights.checked_add(e.kv) else {
        lines.push("Combined estimate overflow; fit unknown.".into());
        return lines.join("\n");
    };
    lines.push(format!("Disk weights + estimated KV: {}", gib(sum)));
    let cpu = config.device == "none" || profile.engine.gpu_layers == Some(0);
    let full = group
        .files
        .first()
        .and_then(|f| f.metadata.layers)
        .is_some_and(|n| profile.engine.gpu_layers.is_some_and(|g| u64::from(g) > n));
    if cpu {
        if let Some(free) = ram {
            lines.push(
                if sum > free {
                    "Warning: combined estimate exceeds available system RAM."
                } else {
                    "Combined estimate is below available system RAM; headroom is not guaranteed."
                }
                .into(),
            );
        }
    } else if full {
        if let Some(d) = devices.iter().find(|d| d.id == config.device) {
            lines.push(if sum>d.free {"Warning: combined estimate exceeds last reported free device memory."}else{"Combined estimate is below last reported free device memory; headroom is not guaranteed."}.into());
        } else {
            lines.push("Full offload requested; exact device capacity unavailable.".into());
        }
    } else {
        lines.push(
            "Partial/automatic offload: device/host allocation cannot be inferred reliably.".into(),
        );
    }
    lines.push("Excludes compute buffers, runtime overhead, projector, actual weight layout and other jobs.".into());
    lines.push("Advisory only. Reduce context or GPU layers if needed; no automatic launch or setting changes.".into());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_fit_parser_accepts_only_exact_measurement_rows() {
        assert_eq!(
            host_fit_bytes("log line\nHost 336 1555 623 \n"),
            Some((336 + 1555 + 623) * 1048576)
        );
        assert_eq!(host_fit_bytes("Host 1 2 3\nHost 4 5 6"), None);
        assert_eq!(host_fit_bytes("Host 1 2 3 --other"), None);
        assert_eq!(host_fit_bytes("Host 1 2 nonsense"), None);
    }
    #[test]
    fn coding_context_aims_at_128k_but_respects_limits_and_overrides() {
        use crate::models::{gguf::Metadata, Candidate};
        let mut group = Group {
            name: "fixture".into(),
            files: vec![Candidate {
                path: "/tmp/fixture.gguf".into(),
                bytes: 8 * 1024_u64.pow(3),
                version: 3,
                metadata: Metadata {
                    architecture: Some("llama".into()),
                    layers: Some(32),
                    heads: Some(32),
                    kv_heads: Some(8),
                    embedding: Some(4096),
                    context: Some(262144),
                    ..Default::default()
                },
            }],
            expected: 1,
            complete: true,
            projector: false,
        };
        let gib = 1024_u64.pow(3);
        let mut profile = Profile::default();
        assert_eq!(
            context_plan(&group, &profile, "auto", "", Some(64 * gib)).tokens,
            Some(131072)
        );
        assert_eq!(
            context_plan(&group, &profile, "none", "", Some(16 * gib)).tokens,
            Some(32768)
        );
        group.files[0].metadata.context = Some(32768);
        assert_eq!(
            context_plan(&group, &profile, "auto", "", Some(64 * gib)).tokens,
            Some(32768)
        );
        profile.engine.context_size = Some(200000);
        assert_eq!(
            context_plan(&group, &profile, "auto", "", Some(gib)).tokens,
            Some(200000)
        );
        profile.engine.context_size = None;
        group.files[0].metadata.architecture = Some("hybrid-unknown".into());
        assert_eq!(
            context_plan(&group, &profile, "auto", "", Some(64 * gib)).tokens,
            None
        );
    }
    #[test]
    fn parse_real_backend_style_and_reject_invalid_capacity() {
        let rows=parse_devices("Available devices:\n  Vulkan0: AMD (RADV) (16304 MiB, 15285 MiB free)\n  ROCm1: AMD (62924 MiB, 62920 MiB free)\n  CUDA0: NVIDIA (10 MiB, 11 MiB free)\n invalid: (unknown)");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "Vulkan0");
        assert_eq!(rows[0].free, 15285 * 1048576);
    }
}
