use crate::cli;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::instructions::{Instructions, tag};
use crate::json;
use crate::protocol::{Call, JsonFormat, error_message};
use crate::provider::{Protocol, Provider, preset};
use crate::response::{Reasoning, Response};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A configured LLM client. Cheap to clone; build once and reuse.
#[derive(Debug, Clone)]
pub struct Conduit {
    http: reqwest::Client,
    provider: Provider,
    timeout: Duration,
}

impl Conduit {
    /// Loads `~/.config/conduit/config.toml` and `~/.config/conduit/<app>/config.toml`, then applies
    /// the `CONDUIT_PROVIDER` and `CONDUIT_MODEL` environment overrides.
    pub fn load(app: &str) -> Result<Self> {
        Self::from_config(&Config::load(app)?)
    }

    pub fn from_config(config: &Config) -> Result<Self> {
        from_config_with_env(config, |key| std::env::var(key).ok())
    }

    pub fn new(provider: Provider) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!("conduit/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("HTTP client builds with static settings");
        Self {
            http,
            provider,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.provider.model = Some(model.into());
        self
    }

    pub fn provider(&self) -> &Provider {
        &self.provider
    }

    /// Starts a request. Nothing is sent until `.send()`, `.text()` or `.json()`.
    pub fn prompt(&self, input: impl Into<String>) -> Request<'_> {
        Request {
            conduit: self,
            input: input.into(),
            system: None,
            context: Vec::new(),
            max_tokens: None,
            temperature: None,
            reasoning: None,
            model: None,
            schema: None,
        }
    }

    /// Models the provider reports as available. CLI providers return their suggested aliases.
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let p = &self.provider;
        if p.protocol.is_cli() {
            let suggested = preset(&p.name)
                .map(|pr| pr.suggested_models)
                .unwrap_or_default();
            return Ok(suggested.iter().map(|m| m.to_string()).collect());
        }
        let url = p.protocol.models_url(&p.endpoint);
        let key = p.api_key()?;
        let req = p
            .protocol
            .authorize(self.http.get(&url), key.as_deref())
            .timeout(CONNECT_TIMEOUT * 2);
        let resp = req
            .send()
            .await
            .map_err(|e| self.transport_error(e, &url))?;
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| self.transport_error(e, &url))?;
        if !(200..300).contains(&status) {
            return Err(self.status_error(status, &body, ""));
        }
        let value: Value = serde_json::from_str(&body).map_err(|e| self.decode_error(e))?;
        Ok(p.protocol.parse_models(value))
    }

    async fn post(&self, call: &Call<'_>) -> Result<Response> {
        let p = &self.provider;
        let key = p.api_key()?;
        let url = p.protocol.generate_url(&p.endpoint);
        let mut req = p
            .protocol
            .authorize(self.http.post(&url), key.as_deref())
            .timeout(self.timeout)
            .json(&p.protocol.body(call));
        for (name, value) in p.protocol.extra_headers(call) {
            req = req.header(name, value);
        }

        let started = Instant::now();
        let resp = req
            .send()
            .await
            .map_err(|e| self.transport_error(e, &url))?;
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| self.transport_error(e, &url))?;
        if !(200..300).contains(&status) {
            return Err(self.status_error(status, &body, call.model));
        }
        let value: Value = serde_json::from_str(&body).map_err(|e| self.decode_error(e))?;
        let parsed = p.protocol.parse(value).map_err(|message| Error::Decode {
            provider: p.name.clone(),
            message,
        })?;
        Ok(Response {
            text: parsed.text,
            provider: p.name.clone(),
            model: parsed.model.unwrap_or_else(|| call.model.to_string()),
            stop: parsed.stop,
            usage: parsed.usage,
            elapsed: started.elapsed(),
        })
    }

    fn transport_error(&self, e: reqwest::Error, url: &str) -> Error {
        let provider = self.provider.name.clone();
        if e.is_timeout() {
            return Error::Timeout {
                provider,
                secs: self.timeout.as_secs(),
            };
        }
        let hint = if self.provider.protocol == Protocol::Ollama {
            " (is Ollama running? start it with `ollama serve`)".to_string()
        } else if self.provider.is_local() {
            " (is the local server running?)".to_string()
        } else {
            String::new()
        };
        Error::Unreachable {
            provider,
            url: url.to_string(),
            hint,
            source: e,
        }
    }

    fn status_error(&self, status: u16, body: &str, model: &str) -> Error {
        let provider = self.provider.name.clone();
        let message = error_message(body);
        match status {
            401 | 403 => Error::Unauthorized {
                provider,
                status,
                message,
            },
            404 if !model.is_empty() && message.to_lowercase().contains("model") => {
                let hint = if self.provider.protocol == Protocol::Ollama {
                    format!(" (run `ollama pull {model}`)")
                } else {
                    String::new()
                };
                Error::ModelNotFound {
                    provider,
                    model: model.to_string(),
                    hint,
                }
            }
            429 => Error::RateLimited { provider, message },
            _ => Error::Api {
                provider,
                status,
                message,
            },
        }
    }

    fn decode_error(&self, e: serde_json::Error) -> Error {
        Error::Decode {
            provider: self.provider.name.clone(),
            message: e.to_string(),
        }
    }
}

