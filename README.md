# conduit

Shared LLM provider infrastructure for CLI tools. Handles provider config, setup wizard, and request dispatch so each tool just provides its own instructions and input.

Used by [knock](https://github.com/michaelyang12/knock) and [suss](https://github.com/michaelyang12/suss).

## Usage

Add to your `Cargo.toml`:

```toml
conduit = { git = "https://github.com/michaelyang12/conduit.git", path = "../conduit" }
```

### Make a request

```rust
use conduit::{Config, Instructions, RequestOptions, request};

let config = Config::load("my-app");

let instructions = Instructions::builder()
    .role("command-line translation assistant")
    .output_format("Return ONLY the command. No explanation.")
    .constraint("use the detected OS and shell context")
    .build();

let response = request(&config, &instructions, "list large files", RequestOptions::default()).await?;
println!("{}", response);
```

### Typed JSON responses

```rust
use conduit::request_json;

#[derive(serde::Deserialize)]
struct Diagnosis {
    cause: String,
    fix: String,
}

let result: Diagnosis = request_json(&config, &instructions, "the error output", opts).await?;
```

### Setup wizard

```rust
// Basic — uses default model lists
conduit::setup::run_setup("my-app", None);

// Custom — add extra models
let options = conduit::setup::SetupOptions {
    extra_openai_models: vec!["o3-mini".to_string()],
    ..Default::default()
};
conduit::setup::run_setup("my-app", Some(options));
```

Config is stored per-app at `~/.config/conduit/<app-name>/config.json`.

### Instructions builder

```rust
let instructions = Instructions::builder()
    .role("error diagnosis assistant")
    .output_format("CAUSE: one sentence\nFIX: the concrete fix")
    .mode("standard", "Concise explanation + one fix.")
    .mode("verbose", "Detailed explanation, common causes, docs.")
    .constraint("no markdown formatting")
    .constraint("don't repeat the error back")
    .build();
```

Or pass a raw string:

```rust
let response = request(&config, "You are a helpful assistant.", input, opts).await?;
```

## Providers

- **OpenAI** — default: `gpt-5.1`, env: `OPENAI_API_KEY`
- **Anthropic** — default: `claude-sonnet-4-20250514`, env: `ANTHROPIC_API_KEY`
- **Ollama** — default: `llama3.2` at `http://localhost:11434`, no API key

## Error handling

```rust
match conduit::request(&config, &instructions, input, opts).await {
    Ok(response) => println!("{}", response),
    Err(conduit::Error::MissingApiKey(key)) => eprintln!("{} not set", key),
    Err(conduit::Error::ProviderError { status: 429, .. }) => eprintln!("rate limited"),
    Err(e) => eprintln!("error: {}", e),
}
```

conduit never prints to stderr or calls `process::exit()` — your app controls the UX.
