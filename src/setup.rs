//! Interactive provider setup for an app, writing `~/.config/conduit/<app>/config.toml`.

use crate::client::Conduit;
use crate::config::{Config, ProviderConfig};
use crate::error::{Error, Result};
use crate::provider::{PRESETS, Protocol, Provider, preset};
use console::style;
use dialoguer::{Confirm, FuzzySelect, Input, Select, theme::ColorfulTheme};
use std::time::Duration;

const CLI_DEFAULT: &str = "(the CLI's default model)";
const TYPE_MODEL: &str = "Type a model name...";
const OTHER_SERVER: &str = "Other OpenAI-compatible server...";

/// Walks the user through choosing a provider and model for `app`, then sends a test prompt.
pub async fn run(app: &str) -> Result<()> {
    let theme = ColorfulTheme::default();
    let path = Config::app_path(app);
    let mut file = Config::read(&path)?;
    let merged = Config::load(app)?;

    println!("{}", style(format!("{app} LLM setup")).bold());
    println!("{}\n", style(format!("Saves to {}", path.display())).dim());

    let name = choose_provider(&theme, &merged)?;
    let mut section = merged.providers.get(&name).cloned().unwrap_or_default();
    if preset(&name).is_none() && section.endpoint.is_none() {
        section = define_custom(&theme)?;
    }
    let mut provider = Provider::resolve(&name, Some(&section))?;

    if let Some(p) = preset(&name).filter(|p| p.local && !p.protocol.is_cli()) {
        let endpoint: String = Input::with_theme(&theme)
            .with_prompt("Server URL")
            .default(provider.endpoint.clone())
            .interact_text()
            .map_err(setup_error)?;
        let endpoint = endpoint.trim_end_matches('/').to_string();
        section.endpoint = (endpoint != p.endpoint).then(|| endpoint.clone());
        provider.endpoint = endpoint;
    }
    check_credentials(&provider).await;

    let model = choose_model(&theme, &provider).await?;
    section.model = model.clone();
    provider.model = model;

    let existing = file.providers.remove(&name).unwrap_or_default();
    file.providers.insert(
        name.clone(),
        ProviderConfig {
            model: section.model,
            endpoint: section.endpoint.or(existing.endpoint),
            api_key_env: section.api_key_env.or(existing.api_key_env),
            protocol: section.protocol.or(existing.protocol),
        },
    );
    file.provider = Some(name);
    file.write(&path)?;
    println!("{} {}", style("Saved").green(), path.display());

    let test = Confirm::with_theme(&theme)
        .with_prompt("Send a test prompt?")
        .default(true)
        .interact()
        .map_err(setup_error)?;
    if test {
        smoke_test(provider).await;
    }
    Ok(())
}

fn choose_provider(theme: &ColorfulTheme, merged: &Config) -> Result<String> {
    let current = merged.active_provider();
    let mut names: Vec<String> = PRESETS.iter().map(|p| p.name.to_string()).collect();
    let mut labels: Vec<String> = PRESETS.iter().map(|p| p.label.to_string()).collect();
    for name in merged.providers.keys().filter(|n| preset(n).is_none()) {
        names.push(name.clone());
        labels.push(format!("{name} (custom)"));
    }
    labels.push(OTHER_SERVER.to_string());

    let choice = Select::with_theme(theme)
        .with_prompt("Provider")
        .items(&labels)
        .default(names.iter().position(|n| n == current).unwrap_or(0))
        .interact()
        .map_err(setup_error)?;
    if choice < names.len() {
        return Ok(names[choice].clone());
    }
    Input::with_theme(theme)
        .with_prompt("Name for this provider")
        .validate_with(|s: &String| {
            if s.trim().is_empty() || preset(s.trim()).is_some() {
                Err("pick a new, non-empty name")
            } else {
                Ok(())
            }
        })
        .interact_text()
        .map(|s: String| s.trim().to_string())
        .map_err(setup_error)
}

fn define_custom(theme: &ColorfulTheme) -> Result<ProviderConfig> {
    let endpoint: String = Input::with_theme(theme)
        .with_prompt("Base URL (e.g. http://localhost:8080/v1)")
        .interact_text()
        .map_err(setup_error)?;
    let key_env: String = Input::with_theme(theme)
        .with_prompt("Environment variable holding the API key (blank for none)")
        .allow_empty(true)
        .interact_text()
        .map_err(setup_error)?;
    Ok(ProviderConfig {
        endpoint: Some(endpoint.trim_end_matches('/').to_string()),
        api_key_env: Some(key_env.trim().to_string()).filter(|k| !k.is_empty()),
        protocol: Some(Protocol::ChatCompletions),
        model: None,
    })
}

async fn check_credentials(provider: &Provider) {
    if provider.protocol.is_cli() {
        let found = tokio::process::Command::new(&provider.endpoint)
            .arg("--version")
            .output()
            .await
            .is_ok_and(|o| o.status.success());
        if !found {
            warn(&format!("`{}` was not found on PATH", provider.endpoint));
        }
        return;
    }
    if let Some(env) = &provider.api_key_env
        && std::env::var(env).map_or(true, |v| v.trim().is_empty())
        && !provider.is_local()
    {
        warn(&format!("{env} is not set. Add it to your shell profile:"));
        println!("  export {env}=\"...\"");
    }
}

async fn choose_model(theme: &ColorfulTheme, provider: &Provider) -> Result<Option<String>> {
    let conduit = Conduit::new(provider.clone()).with_timeout(Duration::from_secs(15));
    let mut models = match conduit.list_models().await {
        Ok(models) => models,
        Err(e) => {
            warn(&format!("couldn't list models: {e}"));
            preset(&provider.name)
                .map(|p| p.suggested_models.iter().map(|m| m.to_string()).collect())
                .unwrap_or_default()
        }
    };
    if provider.protocol == Protocol::Ollama && models.is_empty() {
        warn("no local models found; pull one with `ollama pull <model>`");
    }
    if let Some(current) = &provider.model
        && !models.contains(current)
    {
        models.insert(0, current.clone());
    }

    let mut items = Vec::new();
    if provider.model_optional() {
        items.push(CLI_DEFAULT.to_string());
    }
    items.extend(models);
    items.push(TYPE_MODEL.to_string());

    let default = provider
        .model
        .as_ref()
        .and_then(|m| items.iter().position(|i| i == m))
        .unwrap_or(0);
    let choice = FuzzySelect::with_theme(theme)
        .with_prompt("Model")
        .items(&items)
        .default(default)
        .interact()
        .map_err(setup_error)?;
    match items[choice].as_str() {
        CLI_DEFAULT => Ok(None),
        TYPE_MODEL => Input::with_theme(theme)
            .with_prompt("Model name")
            .interact_text()
            .map(|s: String| Some(s.trim().to_string()))
            .map_err(setup_error),
        model => Ok(Some(model.to_string())),
    }
}

async fn smoke_test(provider: Provider) {
    let conduit = Conduit::new(provider);
    match conduit
        .prompt("Reply with the single word: ready")
        .send()
        .await
    {
        Ok(r) => println!(
            "{} {} replied {:?} in {:.1}s",
            style("OK").green(),
            r.model,
            r.text,
            r.elapsed.as_secs_f32()
        ),
        Err(e) => warn(&format!("test prompt failed: {e}")),
    }
}

fn warn(message: &str) {
    eprintln!("{} {message}", style("warning:").yellow());
}

fn setup_error(e: dialoguer::Error) -> Error {
    Error::Setup(e.to_string())
}
