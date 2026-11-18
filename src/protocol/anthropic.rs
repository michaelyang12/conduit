use super::{Call, JsonFormat, Parsed, usage};
use crate::response::{Reasoning, StopReason};
use serde_json::{Value, json};

const DEFAULT_MAX_TOKENS: u32 = 16_000;

/// Models that accept server-side refusal fallbacks; elsewhere the beta field is rejected.
const FALLBACK_MODELS: &[&str] = &[
    "claude-fable-5-2",
    "claude-fable-5-1",
    "claude-opus-5-5",
    "claude-opus-5",
    "claude-sonnet-5-5",
];

fn supports_fallbacks(model: &str) -> bool {
    FALLBACK_MODELS.contains(&model)
}

pub(super) fn extra_headers(call: &Call) -> Vec<(&'static str, &'static str)> {
    if supports_fallbacks(call.model) {
        vec![("anthropic-beta", "server-side-fallback-2026-07-01")]
    } else {
        Vec::new()
    }
}

/// `temperature` is never sent: current Claude models reject sampling parameters.
pub(super) fn body(call: &Call) -> Value {
    let mut body = json!({
        "model": call.model,
        "max_tokens": call.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "messages": [{ "role": "user", "content": call.input }],
    });
    if let Some(system) = call.system {
        body["system"] = json!(system);
    }
    if supports_fallbacks(call.model) {
        body["fallbacks"] = json!("default");
    }
    let mut output_config = serde_json::Map::new();
    if let Some(r) = call.reasoning {
        let effort = match r {
            Reasoning::Off | Reasoning::Low => "low",
            Reasoning::Medium => "medium",
            Reasoning::High => "high",
        };
        output_config.insert("effort".into(), json!(effort));
    }
    if let Some(JsonFormat::Schema(schema)) = call.json {
        output_config.insert(
            "format".into(),
            json!({ "type": "json_schema", "schema": schema }),
        );
    }
    if !output_config.is_empty() {
        body["output_config"] = Value::Object(output_config);
    }
    body
}

pub(super) fn parse(body: Value) -> Result<Parsed, String> {
    let content = body["content"].as_array().ok_or("missing `content`")?;
    let text: String = content
        .iter()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect();
    let stop = match body["stop_reason"].as_str() {
        Some("end_turn" | "stop_sequence") | None => StopReason::Complete,
        Some("max_tokens") => StopReason::MaxTokens,
        Some("refusal") => StopReason::Refusal,
        Some(other) => StopReason::Other(other.to_string()),
    };
    Ok(Parsed {
        text,
        model: body["model"].as_str().map(str::to_string),
        stop,
        usage: usage(
            &body["usage"]["input_tokens"],
            &body["usage"]["output_tokens"],
        ),
    })
}
