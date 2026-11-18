use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything that can go wrong in conduit. Messages are written to be shown to end users as-is.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid config at {}: {message}", path.display())]
    Config { path: PathBuf, message: String },

    #[error("could not write config at {}: {source}", path.display())]
    ConfigWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "unknown provider '{0}': define it with `protocol` and `endpoint` under [providers.{0}]"
    )]
    UnknownProvider(String),

    #[error(
        "no model configured for {provider}: run setup or set `model` under [providers.{provider}]"
    )]
    MissingModel { provider: String },

    #[error("{env} is not set (needed for {provider})")]
    MissingApiKey { provider: String, env: String },

    #[error("could not reach {provider} at {url}{hint}")]
    Unreachable {
        provider: String,
        url: String,
        hint: String,
        #[source]
        source: reqwest::Error,
    },

    #[error(
        "`{command}` was not found on PATH (needed for {provider}); install it or set `endpoint` to its full path"
    )]
    CommandNotFound { provider: String, command: String },

    #[error("{provider} failed: {message}")]
    Cli { provider: String, message: String },

    #[error("{provider} did not respond within {secs}s")]
    Timeout { provider: String, secs: u64 },

    #[error("model '{model}' not found on {provider}{hint}")]
    ModelNotFound {
        provider: String,
        model: String,
        hint: String,
    },

    #[error("{provider} rejected the credentials ({status}): {message}")]
    Unauthorized {
        provider: String,
        status: u16,
        message: String,
    },

    #[error("{provider} is rate limiting requests: {message}")]
    RateLimited { provider: String, message: String },

    #[error("{provider} returned {status}: {message}")]
    Api {
        provider: String,
        status: u16,
        message: String,
    },

    #[error("unexpected response from {provider}: {message}")]
    Decode { provider: String, message: String },

    #[error("{provider} returned an empty response ({reason})")]
    Empty { provider: String, reason: String },

    #[error("{provider} declined to answer: {message}")]
    Refused { provider: String, message: String },

    #[error("response was cut off at the token limit")]
    Truncated { partial: String },

    #[error("response was not valid JSON: {source}")]
    Json {
        raw: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("setup failed: {0}")]
    Setup(String),
}