fn from_config_with_env(config: &Config, env: impl Fn(&str) -> Option<String>) -> Result<Conduit> {
    let set = |key: &str| env(key).filter(|v| !v.trim().is_empty());
    let name = set("CONDUIT_PROVIDER").unwrap_or_else(|| config.active_provider().to_string());
    let mut provider = Provider::resolve(&name, config.providers.get(&name))?;
    if let Some(model) = set("CONDUIT_MODEL") {
        provider.model = Some(model);
    }
    let mut conduit = Conduit::new(provider);
    if let Some(secs) = config.timeout_secs {
        conduit.timeout = Duration::from_secs(secs);
    }
    Ok(conduit)
}

/// A single prompt being assembled. Every setting is optional; unset means "the provider's default".
#[must_use = "a request does nothing until .send(), .text() or .json() is awaited"]
pub struct Request<'a> {
    conduit: &'a Conduit,
    input: String,
    system: Option<Instructions>,
    context: Vec<(String, String)>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    reasoning: Option<Reasoning>,
    model: Option<String>,
    schema: Option<Value>,
}

impl Request<'_> {
    pub fn system(mut self, instructions: impl Into<Instructions>) -> Self {
        self.system = Some(instructions.into());
        self
    }

    /// Adds a fact about the caller's environment. Context renders as a `<context>` block ahead of the input.
    pub fn context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.push((key.into(), value.into()));
        self
    }

    pub fn max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = Some(n);
        self
    }

    /// Best effort: dropped automatically for models that reject it, and never sent to Anthropic.
    pub fn temperature(mut self, t: f32) -> Self {
        self.temperature = Some(t);
        self
    }

    pub fn reasoning(mut self, r: Reasoning) -> Self {
        self.reasoning = Some(r);
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// A JSON Schema for `.json()`. Enforced natively where the provider supports it, otherwise
    /// included in the instructions.
    pub fn schema(mut self, schema: Value) -> Self {
        self.schema = Some(schema);
        self
    }

    pub async fn send(self) -> Result<Response> {
        self.execute(None).await
    }

    pub async fn text(self) -> Result<String> {
        self.send().await?.into_text()
    }

    pub async fn json<T: DeserializeOwned>(mut self) -> Result<T> {
        let format = match self.schema.take() {
            Some(schema) => JsonFormat::Schema(schema),
            None => JsonFormat::Any,
        };
        let text = self.execute(Some(format)).await?.into_text()?;
        json::parse(&text)
    }

    async fn execute(self, json_format: Option<JsonFormat>) -> Result<Response> {
        let conduit = self.conduit;
        let provider = &conduit.provider;
        let model = self.model.as_deref().or(provider.model.as_deref());
        if model.is_none() && !provider.model_optional() {
            return Err(Error::MissingModel {
                provider: provider.name.clone(),
            });
        }

        let system = compose_system(self.system.as_ref(), json_format.as_ref());
        let input = render_input(&self.input, &self.context);
        let mut call = Call {
            model: model.unwrap_or_default(),
            system: system.as_deref(),
            input: &input,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            reasoning: self.reasoning,
            json: json_format.as_ref(),
        };

        if provider.protocol.is_cli() {
            return cli::run(provider, &call, conduit.timeout).await;
        }
        loop {
            match conduit.post(&call).await {
                Err(Error::Api {
                    status: 400,
                    provider,
                    message,
                }) => {
                    if !relax(&mut call, &message) {
                        return Err(Error::Api {
                            status: 400,
                            provider,
                            message,
                        });
                    }
                }
                other => return other,
            }
        }
    }
}

