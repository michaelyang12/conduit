mod anthropic;
mod chat;
mod ollama;
mod responses;

use crate::provider::{ModelInfo, Protocol};
use crate::response::{Reasoning, StopReason, Usage};
use reqwest::RequestBuilder;
use serde_json::Value;

pub(crate) enum JsonFormat {
    Any,
    Schema(Value),
}

/// One provider-agnostic request, already resolved to concrete values.
pub(crate) struct Call<'a> {
    pub model: &'a str,
    pub system: Option<&'a str>,
    pub input: &'a str,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub reasoning: Option<Reasoning>,
    pub json: Option<&'a JsonFormat>,
}

pub(crate) struct Parsed {
    pub text: String,
    pub model: Option<String>,
    pub stop: StopReason,
    pub usage: Option<Usage>,
}

impl Protocol {
    pub(crate) fn generate_url(self, base: &str) -> String {
        match self {
            Protocol::OpenaiResponses => format!("{base}/responses"),
            Protocol::ChatCompletions => format!("{base}/chat/completions"),
            Protocol::Anthropic => format!("{base}/messages"),
            Protocol::Ollama => format!("{base}/api/chat"),
            Protocol::ClaudeCode | Protocol::Codex => unreachable!("CLI protocols have no URL"),
        }
    }

    pub(crate) fn models_url(self, base: &str) -> String {
        match self {
            Protocol::Ollama => format!("{base}/api/tags"),
            Protocol::Anthropic => format!("{base}/models?limit=1000"),
            _ => format!("{base}/models"),
        }
    }

    pub(crate) fn authorize(self, req: RequestBuilder, key: Option<&str>) -> RequestBuilder {
        match self {
            Protocol::Anthropic => {
                let req = req.header("anthropic-version", "2023-06-01");
                match key {
                    Some(k) => req.header("x-api-key", k),
                    None => req,
                }
            }
            _ => match key {
                Some(k) => req.bearer_auth(k),
                None => req,
            },
        }
    }

    pub(crate) fn extra_headers(self, call: &Call) -> Vec<(&'static str, &'static str)> {
        match self {
            Protocol::Anthropic => anthropic::extra_headers(call),
            _ => Vec::new(),
        }
    }

    pub(crate) fn body(self, call: &Call) -> Value {
        match self {
            Protocol::OpenaiResponses => responses::body(call),
            Protocol::ChatCompletions => chat::body(call),
            Protocol::Anthropic => anthropic::body(call),
            Protocol::Ollama => ollama::body(call),
            Protocol::ClaudeCode | Protocol::Codex => {
                unreachable!("CLI protocols have no HTTP body")
            }
        }
    }

    pub(crate) fn parse(self, body: Value) -> Result<Parsed, String> {
        let mut parsed = match self {
            Protocol::OpenaiResponses => responses::parse(body),
            Protocol::ChatCompletions => chat::parse(body),
            Protocol::Anthropic => anthropic::parse(body),
            Protocol::Ollama => ollama::parse(body),
            Protocol::ClaudeCode | Protocol::Codex => {
                unreachable!("CLI protocols have no HTTP body")
            }
        }?;
        parsed.text = strip_think(&parsed.text).trim().to_string();
        Ok(parsed)
    }

    pub(crate) fn parse_models(self, body: Value) -> Vec<ModelInfo> {
        let list = if self == Protocol::Ollama {
            "models"
        } else {
            "data"
        };
        let mut models: Vec<ModelInfo> = body[list]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                let id = m["id"].as_str().or_else(|| m["name"].as_str())?;
                let description = match self {
                    Protocol::Ollama => ollama_details(&m["details"]),
                    _ => m["display_name"]
                        .as_str()
                        .or_else(|| m["name"].as_str())
                        .filter(|name| *name != id)
                        .map(str::to_string),
                };
                Some(ModelInfo::new(id, description))
            })
            .filter(|m| is_text_model(&m.id))
            .collect();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        models
    }
}

