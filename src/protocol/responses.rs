use super::{Call, JsonFormat, Parsed, temperature, usage};
use crate::response::{Reasoning, StopReason};
use serde_json::{Value, json};

pub(super) fn body(call: &Call) -> Value {
    let mut body = json!({
        "model": call.model,
        "input": call.input,
        "store": false,
    });
    if let Some(system) = call.system {
        body["instructions"] = json!(system);
    }
    if let Some(n) = call.max_tokens {
        body["max_output_tokens"] = json!(n);
    }
    if let Some(t) = call.temperature {
        body["temperature"] = temperature(t);
    }
    if let Some(r) = call.reasoning {
        let effort = match r {
            Reasoning::Off => "none",
            Reasoning::Low => "low",
            Reasoning::Medium => "medium",
            Reasoning::High => "high",
        };
        body["reasoning"] = json!({ "effort": effort });
    }
    match call.json {
        Some(JsonFormat::Any) => body["text"] = json!({ "format": { "type": "json_object" } }),
        Some(JsonFormat::Schema(schema)) => {
            body["text"] = json!({
                "format": { "type": "json_schema", "name": "response", "schema": schema, "strict": false }
            })
        }
        None => {}
    }
    body
}

pub(super) fn parse(body: Value) -> Result<Parsed, String> {
    let output = body["output"].as_array().ok_or("missing `output`")?;
    let mut text = String::new();
    let mut refused = false;
    for part in output
        .iter()
        .filter(|item| item["type"] == "message")
        .filter_map(|item| item["content"].as_array())
        .flatten()
    {
        match part["type"].as_str() {
            Some("output_text") => text.push_str(part["text"].as_str().unwrap_or_default()),
            Some("refusal") => {
                refused = true;
                text.push_str(part["refusal"].as_str().unwrap_or_default());
            }
            _ => {}
        }
    }
    let stop = match (
        body["status"].as_str(),
        body["incomplete_details"]["reason"].as_str(),
    ) {
        _ if refused => StopReason::Refusal,
        (Some("completed"), _) => StopReason::Complete,
        (_, Some("max_output_tokens")) => StopReason::MaxTokens,
        (_, Some("content_filter")) => StopReason::Refusal,
        (status, reason) => StopReason::Other(reason.or(status).unwrap_or("unknown").to_string()),
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
