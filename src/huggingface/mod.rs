//! Hugging Face Hub discovery; only explicitly requested public endpoints are contacted.
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read},
    time::Duration,
};
use url::Url;

#[derive(Debug, Clone, Deserialize)]
pub struct Repository {
    pub id: String,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub gated: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct File {
    pub repo: String,
    pub revision: String,
    pub name: String,
    pub size: u64,
    pub sha256: Option<String>,
}
#[derive(Debug, Deserialize)]
struct Detail {
    sha: String,
    #[serde(default)]
    siblings: Vec<Sibling>,
}
#[derive(Debug, Deserialize)]
struct Sibling {
    rfilename: String,
    size: Option<u64>,
    lfs: Option<Lfs>,
}
#[derive(Debug, Deserialize)]
struct Lfs {
    sha256: Option<String>,
    size: Option<u64>,
}
pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(20))
        .timeout_write(Duration::from_secs(10))
        .redirects(0)
        .build()
}
pub fn request(http: &ureq::Agent, url: &Url) -> ureq::Request {
    let req = http.get(url.as_str());
    if url.scheme() == "https"
        && url.host_str() == Some("huggingface.co")
        && url.port_or_known_default() == Some(443)
    {
        if let Ok(token) = std::env::var("HF_TOKEN") {
            if !token.is_empty() {
                return req.set("Authorization", &format!("Bearer {token}"));
            }
        }
    }
    req
}
pub fn error(e: ureq::Error) -> io::Error {
    match e {
        ureq::Error::Status(code, _) => io::Error::new(
            if matches!(code, 408 | 429 | 500 | 502 | 503 | 504) {
                io::ErrorKind::WouldBlock
            } else {
                io::ErrorKind::Other
            },
            format!("Hugging Face HTTP {code}"),
        ),

        ureq::Error::Transport(_) => io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "Hugging Face connection failed",
        ),
    }
}
fn response_json<T: serde::de::DeserializeOwned>(response: ureq::Response) -> io::Result<T> {
    let mut data = Vec::new();
    response
        .into_reader()
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut data)?;
    if data.len() > 8 * 1024 * 1024 {
        return Err(io::Error::other("Hub response exceeds limit"));
    }
    serde_json::from_slice(&data).map_err(|_| io::Error::other("Invalid Hugging Face response"))
}
fn get<T: serde::de::DeserializeOwned>(url: &Url) -> io::Result<T> {
    response_json(request(&agent(), url).call().map_err(error)?)
}
#[derive(Debug)]
pub struct Page {
    pub repos: Vec<Repository>,
    pub next: Option<Url>,
}
fn next_page(link: &str) -> Option<Url> {
    link.split(',').find_map(|part| {
        let (address, params) = part.trim().split_once('>')?;
        if !params.split(';').any(|p| p.trim() == "rel=\"next\"") {
            return None;
        }
        let url = Url::parse(address.strip_prefix('<')?).ok()?;
        (url.scheme() == "https"
            && url.host_str() == Some("huggingface.co")
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/api/models"
            && url.fragment().is_none())
        .then_some(url)
    })
}
pub fn page(url: &Url) -> io::Result<Page> {
    if url.scheme() != "https"
        || url.host_str() != Some("huggingface.co")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/api/models"
        || url.fragment().is_some()
    {
        return Err(io::Error::other("Invalid Hub pagination URL"));
    }
    let response = request(&agent(), url).call().map_err(error)?;
    let next = response.header("Link").and_then(next_page);
    let repos = response_json(response)?;
    Ok(Page { repos, next })
}
pub fn valid_segment(s: &str) -> bool {
    !s.is_empty()
        && !matches!(s, "." | "..")
        && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
}
pub fn validate(file: &File) -> io::Result<()> {
    let parts: Vec<_> = file.repo.split('/').collect();
    if parts.len() != 2
        || !parts.iter().all(|p| valid_segment(p))
        || !file.name.split('/').all(valid_segment)
        || !file.name.to_ascii_lowercase().ends_with(".gguf")
        || file.size == 0
        || file.revision.len() != 40
        || !file.revision.bytes().all(|b| b.is_ascii_hexdigit())
        || file
            .sha256
            .as_ref()
            .is_some_and(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(io::Error::other("Invalid or unpinned model file"));
    }
    Ok(())
}
pub fn search(query: &str) -> io::Result<Page> {
    let mut url = Url::parse("https://huggingface.co/api/models").expect("constant URL");
    url.query_pairs_mut()
        .append_pair("search", query)
        .append_pair("filter", "gguf")
        .append_pair("sort", "downloads")
        .append_pair("direction", "-1")
        .append_pair("limit", "50");
    page(&url)
}
pub fn files(repo: &str) -> io::Result<Vec<File>> {
    let parts: Vec<_> = repo.split('/').collect();
    if parts.len() != 2 || !parts.iter().all(|p| valid_segment(p)) {
        return Err(io::Error::other("Invalid repository ID"));
    }
    let mut url = Url::parse("https://huggingface.co/api/models/").expect("constant URL");
    url.path_segments_mut()
        .expect("base URL")
        .pop_if_empty()
        .extend(parts);
    url.query_pairs_mut().append_pair("blobs", "true");
    let detail: Detail = get(&url)?;
    let mut files = Vec::new();
    for s in detail.siblings {
        let size = s.size.or_else(|| s.lfs.as_ref().and_then(|l| l.size));
        if let Some(size) = size {
            let file = File {
                repo: repo.into(),
                revision: detail.sha.clone(),
                name: s.rfilename,
                size,
                sha256: s.lfs.and_then(|l| l.sha256),
            };
            if validate(&file).is_ok() {
                files.push(file);
            }
        }
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}
pub fn resolve(file: &File) -> io::Result<Url> {
    validate(file)?;
    let mut url = Url::parse("https://huggingface.co/").expect("constant URL");
    url.path_segments_mut()
        .expect("base URL")
        .pop_if_empty()
        .extend(file.repo.split('/'))
        .push("resolve")
        .push(&file.revision)
        .extend(file.name.split('/'));
    Ok(url)
}
/// Select all conventional shards of a selected file within the exact same directory.
pub fn bundle(files: &[File], index: usize) -> Vec<File> {
    let Some(selected) = files.get(index) else {
        return Vec::new();
    };
    let path = std::path::Path::new(&selected.name);
    let Some((base, _, total)) = crate::models::group::shard(path) else {
        return vec![selected.clone()];
    };
    let mut result: Vec<_> = files
        .iter()
        .filter(|f| {
            let p = std::path::Path::new(&f.name);
            p.parent() == path.parent()
                && crate::models::group::shard(p).is_some_and(|(b, _, t)| b == base && t == total)
        })
        .cloned()
        .collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result.dedup_by(|a, b| a.name == b.name);
    if result.len() as u64 != total {
        return Vec::new();
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    fn file(name: &str) -> File {
        File {
            repo: "owner/repo".into(),
            revision: "a".repeat(40),
            name: name.into(),
            size: 100,
            sha256: None,
        }
    }
    #[test]
    fn pagination_never_leaves_hub() {
        assert!(
            next_page(r#"<https://huggingface.co/api/models?cursor=abc>; rel="next""#).is_some()
        );
        for url in [
            "https://example.com/api/models",
            "https://huggingface.co:444/api/models",
            "https://huggingface.co/api/users",
            "http://huggingface.co/api/models",
        ] {
            assert!(next_page(&format!(r#"<{url}>; rel="next""#)).is_none());
        }
    }
    #[test]
    fn paths_and_bundles() {
        assert!(resolve(&file("../evil.gguf")).is_err());
        assert!(resolve(&file("a/model Q4.gguf"))
            .unwrap()
            .as_str()
            .contains("model%20Q4.gguf"));
        let files = vec![
            file("a/m-00001-of-00002.gguf"),
            file("a/m-00002-of-00002.gguf"),
            file("b/m-00001-of-00002.gguf"),
        ];
        assert_eq!(bundle(&files, 0).len(), 2);
        assert!(bundle(&files, 2).is_empty());
    }
}

pub mod discovery;