fn ollama_details(details: &Value) -> Option<String> {
    let parts: Vec<&str> = ["parameter_size", "quantization_level"]
        .iter()
        .filter_map(|k| details[*k].as_str().filter(|v| !v.is_empty()))
        .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// Model lists from OpenAI-style APIs include embedding, audio and image models that can't chat.
fn is_text_model(id: &str) -> bool {
    const NON_CHAT: &[&str] = &[
        "embed",
        "embedding",
        "whisper",
        "tts",
        "transcribe",
        "audio",
        "realtime",
        "dall-e",
        "image",
        "moderation",
        "davinci",
        "babbage",
        "search",
        "rerank",
        "guard",
    ];
    let lower = id.to_lowercase();
    !NON_CHAT.iter().any(|k| lower.contains(k))
}

/// Pulls a human-readable message out of an error body, whatever shape the provider uses.
pub(crate) fn error_message(body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let message = parsed.as_ref().and_then(|v| {
        v["error"]["message"]
            .as_str()
            .or_else(|| v["error"].as_str())
            .or_else(|| v["message"].as_str())
            .or_else(|| v["detail"].as_str())
            .map(str::to_string)
    });
    message.unwrap_or_else(|| {
        let trimmed = body.trim();
        if trimmed.is_empty() {
            "(no body)".to_string()
        } else {
            trimmed.chars().take(500).collect()
        }
    })
}

/// Reasoning models served by local runtimes often inline their chain of thought in `<think>` tags.
fn strip_think(text: &str) -> &str {
    let trimmed = text.trim_start();
    if trimmed.starts_with("<think>")
        && let Some(end) = trimmed.find("</think>")
    {
        return &trimmed[end + "</think>".len()..];
    }
    text
}

/// `f32` widened to JSON's `f64` picks up noise (0.2 becomes 0.20000000298023224); go through its decimal form.
fn temperature(t: f32) -> Value {
    t.to_string()
        .parse::<f64>()
        .map(Value::from)
        .unwrap_or(Value::Null)
}

fn usage(input: &Value, output: &Value) -> Option<Usage> {
    Some(Usage {
        input_tokens: input.as_u64()? as u32,
        output_tokens: output.as_u64()? as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_modes_use_each_protocol_native_field() {
        let schema = serde_json::json!({"type": "object"});
        let any = JsonFormat::Any;
        let typed = JsonFormat::Schema(schema.clone());
        let call = |json| Call {
            model: "m",
            system: None,
            input: "x",
            max_tokens: None,
            temperature: None,
            reasoning: None,
            json,
        };
        let body = |p: Protocol, j| p.body(&call(Some(j)));

        assert_eq!(
            body(Protocol::OpenaiResponses, &any)["text"]["format"]["type"],
            "json_object"
        );
        assert_eq!(
            body(Protocol::OpenaiResponses, &typed)["text"]["format"]["schema"],
            schema
        );
        assert_eq!(
            body(Protocol::ChatCompletions, &any)["response_format"]["type"],
            "json_object"
        );
        assert_eq!(
            body(Protocol::ChatCompletions, &typed)["response_format"]["json_schema"]["schema"],
            schema
        );
        assert!(
            body(Protocol::Anthropic, &any)
                .get("output_config")
                .is_none()
        );
        assert_eq!(
            body(Protocol::Anthropic, &typed)["output_config"]["format"]["schema"],
            schema
        );
        assert_eq!(body(Protocol::Ollama, &any)["format"], "json");
        assert_eq!(body(Protocol::Ollama, &typed)["format"], schema);
    }

    #[test]
    fn model_lists_drop_non_chat_models_and_keep_names() {
        let openai = serde_json::json!({"data": [
            {"id": "gpt-6-luna"}, {"id": "text-embedding-3-large"}, {"id": "gpt-realtime"}, {"id": "dall-e-3"}
        ]});
        let ids: Vec<String> = Protocol::OpenaiResponses
            .parse_models(openai)
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, ["gpt-6-luna"]);

        let anthropic = serde_json::json!({"data": [{"id": "claude-haiku-5-5", "display_name": "Claude Haiku 5.5"}]});
        let m = &Protocol::Anthropic.parse_models(anthropic)[0];
        assert_eq!(m.description.as_deref(), Some("Claude Haiku 5.5"));

        let ollama = serde_json::json!({"models": [{"name": "qwen3:8b", "details": {"parameter_size": "8.2B", "quantization_level": "Q4_K_M"}}]});
        let m = &Protocol::Ollama.parse_models(ollama)[0];
        assert_eq!(m.description.as_deref(), Some("8.2B, Q4_K_M"));
    }

    #[test]
    fn strips_leading_think_block() {
        assert_eq!(
            strip_think("<think>\nhmm\n</think>\n\nls -la").trim(),
            "ls -la"
        );
        assert_eq!(strip_think("no thinking here"), "no thinking here");
        assert_eq!(strip_think("<think>unterminated"), "<think>unterminated");
    }

    #[test]
    fn extracts_error_messages_from_any_shape() {
        assert_eq!(
            error_message(r#"{"error":{"message":"bad key"}}"#),
            "bad key"
        );
        assert_eq!(
            error_message(r#"{"error":"model not found"}"#),
            "model not found"
        );
        assert_eq!(error_message("plain text"), "plain text");
        assert_eq!(error_message(""), "(no body)");
    }
}
