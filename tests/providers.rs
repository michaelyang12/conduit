use conduit::{Conduit, Error, Protocol, Provider, Reasoning, StopReason};
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn conduit(server: &MockServer, protocol: Protocol, prefix: &str) -> Conduit {
    let provider = Provider::new("mock", protocol, format!("{}{prefix}", server.uri()))
        .with_api_key("test-key")
        .with_model("test-model");
    Conduit::new(provider)
}

#[tokio::test]
async fn openai_responses_round_trip() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(header("authorization", "Bearer test-key"))
        .and(body_partial_json(json!({
            "model": "test-model",
            "instructions": "be brief",
            "input": "hi",
            "max_output_tokens": 50,
            "reasoning": { "effort": "none" },
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "test-model-2026",
            "status": "completed",
            "output": [
                { "type": "reasoning", "summary": [] },
                { "type": "message", "content": [{ "type": "output_text", "text": " hello " }] }
            ],
            "usage": { "input_tokens": 5, "output_tokens": 1 }
        })))
        .mount(&server)
        .await;

    let r = conduit(&server, Protocol::OpenaiResponses, "/v1")
        .prompt("hi")
        .system("be brief")
        .max_tokens(50)
        .reasoning(Reasoning::Off)
        .send()
        .await
        .unwrap();
    assert_eq!(r.text, "hello");
    assert_eq!(r.model, "test-model-2026");
    assert_eq!(r.stop, StopReason::Complete);
    assert_eq!(r.usage.unwrap().output_tokens, 1);
}

#[tokio::test]
async fn openai_incomplete_maps_to_max_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "incomplete",
            "incomplete_details": { "reason": "max_output_tokens" },
            "output": [{ "type": "reasoning", "summary": [] }]
        })))
        .mount(&server)
        .await;
    let r = conduit(&server, Protocol::OpenaiResponses, "")
        .prompt("hi")
        .send()
        .await
        .unwrap();
    assert_eq!(r.stop, StopReason::MaxTokens);
    assert_eq!(r.text, "");
}

#[tokio::test]
async fn chat_completions_round_trip_and_think_stripping() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .and(body_partial_json(json!({
            "messages": [
                { "role": "system", "content": "sys" },
                { "role": "user", "content": "<context>\n<os>macOS</os>\n</context>\n\n<input>\nhi\n</input>" }
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": { "content": "<think>planning</think>\nls -la" },
                "finish_reason": "stop"
            }]
        })))
        .mount(&server)
        .await;

    let r = conduit(&server, Protocol::ChatCompletions, "/v1")
        .prompt("hi")
        .system("sys")
        .context("os", "macOS")
        .send()
        .await
        .unwrap();
    assert_eq!(r.text, "ls -la");
    assert_eq!(r.model, "test-model");
}

#[tokio::test]
async fn anthropic_round_trip() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "test-key"))
        .and(header("anthropic-version", "2023-06-01"))
        .and(body_partial_json(json!({
            "model": "test-model",
            "system": "sys",
            "max_tokens": 16000,
            "output_config": { "effort": "high" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "test-model",
            "content": [
                { "type": "thinking", "thinking": "" },
                { "type": "text", "text": "part one, " },
                { "type": "text", "text": "part two" }
            ],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 3, "output_tokens": 4 }
        })))
        .mount(&server)
        .await;

    let r = conduit(&server, Protocol::Anthropic, "/v1")
        .prompt("hi")
        .system("sys")
        .temperature(0.3)
        .reasoning(Reasoning::High)
        .send()
        .await
        .unwrap();
    assert_eq!(r.text, "part one, part two");
    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(
        body.get("temperature").is_none(),
        "temperature is never sent to Anthropic"
    );
}

#[tokio::test]
async fn ollama_round_trip() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .and(body_partial_json(json!({
            "stream": false,
            "think": false,
            "options": { "num_predict": 64, "temperature": 0.0 }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "test-model",
            "message": { "role": "assistant", "content": "1 2 3" },
            "done_reason": "length",
            "prompt_eval_count": 10,
            "eval_count": 64
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "models": [{ "name": "qwen3:8b" }, { "name": "llama3.2:latest" }]
        })))
        .mount(&server)
        .await;

    let c = Conduit::new(
        Provider::new("ollama", Protocol::Ollama, server.uri()).with_model("test-model"),
    );
    let r = c
        .prompt("count")
        .max_tokens(64)
        .temperature(0.0)
        .reasoning(Reasoning::Off)
        .send()
        .await
        .unwrap();
    assert_eq!(r.text, "1 2 3");
    assert_eq!(r.stop, StopReason::MaxTokens);
    assert_eq!(
        c.list_models().await.unwrap(),
        vec!["llama3.2:latest", "qwen3:8b"]
    );
}

#[tokio::test]
async fn ollama_missing_model_suggests_pull() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(404).set_body_json(
            json!({ "error": "model \"qwen3:8b\" not found, try pulling it first" }),
        ))
        .mount(&server)
        .await;
    let c = Conduit::new(
        Provider::new("ollama", Protocol::Ollama, server.uri()).with_model("qwen3:8b"),
    );
    let err = c.prompt("hi").send().await.unwrap_err();
    assert!(matches!(err, Error::ModelNotFound { .. }));
    assert!(err.to_string().contains("ollama pull qwen3:8b"), "{err}");
}

#[tokio::test]
async fn unreachable_ollama_hints_at_serve() {
    let c = Conduit::new(
        Provider::new("ollama", Protocol::Ollama, "http://127.0.0.1:9").with_model("m"),
    );
    let err = c.prompt("hi").send().await.unwrap_err();
    assert!(matches!(err, Error::Unreachable { .. }), "{err:?}");
    assert!(err.to_string().contains("ollama serve"), "{err}");
}

#[tokio::test]
async fn rejected_temperature_is_dropped_and_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "temperature": 0.2 })))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "message": "Unsupported parameter: 'temperature' is not supported with this model.", "param": "temperature" }
        })))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "completed",
            "output": [{ "type": "message", "content": [{ "type": "output_text", "text": "ok" }] }]
        })))
        .with_priority(2)
        .mount(&server)
        .await;

    let r = conduit(&server, Protocol::OpenaiResponses, "")
        .prompt("hi")
        .temperature(0.2)
        .send()
        .await
        .unwrap();
    assert_eq!(r.text, "ok");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn status_codes_map_to_typed_errors() {
    for (status, check) in [
        (
            401,
            (|e: &Error| matches!(e, Error::Unauthorized { .. })) as fn(&Error) -> bool,
        ),
        (429, |e| matches!(e, Error::RateLimited { .. })),
        (500, |e| matches!(e, Error::Api { status: 500, .. })),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_json(json!({ "error": { "message": "nope" } })),
            )
            .mount(&server)
            .await;
        let err = conduit(&server, Protocol::ChatCompletions, "")
            .prompt("hi")
            .send()
            .await
            .unwrap_err();
        assert!(check(&err), "{status}: {err:?}");
    }
}

#[tokio::test]
async fn missing_model_is_reported_before_any_request() {
    let c = Conduit::new(Provider::new(
        "lmstudio",
        Protocol::ChatCompletions,
        "http://127.0.0.1:9",
    ));
    assert!(matches!(
        c.prompt("hi").send().await,
        Err(Error::MissingModel { .. })
    ));
}
