//! Per-model inference preferences. Backend capability validation is separate.
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub engine: Engine,
    pub generation: Generation,
    pub agent_thinking: Option<Thinking>,
    pub tool_preflight: bool,
    /// Disable model-card fallbacks without discarding explicit overrides.
    pub backend_defaults: bool,
}
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Engine {
    pub context_size: Option<u32>,
    pub gpu_layers: Option<u32>,
    pub threads: Option<u32>,
    pub batch_size: Option<u32>,
    pub ubatch_size: Option<u32>,
    pub flash_attention: Option<bool>,
    pub cache_type_k: Option<String>,
    pub cache_type_v: Option<String>,
    pub projector: Option<String>,
}
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Generation {
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    pub min_p: Option<f32>,
    pub repeat_penalty: Option<f32>,
    pub presence_penalty: Option<f32>,
    pub seed: Option<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Thinking {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
    Auto,
}
impl Thinking {
    pub const ALL: [Self; 8] = [
        Self::Off,
        Self::Minimal,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::Xhigh,
        Self::Max,
        Self::Auto,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
            Self::Auto => "auto",
        }
    }
}
pub const RECOMMENDATION_REPO: &str =
    "Qwen3.8-Flash-Next-Turbo-Brilliance-50x-Reasoning-Instruct-GSQ-RCO-GGUF";
