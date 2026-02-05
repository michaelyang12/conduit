use crate::config::{Config, Provider};
use colored::*;
use std::io::{self, BufRead, Write};
use std::process::Command;

const DEFAULT_OPENAI_MODELS: &[&str] = &[
    "gpt-5.1",
    "gpt-4o",
    "gpt-4o-mini",
    "o1-mini",
];

const DEFAULT_ANTHROPIC_MODELS: &[&str] = &[
    "claude-sonnet-4-20250514",
    "claude-opus-4-20250514",
    "claude-3-5-haiku-20241022",
];

/// Options for customizing the setup wizard.
#[derive(Default)]
pub struct SetupOptions {
    /// Additional OpenAI models to show (appended after defaults)
    pub extra_openai_models: Vec<String>,
    /// Additional Anthropic models to show (appended after defaults)
    pub extra_anthropic_models: Vec<String>,
}

/// Run the interactive setup wizard for the given app.
pub fn run_setup(app_name: &str, options: Option<SetupOptions>) {
    let options = options.unwrap_or_default();

    println!("{}", format!("{} configuration\n", app_name).bold());

    let current = Config::load(app_name);
    let current_provider_str = match current.provider {
        Provider::OpenAI => "openai",
        Provider::Anthropic => "anthropic",
        Provider::Ollama => "ollama",
    };
    let current_model = match current.provider {
        Provider::OpenAI => current.openai_model(),
        Provider::Anthropic => current.anthropic_model(),
        Provider::Ollama => current.ollama_model(),
    };
    println!(
        "Current: {} / {}\n",
        current_provider_str.cyan(),
        current_model.cyan()
    );

    // Select provider
    println!("Provider:");
    let providers = [
        ("OpenAI", Provider::OpenAI),
        ("Anthropic", Provider::Anthropic),
        ("Ollama (local)", Provider::Ollama),
    ];
    for (i, (name, p)) in providers.iter().enumerate() {
        let is_current =
            std::mem::discriminant(p) == std::mem::discriminant(&current.provider);
        let marker = if is_current {
            " ← selected".cyan().to_string()
        } else {
            String::new()
        };
        println!("  {}. {}{}", i + 1, name, marker);
    }
    print!("\n> ");
    io::stdout().flush().unwrap();

    let provider_choice = read_line();
    let provider = match provider_choice.trim() {
        "" => current.provider.clone(),
        "1" => Provider::OpenAI,
        "2" => Provider::Anthropic,
        "3" => Provider::Ollama,
        _ => {
            eprintln!("{}", "Invalid choice".red());
            return;
        }
    };

    // Check API key / Ollama status
    match &provider {
        Provider::OpenAI => {
            if std::env::var("OPENAI_API_KEY").is_err() {
                eprintln!("\n{}", "Warning: OPENAI_API_KEY not set".yellow());
                eprintln!("Add to your shell profile:");
                eprintln!("  export OPENAI_API_KEY=\"your_key_here\"\n");
            } else {
                println!("\n{}", "OPENAI_API_KEY found".green());
            }
        }
        Provider::Anthropic => {
            if std::env::var("ANTHROPIC_API_KEY").is_err() {
                eprintln!("\n{}", "Warning: ANTHROPIC_API_KEY not set".yellow());
                eprintln!("Add to your shell profile:");
                eprintln!("  export ANTHROPIC_API_KEY=\"your_key_here\"\n");
            } else {
                println!("\n{}", "ANTHROPIC_API_KEY found".green());
            }
        }
        Provider::Ollama => {
            if !check_ollama_running() {
                eprintln!(
                    "\n{}",
                    "Warning: Ollama doesn't appear to be running".yellow()
                );
                eprintln!("Start it with: ollama serve\n");
            } else {
                println!("\n{}", "Ollama is running".green());
            }
        }
    }

    // Select model
    let model = select_model(&provider, &current, &options);

    // Build and save config
    let mut config = Config::default();
    config.provider = provider.clone();

    match provider {
        Provider::OpenAI => config.openai_model = model,
        Provider::Anthropic => config.anthropic_model = model,
        Provider::Ollama => config.ollama_model = model,
    }

    if let Err(e) = config.save(app_name) {
        eprintln!("{}", format!("Failed to save config: {}", e).red());
        return;
    }

    println!(
        "\n{}",
        format!(
            "Configuration saved to ~/.config/conduit/{}/config.json",
            app_name
        )
        .green()
    );
}

fn select_model(
    provider: &Provider,
    current_config: &Config,
    options: &SetupOptions,
) -> Option<String> {
    let models: Vec<String> = match provider {
        Provider::OpenAI => {
            let mut m: Vec<String> =
                DEFAULT_OPENAI_MODELS.iter().map(|s| s.to_string()).collect();
            m.extend(options.extra_openai_models.iter().cloned());
            m
        }
        Provider::Anthropic => {
            let mut m: Vec<String> = DEFAULT_ANTHROPIC_MODELS
                .iter()
                .map(|s| s.to_string())
                .collect();
            m.extend(options.extra_anthropic_models.iter().cloned());
            m
        }
        Provider::Ollama => {
            print!("Fetching models...");
            io::stdout().flush().unwrap();
            let m = get_ollama_models();
            print!("\r                   \r");
            m
        }
    };

    if models.is_empty() {
        eprintln!("{}", "No models available".yellow());
        return None;
    }

    let current_model = match provider {
        Provider::OpenAI => current_config.openai_model(),
        Provider::Anthropic => current_config.anthropic_model(),
        Provider::Ollama => current_config.ollama_model(),
    };

    let default_model = match provider {
        Provider::OpenAI => "gpt-5.1",
        Provider::Anthropic => "claude-sonnet-4-20250514",
        Provider::Ollama => models.first().map(|s| s.as_str()).unwrap_or(""),
    };

    println!("\nModel:");
    for (i, model) in models.iter().enumerate() {
        let is_default = model == default_model;
        let is_selected = model == current_model;
        let marker = match (is_default, is_selected) {
            (true, true) => " (default) ← selected".cyan().to_string(),
            (true, false) => " (default)".dimmed().to_string(),
            (false, true) => " ← selected".cyan().to_string(),
            (false, false) => String::new(),
        };
        println!("  {}. {}{}", i + 1, model, marker);
    }
    print!("\n> ");
    io::stdout().flush().unwrap();

    let choice = read_line();
    let choice = choice.trim();

    if choice.is_empty() {
        println!("{}", format!("Keeping current: {}", current_model).dimmed());
        // Return current if it differs from default, otherwise None (use default)
        if current_model != default_model {
            return Some(current_model.to_string());
        }
        return None;
    }

    match choice.parse::<usize>() {
        Ok(n) if n >= 1 && n <= models.len() => {
            let chosen = &models[n - 1];
            if chosen == default_model {
                None // Store None so config falls back to default
            } else {
                Some(chosen.clone())
            }
        }
        _ => {
            eprintln!("{}", "Invalid choice, keeping current".yellow());
            if current_model != default_model {
                Some(current_model.to_string())
            } else {
                None
            }
        }
    }
}

fn get_ollama_models() -> Vec<String> {
    let output = Command::new("ollama").arg("list").output();

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .skip(1)
            .filter_map(|line| line.split_whitespace().next().map(|s| s.to_string()))
            .collect(),
        _ => vec![],
    }
}

fn check_ollama_running() -> bool {
    reqwest::blocking::get("http://localhost:11434/api/tags").is_ok()
}

fn read_line() -> String {
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input).unwrap();
    input
}
