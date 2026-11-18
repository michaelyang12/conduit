//! Opinionated LLM client for CLI tools: one prompt in, text or JSON out.
//!
//! ```no_run
//! # async fn demo() -> conduit::Result<()> {
//! let llm = conduit::Conduit::load("my-app")?;
//! let answer = llm
//!     .prompt("list the 5 largest files here")
//!     .system("You convert requests into a single shell command.")
//!     .context("os", "macOS")
//!     .text()
//!     .await?;
//! # Ok(()) }
//! ```
//!
//! Providers are data ([`provider::PRESETS`] plus `[providers.<name>]` config sections); the code
//! only knows a handful of [`Protocol`]s: OpenAI Responses, Chat Completions, Anthropic Messages,
//! Ollama, and the Claude Code / Codex CLIs.

mod cli;
mod client;
pub mod config;
mod error;
mod instructions;
mod json;
mod protocol;
pub mod provider;
mod response;
#[cfg(feature = "setup")]
pub mod setup;

pub use client::{Conduit, DEFAULT_TIMEOUT, Request};
pub use config::{Config, ProviderConfig};
pub use error::{Error, Result};
pub use instructions::{Instructions, InstructionsBuilder};
pub use provider::{ModelInfo, Protocol, Provider};
pub use response::{Reasoning, Response, StopReason, Usage};
