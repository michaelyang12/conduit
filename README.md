# conduit

Opinionated LLM client for CLI tools. One prompt in, text or JSON out, against whichever provider the user picked. You choose the prompt; conduit handles provider config, the setup wizard, request formats, retries and readable errors.

Used by [knock](https://github.com/michaelyang12/knock) and [suss](https://github.com/michaelyang12/suss).

```toml
[dependencies]
conduit = { git = "https://github.com/michaelyang12/conduit.git" }
```

## Usage

```rust
use conduit::{Conduit, Instructions, Reasoning};

let llm = Conduit::load("my-app")?;

let instructions = Instructions::builder()
    .role("You convert requests into a single shell command.")
    .format("Only the command. No explanation, no markdown.")
    .rule("Prefer tools that ship with the OS.")
    .example("show disk usage of this folder", "du -sh .")
    .build();

let command = llm
    .prompt("find the 5 largest files here")
    .system(&instructions)
    .context("os", "macOS")
    .context("shell", "zsh")
    .max_tokens(256)
    .reasoning(Reasoning::Off)
    .text()
    .await?;
```

Every builder setting is optional. If you leave one unset, the provider's default is used.

### Typed JSON

```rust
#[derive(serde::Deserialize)]
struct Diagnosis { cause: String, fix: String }

let d: Diagnosis = llm
    .prompt(error_output)
    .system("Diagnose the error.")
    .schema(serde_json::json!({
        "type": "object",
        "properties": { "cause": {"type": "string"}, "fix": {"type": "string"} },
        "required": ["cause", "fix"]
    }))
    .json()
    .await?;
```

Where the provider supports it, conduit uses its built-in JSON mode or schema enforcement. It also tells the model to answer in JSON, and when parsing it tolerates code fences and extra text around the JSON. `.schema()` is optional.

### Full response

`.send()` returns a `Response` with `text`, `model`, `stop` (`Complete`, `MaxTokens`, `Refusal`), `usage` and `elapsed`, for when you want to handle truncated output yourself.

### Setup wizard

```rust
conduit::setup::run("my-app").await?;
```

The wizard lets the user pick a provider, lists that provider's models live, checks credentials, saves the choice and sends a test prompt. It's behind the default `setup` feature; use `default-features = false` to drop it.

## Providers

| Name | How it connects | Credentials |
|---|---|---|
| `openai` | Responses API | `OPENAI_API_KEY` |
| `anthropic` | Messages API | `ANTHROPIC_API_KEY` |
| `claude-code` | `claude -p` subprocess | your Claude Code sign-in |
| `codex` | `codex exec` subprocess | your Codex sign-in |
| `ollama` | native `/api/chat` at `localhost:11434` | none |
| `lmstudio` | Chat Completions at `localhost:1234/v1` | none |
| `gemini`, `groq`, `openrouter`, `deepseek` | Chat Completions | `GEMINI_API_KEY`, `GROQ_API_KEY`, ... |

The `claude-code` and `codex` providers use whatever account the CLI is signed in with, including subscriptions. They're slower (2-4s of startup) and ignore `max_tokens` and `temperature`.

## Config

The wizard writes `~/.config/conduit/<app>/config.toml`. Settings in `~/.config/conduit/config.toml` apply to every app, and each app's file overrides them field by field.

```toml
provider = "ollama"

[providers.ollama]
model = "qwen3:8b"
endpoint = "http://gpu-box:11434"

[providers.work]                  # any OpenAI-compatible server
protocol = "chat-completions"     # or openai-responses, anthropic, ollama
endpoint = "https://llm.internal.example.com/v1"
api_key_env = "WORK_LLM_KEY"
model = "gpt-5.1"
```

`CONDUIT_PROVIDER` and `CONDUIT_MODEL` override the config for one run:

```sh
CONDUIT_PROVIDER=ollama CONDUIT_MODEL=llama3.2 my-app "..."
```

## Errors

Every failure is a `conduit::Error`. The messages are written for end users and include the fix:

```text
could not reach ollama at http://localhost:11434/api/chat (is Ollama running? start it with `ollama serve`)
model 'qwen3:8b' not found on ollama (run `ollama pull qwen3:8b`)
OPENAI_API_KEY is not set (needed for openai)
```

Match on variants (`MissingApiKey`, `Unreachable`, `ModelNotFound`, `RateLimited`, `Truncated`, `Json`, ...) when you want to handle a case differently. If a model rejects an optional setting (temperature, reasoning or JSON mode), conduit drops it and retries once. conduit never prints anything outside the setup wizard and never exits the process.
