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
#[derive(Debug, Clone, Default)]
pub struct ModelProperties {
    pub context: Option<u64>,
    pub generation: crate::models::profile::Generation,
    pub tools: Option<bool>,
    pub reasoning_effort: Option<bool>,
    pub levels: Vec<crate::models::profile::Thinking>,
    pub rejected: Vec<crate::models::profile::Thinking>,
}
impl ModelProperties {
    pub fn from_props(props: &serde_json::Value) -> Self {
        use crate::models::profile::{Generation, Thinking};
        let defaults = &props["default_generation_settings"];
        let params = &defaults["params"];
        let float = |key: &str, min: f64, max: f64| {
            params[key]
                .as_f64()
                .filter(|v| v.is_finite() && *v >= min && *v <= max)
                .map(|v| v as f32)
        };
        let generation = Generation {
            temperature: float("temperature", 0., f32::MAX as f64),
            top_p: float("top_p", 0., 1.),
            top_k: params["top_k"].as_u64().and_then(|n| u32::try_from(n).ok()),
            min_p: float("min_p", 0., 1.),
            repeat_penalty: float("repeat_penalty", f32::MIN_POSITIVE as f64, f32::MAX as f64),
            presence_penalty: float("presence_penalty", -2., 2.),
            seed: None,
        };
        let caps = &props["chat_template_caps"];
        let levels = |key: &str| {
            caps[key]
                .as_array()
                .into_iter()
                .flatten()
                .take(32)
                .filter_map(|value| {
                    let label = value.as_str()?;
                    if label == "none" {
                        return Some(Thinking::Off);
                    }
                    Thinking::ALL.iter().copied().find(|t| t.label() == label)
                })
                .collect::<Vec<_>>()
        };
        Self {
            context: defaults["n_ctx"].as_u64().filter(|n| *n >= 128),
            generation,
            tools: caps["supports_tool_calls"]
                .as_bool()
                .or_else(|| caps["supports_tools"].as_bool()),
            reasoning_effort: caps["supports_reasoning_effort"].as_bool(),
            levels: levels("reasoning_effort_levels"),
            rejected: levels("reasoning_effort_rejected"),
        }
    }
    /// Manual preferences are not silently downgraded. Auto is conservative
    /// when a backend has no evidence for graded reasoning.
    pub fn choose_thinking(
        &self,
        explicit: Option<crate::models::profile::Thinking>,
        suggested: Option<crate::models::profile::Thinking>,
    ) -> io::Result<Option<crate::models::profile::Thinking>> {
        use crate::models::profile::Thinking;
        if let Some(value) = explicit {
            if self.rejected.contains(&value)
                || (!self.levels.is_empty()
                    && !matches!(value, Thinking::Off | Thinking::Auto)
                    && !self.levels.contains(&value))
            {
                return Err(io::Error::other("Loaded template rejects this reasoning effort; choose a supported level or Auto"));
            }
            return Ok((value != Thinking::Auto).then_some(value));
        }
        if let Some(value) =
            suggested.filter(|v| self.levels.contains(v) && !self.rejected.contains(v))
        {
            return Ok(Some(value));
        }
        if suggested.is_some() && self.reasoning_effort == Some(true) {
            for value in [
                Thinking::Medium,
                Thinking::Low,
                Thinking::High,
                Thinking::Xhigh,
                Thinking::Minimal,
                Thinking::Max,
            ] {
                if self.levels.contains(&value) && !self.rejected.contains(&value) {
                    return Ok(Some(value));
                }
            }
        }
        Ok(None)
    }
}
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub router: bool,
    pub models: Vec<Model>,
    pub properties: std::collections::BTreeMap<String, ModelProperties>,
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
        // A failed or denied props request is not evidence of a single-model server.
        let props = self.get("props")?;
        let router = match props.get("role").and_then(|v| v.as_str()) {
            Some("router") => true,
            Some("model" | "server" | "single") | None if props.is_object() => false,
            _ => return Err(io::Error::other("Unrecognized server role")),
        };
        let data = self.get(if router { "models" } else { "v1/models" })?;
        let list = data.get("data").cloned().unwrap_or(data);
        let models: Vec<Model> =
            serde_json::from_value(list).map_err(|_| io::Error::other("Invalid model list"))?;
        let mut properties = std::collections::BTreeMap::new();
        if router {
            // Managed routers load one model at a time. Bound queries for
            // attached external routers and never trigger an automatic load.
            for model in models.iter().filter(|m| m.state() == "loaded").take(1) {
                let query = url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("model", &model.id)
                    .append_pair("autoload", "false")
                    .finish();
                if let Ok(value) = self.get(&format!("props?{query}")) {
                    properties.insert(model.id.clone(), ModelProperties::from_props(&value));
                }
            }
        } else if let Some(model) = models.first() {
            properties.insert(model.id.clone(), ModelProperties::from_props(&props));
        }
        Ok(Snapshot {
            router,
            models,
            properties,
        })
    }
    /// Diagnostic only: the advertised functions are synthetic and never executed.
    /// Inherit the server sampler settings; do not substitute a greedy sampler.
    pub fn tool_preflight(
        &self,
        model: &str,
        thinking: Option<crate::models::profile::Thinking>,
        responses: bool,
    ) -> io::Result<()> {
        if !matches!(
            self.base.host_str(),
            Some("127.0.0.1" | "localhost" | "[::1]")
        ) {
            return Err(io::Error::other(
                "Tool preflight is limited to loopback inference",
            ));
        }
        let mut body = serde_json::json!({
            "model":model,"messages":[{"role":"system","content":"You are a precise coding agent. Use a tool when asked."},{"role":"user","content":"Use shell exactly once with command \"git log --oneline -8\" and cwd \"/tmp/llamancer-fixture\". Do not modify any text in the arguments."}],
            "tools":[{"type":"function","function":{"name":"shell","description":"Run shell command in working directory","parameters":{"type":"object","properties":{"command":{"type":"string"},"cwd":{"type":"string"}},"required":["command","cwd"],"additionalProperties":false}}},{"type":"function","function":{"name":"read_file","description":"Read a file","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}}],
            "max_tokens":2048,"stream":false
        });
        if let Some(t) = thinking.filter(|t| *t != crate::models::profile::Thinking::Auto) {
            body["chat_template_kwargs"] =
                serde_json::json!({"enable_thinking":t!=crate::models::profile::Thinking::Off});
        }
        if responses {
            let mut input = serde_json::json!({
                "model":model,"instructions":body["messages"][0]["content"],
                "input":[{"role":"user","content":body["messages"][1]["content"]}],
                "tools":body["tools"].as_array().unwrap().iter().map(|t|{
                    let mut f=t["function"].clone();f["type"]=serde_json::json!("function");f
                }).collect::<Vec<_>>(),
                "max_output_tokens":2048,"stream":true
            });
            if let Some(t) = thinking.filter(|t| *t != crate::models::profile::Thinking::Auto) {
                input["reasoning"] = serde_json::json!({"effort":if t==crate::models::profile::Thinking::Off{"none"}else{t.label()}});
            }
            let response = self
                .request("POST", "v1/responses")?
                .timeout(Duration::from_secs(180))
                .send_json(input)
                .map_err(|e|match e{ureq::Error::Status(400,_)=>io::Error::other("Responses probe rejected (HTTP 400); check reasoning level and backend/template support"),other=>http_error(other)})?;
            let mut bytes = Vec::new();
            response
                .into_reader()
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 4 * 1024 * 1024 {
                return Err(io::Error::other("Probe response too large"));
            }
            return validate_responses_probe(&bytes);
        }
        let response = json(
            self.request("POST", "v1/chat/completions")?
                .timeout(Duration::from_secs(180))
                .send_json(body)
                .map_err(http_error)?,
        )?;
        validate_tool_probe(&response)
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
mod property_tests {
    use super::*;
    use crate::models::profile::Thinking;
    #[test]
    fn runtime_reasoning_caps_and_real_parameters() {
        let p = ModelProperties::from_props(
            &serde_json::json!({"default_generation_settings":{"n_ctx":131072,"params":{"temperature":0.7,"top_k":20,"presence_penalty":1.5}},"chat_template_caps":{"supports_tool_calls":true,"supports_reasoning_effort":true,"reasoning_effort_levels":["low","medium","xhigh"],"reasoning_effort_rejected":["high"]}}),
        );
        assert_eq!(p.context, Some(131072));
        assert_eq!(p.generation.temperature, Some(0.7));
        assert_eq!(
            p.choose_thinking(None, Some(Thinking::Medium)).unwrap(),
            Some(Thinking::Medium)
        );
        assert!(p.choose_thinking(Some(Thinking::High), None).is_err());
        assert_eq!(p.choose_thinking(Some(Thinking::Auto), None).unwrap(), None);
        assert_eq!(
            ModelProperties::default()
                .choose_thinking(None, Some(Thinking::Medium))
                .unwrap(),
            None
        );
    }
}
fn validate_responses_probe(bytes: &[u8]) -> io::Result<()> {
    let invalid = || {
        io::Error::other(
            "Responses tool probe failed: missing completion, malformed or incorrect arguments",
        )
    };
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut completed = None;
    let mut streamed = String::new();
    let mut done = None;
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(data).map_err(|_| invalid())?;
        match v["type"].as_str() {
            Some("response.function_call_arguments.delta") => {
                streamed.push_str(v["delta"].as_str().ok_or_else(invalid)?)
            }
            Some("response.function_call_arguments.done") => {
                if done.is_some() {
                    return Err(invalid());
                }
                done = Some(v["arguments"].as_str().ok_or_else(invalid)?.to_owned());
            }
            Some("response.completed") => {
                if completed.is_some() {
                    return Err(invalid());
                }
                completed = Some(v["response"].clone());
            }
            Some("response.failed" | "response.incomplete" | "error") => return Err(invalid()),
            _ => {}
        }
    }
    let r = completed.ok_or_else(invalid)?;
    if r["status"] != "completed" {
        return Err(invalid());
    }
    let calls: Vec<_> = r["output"]
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .filter(|o| o["type"] == "function_call")
        .collect();
    if calls.len() != 1 {
        return Err(invalid());
    }
    let f = calls[0];
    let args = f["arguments"].as_str().ok_or_else(invalid)?;
    if done.as_deref() != Some(args) || streamed != args {
        return Err(invalid());
    }
    validate_tool_probe(
        &serde_json::json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"function":{"name":f["name"],"arguments":args}}]}}]}),
    )
}
fn validate_tool_probe(response: &serde_json::Value) -> io::Result<()> {
    let invalid = || {
        io::Error::other(
            "Tool compatibility probe failed: truncated, malformed or incorrect arguments",
        )
    };
    let c = response["choices"]
        .as_array()
        .filter(|a| a.len() == 1)
        .and_then(|a| a.first())
        .ok_or_else(invalid)?;
    if c["finish_reason"] != "tool_calls" {
        return Err(invalid());
    }
    let calls = c["message"]["tool_calls"]
        .as_array()
        .filter(|a| a.len() == 1)
        .ok_or_else(invalid)?;
    let f = &calls[0]["function"];
    let arguments: serde_json::Value =
        serde_json::from_str(f["arguments"].as_str().ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
    if f["name"] != "shell"
        || arguments
            != serde_json::json!({"command":"git log --oneline -8","cwd":"/tmp/llamancer-fixture"})
    {
        return Err(invalid());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn responses_probe_requires_exact_stream_and_completion() {
        let args =
            serde_json::json!({"command":"git log --oneline -8","cwd":"/tmp/llamancer-fixture"})
                .to_string();
        let events = [
            serde_json::json!({"type":"response.function_call_arguments.delta","delta":args}),
            serde_json::json!({"type":"response.function_call_arguments.done","arguments":args}),
            serde_json::json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"function_call","name":"shell","arguments":args}]}}),
        ];
        let s = events
            .iter()
            .map(|e| format!("data: {e}\n\n"))
            .collect::<String>();
        assert!(super::validate_responses_probe(s.as_bytes()).is_ok());
        assert!(super::validate_responses_probe(
            s.replace("response.completed", "response.incomplete")
                .as_bytes()
        )
        .is_err());
        assert!(
            super::validate_responses_probe(s.replace("fixture", "garbage").as_bytes()).is_err()
        );
    }
    #[test]
    fn probe_rejects_corruption_and_truncation() {
        let mut r = serde_json::json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"function":{"name":"shell","arguments":"{\"command\":\"git log --oneline -8\",\"cwd\":\"/tmp/llamancer-fixture\"}"}}]}}]});
        assert!(super::validate_tool_probe(&r).is_ok());
        r["choices"][0]["finish_reason"] = serde_json::json!("length");
        assert!(super::validate_tool_probe(&r).is_err());
        r["choices"][0]["finish_reason"] = serde_json::json!("tool_calls");
        r["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"] = serde_json::json!(
            "{\"command\":\"git log --oneline -8\",\"cwd\":\"/tmp/llamancer-fixture</invoke>\"}"
        );
        assert!(super::validate_tool_probe(&r).is_err());
    }
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
