# AGENTS.md

conduit is a Rust library crate (edition 2024) that gives CLI tools one opinionated way to call an LLM: a prompt goes in, text or JSON comes out. It works with OpenAI, Anthropic, Ollama, any OpenAI-compatible server, and the Claude Code / Codex CLIs. Apps such as knock and suss depend on it through git.

## Commands

```sh
cargo build                                  # default features include the `setup` wizard
cargo build --no-default-features            # library without inquire/console
cargo test                                   # unit tests + tests/providers.rs (wiremock, no network)
cargo test --test providers ollama_round_trip   # one integration test
cargo test json::tests                       # one module's unit tests
cargo clippy --all-targets
cargo fmt
CONDUIT_PROVIDER=claude-code cargo run --example ask -- "your prompt"   # live smoke test
```

Before finishing any change, run: `cargo fmt && cargo clippy --all-targets && cargo test`

## Layout

| Path | What |
|---|---|
| `src/provider.rs` | `Protocol` enum, `PRESETS` table of built-in providers, `Provider::resolve` |
| `src/protocol.rs`, `src/protocol/*.rs` | One file per HTTP wire format: `body(&Call) -> Value` and `parse(Value) -> Parsed` |
| `src/cli.rs` | `claude-code` / `codex` protocols, run as subprocesses |
| `src/client.rs` | `Conduit` (HTTP client, error mapping) and the `Request` builder; the retry that drops rejected params (`relax`) |
| `src/config.rs` | TOML config: global `~/.config/conduit/config.toml` layered under `~/.config/conduit/<app>/config.toml` |
| `src/response.rs` | `Response`, `StopReason`, `Reasoning`; `Response::into_text` is the output policy behind `.text()` / `.json()` |
| `src/setup.rs` | Interactive wizard, behind the `setup` feature |
| `tests/providers.rs` | Wiremock round trips for each protocol plus error mapping |

## Conventions

- Providers are data and protocols are code. A new OpenAI-compatible provider is one `Preset` entry in `PRESETS` with no new code. A new wire format is a new `Protocol` variant plus a `src/protocol/<name>.rs` that follows `src/protocol/ollama.rs`.
- Stay app-agnostic. Nothing in this crate should know about knock, suss, or any one app's prompts. App-specific behavior belongs on the request builder (`.system`, `.context`, `.max_tokens`, `.reasoning`), not in defaults.
- Request settings are `Option`s, and unset means "provider default". Only send a field when the caller set it.
- Errors are one `Error` enum (thiserror) whose messages are shown to end users as-is, so include the fix in the message (`run \`ollama pull X\``, the env var to export). The library never prints, except in `setup.rs`, and never exits the process.
- Map HTTP failures in `Conduit::status_error` / `transport_error`, not inside protocol modules. Protocol `parse` returns `Result<Parsed, String>`, and the client wraps that in `Error::Decode`.
- Optional parameters a model rejects (temperature, reasoning, JSON mode) are dropped and retried in `relax`. Extend its keyword lists rather than adding model-name checks.
- `temperature` is never sent to Anthropic, because current Claude models return a 400 for sampling params.
- CLI protocols run in `std::env::temp_dir()` with tools disabled and no saved session or user config. This keeps the user's project files and hooks out of the prompt.
- Build JSON bodies with `serde_json::json!` and read responses by indexing into `Value`. Don't add typed structs for each provider's schema.
- Tests: pure logic gets inline `#[cfg(test)]` modules; anything over HTTP goes in `tests/providers.rs` against `MockServer`. Code that reads the environment takes an `env` closure so tests don't need `set_var` (`client::from_config_with_env`).

## Don'ts

- Don't reuse or extract OAuth tokens from the Claude Code or Codex sign-in. Go through the CLIs' supported headless modes (`claude -p`, `codex exec`).
- Don't hardcode model lists as the source of truth. The wizard lists models live through `list_models`, and `suggested_models` is only a fallback.
- Don't add comments except for non-obvious flow or TODOs.

## Git

Conventional-commit prefixes (`feat:`, `docs:`, `fix:`). Commit or push only when asked.