pub const RECOMMENDATION_REV: &str = "096fef2f967efa50c25529adda0c59637e2b4477";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recommendation {
    FlashNext,
    Qwen27B,
    DavidAu,
}
impl Recommendation {
    /// Model-card provenance. Unsloth entries are reviewed repository-level
    /// guidance, not a claim to have reviewed every future weight revision.
    pub fn source(self) -> &'static str {
        match self {
            Self::FlashNext => "https://huggingface.co/unsloth/Qwen3.8-Flash-Next-GGUF/blob/2c41bd2a0b3f51c503c11f1c7ed2e6bb34036beb/README.md",
            Self::Qwen27B => "https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/blob/408fcc1807ab264d8cd644a7c4c0f58fbd32eebc/README.md",
            Self::DavidAu => "https://huggingface.co/DavidAU/Qwen3.8-Flash-Next-Turbo-Brilliance-50x-Reasoning-Instruct-GSQ-RCO-GGUF/blob/096fef2f967efa50c25529adda0c59637e2b4477/README.md",
        }
    }
    pub fn publisher(self) -> &'static str {
        match self {
            Self::FlashNext | Self::Qwen27B => "Unsloth / Qwen model card",
            Self::DavidAu => "DavidAU pinned model card",
        }
    }
    fn defaults(self, thinking: Option<Thinking>) -> Option<Generation> {
        let instruct = thinking == Some(Thinking::Off);
        if self == Self::DavidAu && instruct {
            // The existing DavidAU review covered thinking mode only.
            return None;
        }
        Some(Generation {
            temperature: Some(if instruct { 0.7 } else { 1.0 }),
            top_p: Some(if instruct { 0.80 } else { 0.95 }),
            top_k: Some(20),
            min_p: Some(0.0),
            repeat_penalty: Some(1.0),
            presence_penalty: if self == Self::DavidAu {
                None
            } else {
                Some(if instruct { 1.5 } else { 0.0 })
            },
            seed: None,
        })
    }
}
fn path_recommendation(path: &std::path::Path) -> Option<(Recommendation, &std::path::Path)> {
    // Managed layout: owner/repository/revision/[variant directories]/file.
    // Match full owner and repository, not the filename or a fuzzy model name.
    for rev in path.ancestors().skip(1) {
        let Some(hash) = rev.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let Some(repo) = rev.parent() else { continue };
        let name = repo.file_name().and_then(|n| n.to_str());
        let owner = repo
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str());
        let preset = match (owner, name) {
            (Some("unsloth"), Some("Qwen3.8-Flash-Next-GGUF")) => Recommendation::FlashNext,
            (Some("unsloth"), Some("Qwen3.8-27B-GGUF")) => Recommendation::Qwen27B,
            (Some("DavidAU"), Some(RECOMMENDATION_REPO)) if hash == RECOMMENDATION_REV => {
                Recommendation::DavidAu
            }
            _ => continue,
        };
        return Some((preset, rev));
    }
    None
}
pub fn recommendation(group: &super::group::Group) -> Option<Recommendation> {
    if !group.complete || group.projector {
        return None;
    }
    let first = path_recommendation(&group.files.first()?.path)?;
    group
        .files
        .iter()
        .all(|f| path_recommendation(&f.path) == Some(first))
        .then_some(first.0)
}
pub fn thinking_options(group: &super::group::Group) -> &'static [Option<Thinking>] {
    const QWEN: &[Option<Thinking>] = &[
        None,
        Some(Thinking::Off),
        Some(Thinking::Low),
        Some(Thinking::Medium),
        Some(Thinking::Xhigh),
        Some(Thinking::Auto),
    ];
    const GENERIC: &[Option<Thinking>] = &[
        None,
        Some(Thinking::Off),
        Some(Thinking::Minimal),
        Some(Thinking::Low),
        Some(Thinking::Medium),
        Some(Thinking::High),
        Some(Thinking::Xhigh),
        Some(Thinking::Max),
        Some(Thinking::Auto),
    ];
    match recommendation(group) {
        Some(_) => QWEN,
        None => GENERIC,
    }
}
pub fn thinking_label(value: Option<Thinking>) -> &'static str {
    value
        .map(Thinking::label)
        .unwrap_or("Recommended / inherit")
}
impl Profile {
    pub fn recommendation_source(
        &self,
        group: &super::group::Group,
        omp: bool,
    ) -> Option<Recommendation> {
        if self.backend_defaults {
            return None;
        }
        recommendation(group).filter(|preset| {
            preset
                .defaults(if omp { self.agent_thinking } else { None })
                .is_some()
        })
    }
    /// Effective configuration only. Never materialize recommendations into
    /// the saved profile: mode changes should update inherited fields.
    pub fn with_model_defaults(mut self, group: &super::group::Group, omp: bool) -> Self {
        let Some(preset) = recommendation(group) else {
            return self;
        };
        if self.backend_defaults {
            return self;
        }
        if omp && self.agent_thinking.is_none() {
            self.agent_thinking = Some(Thinking::Medium);
        }
        if let Some(defaults) = preset.defaults(if omp { self.agent_thinking } else { None }) {
            let g = &mut self.generation;
            g.temperature = g.temperature.or(defaults.temperature);
            g.top_p = g.top_p.or(defaults.top_p);
            g.top_k = g.top_k.or(defaults.top_k);
            g.min_p = g.min_p.or(defaults.min_p);
            g.repeat_penalty = g.repeat_penalty.or(defaults.repeat_penalty);
            g.presence_penalty = g.presence_penalty.or(defaults.presence_penalty);
        }
        self
    }
    pub fn validate_thinking(&self, group: &super::group::Group) -> io::Result<()> {
        if !thinking_options(group).contains(&self.agent_thinking) {
            return Err(invalid("This model supports off, low, medium, xhigh or auto reasoning; choose a supported level in Launch"));
        }
        Ok(())
    }
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
const CACHE_TYPES: &[&str] = &[
    "f32", "f16", "bf16", "q8_0", "q4_0", "q4_1", "q5_0", "q5_1", "iq4_nl",
];
impl Profile {
    pub fn validate(&self) -> io::Result<()> {
        let e = &self.engine;
        let g = &self.generation;
        if [e.context_size, e.threads, e.batch_size, e.ubatch_size]
            .into_iter()
            .flatten()
            .any(|n| n == 0 || n > i32::MAX as u32)
            || e.gpu_layers.is_some_and(|n| n > i32::MAX as u32)
            || g.top_k.is_some_and(|n| n > i32::MAX as u32)
        {
            return Err(invalid("Invalid integer parameter"));
        }
        if let (Some(batch), Some(ubatch)) = (e.batch_size, e.ubatch_size) {
            if ubatch > batch {
                return Err(invalid("Micro-batch must not exceed batch size"));
            }
        }
        if let Some(path) = &e.projector {
            let path = std::path::Path::new(path);
            if !path.is_absolute()
                || !path
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("gguf"))
            {
                return Err(invalid("Projector must be an absolute GGUF path"));
            }
        }
        for cache in [&e.cache_type_k, &e.cache_type_v].into_iter().flatten() {
            if !CACHE_TYPES.contains(&cache.as_str()) {
                return Err(invalid("Unsupported cache type preference"));
            }
        }
        for (value, min, max) in [
            (g.temperature, 0.0, f32::MAX),
            (g.top_p, 0.0, 1.0),
            (g.min_p, 0.0, 1.0),
            (g.repeat_penalty, f32::MIN_POSITIVE, f32::MAX),
            (g.presence_penalty, -2.0, 2.0),
        ] {
            if value.is_some_and(|v| !v.is_finite() || v < min || v > max) {
                return Err(invalid("Generation value outside allowed range"));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Context,
    GpuLayers,
    Threads,
    Batch,
    Ubatch,
    Flash,
    CacheK,
    CacheV,
    Projector,
    Temperature,
    TopP,
    TopK,
    MinP,
    RepeatPenalty,
    PresencePenalty,
    Seed,
}
impl Field {
    pub const ALL: [Self; 16] = [
        Self::Context,
        Self::GpuLayers,
        Self::Threads,
        Self::Batch,
        Self::Ubatch,
        Self::Flash,
        Self::CacheK,
        Self::CacheV,
        Self::Projector,
        Self::Temperature,
        Self::TopP,
        Self::TopK,
        Self::MinP,
        Self::RepeatPenalty,
        Self::PresencePenalty,
        Self::Seed,
    ];
    pub const BASIC: [Self; 5] = [
        Self::Context,
        Self::GpuLayers,
        Self::Temperature,
        Self::TopP,
        Self::TopK,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Context => "Engine / Context size",
            Self::GpuLayers => "Engine / GPU layers",
            Self::Threads => "Engine / Threads",
            Self::Batch => "Engine / Batch size",
            Self::Ubatch => "Engine / Micro-batch",
            Self::Flash => "Engine / Flash attention",
            Self::CacheK => "Engine / KV cache K",
            Self::CacheV => "Engine / KV cache V",
            Self::Projector => "Engine / Multimodal projector",
            Self::Temperature => "Generation / Temperature",
            Self::TopP => "Generation / Top-p",
            Self::TopK => "Generation / Top-k",
            Self::MinP => "Generation / Min-p",
            Self::RepeatPenalty => "Generation / Repeat penalty",
            Self::PresencePenalty => "Generation / Presence penalty",
            Self::Seed => "Generation / Seed",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Self::Flash => "auto, true or false",
            Self::Projector => "auto or absolute path to a projector GGUF",
            Self::CacheK | Self::CacheV => {
                "auto or f16, bf16, q8_0, q4_0, q4_1, q5_0, q5_1, iq4_nl, f32"
            }
            Self::TopP | Self::MinP => "auto or a number from 0 to 1",
            Self::Temperature => "auto or a non-negative number",
            Self::RepeatPenalty => "auto or a positive number",
            Self::PresencePenalty => "auto or a number from -2 to 2",
            Self::GpuLayers | Self::TopK | Self::Seed => "auto or a non-negative integer",
            _ => "auto or a positive integer",
        }
    }
    pub fn value(self, p: &Profile) -> String {
        fn display<T: ToString>(v: Option<T>) -> String {
            v.map(|n| n.to_string()).unwrap_or_else(|| "auto".into())
        }
        let e = &p.engine;
        let g = &p.generation;
        match self {
            Self::Context => display(e.context_size),
            Self::GpuLayers => display(e.gpu_layers),
            Self::Threads => display(e.threads),
            Self::Batch => display(e.batch_size),
            Self::Ubatch => display(e.ubatch_size),
            Self::Flash => display(e.flash_attention),
            Self::CacheK => display(e.cache_type_k.as_deref()),
            Self::CacheV => display(e.cache_type_v.as_deref()),
            Self::Projector => display(e.projector.as_deref()),
            Self::Temperature => display(g.temperature),
            Self::TopP => display(g.top_p),
            Self::TopK => display(g.top_k),
            Self::MinP => display(g.min_p),
            Self::RepeatPenalty => display(g.repeat_penalty),
            Self::PresencePenalty => display(g.presence_penalty),
            Self::Seed => display(g.seed),
        }
    }
    pub fn set(self, p: &mut Profile, value: &str) -> io::Result<()> {
        fn parse<T: std::str::FromStr>(s: &str) -> io::Result<Option<T>> {
            if s.is_empty() || s == "auto" {
                Ok(None)
            } else {
                s.parse()
                    .map(Some)
                    .map_err(|_| invalid("Invalid parameter value"))
            }
        }
        let value = value.trim();
        let mut draft = p.clone();
        let e = &mut draft.engine;
        let g = &mut draft.generation;
        match self {
            Self::Context => e.context_size = parse(value)?,
            Self::GpuLayers => e.gpu_layers = parse(value)?,
            Self::Threads => e.threads = parse(value)?,
            Self::Batch => e.batch_size = parse(value)?,
            Self::Ubatch => e.ubatch_size = parse(value)?,
            Self::Flash => e.flash_attention = parse(value)?,
            Self::CacheK => e.cache_type_k = parse(value)?,
            Self::CacheV => e.cache_type_v = parse(value)?,
            Self::Projector => e.projector = parse(value)?,
            Self::Temperature => g.temperature = parse(value)?,
            Self::TopP => g.top_p = parse(value)?,
            Self::TopK => g.top_k = parse(value)?,
            Self::MinP => g.min_p = parse(value)?,
            Self::RepeatPenalty => g.repeat_penalty = parse(value)?,
            Self::PresencePenalty => g.presence_penalty = parse(value)?,
            Self::Seed => g.seed = parse(value)?,
        }
        draft.validate()?;
        *p = draft;
        Ok(())
    }
}
#[derive(Debug)]
pub struct Editor {
    pub id: PathBuf,
    pub draft: Profile,
    pub advanced: bool,
    pub cursor: usize,
    pub input: Option<String>,
}
impl Editor {
    pub fn new(id: PathBuf, draft: Profile) -> Self {
        Self {
            id,
            draft,
            advanced: false,
            cursor: 0,
            input: None,
        }
    }
    pub fn fields(&self) -> &'static [Field] {
        if self.advanced {
            &Field::ALL
        } else {
            &Field::BASIC
        }
    }
    pub fn selected(&self) -> Field {
        self.fields()[self.cursor]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample(owner: &str, repo: &str, revision: &str) -> super::super::group::Group {
        super::super::group::Group {
            name: "fixture".into(),
            files: vec![super::super::Candidate {
                path: format!("/models/{owner}/{repo}/{revision}/UD-Q4_K_XL/model.gguf").into(),
                bytes: 1,
                version: 3,
                metadata: Default::default(),
            }],
            expected: 1,
            complete: true,
            projector: false,
        }
    }
    #[test]
    fn reviewed_defaults_follow_mode_and_preserve_overrides() {
        for repo in ["Qwen3.8-Flash-Next-GGUF", "Qwen3.8-27B-GGUF"] {
            let group = sample("unsloth", repo, &"a".repeat(40));
            let mut stored = Profile::default();
            stored.engine.context_size = Some(131072);
            stored.generation.temperature = Some(0.42);
            let effective = stored.clone().with_model_defaults(&group, true);
            assert_eq!(effective.engine.context_size, Some(131072));
            assert_eq!(effective.agent_thinking, Some(Thinking::Medium));
            assert_eq!(effective.generation.temperature, Some(0.42));
            assert_eq!(effective.generation.top_p, Some(0.95));
            assert_eq!(stored.generation.top_p, None);
            stored.agent_thinking = Some(Thinking::Off);
            let effective = stored.clone().with_model_defaults(&group, true);
            assert_eq!(effective.generation.top_p, Some(0.80));
            assert_eq!(effective.generation.presence_penalty, Some(1.5));
            stored.backend_defaults = true;
            assert_eq!(stored.clone().with_model_defaults(&group, true), stored);
            stored.agent_thinking = Some(Thinking::High);
            assert!(stored.validate_thinking(&group).is_err());
        }
        let unrelated = sample("someone-else", "Qwen3.8-Flash-Next-GGUF", &"a".repeat(40));
        assert_eq!(
            Profile::default().with_model_defaults(&unrelated, true),
            Profile::default()
        );
    }
    #[test]
    fn valid_preferences_and_auto() {
        let mut p = Profile::default();
        Field::GpuLayers.set(&mut p, "0").unwrap();
        Field::Context.set(&mut p, "32768").unwrap();
        Field::Temperature.set(&mut p, "0.2").unwrap();
        assert_eq!(p.engine.gpu_layers, Some(0));
        assert_eq!(p.engine.context_size, Some(32768));
        Field::Context.set(&mut p, "auto").unwrap();
        assert_eq!(p.engine.context_size, None);
    }
    #[test]
    fn invalid_values_preserve_draft() {
        let mut p = Profile::default();
        for (field, value) in [
            (Field::Context, "0"),
            (Field::TopP, "1.1"),
            (Field::Temperature, "NaN"),
            (Field::RepeatPenalty, "0"),
            (Field::CacheK, "invalid"),
        ] {
            assert!(field.set(&mut p, value).is_err());
            assert_eq!(p, Profile::default());
        }
        Field::Batch.set(&mut p, "512").unwrap();
        assert!(Field::Ubatch.set(&mut p, "1024").is_err());
        assert_eq!(p.engine.ubatch_size, None);
    }
}
