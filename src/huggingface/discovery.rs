//! Background discovery with explicit user-triggered network access.
use super::{File, Page, Ranking, Repository, SearchSpec};
use crate::jobs::Job;
use std::io;
enum ResultSet {
    Repos(Page),
    More(Page),
    Files(Vec<File>),
}
#[derive(Default)]
pub struct Discovery {
    pub query: String,
    pub editing: bool,
    pub cursor: usize,
    pub repos: Vec<Repository>,
    pub files: Vec<File>,
    all_variants: Vec<Variant>,
    pub variants: Vec<Variant>,
    pub ranking: Ranking,
    pub variant_sort: VariantSort,
    pub variant_filter: String,
    pub repo: Option<String>,
    pub status: String,
    pub next: Option<url::Url>,
    job: Option<Job<ResultSet>>,
    spec: SearchSpec,
}
#[derive(Debug, Clone)]
pub struct Variant {
    pub name: String,
    pub files: Vec<File>,
    pub expected: u64,
}
impl Variant {
    pub fn bytes(&self) -> u64 {
        self.files
            .iter()
            .fold(0u64, |sum, f| sum.saturating_add(f.size))
    }
    pub fn complete(&self) -> bool {
        self.files.len() as u64 == self.expected
    }
    pub fn draft(&self) -> bool {
        crate::models::group::mtp_draft(std::path::Path::new(&self.name))
    }
    pub fn projector(&self) -> bool {
        self.name
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .starts_with("mmproj")
    }
    pub fn downloadable(&self) -> bool {
        self.complete() && !self.draft()
    }
    fn role(&self) -> u8 {
        if self.draft() {
            2
        } else if self.projector() {
            1
        } else {
            0
        }
    }
    fn rank(&self) -> u8 {
        let n = self.name.to_ascii_lowercase();
        if n.rsplit('/').next().unwrap_or("").starts_with("mmproj") {
            return 4;
        }
        if n.contains("q4_k_m") {
            0
        } else if n.contains("q5_k_m") {
            1
        } else if n.contains("q4") || n.contains("q5") {
            2
        } else {
            3
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VariantSort {
    #[default]
    Balanced,
    Smallest,
    Largest,
    Name,
}
impl VariantSort {
    pub fn next(self) -> Self {
        match self {
            Self::Balanced => Self::Smallest,
            Self::Smallest => Self::Largest,
            Self::Largest => Self::Name,
            Self::Name => Self::Balanced,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Balanced Q4/Q5 first",
            Self::Smallest => "Smallest first",
            Self::Largest => "Largest first",
            Self::Name => "Name A-Z",
        }
    }
}
fn group_variants(files: Vec<File>) -> Vec<Variant> {
    use std::{collections::BTreeMap, path::Path};
    let mut groups: BTreeMap<(String, String, String, u64), Vec<File>> = BTreeMap::new();
    for f in files {
        let p = Path::new(&f.name);
        let key = if let Some((base, _, total)) = crate::models::group::shard(p) {
            (
                p.with_file_name(format!("{base}.gguf"))
                    .to_string_lossy()
                    .into_owned(),
                total,
            )
        } else {
            (f.name.clone(), 1)
        };
        groups
            .entry((f.repo.clone(), f.revision.clone(), key.0, key.1))
            .or_default()
            .push(f);
    }
    groups
        .into_iter()
        .map(|((_, _, name, expected), mut files)| {
            files.sort_by(|a, b| a.name.cmp(&b.name));
            files.dedup_by(|a, b| a.name == b.name);
            Variant {
                name,
                files,
                expected,
            }
        })
        .collect()
}
impl Discovery {
    pub fn rebuild_variants(&mut self) {
        let query = self.variant_filter.to_ascii_lowercase();
        self.variants = self
            .all_variants
            .iter()
            .filter(|v| v.name.to_ascii_lowercase().contains(&query))
            .cloned()
            .collect();
        self.variants.sort_by(|a, b| {
            let ord = match self.variant_sort {
                VariantSort::Balanced => a
                    .rank()
                    .cmp(&b.rank())
                    .then_with(|| a.bytes().cmp(&b.bytes())),
                VariantSort::Smallest => a.bytes().cmp(&b.bytes()),
                VariantSort::Largest => b.bytes().cmp(&a.bytes()),
                VariantSort::Name => a.name.cmp(&b.name),
            };
            a.role()
                .cmp(&b.role())
                .then(ord)
                .then_with(|| a.name.cmp(&b.name))
        });
        self.files = self.variants.iter().map(|v| v.files[0].clone()).collect();
        self.cursor = 0;
    }
    pub fn selected_bundle(&self) -> Vec<File> {
        self.variants
            .get(self.cursor)
            .filter(|v| v.downloadable())
            .map(|v| v.files.clone())
            .unwrap_or_default()
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    pub fn len(&self) -> usize {
        if self.repo.is_some() {
            self.variants.len()
        } else {
            self.repos.len()
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn move_cursor(&mut self, down: bool) {
        self.cursor = if down {
            (self.cursor + 1).min(self.len().saturating_sub(1))
        } else {
            self.cursor.saturating_sub(1)
        };
    }
    pub fn search(&mut self) -> io::Result<()> {
        if self.busy() {
            return Err(io::Error::other("Discovery request pending"));
        }
        let spec = SearchSpec::parse(&self.query)?;
        if let Some(repo) = spec.repo.clone() {
            let selected = repo.clone();
            self.job = Some(Job::start(move |_| {
                super::files(&repo).map(ResultSet::Files)
            })?);
            self.repo = Some(selected);
            self.files.clear();
            self.all_variants.clear();
            self.variants.clear();
            self.variant_filter = spec.quant.clone().unwrap_or_default();
            self.repos.clear();
            self.next = None;
            self.cursor = 0;
            self.status = "Fetching exact repository…".into();
        } else {
            let url = spec.ranked_url(self.ranking);
            self.job = Some(Job::start(move |_| {
                super::page(&url).map(ResultSet::Repos)
            })?);
            self.status = "Searching Hugging Face…".into();
        }
        self.spec = spec;
        self.editing = false;
        Ok(())
    }
    pub fn more(&mut self) -> io::Result<()> {
        if self.busy() || self.repo.is_some() {
            return Err(io::Error::other("Return to repository search first"));
        }
        let next = self
            .next
            .clone()
            .ok_or_else(|| io::Error::other("No additional page"))?;
        self.job = Some(Job::start(move |_| {
            super::page(&next).map(ResultSet::More)
        })?);
        self.status = "Fetching next page…".into();
        Ok(())
    }
    pub fn open(&mut self) -> io::Result<()> {
        if self.busy() || self.repo.is_some() {
            return Err(io::Error::other("Select a repository"));
        }
        let repo = self
            .repos
            .get(self.cursor)
            .ok_or_else(|| io::Error::other("No repository selected"))?
            .id
            .clone();
        let selected = repo.clone();
        self.job = Some(Job::start(move |_| {
            super::files(&repo).map(ResultSet::Files)
        })?);
        self.repo = Some(selected);
        self.files.clear();
        self.all_variants.clear();
        self.variants.clear();
        self.variant_filter = self.spec.quant.clone().unwrap_or_default();
        self.cursor = 0;
        self.status = "Fetching revision and files (access may require a read token)…".into();
        Ok(())
    }
    pub fn back(&mut self) {
        if !self.busy() {
            self.repo = None;
            self.files.clear();
            self.all_variants.clear();
            self.variants.clear();
            self.variant_filter.clear();
            self.cursor = 0;
        }
    }
    pub fn shutdown(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel();
            job.finish();
        }
    }
    pub fn tick(&mut self) {
        if let Some(job) = self.job.as_mut() {
            if let Some(result) = job.poll() {
                if let Some(job) = self.job.take() {
                    job.finish();
                }
                match result {
                    Ok(ResultSet::Repos(page)) => {
                        let repos = page.repos;
                        self.next = page.next;
                        self.status = format!("{} repositories", repos.len());
                        self.repos = repos;
                        self.repo = None;
                        self.cursor = 0;
                    }
                    Ok(ResultSet::More(page)) => {
                        let before = self.repos.len();
                        let mut seen: std::collections::HashSet<_> =
                            self.repos.iter().map(|r| r.id.clone()).collect();
                        self.repos
                            .extend(page.repos.into_iter().filter(|r| seen.insert(r.id.clone())));
                        self.next = page.next.filter(|next| Some(next) != self.next.as_ref());
                        self.status = format!(
                            "{} repositories (+{})",
                            self.repos.len(),
                            self.repos.len() - before
                        );
                    }
                    Ok(ResultSet::Files(files)) => {
                        self.all_variants = group_variants(files);
                        self.rebuild_variants();
                        self.status =
                            format!("{} variants · Enter to download", self.variants.len());
                    }
                    Err(e) => {
                        self.status = e.to_string();
                        self.repo = None;
                        self.cursor = 0;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn file(name: &str, size: u64) -> File {
        File {
            repo: "owner/repo".into(),
            revision: "a".repeat(40),
            name: name.into(),
            size,
            sha256: None,
        }
    }
    #[test]
    fn grouped_variants_filter_sort_and_bundle() {
        let mut d = Discovery {
            repo: Some("owner/repo".into()),
            all_variants: group_variants(vec![
                file("A-Q4_K_M-00001-of-00002.gguf", 3),
                file("A-Q4_K_M-00002-of-00002.gguf", 4),
                file("A-Q2_K.gguf", 2),
                file("B-Q8-00001-of-00002.gguf", 9),
            ]),
            ..Default::default()
        };
        d.rebuild_variants();
        assert_eq!(d.len(), 3);
        assert_eq!(d.selected_bundle().len(), 2);
        assert_eq!(d.variants[0].bytes(), 7);
        d.variant_sort = VariantSort::Smallest;
        d.rebuild_variants();
        assert_eq!(d.selected_bundle()[0].size, 2);
        d.variant_filter = "q8".into();
        d.rebuild_variants();
        assert_eq!(d.len(), 1);
        assert!(d.selected_bundle().is_empty());
    }
    #[test]
    fn auxiliary_drafts_never_rank_or_download_as_standalone_models() {
        let mut d = Discovery {
            repo: Some("owner/repo".into()),
            all_variants: group_variants(vec![
                file("MTP/mtp-model-Q4_K_M.gguf", 2),
                file("model-Q4_K_M.gguf", 100),
                file("mmproj-F16.gguf", 1),
            ]),
            ..Default::default()
        };
        for sort in [
            VariantSort::Balanced,
            VariantSort::Smallest,
            VariantSort::Largest,
            VariantSort::Name,
        ] {
            d.variant_sort = sort;
            d.rebuild_variants();
            assert_eq!(d.variants[0].name, "model-Q4_K_M.gguf");
            assert!(d.variants[1].projector());
            assert!(d.variants[2].draft());
            d.cursor = 2;
            assert!(d.selected_bundle().is_empty());
        }
    }
    #[test]
    fn ranks_use_real_hub_parameters() {
        let spec = SearchSpec::parse("qwen author:unsloth").unwrap();
        for (rank, expected) in [
            (Ranking::Downloads, "downloads"),
            (Ranking::Trending, "trendingScore"),
            (Ranking::Newest, "createdAt"),
            (Ranking::Updated, "lastModified"),
            (Ranking::Likes, "likes"),
        ] {
            let url = spec.ranked_url(rank);
            assert!(url.query_pairs().any(|(k, v)| k == "sort" && v == expected));
            assert!(url
                .query_pairs()
                .any(|(k, v)| k == "author" && v == "unsloth"));
        }
    }
}
