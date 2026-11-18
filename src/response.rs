use crate::error::{Error, Result};
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
        match self.stop {
            StopReason::MaxTokens => Err(Error::Truncated { partial: self.text }),
            StopReason::Refusal => Err(Error::Refused {
                provider: self.provider,
                message: if self.text.is_empty() {
                    "no explanation given".to_string()
                } else {
                    self.text
                },
            }),
            _ if self.text.is_empty() => Err(Error::Empty {
                provider: self.provider,
                reason: match self.stop {
                    StopReason::Other(reason) => reason,
                    _ => "the model returned no text".to_string(),
                },
            }),
            _ => Ok(self.text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(text: &str, stop: StopReason) -> Response {
        Response {
            text: text.to_string(),
            provider: "p".into(),
            model: "m".into(),
            stop,
            usage: None,
            elapsed: Duration::ZERO,
        }
    }

    #[test]
    fn only_complete_nonempty_text_is_accepted() {
        assert_eq!(
            response("ls", StopReason::Complete).into_text().unwrap(),
            "ls"
        );
        assert!(matches!(
            response("rm -rf /tmp/fo", StopReason::MaxTokens).into_text(),
            Err(Error::Truncated { partial }) if partial == "rm -rf /tmp/fo"
        ));
        assert!(matches!(
            response("", StopReason::Refusal).into_text(),
            Err(Error::Refused { .. })
        ));
        assert!(matches!(
            response("", StopReason::Complete).into_text(),
            Err(Error::Empty { .. })
        ));
        assert_eq!(
            response("x", StopReason::Other("pause".into()))
                .into_text()
                .unwrap(),
            "x"
        );
    }
}
