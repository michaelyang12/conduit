//! Interactive provider setup for an app, writing `~/.config/conduit/<app>/config.toml`.
//!
//! The wizard runs on the terminal's alternate screen and redraws one frame per step, so going
//! back (Esc) leaves nothing behind. Esc on the first step or Ctrl-C quits without saving.

use crate::client::{Conduit, suggested_models};
use crate::config::{Config, ProviderConfig};
use crate::error::{Error, Result};
use crate::provider::{ModelInfo, PRESETS, Protocol, Provider, preset};
use console::style;
use inquire::error::InquireError;
use inquire::list_option::ListOption;
use inquire::ui::{Color, RenderConfig, StyleSheet, Styled};
use inquire::validator::Validation;
use inquire::{Select, Text};
use std::fmt;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::time::Duration;

const LIST_HELP: &str = "↑↓ move · type to filter · enter select · esc back";
const TEXT_HELP: &str = "enter confirm · esc back";
const PING_TIMEOUT: Duration = Duration::from_millis(600);
const CANCELLED: &str = "cancelled";

/// Walks the user through choosing a provider and model for `app`.
pub async fn run(app: &str) -> Result<()> {
    inquire::set_global_render_config(theme());
    let path = Config::app_path(app);
    let mut file = Config::read(&path)?;
    let merged = Config::load(app)?;

    let screen = Screen::enter(app, &merged);
    match Wizard::new(&merged, screen).run().await {
        Ok(Some(choice)) => {
            save(&mut file, &path, &choice)?;
            print_saved(app, &choice, &path);
            Ok(())
        }
        Ok(None) => {
            eprintln!("{}", style("Setup cancelled. Nothing was saved.").dim());
            Ok(())
        }
        Err(Error::Setup(reason)) if reason == CANCELLED => {
            eprintln!("{}", style("Setup cancelled. Nothing was saved.").dim());
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Owns the alternate screen; leaving it on drop restores the user's terminal even on errors.
struct Screen {
    active: bool,
    title: String,
    subtitle: String,
}

impl Screen {
    fn enter(app: &str, merged: &Config) -> Self {
        let active = std::io::stderr().is_terminal();
        if active {
            eprint!("\x1b[?1049h");
        }
        let name = merged.active_provider();
        let model = Provider::resolve(name, merged.providers.get(name))
            .ok()
            .and_then(|p| p.model)
            .unwrap_or_else(|| "default model".to_string());
        Self {
            active,
            title: format!("{app} setup"),
            subtitle: format!("Currently {name} · {model}"),
        }
    }

    /// Clears the screen and draws the header, completed steps and the current step's notes.
    fn frame(&self, crumbs: &[(String, String)], notes: &[String]) {
        let mut out = String::new();
        if self.active {
            out.push_str("\x1b[H\x1b[2J");
        }
        out.push_str(&format!(
            "\n  {}  {}\n\n",
            style(&self.title).bold(),
            style(&self.subtitle).dim()
        ));
        for (label, value) in crumbs {
            out.push_str(&format!(
                "{} {} {}\n",
                style("◆").green(),
                style(format!("{label:<9}")).dim(),
                value
            ));
        }
        if !crumbs.is_empty() {
            out.push('\n');
        }
        for note in notes {
            out.push_str(&format!("  {note}\n"));
        }
        if !notes.is_empty() {
            out.push('\n');
        }
        eprint!("{out}");
        let _ = std::io::stderr().flush();
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        if self.active {
            eprint!("\x1b[?1049l");
            let _ = std::io::stderr().flush();
        }
    }
}

struct Choice {
    name: String,
    section: ProviderConfig,
    model: Option<String>,
}

#[derive(Clone, Copy)]
enum Step {
    Provider,
    Custom,
    Endpoint,
    Model,
}

struct Wizard<'a> {
    merged: &'a Config,
    screen: Screen,
    crumbs: Vec<(String, String)>,
    name: String,
    section: ProviderConfig,
}

impl<'a> Wizard<'a> {
    fn new(merged: &'a Config, screen: Screen) -> Self {
        Self {
            merged,
            screen,
            crumbs: Vec::new(),
            name: merged.active_provider().to_string(),
            section: ProviderConfig::default(),
        }
    }

    async fn run(mut self) -> Result<Option<Choice>> {
        let mut history: Vec<Step> = Vec::new();
        let mut step = Step::Provider;
        loop {
            let next = match step {
                Step::Provider => match self.pick_provider().await? {
                    None => return Ok(None),
                    Some(next) => Some(next),
                },
                Step::Custom => self.define_custom()?,
                Step::Endpoint => self.ask_endpoint()?,
                Step::Model => match self.pick_model().await? {
                    None => None,
                    Some(model) => {
                        return Ok(Some(Choice {
                            name: self.name,
                            section: self.section,
                            model,
                        }));
                    }
                },
            };
            match next {
                Some(next) => history.push(std::mem::replace(&mut step, next)),
                None => match history.pop() {
                    Some(previous) => {
                        self.crumbs.pop();
                        step = previous;
                    }
                    None => return Ok(None),
                },
            }
        }
    }

    fn provider(&self) -> Result<Provider> {
        Provider::resolve(&self.name, Some(&self.section))
    }

    async fn pick_provider(&mut self) -> Result<Option<Step>> {
        self.screen.frame(
            &self.crumbs,
            &[style("Checking providers...").dim().to_string()],
        );
        let current = self.merged.active_provider();
        let mut rows: Vec<(String, String, String)> = Vec::new();
        let customs = self.merged.providers.keys().filter(|n| preset(n).is_none());
        let names = PRESETS
            .iter()
            .map(|p| (p.name.to_string(), p.label.to_string()))
            .chain(customs.map(|n| (n.clone(), n.clone())));
        for (name, label) in names {
            let status = match Provider::resolve(&name, self.merged.providers.get(&name)) {
                Ok(p) => status(&p).await,
                Err(_) => style("incomplete").yellow().to_string(),
            };
            rows.push((name, label, status));
        }

        let width = rows.iter().map(|r| r.1.chars().count()).max().unwrap_or(0);
        let mut items: Vec<Item<Option<String>>> = rows
            .into_iter()
            .map(|(name, label, status)| {
                let marker = if name == current {
                    format!("  {}", style("current").cyan())
                } else {
                    String::new()
                };
                Item {
                    label: format!("{label:<width$}  {status}{marker}"),
                    short: label,
                    value: Some(name),
                }
            })
            .collect();
        items.push(Item {
            label: "Add an OpenAI-compatible server...".to_string(),
            short: "New server".to_string(),
            value: None,
        });
        let start = items
            .iter()
            .position(|i| i.value.as_deref() == Some(self.name.as_str()))
            .unwrap_or(0);

        self.screen.frame(&self.crumbs, &[]);
        let Some(picked) = ask(Select::new("Provider", items)
            .with_starting_cursor(start)
            .with_page_size(14)
            .with_help_message(LIST_HELP)
            .with_formatter(&short_formatter)
            .prompt())?
        else {
            return Ok(None);
        };
        self.crumbs.push(("Provider".into(), picked.short.clone()));
        let Some(name) = picked.value else {
            self.section = ProviderConfig::default();
            return Ok(Some(Step::Custom));
        };
        self.section = self
            .merged
            .providers
            .get(&name)
            .cloned()
            .unwrap_or_default();
        self.name = name;
        let provider = self.provider()?;
        if provider.is_local() && !provider.protocol.is_cli() && !ping(&provider).await {
            Ok(Some(Step::Endpoint))
        } else {
            Ok(Some(Step::Model))
        }
    }

    fn define_custom(&mut self) -> Result<Option<Step>> {
        self.screen.frame(
            &self.crumbs,
            &[
                style("Any server that speaks the OpenAI Chat Completions API.")
                    .dim()
                    .to_string(),
            ],
        );
        let Some(name) = ask(Text::new("Name")
            .with_placeholder("work-llm")
            .with_help_message(TEXT_HELP)
            .with_validator(|s: &str| {
                Ok(if s.trim().is_empty() || preset(s.trim()).is_some() {
                    Validation::Invalid("pick a new, non-empty name".into())
                } else {
                    Validation::Valid
                })
            })
            .prompt())?
        else {
            return Ok(None);
        };
        let Some(endpoint) = ask(Text::new("Base URL")
            .with_placeholder("http://localhost:8080/v1")
            .with_help_message(TEXT_HELP)
            .with_validator(|s: &str| {
                Ok(if s.starts_with("http://") || s.starts_with("https://") {
                    Validation::Valid
                } else {
                    Validation::Invalid("starts with http:// or https://".into())
                })
            })
            .prompt())?
        else {
            return Ok(None);
        };
        let Some(key_env) = ask(Text::new("API key environment variable")
            .with_placeholder("blank if none")
            .with_help_message(TEXT_HELP)
            .prompt())?
        else {
            return Ok(None);
        };
        self.name = name.trim().to_string();
        self.section = ProviderConfig {
            endpoint: Some(endpoint.trim().trim_end_matches('/').to_string()),
            api_key_env: Some(key_env.trim().to_string()).filter(|k| !k.is_empty()),
            protocol: Some(Protocol::ChatCompletions),
            model: None,
        };
        if let Some(last) = self.crumbs.last_mut() {
            last.1 = self.name.clone();
        }
        Ok(Some(Step::Model))
    }

    fn ask_endpoint(&mut self) -> Result<Option<Step>> {
        let provider = self.provider()?;
        self.screen.frame(
            &self.crumbs,
            &[format!(
                "{} {} isn't responding at {}. Start it, or point setup at another address.",
                style("!").yellow().bold(),
                provider.name,
                style(&provider.endpoint).bold()
            )],
        );
        let Some(url) = ask(Text::new("Server URL")
            .with_default(&provider.endpoint)
            .with_help_message("enter to keep or change · esc back")
            .prompt())?
        else {
            return Ok(None);
        };
        let url = url.trim().trim_end_matches('/').to_string();
        let default = preset(&self.name).map(|p| p.endpoint);
        self.section.endpoint = (Some(url.as_str()) != default).then(|| url.clone());
        self.crumbs.push(("Server".into(), url));
        Ok(Some(Step::Model))
    }

    async fn pick_model(&mut self) -> Result<Option<Option<String>>> {
        let provider = self.provider()?;
        let mut notes = Vec::new();
        if let Some(env) = &provider.api_key_env
            && !env_set(env)
            && !provider.is_local()
        {
            notes.push(format!(
                "{} {} is not set. Add {} to your shell profile.",
                style("!").yellow().bold(),
                style(env).bold(),
                style(format!("export {env}=\"...\"")).cyan()
            ));
        }
        self.screen.frame(
            &self.crumbs,
            &[
                notes.clone(),
                vec![style("Loading models...").dim().to_string()],
            ]
            .concat(),
        );
        let models = match Conduit::new(provider.clone())
            .with_timeout(Duration::from_secs(15))
            .list_models()
            .await
        {
            Ok(models) => {
                if models.is_empty() && provider.protocol == Protocol::Ollama {
                    notes.push(format!(
                        "{} No models installed. Pull one with {}",
                        style("!").yellow().bold(),
                        style("ollama pull <model>").cyan()
                    ));
                }
                models
            }
            Err(e) => {
                if provider.api_key_env.as_deref().is_none_or(env_set) {
                    notes.push(format!(
                        "{} Couldn't list models ({e}); showing suggestions.",
                        style("!").yellow().bold()
                    ));
                }
                suggested_models(&provider)
            }
        };

        loop {
            let items = model_items(&provider, &models);
            let start = provider
                .model
                .as_ref()
                .and_then(|m| {
                    items
                        .iter()
                        .position(|i| matches!(&i.value, ModelPick::Model(id) if id == m))
                })
                .unwrap_or(0);
            self.screen.frame(&self.crumbs, &notes);
            let Some(picked) = ask(Select::new("Model", items)
                .with_starting_cursor(start)
                .with_page_size(14)
                .with_help_message(LIST_HELP)
                .with_formatter(&short_formatter)
                .prompt())?
            else {
                return Ok(None);
            };
            match picked.value {
                ModelPick::Default => return Ok(Some(None)),
                ModelPick::Model(id) => return Ok(Some(Some(id))),
                ModelPick::Other => {
                    let typed = ask(Text::new("Model ID")
                        .with_placeholder("the exact ID the provider expects")
                        .with_help_message(TEXT_HELP)
                        .prompt())?;
                    if let Some(id) = typed
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                    {
                        return Ok(Some(Some(id)));
                    }
                }
            }
        }
    }
}

struct Item<T> {
    label: String,
    short: String,
    value: T,
}

impl<T> fmt::Display for Item<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

fn short_formatter<T>(option: ListOption<&Item<T>>) -> String {
    option.value.short.clone()
}

enum ModelPick {
    Default,
    Model(String),
    Other,
}

fn model_items(provider: &Provider, models: &[ModelInfo]) -> Vec<Item<ModelPick>> {
    let current = provider.model.as_deref();
    let mut ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    if provider.model_optional() {
        ids.push("Default");
    }
    if let Some(c) = current {
        ids.push(c);
    }
    let width = ids
        .iter()
        .map(|id| id.chars().count())
        .max()
        .unwrap_or(0)
        .min(44);
    let marker = |id: &str| {
        if current == Some(id) {
            format!("  {}", style("current").cyan())
        } else {
            String::new()
        }
    };

    let mut items = Vec::new();
    if provider.model_optional() {
        let note = format!("let {} choose", provider.endpoint);
        items.push(Item {
            label: format!("{:<width$}  {}", "Default", style(note).dim()),
            short: "CLI default".to_string(),
            value: ModelPick::Default,
        });
    }
    if let Some(c) = current
        && !models.iter().any(|m| m.id == c)
    {
        items.push(Item {
            label: format!("{c:<width$}{}", marker(c)),
            short: c.to_string(),
            value: ModelPick::Model(c.to_string()),
        });
    }
    for m in models {
        let description = m.description.as_deref().unwrap_or_default();
        items.push(Item {
            label: format!(
                "{:<width$}  {}{}",
                m.id,
                style(description).dim(),
                marker(&m.id)
            ),
            short: m.id.clone(),
            value: ModelPick::Model(m.id.clone()),
        });
    }
    items.push(Item {
        label: "Other model ID...".to_string(),
        short: "Other".to_string(),
        value: ModelPick::Other,
    });
    items
}

async fn status(provider: &Provider) -> String {
    if provider.protocol.is_cli() {
        return if on_path(&provider.endpoint) {
            format!(
                "{} {}",
                style("installed").green(),
                style(format!("· uses your {} sign-in", provider.endpoint)).dim()
            )
        } else {
            style(format!("`{}` not installed", provider.endpoint))
                .yellow()
                .to_string()
        };
    }
    if provider.is_local() {
        return if ping(provider).await {
            format!("{} {}", style("running").green(), style("· local").dim())
        } else {
            format!(
                "{} {}",
                style("not running").yellow(),
                style("· local").dim()
            )
        };
    }
    match &provider.api_key_env {
        Some(env) if !env_set(env) => style(format!("needs {env}")).yellow().to_string(),
        _ => style("ready").green().to_string(),
    }
}

fn on_path(command: &str) -> bool {
    if command.contains('/') {
        return Path::new(command).is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(command).is_file()))
        .unwrap_or(false)
}

fn env_set(env: &str) -> bool {
    std::env::var(env).is_ok_and(|v| !v.trim().is_empty())
}

async fn ping(provider: &Provider) -> bool {
    let probe = Conduit::new(provider.clone()).with_timeout(PING_TIMEOUT);
    tokio::time::timeout(PING_TIMEOUT, probe.list_models())
        .await
        .is_ok_and(|r| r.is_ok())
}

fn save(file: &mut Config, path: &Path, choice: &Choice) -> Result<()> {
    let existing = file.providers.remove(&choice.name).unwrap_or_default();
    file.providers.insert(
        choice.name.clone(),
        ProviderConfig {
            model: choice.model.clone(),
            endpoint: choice.section.endpoint.clone().or(existing.endpoint),
            api_key_env: choice.section.api_key_env.clone().or(existing.api_key_env),
            protocol: choice.section.protocol.or(existing.protocol),
        },
    );
    file.provider = Some(choice.name.clone());
    file.write(path)
}

fn print_saved(app: &str, choice: &Choice, path: &Path) {
    let model = choice.model.as_deref().unwrap_or("default model");
    let shown = std::env::var("HOME")
        .ok()
        .and_then(|home| {
            path.strip_prefix(&home)
                .ok()
                .map(|rest| format!("~/{}", rest.display()))
        })
        .unwrap_or_else(|| path.display().to_string());
    eprintln!(
        "{} {app} now uses {} · {}",
        style("◆").green(),
        style(&choice.name).bold(),
        style(model).bold()
    );
    eprintln!("  {}", style(format!("saved to {shown}")).dim());
}

/// `Ok(None)` means Esc (go back). Ctrl-C becomes a cancellation that `run` reports.
fn ask<T>(result: std::result::Result<T, InquireError>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(InquireError::OperationCanceled) => Ok(None),
        Err(InquireError::OperationInterrupted) => Err(Error::Setup(CANCELLED.to_string())),
        Err(InquireError::NotTTY) => Err(Error::Setup(
            "setup needs an interactive terminal".to_string(),
        )),
        Err(e) => Err(Error::Setup(e.to_string())),
    }
}

fn theme() -> RenderConfig<'static> {
    RenderConfig::default_colored()
        .with_prompt_prefix(Styled::new("◇").with_fg(Color::LightCyan))
        .with_answered_prompt_prefix(Styled::new("◆").with_fg(Color::LightGreen))
        .with_highlighted_option_prefix(Styled::new("›").with_fg(Color::LightCyan))
        .with_scroll_up_prefix(Styled::new("↑").with_fg(Color::DarkGrey))
        .with_scroll_down_prefix(Styled::new("↓").with_fg(Color::DarkGrey))
        .with_help_message(StyleSheet::new().with_fg(Color::DarkGrey))
        .with_answer(StyleSheet::new().with_fg(Color::LightGreen))
        .with_selected_option(Some(StyleSheet::new().with_fg(Color::LightCyan)))
        .with_canceled_prompt_indicator(Styled::new("").with_fg(Color::DarkGrey))
}