/// Drops optional parameters a provider rejected so the request can be retried.
/// Returns false when nothing was dropped, which ends the retry loop.
fn relax(call: &mut Call, message: &str) -> bool {
    let message = message.to_lowercase();
    let mut dropped = false;
    if call.temperature.is_some() && message.contains("temperature") {
        call.temperature = None;
        dropped = true;
    }
    if call.reasoning.is_some()
        && ["reasoning", "effort", "think"]
            .iter()
            .any(|k| message.contains(k))
    {
        call.reasoning = None;
        dropped = true;
    }
    if call.json.is_some()
        && [
            "response_format",
            "json_schema",
            "json_object",
            "text.format",
            "output_config",
        ]
        .iter()
        .any(|k| message.contains(k))
    {
        call.json = None;
        dropped = true;
    }
    dropped
}

fn compose_system(
    system: Option<&Instructions>,
    json_format: Option<&JsonFormat>,
) -> Option<String> {
    let json_rule = json_format.map(|format| match format {
        JsonFormat::Schema(schema) => json::instruction(Some(schema)),
        JsonFormat::Any => json::instruction(None),
    });
    match (system.map(Instructions::as_str), json_rule) {
        (Some(s), Some(rule)) => Some(format!("{s}\n\n{rule}")),
        (Some(s), None) => Some(s.to_string()),
        (None, rule) => rule,
    }
}

fn render_input(input: &str, context: &[(String, String)]) -> String {
    if context.is_empty() {
        return input.to_string();
    }
    let facts: Vec<String> = context
        .iter()
        .map(|(k, v)| format!("<{k}>{v}</{k}>"))
        .collect();
    format!(
        "{}\n\n{}",
        tag("context", &facts.join("\n")),
        tag("input", input)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderConfig;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            vars.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn env_overrides_config() {
        let mut config = Config {
            provider: Some("anthropic".into()),
            ..Default::default()
        };
        config.providers.insert(
            "ollama".into(),
            ProviderConfig {
                model: Some("llama3.2".into()),
                ..Default::default()
            },
        );
        let c = from_config_with_env(&config, env(&[])).unwrap();
        assert_eq!(c.provider().name, "anthropic");

        let c = from_config_with_env(&config, env(&[("CONDUIT_PROVIDER", "ollama")])).unwrap();
        assert_eq!(c.provider().model.as_deref(), Some("llama3.2"));

        let vars = [
            ("CONDUIT_PROVIDER", "ollama"),
            ("CONDUIT_MODEL", "qwen3:8b"),
        ];
        let c = from_config_with_env(&config, env(&vars)).unwrap();
        assert_eq!(c.provider().model.as_deref(), Some("qwen3:8b"));
    }

    #[test]
    fn unknown_provider_without_definition_is_an_error() {
        let config = Config {
            provider: Some("nope".into()),
            ..Default::default()
        };
        assert!(matches!(
            from_config_with_env(&config, env(&[])),
            Err(Error::UnknownProvider(name)) if name == "nope"
        ));
    }

    #[test]
    fn context_wraps_input() {
        let rendered = render_input("list files", &[("os".into(), "macOS".into())]);
        assert_eq!(
            rendered,
            "<context>\n<os>macOS</os>\n</context>\n\n<input>\nlist files\n</input>"
        );
        assert_eq!(render_input("plain", &[]), "plain");
    }

    #[test]
    fn relax_drops_only_rejected_parameters() {
        let mut call = Call {
            model: "m",
            system: None,
            input: "",
            max_tokens: None,
            temperature: Some(0.2),
            reasoning: Some(Reasoning::Low),
            json: None,
        };
        assert!(relax(&mut call, "Unsupported parameter: 'temperature'"));
        assert_eq!(call.temperature, None);
        assert_eq!(call.reasoning, Some(Reasoning::Low));
        assert!(!relax(&mut call, "Unsupported parameter: 'temperature'"));
    }
}
