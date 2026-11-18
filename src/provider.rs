use crate::config::ProviderConfig;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// The wire format a provider speaks. Providers are data; protocols are code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Protocol {
    /// OpenAI's Responses API (`POST /responses`).
    OpenaiResponses,
    /// The OpenAI-compatible Chat Completions API (`POST /chat/completions`) spoken by most providers and local servers.
    ChatCompletions,
    /// Anthropic's Messages API (`POST /messages`).
    Anthropic,
    /// Ollama's native API (`POST /api/chat`).
    Ollama,
    /// The Claude Code CLI in headless mode (`claude -p`), using whatever account it is signed in with.
    ClaudeCode,
    /// The Codex CLI in non-interactive mode (`codex exec`), using whatever account it is signed in with.
    Codex,
}

impl Protocol {
    pub const ALL: [Protocol; 6] = [
        Protocol::OpenaiResponses,
        Protocol::ChatCompletions,
        Protocol::Anthropic,
        Protocol::Ollama,
        Protocol::ClaudeCode,
        Protocol::Codex,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::OpenaiResponses => "openai-responses",
            Protocol::ChatCompletions => "chat-completions",
            Protocol::Anthropic => "anthropic",
            Protocol::Ollama => "ollama",
            Protocol::ClaudeCode => "claude-code",
            Protocol::Codex => "codex",
        }
    }

    /// CLI protocols run a local executable; `endpoint` is the command instead of a URL.
    pub fn is_cli(self) -> bool {
        matches!(self, Protocol::ClaudeCode | Protocol::Codex)
    }
}

/// A built-in provider definition.
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    pub name: &'static str,
    pub label: &'static str,
    pub protocol: Protocol,
    pub endpoint: &'static str,
    pub api_key_env: Option<&'static str>,
    pub default_model: Option<&'static str>,
    /// `(id, description)` pairs shown when the provider can't list its models itself.
    pub suggested_models: &'static [(&'static str, &'static str)],
    pub local: bool,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "claude-code",
        label: "Claude Code",
        protocol: Protocol::ClaudeCode,
        endpoint: "claude",
        api_key_env: None,
        default_model: None,
        suggested_models: &[
            ("claude-fable-5-2", "Fable 5.2: most capable, slowest"),
            ("claude-opus-5-5", "Opus 5.5: deep reasoning"),
            (
                "claude-sonnet-5-5",
                "Sonnet 5.5: balanced speed and quality",
            ),
            ("claude-haiku-5-5", "Haiku 5.5: fastest"),
            ("claude-opus-5", "Opus 5"),
            ("claude-sonnet-5", "Sonnet 5"),
            ("claude-opus-4-8", "Opus 4.8"),
        ],
        local: true,
    },
    Preset {
        name: "codex",
        label: "Codex",
        protocol: Protocol::Codex,
        endpoint: "codex",
        api_key_env: None,
        default_model: None,
        suggested_models: &[],
        local: true,
    },
    Preset {
        name: "anthropic",
        label: "Anthropic",
        protocol: Protocol::Anthropic,
        endpoint: "https://api.anthropic.com/v1",
        api_key_env: Some("ANTHROPIC_API_KEY"),
        default_model: Some("claude-sonnet-5-5"),
        suggested_models: &[
            ("claude-fable-5-2", "Fable 5.2: most capable, slowest"),
            ("claude-opus-5-5", "Opus 5.5: deep reasoning"),
            (
                "claude-sonnet-5-5",
                "Sonnet 5.5: balanced speed and quality",
            ),
            ("claude-haiku-5-5", "Haiku 5.5: fastest"),
            ("claude-opus-5", "Opus 5"),
            ("claude-sonnet-5", "Sonnet 5"),
            ("claude-opus-4-8", "Opus 4.8"),
        ],
        local: false,
    },
    Preset {
        name: "openai",
        label: "OpenAI",
        protocol: Protocol::OpenaiResponses,
        endpoint: "https://api.openai.com/v1",
        api_key_env: Some("OPENAI_API_KEY"),
        default_model: Some("gpt-5.1"),
        suggested_models: &[
            ("gpt-5.1", "GPT-5.1"),
            ("gpt-5-mini", "GPT-5 mini: faster, cheaper"),
            ("gpt-5-nano", "GPT-5 nano: fastest"),
        ],
        local: false,
    },
    Preset {
        name: "gemini",
        label: "Google Gemini",
        protocol: Protocol::ChatCompletions,
        endpoint: "https://generativelanguage.googleapis.com/v1beta/openai",
        api_key_env: Some("GEMINI_API_KEY"),
        default_model: Some("gemini-2.5-flash"),
        suggested_models: &[
            ("gemini-2.5-flash", "fast"),
            ("gemini-2.5-pro", "most capable"),
        ],
        local: false,
    },
    Preset {
        name: "groq",
        label: "Groq",
        protocol: Protocol::ChatCompletions,
        endpoint: "https://api.groq.com/openai/v1",
        api_key_env: Some("GROQ_API_KEY"),
        default_model: Some("llama-3.3-70b-versatile"),
        suggested_models: &[
            ("llama-3.3-70b-versatile", "Llama 3.3 70B"),
            ("llama-3.1-8b-instant", "Llama 3.1 8B: fastest"),
        ],
        local: false,
    },
    Preset {
        name: "openrouter",
        label: "OpenRouter",
        protocol: Protocol::ChatCompletions,
        endpoint: "https://openrouter.ai/api/v1",
        api_key_env: Some("OPENROUTER_API_KEY"),
        default_model: Some("openrouter/auto"),
        suggested_models: &[("openrouter/auto", "OpenRouter picks the model")],
        local: false,
    },
    Preset {
        name: "deepseek",
        label: "DeepSeek",
        protocol: Protocol::ChatCompletions,
        endpoint: "https://api.deepseek.com/v1",
        api_key_env: Some("DEEPSEEK_API_KEY"),
        default_model: Some("deepseek-chat"),
        suggested_models: &[
            ("deepseek-chat", "DeepSeek chat"),
            ("deepseek-reasoner", "DeepSeek reasoning model"),
        ],
        local: false,
    },
    Preset {
        name: "ollama",
        label: "Ollama",
        protocol: Protocol::Ollama,
        endpoint: "http://localhost:11434",
        api_key_env: None,
        default_model: None,
        suggested_models: &[],
        local: true,
    },
    Preset {
        name: "lmstudio",
        label: "LM Studio",
        protocol: Protocol::ChatCompletions,
        endpoint: "http://localhost:1234/v1",
        api_key_env: None,
        default_model: None,
        suggested_models: &[],
        local: true,
    },
];

