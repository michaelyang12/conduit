use super::{Call, JsonFormat, Parsed, temperature, usage};
use crate::response::{Reasoning, StopReason};
use serde_json::{Value, json};

pub(super) fn body(call: &Call) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = call.system {
        messages.push(json!({ "role": "system", "content": system }));
    }
    messages.push(json!({ "role": "user", "content": call.input }));
    let mut body = json!({ "model": call.model, "messages": messages, "stream": false });

    let mut options = serde_json::Map::new();
    if let Some(n) = call.max_tokens {
        options.insert("num_predict".into(), json!(n));
    }
    if let Some(t) = call.temperature {
        options.insert("temperature".into(), temperature(t));
    }
    if !options.is_empty() {
        body["options"] = Value::Object(options);
    }
    match call.reasoning {
        Some(Reasoning::Off) => body["think"] = json!(false),
        Some(_) => body["think"] = json!(true),
        None => {}
    }
    match call.json {
        Some(JsonFormat::Any) => body["format"] = json!("json"),
        Some(JsonFormat::Schema(schema)) => body["format"] = schema.clone(),
        None => {}
    }
    body
}

pub(super) fn parse(body: Value) -> Result<Parsed, String> {
    let text = body["message"]["content"]
        .as_str()
        .ok_or("missing `message.content`")?
        .to_string();
    let stop = match body["done_reason"].as_str() {
        Some("stop") | None => StopReason::Complete,
        Some("length") => StopReason::MaxTokens,
        Some(other) => StopReason::Other(other.to_string()),
    };
    Ok(Parsed {
        text,
        model: body["model"].as_str().map(str::to_string),
        stop,
        usage: usage(&body["prompt_eval_count"], &body["eval_count"]),
    })
}
