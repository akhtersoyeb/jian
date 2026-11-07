use crate::config::Config;
use crate::llm::ollama::{canonical_model, Ollama};
use crate::ui;
use anyhow::Result;

pub async fn run(cfg: &Config) -> Result<()> {
    println!("Jian Code v{}\n", env!("CARGO_PKG_VERSION"));
    let mut failed = false;

    // --- config ---
    match Config::path() {
        Some(p) if p.exists() => ui::check_ok(&format!("config file: {}", p.display())),
        Some(p) => ui::check_warn(&format!(
            "no config file; using defaults. To customize, create {}",
            p.display()
        )),
        None => ui::check_warn("couldn't determine a config directory; using defaults"),
    }
    ui::info(&format!(
        "  model={}  host={}  num_ctx={}  think={}",
        cfg.model,
        cfg.host,
        cfg.num_ctx,
        cfg.think
            .map_or(String::from("model default"), |t| t.to_string())
    ));
    if cfg.host.starts_with("https://") {
        ui::check_fail(
            "https hosts aren't supported (TLS is disabled in this build); use an http:// URL",
        );
        failed = true;
    }

    // --- server ---
    let client = Ollama::new(&cfg.host);
    match client.version().await {
        Ok(v) => ui::check_ok(&format!("Ollama {v} reachable at {}", cfg.host)),
        Err(_) => {
            ui::check_fail(&format!(
                "can't reach Ollama at {}. Start it with `ollama serve` (or open the Ollama app).",
                cfg.host
            ));
            std::process::exit(1);
        }
    }

    // --- model ---
    let want = canonical_model(&cfg.model);
    let mut installed = false;
    match client.installed_models().await {
        Ok(models) if models.contains(&want) => {
            installed = true;
            ui::check_ok(&format!("model '{}' is installed", cfg.model));
        }
        Ok(models) => {
            failed = true;
            ui::check_fail(&format!(
                "model '{}' isn't installed. Run: ollama pull {}",
                cfg.model, cfg.model
            ));
            if !models.is_empty() {
                ui::info(&format!("  installed models: {}", models.join(", ")));
            }
        }
        Err(e) => {
            failed = true;
            ui::check_fail(&format!("couldn't list models: {e:#}"));
        }
    }
    if installed {
        match client.capabilities(&cfg.model).await {
            Ok(caps) if caps.is_empty() => ui::check_warn(
                "couldn't read model capabilities (older Ollama?); tool calling is unverified",
            ),
            Ok(caps) => {
                if caps.iter().any(|c| c == "tools") {
                    ui::check_ok("model supports tool calling");
                } else {
                    failed = true;
                    ui::check_fail(&format!(
                        "model doesn't advertise tool calling (capabilities: {}); Jian Code needs a tools-capable model",
                        caps.join(", ")
                    ));
                }
                if cfg.think == Some(true) && !caps.iter().any(|c| c == "thinking") {
                    ui::check_warn("think = true, but the model doesn't advertise thinking");
                }
            }
            Err(e) => ui::check_warn(&format!("couldn't query model capabilities: {e:#}")),
        }
    }

    // --- git ---
    match std::process::Command::new("git").arg("--version").output() {
        Ok(o) if o.status.success() => ui::check_ok(String::from_utf8_lossy(&o.stdout).trim()),
        _ => ui::check_warn("git not found; git status won't be included in the prompt"),
    }

    ui::info(&format!(
        "\nproject root would be: {}",
        std::env::current_dir()?.display()
    ));
    println!();
    if failed {
        ui::check_fail("problems found (see above)");
        std::process::exit(1);
    }
    ui::check_ok("all good. Run `jian` inside a project directory.");
    Ok(())
}
