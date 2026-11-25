//! Library presentation, stable path identities and filtering.
use super::group::Group;
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Name,
    SizeAscending,
    SizeDescending,
}
impl Sort {
    pub fn next(self) -> Self {
        match self {
            Self::Name => Self::SizeAscending,
            Self::SizeAscending => Self::SizeDescending,
            Self::SizeDescending => Self::Name,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::SizeAscending => "Size ↑",
            Self::SizeDescending => "Size ↓",
        }
    }
}
/// GGUF general.file_type follows llama_ftype, not the ggml tensor type enum.
/// Unknown future identifiers remain visible as numeric IDs.
pub fn quantization(id: Option<u64>) -> String {
    match id {
        Some(n) => match n {
            0 => "F32",
            1 => "F16",
            2 => "Q4_0",
            3 => "Q4_1",
            7 => "Q8_0",
            8 => "Q5_0",
            9 => "Q5_1",
            10 => "Q2_K",
            11 => "Q3_K_S",
            12 => "Q3_K_M",
            13 => "Q3_K_L",
            14 => "Q4_K_S",
            15 => "Q4_K_M",
            16 => "Q5_K_S",
            17 => "Q5_K_M",
            18 => "Q6_K",
            19 => "IQ2_XXS",
            20 => "IQ2_XS",
            21 => "Q2_K_S",
            22 => "IQ3_XS",
            23 => "IQ3_XXS",
            24 => "IQ1_S",
            25 => "IQ4_NL",
            26 => "IQ3_S",
            27 => "IQ3_M",
            28 => "IQ2_S",
            29 => "IQ2_M",
            30 => "IQ4_XS",
            31 => "IQ1_M",
            32 => "BF16",
            36 => "TQ1_0",
            37 => "TQ2_0",
            38 => "MXFP4_MOE",
            _ => return format!("Type {n}"),
        }
        .into(),
        None => "Unknown".into(),
    }
}
pub fn identity(group: &Group) -> PathBuf {
    let path = &group.files[0].path;
    if let Some((base, _, total)) = super::group::shard(path) {
        path.with_file_name(format!("{base}-00001-of-{total:05}.gguf"))
    } else {
        path.clone()
    }
}
pub fn visible(
    groups: &[Group],
    query: &str,
    favorites: &BTreeSet<PathBuf>,
    only_favorites: bool,
    sort: Sort,
) -> Vec<usize> {
    let tokens: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut indices: Vec<_> = groups
        .iter()
        .enumerate()
        .filter_map(|(i, g)| {
            if only_favorites && !favorites.contains(&identity(g)) {
                return None;
            }
            let m = &g.files[0].metadata;
            let haystack = format!(
                "{} {} {} {} {}",
                g.name,
                m.name.as_deref().unwrap_or(""),
                m.architecture.as_deref().unwrap_or(""),
                quantization(m.file_type),
                g.files[0].path.display()
            )
            .to_lowercase();
            tokens.iter().all(|t| haystack.contains(t)).then_some(i)
        })
        .collect();
    indices.sort_by(|&a, &b| {
        let (a, b) = (&groups[a], &groups[b]);
        let order = match sort {
            Sort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Sort::SizeAscending => a.bytes().cmp(&b.bytes()),
            Sort::SizeDescending => b.bytes().cmp(&a.bytes()),
        };
        order.then_with(|| identity(a).cmp(&identity(b)))
    });
    indices
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{gguf::Metadata, Candidate};
    fn sample(name: &str, bytes: u64, kind: u64) -> Group {
        Group {
            name: name.into(),
            files: vec![Candidate {
                path: format!("/models/{name}.gguf").into(),
                bytes,
                version: 3,
                metadata: Metadata {
                    architecture: Some("qwen".into()),
                    file_type: Some(kind),
                    ..Default::default()
                },
            }],
            expected: 1,
            complete: true,
            projector: false,
        }
    }
    #[test]
    fn filters_and_sorts() {
        let groups = vec![sample("Z", 100, 15), sample("A", 200, 7)];
        let favorites = BTreeSet::from([identity(&groups[0])]);
        assert_eq!(
            visible(&groups, "qwen q4_k_m", &favorites, false, Sort::Name),
            vec![0]
        );
        assert_eq!(
            visible(&groups, "", &favorites, false, Sort::SizeDescending),
            vec![1, 0]
        );
        assert_eq!(visible(&groups, "", &favorites, true, Sort::Name), vec![0]);
        assert_eq!(quantization(Some(999)), "Type 999");
    }
}
