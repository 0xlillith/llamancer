//! Hugging Face Hub discovery; authentication is limited to the exact HTTPS Hub origin.
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
    #[serde(default)]
    pub private: bool,
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
fn token_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let absolute = |key: &str| {
        std::env::var_os(key)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    absolute("HF_TOKEN_PATH")
        .or_else(|| absolute("HF_HOME").map(|p| p.join("token")))
        .or_else(|| absolute("XDG_CACHE_HOME").map(|p| p.join("huggingface/token")))
        .or_else(|| absolute("HOME").map(|p| p.join(".cache/huggingface/token")))
}
fn valid_token(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= 4096 && value.bytes().all(|b| b.is_ascii_graphic()))
        .then(|| value.to_owned())
}
fn hub_token() -> Option<String> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Ok(value) = std::env::var("HF_TOKEN") {
        return valid_token(&value);
    }
    let path = token_path()?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    file.take(4097).read_to_string(&mut text).ok()?;
    valid_token(&text)
}
fn hub_origin(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("huggingface.co")
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}
pub fn request(http: &ureq::Agent, url: &Url) -> ureq::Request {
    let req = http.get(url.as_str());
    if hub_origin(url) {
        if let Some(token) = hub_token() {
            return req.set("Authorization", &format!("Bearer {token}"));
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
            match code {401|403=>format!("Hugging Face HTTP {code}: sign in with a read token and accept gated access on the repository page"),404=>"Hugging Face HTTP 404: repository or revision unavailable (private repositories require read access)".into(),_=>format!("Hugging Face HTTP {code}")},
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
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SearchSpec {
    pub query: String,
    pub license: Option<String>,
    pub quant: Option<String>,
    pub repo: Option<String>,
}
impl SearchSpec {
    pub fn parse(input: &str) -> io::Result<Self> {
        let mut spec = Self::default();
        let mut terms = Vec::new();
        for word in input.split_whitespace() {
            if let Some(value) = word.strip_prefix("license:") {
                if value.is_empty()
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    || spec.license.is_some()
                {
                    return Err(io::Error::other("Use one license:identifier filter"));
                }
                spec.license = Some(value.to_ascii_lowercase());
            } else if let Some(value) = word.strip_prefix("quant:") {
                if value.is_empty()
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                    || spec.quant.is_some()
                {
                    return Err(io::Error::other("Use one quant:Q4_K_M filter"));
                }
                spec.quant = Some(value.to_ascii_lowercase());
            } else if let Some(value) = word.strip_prefix('@') {
                if value.split('/').count() != 2
                    || !value.split('/').all(valid_segment)
                    || spec.repo.is_some()
                {
                    return Err(io::Error::other(
                        "Use @owner/repository for an exact repository",
                    ));
                }
                spec.repo = Some(value.into());
            } else {
                terms.push(word);
            }
        }
        spec.query = terms.join(" ");
        if spec.repo.is_some() && (!spec.query.is_empty() || spec.license.is_some()) {
            return Err(io::Error::other(
                "Exact repository searches support only quant filters",
            ));
        }
        Ok(spec)
    }
    pub fn url(&self) -> Url {
        let mut url = Url::parse("https://huggingface.co/api/models").expect("constant URL");
        url.query_pairs_mut()
            .append_pair("search", &self.query)
            .append_pair("filter", "gguf")
            .append_pair("sort", "downloads")
            .append_pair("direction", "-1")
            .append_pair("limit", "50");
        if let Some(license) = &self.license {
            url.query_pairs_mut()
                .append_pair("filter", &format!("license:{license}"));
        }
        url
    }
    pub fn filter_files(&self, files: Vec<File>) -> Vec<File> {
        let Some(quant) = &self.quant else {
            return files;
        };
        files
            .into_iter()
            .filter(|f| {
                let name = std::path::Path::new(&f.name)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_ascii_lowercase();
                // Keep projector candidates visible; all matching conventional shards share the same quantization stem.
                name.starts_with("mmproj") || name.contains(quant)
            })
            .collect()
    }
}
pub fn search(query: &str) -> io::Result<Page> {
    page(&SearchSpec::parse(query)?.url())
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
                && f.repo == selected.repo
                && f.revision == selected.revision
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
    fn filters_and_private_repo_addressing() {
        let spec = SearchSpec::parse("qwen license:apache-2.0 quant:Q4_K_M").unwrap();
        assert_eq!(spec.query, "qwen");
        assert!(spec
            .url()
            .query_pairs()
            .any(|(k, v)| k == "filter" && v == "license:apache-2.0"));
        let exact = SearchSpec::parse("@owner/private quant:Q4_K_M").unwrap();
        assert_eq!(exact.repo.as_deref(), Some("owner/private"));
        let filtered = exact.filter_files(vec![
            file("x-Q4_K_M-00001-of-00002.gguf"),
            file("x-Q4_K_M-00002-of-00002.gguf"),
            file("x-Q8_0.gguf"),
            file("mmproj-F16.gguf"),
        ]);
        assert_eq!(filtered.len(), 3);
        assert_eq!(bundle(&filtered, 0).len(), 2);
        assert!(SearchSpec::parse("@owner/../bad").is_err());
        assert!(!hub_origin(
            &Url::parse("https://huggingface.co:444/x").unwrap()
        ));
        assert!(!hub_origin(
            &Url::parse("https://user@huggingface.co/x").unwrap()
        ));
        assert!(!hub_origin(&Url::parse("https://example.org/x").unwrap()));
        assert!(valid_token("foo\nbar").is_none());
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
