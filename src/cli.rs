use crate::error::{Error, Result};
use crate::protocol::{Call, JsonFormat};
use crate::provider::{ModelInfo, Protocol, Provider};
use crate::response::{Reasoning, Response, StopReason, Usage};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Without a system prompt, `claude -p` would run with its coding-agent prompt.
const DEFAULT_CLAUDE_SYSTEM: &str =
    "You are a helpful assistant. Answer the user's request directly.";

pub(crate) async fn run(
    provider: &Provider,
    call: &Call<'_>,
    timeout: Duration,
) -> Result<Response> {
    let started = Instant::now();
    match provider.protocol {
        Protocol::ClaudeCode => {
            let output = exec(provider, &claude_args(call), call.input, timeout).await?;
            let mut response = parse_claude(&provider.name, &output)?;
            if response.model.is_empty() {
                response.model = call.model.to_string();
            }
            response.elapsed = started.elapsed();
            Ok(response)
        }
        Protocol::Codex => {
            let out = TempFile::new("txt");
            let schema = match call.json {
                Some(JsonFormat::Schema(schema)) => {
                    let file = TempFile::new("json");
                    std::fs::write(&file.0, schema.to_string())
                        .map_err(|e| cli_error(provider, e))?;
                    Some(file)
                }
                _ => None,
            };
            let args = codex_args(call, &out.0, schema.as_ref().map(|f| f.0.as_path()));
            let output = exec(provider, &args, call.input, timeout).await?;
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() {
                return Err(codex_error(&provider.name, &stderr));
            }
            let text = std::fs::read_to_string(&out.0).map_err(|e| cli_error(provider, e))?;
            Ok(Response {
                text: text.trim().to_string(),
                provider: provider.name.clone(),
                model: codex_model(&stderr).unwrap_or(call.model).to_string(),
                stop: StopReason::Complete,
                usage: None,
                elapsed: started.elapsed(),
            })
        }
        _ => unreachable!("cli::run called for an HTTP protocol"),
    }
}

/// Reads Codex's model catalog. Only models Codex itself lists in its picker are returned.
pub(crate) async fn codex_models(provider: &Provider) -> Result<Vec<ModelInfo>> {
    let output = exec(
        provider,
        &["debug".into(), "models".into()],
        "",
        Duration::from_secs(20),
    )
    .await?;
    if !output.status.success() {
        return Err(codex_error(
            &provider.name,
            &String::from_utf8_lossy(&output.stderr),
        ));
    }
    let body: Value =
        serde_json::from_slice(&output.stdout).map_err(|e| cli_error(provider, e.into()))?;
    Ok(parse_codex_models(&body))
}

fn parse_codex_models(body: &Value) -> Vec<ModelInfo> {
    let mut listed: Vec<&Value> = body["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["visibility"] == "list")
        .collect();
    listed.sort_by_key(|m| m["priority"].as_i64().unwrap_or(i64::MAX));
    listed
        .into_iter()
        .filter_map(|m| {
            let id = m["slug"].as_str()?;
            let description = m["description"].as_str().map(str::to_string);
            Some(ModelInfo::new(id, description))
        })
        .collect()
}

async fn exec(
    provider: &Provider,
    args: &[String],
    stdin: &str,
    timeout: Duration,
) -> Result<std::process::Output> {
    let mut child = Command::new(&provider.endpoint)
        .args(args)
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::CommandNotFound {
                provider: provider.name.clone(),
                command: provider.endpoint.clone(),
            },
            _ => cli_error(provider, e),
        })?;
    let mut pipe = child.stdin.take().expect("stdin is piped");
    pipe.write_all(stdin.as_bytes())
        .await
        .map_err(|e| cli_error(provider, e))?;
    drop(pipe);
    tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| Error::Timeout {
            provider: provider.name.clone(),
            secs: timeout.as_secs(),
        })?
        .map_err(|e| cli_error(provider, e))
}

fn claude_args(call: &Call) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--output-format",
        "json",
        "--tools",
        "",
        "--no-session-persistence",
        "--strict-mcp-config",
        "--system-prompt",
        call.system.unwrap_or(DEFAULT_CLAUDE_SYSTEM),
    ]
    .map(String::from)
    .into();
    if !call.model.is_empty() {
        args.extend(["--model".into(), call.model.into()]);
    }
    if let Some(r) = call.reasoning {
        args.extend(["--effort".into(), effort(r).into()]);
    }
    if let Some(JsonFormat::Schema(schema)) = call.json {
        args.extend(["--json-schema".into(), schema.to_string()]);
    }
    args
}