pub fn preset(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.name == name)
}

/// A model a provider offers, as shown by the setup wizard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub description: Option<String>,
}

impl ModelInfo {
    pub fn new(id: impl Into<String>, description: Option<String>) -> Self {
        Self {
            id: id.into(),
            description: description.filter(|d| !d.trim().is_empty()),
        }
    }
}

/// A fully resolved provider: where to send requests and how.
#[derive(Debug, Clone)]
pub struct Provider {
    pub name: String,
    pub protocol: Protocol,
    pub endpoint: String,
    pub api_key_env: Option<String>,
    /// An explicit key that takes precedence over `api_key_env`. Never read from or written to config files.
    pub api_key: Option<String>,
    pub model: Option<String>,
}

impl Provider {
    pub fn new(name: impl Into<String>, protocol: Protocol, endpoint: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            protocol,
            endpoint: endpoint.into(),
            api_key_env: None,
            api_key: None,
            model: None,
        }
    }

    pub fn from_preset(preset: &Preset) -> Self {
        Self {
            name: preset.name.to_string(),
            protocol: preset.protocol,
            endpoint: preset.endpoint.to_string(),
            api_key_env: preset.api_key_env.map(str::to_string),
            api_key: None,
            model: preset.default_model.map(str::to_string),
        }
    }

    /// Resolves a provider by name, layering a config section over the built-in preset if one exists.
    pub fn resolve(name: &str, overrides: Option<&ProviderConfig>) -> Result<Self> {
        let mut provider = match (preset(name), overrides) {
            (Some(p), _) => Self::from_preset(p),
            (
                None,
                Some(ProviderConfig {
                    protocol: Some(protocol),
                    endpoint: Some(url),
                    ..
                }),
            ) => Self::new(name, *protocol, url.clone()),
            (None, _) => return Err(Error::UnknownProvider(name.to_string())),
        };
        if let Some(o) = overrides {
            if let Some(protocol) = o.protocol {
                provider.protocol = protocol;
            }
            if let Some(url) = &o.endpoint {
                provider.endpoint = url.clone();
            }
            if o.api_key_env.is_some() {
                provider.api_key_env = o.api_key_env.clone();
            }
            if o.model.is_some() {
                provider.model = o.model.clone();
            }
        }
        provider.endpoint = provider.endpoint.trim_end_matches('/').to_string();
        Ok(provider)
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    pub fn with_api_key_env(mut self, env: impl Into<String>) -> Self {
        self.api_key_env = Some(env.into());
        self
    }

    pub fn is_local(&self) -> bool {
        self.protocol.is_cli()
            || self.endpoint.contains("://localhost")
            || self.endpoint.contains("://127.0.0.1")
    }

    /// Whether requests can run without a model name, falling back to the tool's own default.
    pub fn model_optional(&self) -> bool {
        self.protocol.is_cli()
    }

    pub(crate) fn api_key(&self) -> Result<Option<String>> {
        if let Some(key) = &self.api_key {
            return Ok(Some(key.clone()));
        }
        match &self.api_key_env {
            None => Ok(None),
            Some(env) => match std::env::var(env) {
                Ok(key) if !key.trim().is_empty() => Ok(Some(key)),
                _ if self.is_local() => Ok(None),
                _ => Err(Error::MissingApiKey {
                    provider: self.name.clone(),
                    env: env.clone(),
                }),
            },
        }
    }
}
