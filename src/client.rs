use crate::config::{Config, Provider};
use crate::instructions::Instructions;
use async_openai::{
    Client, config::OpenAIConfig, types::responses::CreateResponseArgs,
};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Options for an LLM request.
pub struct RequestOptions {
    pub max_tokens: u32,
    pub temperature: f32,
    /// Override the model from config. None = use config's model.
    pub model: Option<String>,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self {
            max_tokens: 512,
            temperature: 0.2,
            model: None,
        }
    }
}

/// Errors returned by conduit requests.
#[derive(Debug)]
pub enum Error {
    /// API key environment variable not set
    MissingApiKey(String),
    /// Couldn't reach the provider
    NetworkError(String),
    /// Provider returned non-200 status
    ProviderError { status: u16, message: String },
    /// request_json couldn't parse the response
    ParseError { raw: String, source: serde_json::Error },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::MissingApiKey(key) => write!(f, "{} not set", key),
            Error::NetworkError(msg) => write!(f, "network error: {}", msg),
            Error::ProviderError { status, message } => {
                write!(f, "provider error ({}): {}", status, message)
            }
            Error::ParseError { source, .. } => {
                write!(f, "failed to parse response: {}", source)
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::ParseError { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Send a request and get a string response.
pub async fn request(
    config: &Config,
    instructions: impl Into<Instructions>,
    input: &str,
    options: RequestOptions,
) -> Result<String, Error> {
    let instructions = instructions.into();
    match config.provider {
        Provider::OpenAI => request_openai(config, &instructions, input, &options).await,
        Provider::Anthropic => request_anthropic(config, &instructions, input, &options).await,
        Provider::Ollama => request_ollama(config, &instructions, input, &options).await,
    }
}

/// Send a request and parse the response as JSON into type T.
pub async fn request_json<T: serde::de::DeserializeOwned>(
    config: &Config,
    instructions: impl Into<Instructions>,
    input: &str,
    options: RequestOptions,
) -> Result<T, Error> {
    let instructions = instructions.into();
    // Append JSON instruction
    let json_suffix = "\n\nIMPORTANT: Respond with valid JSON only. No markdown, no code fences, no explanation. Just the JSON object.";
    let augmented = format!("{}{}", instructions.as_str(), json_suffix);
    let instructions = Instructions::from(augmented);

    let raw = request(config, instructions, input, options).await?;

    // Try to parse, stripping common LLM wrapping
    let cleaned = raw.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    serde_json::from_str(cleaned).map_err(|e| Error::ParseError {
        raw: raw.clone(),
        source: e,
    })
}

async fn request_openai(
    config: &Config,
    instructions: &Instructions,
    input: &str,
    options: &RequestOptions,
) -> Result<String, Error> {
    let model = options.model.as_deref().unwrap_or(config.openai_model());
    let client: Client<OpenAIConfig> = Client::new();

    let request = CreateResponseArgs::default()
        .model(model)
        .instructions(instructions.as_str())
        .input(input)
        .temperature(options.temperature)
        .max_output_tokens(options.max_tokens)
        .build()
        .map_err(|e| Error::NetworkError(e.to_string()))?;

    let response = client
        .responses()
        .create(request)
        .await
        .map_err(|e| Error::NetworkError(e.to_string()))?;

    response
        .output_text()
        .map(|t| t.clone())
        .ok_or_else(|| Error::ProviderError {
            status: 200,
            message: "empty response".to_string(),
        })
}

async fn request_anthropic(
    config: &Config,
    instructions: &Instructions,
    input: &str,
    options: &RequestOptions,
) -> Result<String, Error> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| Error::MissingApiKey("ANTHROPIC_API_KEY".to_string()))?;

    let model = options.model.as_deref().unwrap_or(config.anthropic_model());

    let request_body = AnthropicRequest {
        model: model.to_string(),
        max_tokens: options.max_tokens,
        system: instructions.as_str().to_string(),
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: input.to_string(),
        }],
    };

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| Error::NetworkError(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let message = response.text().await.unwrap_or_default();
        return Err(Error::ProviderError { status, message });
    }

    let body: AnthropicResponse = response
        .json()
        .await
        .map_err(|e| Error::NetworkError(e.to_string()))?;

    body.content
        .first()
        .map(|c| c.text.clone())
        .ok_or_else(|| Error::ProviderError {
            status: 200,
            message: "empty response".to_string(),
        })
}

async fn request_ollama(
    config: &Config,
    instructions: &Instructions,
    input: &str,
    options: &RequestOptions,
) -> Result<String, Error> {
    let model = options.model.as_deref().unwrap_or(config.ollama_model());
    let url = format!("{}/api/chat", config.ollama_url());

    let request_body = OllamaRequest {
        model: model.to_string(),
        messages: vec![
            OllamaMessage {
                role: "system".to_string(),
                content: instructions.as_str().to_string(),
            },
            OllamaMessage {
                role: "user".to_string(),
                content: input.to_string(),
            },
        ],
        stream: false,
        options: OllamaOptions {
            temperature: options.temperature,
            num_predict: options.max_tokens,
        },
    };

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| Error::NetworkError(e.to_string()))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let message = response.text().await.unwrap_or_default();
        return Err(Error::ProviderError {
            status,
            message: format!("{}. Is Ollama running?", message),
        });
    }

    let body: OllamaResponse = response
        .json()
        .await
        .map_err(|e| Error::NetworkError(e.to_string()))?;

    Ok(body.message.content)
}

// --- Anthropic types ---

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<AnthropicMessage>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    text: String,
}

// --- Ollama types ---

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Serialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OllamaOptions {
    temperature: f32,
    num_predict: u32,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaResponseMessage,
}

#[derive(Deserialize)]
struct OllamaResponseMessage {
    content: String,
}
