//! Background discovery with explicit user-triggered network access.
use super::{File, Repository};
use crate::jobs::Job;
use std::io;
enum ResultSet {
    Repos(Vec<Repository>),
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
    job: Option<Job<ResultSet>>,
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
        let query = self.query.clone();
        self.job = Some(Job::start(move |_| {
            super::search(&query).map(ResultSet::Repos)
        })?);
        self.status = "Searching Hugging Face…".into();
        self.editing = false;
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
        self.cursor = 0;
        self.status = "Fetching revision and files…".into();
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
                    Ok(ResultSet::Repos(repos)) => {
                        self.status = format!("{} repositories", repos.len());
                        self.repos = repos;
                        self.repo = None;
                        self.cursor = 0;
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
