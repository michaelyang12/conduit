use super::{Call, JsonFormat, Parsed, temperature, usage};
use crate::response::{Reasoning, StopReason};
use serde_json::{Value, json};

pub(super) fn body(call: &Call) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = call.system {
        messages.push(json!({ "role": "system", "content": system }));
    }
    messages.push(json!({ "role": "user", "content": call.input }));
    let mut body = json!({ "model": call.model, "messages": messages });
    if let Some(n) = call.max_tokens {
        body["max_tokens"] = json!(n);
    }
    if let Some(t) = call.temperature {
        body["temperature"] = temperature(t);
    }
    match call.reasoning {
        Some(Reasoning::Low) => body["reasoning_effort"] = json!("low"),
        Some(Reasoning::Medium) => body["reasoning_effort"] = json!("medium"),
        Some(Reasoning::High) => body["reasoning_effort"] = json!("high"),
        Some(Reasoning::Off) | None => {}
    }
    match call.json {
        Some(JsonFormat::Any) => body["response_format"] = json!({ "type": "json_object" }),
        Some(JsonFormat::Schema(schema)) => {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": { "name": "response", "schema": schema }
            })
        }
        None => {}
    }
    body
}

pub(super) fn parse(body: Value) -> Result<Parsed, String> {
    let choice = body["choices"].get(0).ok_or("missing `choices`")?;
    let message = &choice["message"];
    let refusal = message["refusal"].as_str().filter(|r| !r.is_empty());
    let text = message["content"]
        .as_str()
        .or(refusal)
        .unwrap_or_default()
        .to_string();
    let stop = match choice["finish_reason"].as_str() {
        _ if refusal.is_some() => StopReason::Refusal,
        Some("stop") | None => StopReason::Complete,
        Some("length") => StopReason::MaxTokens,
        Some("content_filter") => StopReason::Refusal,
        Some(other) => StopReason::Other(other.to_string()),
    };
    Ok(Parsed {
        text,
        model: body["model"].as_str().map(str::to_string),
        stop,
        usage: usage(
            &body["usage"]["prompt_tokens"],
            &body["usage"]["completion_tokens"],
        ),
    })
}
