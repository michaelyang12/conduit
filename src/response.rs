use crate::error::Result;
use std::time::Duration;

/// How hard the model should think before answering. Mapped onto each provider's own knob
/// (`reasoning.effort`, `output_config.effort`, Ollama's `think`, the CLIs' effort flags).
/// Unset means "the provider's default".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reasoning {
    /// As little as the provider allows. Some models cannot fully disable reasoning; they get the lowest level.
    Off,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// The model finished on its own.
    Complete,
    /// The model hit the `max_tokens` limit; `text` is partial.
    MaxTokens,
    /// The provider declined to answer; `text` may hold its explanation.
    Refusal,
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// A completed request, with enough metadata to decide what to do with partial or refused output.
#[derive(Debug, Clone)]
pub struct Response {
    pub text: String,
    pub provider: String,
    pub model: String,
    pub stop: StopReason,
    pub usage: Option<Usage>,
    pub elapsed: Duration,
}

impl Response {
    /// The policy behind `Request::text()` and `Request::json()`: turn a raw response into usable text,
    /// or an error when the output can't be trusted.
    pub fn into_text(self) -> Result<String> {
        // TODO(user): decide how truncated, refused and empty responses surface to callers.
        todo!("Response::into_text policy")
    }
}
