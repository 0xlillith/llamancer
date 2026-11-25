//! Conservative estimates for conventional attention models, not a VRAM capacity promise.
use super::{group::Group, profile::Profile};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Estimate {
    pub weights: u64,
    pub kv: u64,
    pub context: u64,
}
fn bytes_for(elements: u64, kind: Option<&str>) -> Option<u64> {
    let (block, bytes) = match kind.unwrap_or("f16") {
        "f32" => (1, 4),
        "f16" | "bf16" => (1, 2),
        "q8_0" => (32, 34),
        "q4_0" | "iq4_nl" => (32, 18),
        "q4_1" => (32, 20),
        "q5_0" => (32, 22),
        "q5_1" => (32, 24),
        _ => return None,
    };
    elements
        .checked_add(block - 1)?
        .checked_div(block)?
        .checked_mul(bytes)
}
pub fn estimate(group: &Group, profile: &Profile) -> Option<Estimate> {
    let m = &group.files.first()?.metadata;
    // Exclude hybrid/recurrent, MLA and unfamiliar architectures rather than guessing.
    if !group.complete
        || group.projector
        || !matches!(m.architecture.as_deref(), Some("llama" | "qwen2" | "qwen3"))
    {
        return None;
    }
    let layers = m.layers.filter(|&n| n > 0)?;
    let heads = m.heads.filter(|&n| n > 0)?;
    let kv_heads = m.kv_heads.filter(|&n| n > 0 && n <= heads)?;
    let embedding = m.embedding.filter(|&n| n > 0 && n % heads == 0)?;
    let key = m.key_length.unwrap_or(embedding / heads);
    let value = m.value_length.unwrap_or(embedding / heads);
    let context = profile
        .engine
        .context_size
        .map(u64::from)
        .or(m.context)
        .filter(|&n| n > 0)?;
    let rows = layers.checked_mul(context)?.checked_mul(kv_heads)?;
    let k = bytes_for(key, profile.engine.cache_type_k.as_deref())?.checked_mul(rows)?;
    let v = bytes_for(value, profile.engine.cache_type_v.as_deref())?.checked_mul(rows)?;
    Some(Estimate {
        weights: group.bytes(),
        kv: k.checked_add(v)?,
        context,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{gguf::Metadata, Candidate};
    #[test]
    fn gqa_and_quantized_cache() {
        let mut group = Group {
            name: "test".into(),
            files: vec![Candidate {
                path: "/tmp/test.gguf".into(),
                bytes: 100,
                version: 3,
                metadata: Metadata {
                    architecture: Some("llama".into()),
                    layers: Some(32),
                    heads: Some(32),
                    kv_heads: Some(8),
                    embedding: Some(4096),
                    context: Some(4096),
                    ..Default::default()
                },
            }],
            expected: 1,
            complete: true,
            projector: false,
        };
        let mut p = Profile::default();
        assert_eq!(estimate(&group, &p).unwrap().kv, 536870912);
        p.engine.context_size = Some(2048);
        p.engine.cache_type_k = Some("q8_0".into());
        p.engine.cache_type_v = Some("q8_0".into());
        assert_eq!(estimate(&group, &p).unwrap().kv, 142606336);
        group.files[0].metadata.architecture = Some("unknown".into());
        assert!(estimate(&group, &p).is_none());
    }
}
