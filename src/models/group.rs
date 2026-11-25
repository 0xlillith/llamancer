//! Conservative shard grouping. Completeness means all expected shard numbers are present, not loadability.
use super::Candidate;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub files: Vec<Candidate>,
    pub expected: u64,
    pub complete: bool,
    pub projector: bool,
}
impl Group {
    pub fn bytes(&self) -> u64 {
        self.files
            .iter()
            .fold(0u64, |sum, f| sum.saturating_add(f.bytes))
    }
    pub fn elements(&self) -> u64 {
        self.files
            .iter()
            .fold(0u64, |sum, f| sum.saturating_add(f.metadata.elements))
    }
}
pub(super) fn shard(path: &Path) -> Option<(&str, u64, u64)> {
    let stem = path.file_stem()?.to_str()?;
    let (left, total) = stem.rsplit_once("-of-")?;
    let (base, index) = left.rsplit_once('-')?;
    if index.len() != 5
        || total.len() != 5
        || !index
            .bytes()
            .chain(total.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let index = index.parse().ok()?;
    let total = total.parse().ok()?;
    if index == 0 || total < 2 || index > total {
        return None;
    }
    Some((base, index, total))
}
pub fn group(files: Vec<Candidate>) -> Vec<Group> {
    let mut groups = Vec::new();
    let mut shards: BTreeMap<(PathBuf, String, u64), Vec<(u64, Candidate)>> = BTreeMap::new();
    for f in files {
        if let Some((base, index, total)) = shard(&f.path) {
            let key = (
                f.path.parent().unwrap_or(Path::new("")).to_path_buf(),
                base.to_owned(),
                total,
            );
            shards.entry(key).or_default().push((index, f));
        } else {
            let name = f
                .path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let expected = f.metadata.split_count.unwrap_or(1);
            let complete = expected == 1 && f.metadata.split_no.unwrap_or(0) == 0;
            let projector = name.to_ascii_lowercase().starts_with("mmproj");
            groups.push(Group {
                name,
                files: vec![f],
                expected,
                complete,
                projector,
            });
        }
    }
    for ((_, name, expected), mut entries) in shards {
        entries.sort_by_key(|(i, _)| *i);
        let indices: BTreeSet<_> = entries.iter().map(|(i, _)| *i).collect();
        let consistent = entries.iter().all(|(i, f)| {
            f.metadata.split_count == Some(expected) && f.metadata.split_no == Some(i - 1)
        });
        let complete =
            consistent && indices.len() as u64 == expected && entries.len() == indices.len();
        let projector = name.to_ascii_lowercase().starts_with("mmproj");
        groups.push(Group {
            name,
            files: entries.into_iter().map(|(_, f)| f).collect(),
            expected,
            complete,
            projector,
        });
    }
    groups.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.files[0].path.cmp(&b.files[0].path))
    });
    groups
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::gguf::Metadata;
    fn f(i: u64) -> Candidate {
        Candidate {
            path: format!("/models/test-{i:05}-of-00002.gguf").into(),
            bytes: 100,
            version: 3,
            metadata: Metadata {
                split_no: Some(i - 1),
                split_count: Some(2),
                ..Default::default()
            },
        }
    }
    #[test]
    fn complete_and_missing() {
        let all = group(vec![f(2), f(1)]);
        assert_eq!(all.len(), 1);
        assert!(all[0].complete);
        assert_eq!(all[0].bytes(), 200);
        assert!(!group(vec![f(2)])[0].complete);
    }
    #[test]
    fn mismatch() {
        let mut bad = f(2);
        bad.metadata.split_no = Some(0);
        assert!(!group(vec![f(1), bad])[0].complete);
    }
}