fn parse_claude(provider: &str, output: &std::process::Output) -> Result<Response> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let body: Value = serde_json::from_str(stdout.trim()).map_err(|_| {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        Error::Cli {
            provider: provider.to_string(),
            message: detail.chars().take(500).collect(),
        }
    })?;
    let result = body["result"].as_str().unwrap_or_default().to_string();
    if body["is_error"].as_bool().unwrap_or(false) {
        let lower = result.to_lowercase();
        return Err(if lower.contains("selected model") {
            Error::ModelNotFound {
                provider: provider.to_string(),
                model: String::new(),
                hint: format!(": {result}"),
            }
        } else if lower.contains("log in") || lower.contains("login") {
            Error::Unauthorized {
                provider: provider.to_string(),
                status: 401,
                message: format!("{result} (run `claude` and sign in)"),
            }
        } else {
            Error::Cli {
                provider: provider.to_string(),
                message: result,
            }
        });
    }
    let text = match &body["structured_output"] {
        Value::Null => result,
        structured => structured.to_string(),
    };
    let stop = match body["stop_reason"].as_str() {
        Some("max_tokens") => StopReason::MaxTokens,
        Some("refusal") => StopReason::Refusal,
        _ => StopReason::Complete,
    };
    let model = body["modelUsage"]
        .as_object()
        .and_then(|m| m.keys().next().cloned())
        .unwrap_or_default();
    let usage = Some(Usage {
        input_tokens: body["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
        output_tokens: body["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
    });
    Ok(Response {
        text: text.trim().to_string(),
        provider: provider.to_string(),
        model,
        stop,
        usage,
        elapsed: Duration::ZERO,
    })
}

fn codex_args(call: &Call, out: &Path, schema: Option<&Path>) -> Vec<String> {
    let mut args: Vec<String> = [
        "exec",
        "--skip-git-repo-check",
        "--ephemeral",
        "--ignore-rules",
        "--ignore-user-config",
        "--sandbox",
        "read-only",
        "--color",
        "never",
    ]
    .map(String::from)
    .into();
    args.extend(["--output-last-message".into(), out.display().to_string()]);
    if !call.model.is_empty() {
        args.extend(["--model".into(), call.model.into()]);
    }
    if let Some(system) = call.system {
        let quoted = Value::String(system.to_string()).to_string();
        args.extend(["-c".into(), format!("developer_instructions={quoted}")]);
    }
    if let Some(r) = call.reasoning {
        args.extend([
            "-c".into(),
            format!("model_reasoning_effort=\"{}\"", effort(r)),
        ]);
    }
    if let Some(path) = schema {
        args.extend(["--output-schema".into(), path.display().to_string()]);
    }
    args
}

fn codex_model(stderr: &str) -> Option<&str> {
    stderr
        .lines()
        .find_map(|line| line.strip_prefix("model: "))
        .map(str::trim)
}

fn codex_error(provider: &str, stderr: &str) -> Error {
    let tail: String = stderr
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(5)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    let lower = tail.to_lowercase();
    if lower.contains("login") || lower.contains("log in") || lower.contains("not authenticated") {
        return Error::Unauthorized {
            provider: provider.to_string(),
            status: 401,
            message: format!("{tail} (run `codex login`)"),
        };
    }
    Error::Cli {
        provider: provider.to_string(),
        message: tail,
    }
}

fn effort(r: Reasoning) -> &'static str {
    match r {
        Reasoning::Off | Reasoning::Low => "low",
        Reasoning::Medium => "medium",
        Reasoning::High => "high",
    }
}

fn cli_error(provider: &Provider, e: std::io::Error) -> Error {
    Error::Cli {
        provider: provider.name.clone(),
        message: e.to_string(),
    }
}

struct TempFile(PathBuf);

impl TempFile {
    fn new(ext: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let name = format!("conduit-{}-{nanos}-{n}.{ext}", std::process::id());
        Self(std::env::temp_dir().join(name))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call<'a>(system: Option<&'a str>, json: Option<&'a JsonFormat>) -> Call<'a> {
        Call {
            model: "haiku",
            system,
            input: "hi",
            max_tokens: None,
            temperature: None,
            reasoning: Some(Reasoning::Off),
            json,
        }
    }

    #[test]
    fn claude_runs_without_tools_or_sessions() {
        let schema = JsonFormat::Schema(serde_json::json!({"type": "object"}));
        let args = claude_args(&call(Some("be brief"), Some(&schema)));
        let joined = args.join(" ");
        assert!(joined.contains("--tools  --no-session-persistence"));
        assert!(joined.contains("--system-prompt be brief"));
        assert!(joined.contains("--model haiku"));
        assert!(joined.contains("--effort low"));
        assert!(joined.contains(r#"--json-schema {"type":"object"}"#));
    }

    #[test]
    fn codex_quotes_instructions_as_toml() {
        let out = PathBuf::from("/tmp/out.txt");
        let args = codex_args(&call(Some("say \"hi\"\nthen stop"), None), &out, None);
        assert!(args.contains(&r#"developer_instructions="say \"hi\"\nthen stop""#.to_string()));
        assert!(args.contains(&"read-only".to_string()));
        assert!(args.contains(&r#"model_reasoning_effort="low""#.to_string()));
    }

    #[test]
    fn codex_catalog_keeps_listed_models_in_priority_order() {
        let body = serde_json::json!({"models": [
            {"slug": "hidden", "visibility": "hide", "priority": 1},
            {"slug": "gpt-5.6-terra", "visibility": "list", "priority": 3, "description": "Legacy"},
            {"slug": "gpt-6-luna", "visibility": "list", "priority": 2, "description": "Fast"}
        ]});
        let ids: Vec<String> = parse_codex_models(&body)
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, ["gpt-6-luna", "gpt-5.6-terra"]);
    }

    #[test]
    fn reads_codex_model_from_banner() {
        let stderr = "OpenAI Codex v0.162.1\n--------\nmodel: gpt-6-luna\nprovider: openai\n";
        assert_eq!(codex_model(stderr), Some("gpt-6-luna"));
    }
}
