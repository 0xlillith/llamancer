use serde::Deserialize;
use std::{
    io::{self, Read},
    time::Duration,
};
use url::Url;

#[derive(Debug, Clone, Deserialize)]
pub struct Model {
    pub id: String,
    #[serde(default)]
    pub status: Option<serde_json::Value>,
    #[serde(default)]
    pub meta: Option<serde_json::Value>,
}
impl Model {
    pub fn context_limit(&self) -> Option<u64> {
        self.meta
            .as_ref()
            .and_then(|meta| meta.get("n_ctx"))
            .and_then(|v| v.as_u64())
            .filter(|&n| n >= 128)
    }
    pub fn state(&self) -> &str {
        self.status
            .as_ref()
            .and_then(|v| {
                v.as_str()
                    .or_else(|| v.get("value").and_then(|v| v.as_str()))
            })
            .unwrap_or("available")
    }
}
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub router: bool,
    pub models: Vec<Model>,
}
#[derive(Clone)]
pub struct Client {
    base: Url,
    http: ureq::Agent,
    key: Option<std::sync::Arc<super::auth::Key>>,
}
pub fn endpoint(value: &str) -> io::Result<Url> {
    let mut url = Url::parse(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid endpoint"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/" | "/v1" | "/v1/")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Expected HTTP(S) server root without credentials",
        ));
    }
    url.set_path("/");
    Ok(url)
}
fn http_error(e: ureq::Error) -> io::Error {
    match e {
        ureq::Error::Status(code, _) => io::Error::other(format!("HTTP {code}")),
        ureq::Error::Transport(_) => io::Error::other("Server connection failed"),
    }
}
fn json(response: ureq::Response) -> io::Result<serde_json::Value> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(io::Error::other("API response too large"));
    }
    serde_json::from_slice(&bytes).map_err(|_| io::Error::other("Invalid API response"))
}
impl Client {
    pub fn new(value: &str) -> io::Result<Self> {
        Ok(Self {
            base: endpoint(value)?,
            key: None,
            http: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(5))
                .redirects(0)
                .build(),
        })
    }
    pub fn from_config(config: &crate::config::Config) -> io::Result<Self> {
        let mut client = Self::new(&config.endpoint)?;
        client.key = super::auth::Key::for_endpoint(config.api_key_file.as_deref(), &client.base)?
            .map(std::sync::Arc::new);
        Ok(client)
    }
    fn request(&self, method: &str, path: &str) -> io::Result<ureq::Request> {
        let request = self.http.request(
            method,
            self.base.join(path).map_err(io::Error::other)?.as_str(),
        );
        Ok(match &self.key {
            Some(key) => request.set("Authorization", &format!("Bearer {}", key.expose())),
            None => request,
        })
    }
    fn get(&self, path: &str) -> io::Result<serde_json::Value> {
        json(self.request("GET", path)?.call().map_err(http_error)?)
    }
    pub fn snapshot(&self) -> io::Result<Snapshot> {
        self.get("health")?;
        let router = self
            .get("props")
            .ok()
            .and_then(|v| {
                v.get("role")
                    .and_then(|v| v.as_str())
                    .map(|s| s == "router")
            })
            .unwrap_or(false);
        let data = self.get(if router { "models" } else { "v1/models" })?;
        let list = data.get("data").cloned().unwrap_or(data);
        let models =
            serde_json::from_value(list).map_err(|_| io::Error::other("Invalid model list"))?;
        Ok(Snapshot { router, models })
    }
    pub fn set_loaded(&self, model: &str, load: bool) -> io::Result<()> {
        let route = if load { "models/load" } else { "models/unload" };
        let result = json(
            self.request("POST", route)?
                .send_json(serde_json::json!({"model":model}))
                .map_err(http_error)?,
        )?;
        if result.get("success").and_then(|v| v.as_bool()) == Some(true) {
            Ok(())
        } else {
            Err(io::Error::other("Router did not confirm operation"))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_validation() {
        assert_eq!(
            endpoint("http://localhost:8080/v1").unwrap().as_str(),
            "http://localhost:8080/"
        );
        for invalid in [
            "file:///tmp/x",
            "http://user:pass@localhost",
            "http://localhost/?token=x",
        ] {
            assert!(endpoint(invalid).is_err());
        }
    }
    #[test]
    fn parses_router_status() {
        let m: Model =
            serde_json::from_value(serde_json::json!({"id":"test","status":{"value":"loaded"}}))
                .unwrap();
        assert_eq!(m.state(), "loaded");
    }
}
