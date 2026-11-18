use crate::error::{Error, Result};
use crate::provider::Protocol;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_PROVIDER: &str = "openai";

/// The on-disk config. A global file (`~/.config/conduit/config.toml`) is layered under a per-app
/// file (`~/.config/conduit/<app>/config.toml`), so a provider set up once works for every app.
///
/// ```toml
/// provider = "ollama"
///
/// [providers.ollama]
/// model = "qwen3:8b"
///
/// [providers.work-proxy]
/// protocol = "chat-completions"
/// endpoint = "https://llm.internal.example.com/v1"
/// api_key_env = "WORK_LLM_KEY"
/// model = "gpt-5.1"
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub providers: BTreeMap<String, ProviderConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<Protocol>,
}

impl ProviderConfig {
    fn merge(self, over: ProviderConfig) -> ProviderConfig {
        ProviderConfig {
            model: over.model.or(self.model),
            endpoint: over.endpoint.or(self.endpoint),
            api_key_env: over.api_key_env.or(self.api_key_env),
            protocol: over.protocol.or(self.protocol),
        }
    }
}

impl Config {
    /// `$XDG_CONFIG_HOME/conduit`, falling back to `~/.config/conduit`.
    pub fn dir() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("conduit")
    }

    pub fn global_path() -> PathBuf {
        Self::dir().join("config.toml")
    }

    pub fn app_path(app: &str) -> PathBuf {
        Self::dir().join(app).join("config.toml")
    }

    /// Loads the global config with the app's config layered on top.
    pub fn load(app: &str) -> Result<Self> {
        let global = Self::read(&Self::global_path())?;
        let app = Self::read(&Self::app_path(app))?;
        Ok(global.merge(app))
    }

    /// Reads a single config file. A missing file is an empty config; a malformed one is an error.
    pub fn read(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| Error::Config {
                path: path.to_path_buf(),
                message: e.to_string(),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Config {
                path: path.to_path_buf(),
                message: e.to_string(),
            }),
        }
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        let write_err = |source| Error::ConfigWrite {
            path: path.to_path_buf(),
            source,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(write_err)?;
        }
        let text = toml::to_string_pretty(self).expect("config always serializes");
        std::fs::write(path, text).map_err(write_err)
    }

    /// Returns `self` with every field set in `over` taking precedence.
    pub fn merge(mut self, over: Config) -> Config {
        for (name, section) in over.providers {
            let merged = match self.providers.remove(&name) {
                Some(base) => base.merge(section),
                None => section,
            };
            self.providers.insert(name, merged);
        }
        Config {
            provider: over.provider.or(self.provider),
            timeout_secs: over.timeout_secs.or(self.timeout_secs),
            providers: self.providers,
        }
    }

    pub fn active_provider(&self) -> &str {
        self.provider.as_deref().unwrap_or(DEFAULT_PROVIDER)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_config_overrides_global_per_field() {
        let global: Config = toml::from_str(
            r#"
            provider = "ollama"
            [providers.ollama]
            model = "llama3.2"
            endpoint = "http://gpu-box:11434"
            "#,
        )
        .unwrap();
        let app: Config = toml::from_str(
            r#"
            [providers.ollama]
            model = "qwen3:8b"
            "#,
        )
        .unwrap();
        let merged = global.merge(app);
        assert_eq!(merged.active_provider(), "ollama");
        let ollama = &merged.providers["ollama"];
        assert_eq!(ollama.model.as_deref(), Some("qwen3:8b"));
        assert_eq!(ollama.endpoint.as_deref(), Some("http://gpu-box:11434"));
    }

    #[test]
    fn typos_are_rejected() {
        let err = toml::from_str::<Config>("[providers.openai]\nmodle = \"x\"").unwrap_err();
        assert!(err.to_string().contains("modle"));
    }

    #[test]
    fn round_trips_through_toml() {
        let mut config = Config {
            provider: Some("work".into()),
            ..Default::default()
        };
        config.providers.insert(
            "work".into(),
            ProviderConfig {
                protocol: Some(Protocol::ChatCompletions),
                endpoint: Some("https://llm.example.com/v1".into()),
                ..Default::default()
            },
        );
        let text = toml::to_string_pretty(&config).unwrap();
        assert!(text.contains("protocol = \"chat-completions\""));
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), config);
    }
}
