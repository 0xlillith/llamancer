//! Background discovery with explicit user-triggered network access.
use super::{File, Page, Repository, SearchSpec};
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
    pub repo: Option<String>,
    pub status: String,
    pub next: Option<url::Url>,
    job: Option<Job<ResultSet>>,
    spec: SearchSpec,
}
impl Discovery {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    pub fn len(&self) -> usize {
        if self.repo.is_some() {
            self.files.len()
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
            let query_spec = spec.clone();
            let selected = repo.clone();
            self.job = Some(Job::start(move |_| {
                super::files(&repo).map(|files| ResultSet::Files(query_spec.filter_files(files)))
            })?);
            self.repo = Some(selected);
            self.files.clear();
            self.repos.clear();
            self.next = None;
            self.cursor = 0;
            self.status = "Fetching exact repository…".into();
        } else {
            let url = spec.url();
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
        let spec = self.spec.clone();
        self.job = Some(Job::start(move |_| {
            super::files(&repo).map(|files| ResultSet::Files(spec.filter_files(files)))
        })?);
        self.repo = Some(selected);
        self.files.clear();
        self.cursor = 0;
        self.status = "Fetching revision and files (access may require a read token)…".into();
        Ok(())
    }
    pub fn back(&mut self) {
        if !self.busy() {
            self.repo = None;
            self.files.clear();
            self.cursor = 0;
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
                        self.status = format!("{} GGUF files with known sizes", files.len());
                        self.files = files;
                        self.cursor = 0;
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
