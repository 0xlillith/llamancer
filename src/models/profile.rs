//! Per-model inference preferences. Backend capability validation is separate.
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub engine: Engine,
    pub generation: Generation,
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
}
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Generation {
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    pub min_p: Option<f32>,
    pub repeat_penalty: Option<f32>,
    pub seed: Option<u32>,
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
    Temperature,
    TopP,
    TopK,
    MinP,
    RepeatPenalty,
    Seed,
}
impl Field {
    pub const ALL: [Self; 14] = [
        Self::Context,
        Self::GpuLayers,
        Self::Threads,
        Self::Batch,
        Self::Ubatch,
        Self::Flash,
        Self::CacheK,
        Self::CacheV,
        Self::Temperature,
        Self::TopP,
        Self::TopK,
        Self::MinP,
        Self::RepeatPenalty,
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
            Self::Temperature => "Generation / Temperature",
            Self::TopP => "Generation / Top-p",
            Self::TopK => "Generation / Top-k",
            Self::MinP => "Generation / Min-p",
            Self::RepeatPenalty => "Generation / Repeat penalty",
            Self::Seed => "Generation / Seed",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Self::Flash => "auto, true or false",
            Self::CacheK | Self::CacheV => {
                "auto or f16, bf16, q8_0, q4_0, q4_1, q5_0, q5_1, iq4_nl, f32"
            }
            Self::TopP | Self::MinP => "auto or a number from 0 to 1",
            Self::Temperature => "auto or a non-negative number",
            Self::RepeatPenalty => "auto or a positive number",
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
            Self::Temperature => display(g.temperature),
            Self::TopP => display(g.top_p),
            Self::TopK => display(g.top_k),
            Self::MinP => display(g.min_p),
            Self::RepeatPenalty => display(g.repeat_penalty),
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
            Self::Temperature => g.temperature = parse(value)?,
            Self::TopP => g.top_p = parse(value)?,
            Self::TopK => g.top_k = parse(value)?,
            Self::MinP => g.min_p = parse(value)?,
            Self::RepeatPenalty => g.repeat_penalty = parse(value)?,
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
