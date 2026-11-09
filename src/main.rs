mod agent;
mod config;
mod context;
mod doctor;
mod llm;
mod permissions;
mod tools;
mod ui;
mod transcript;


use serde_json::json;
use transcript::Transcript;

use anyhow::Result;
use clap::Parser;
use llm::ollama::Ollama;
use llm::Message;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::io::Write;

#[derive(clap::Subcommand)]
enum Command {
    /// Check Ollama, the model and the config, and explain how to fix problems
    Doctor,
}

#[derive(Parser)]
#[command(
    name = "jian",
    version,
    about = "Jian Code: a local coding agent for your terminal"
)]
struct Cli {
    /// Ollama model to use (overrides config)
    #[arg(long, short)]
    model: Option<String>,
    /// Ollama host URL (overrides config)
    #[arg(long)]
    host: Option<String>,
    /// Run one prompt non-interactively. The final answer goes to stdout, everything else to stderr
    #[arg(long, short)]
    prompt: Option<String>,
    /// Auto-approve every edit and command. Dangerous: only for throwaway dirs and scripts
    #[arg(long, short = 'y')]
    yes: bool,

    #[command(subcommand)]
    command: Option<Command>,

    /// Force thinking on or off (true/false). Default: the model decides
        #[arg(long, action = clap::ArgAction::Set, value_parser = clap::value_parser!(bool))]
        think: Option<bool>,
        /// Context window in tokens (overrides config)
        #[arg(long)]
        num_ctx: Option<u32>,
        /// Sampling temperature (overrides config)
        #[arg(long)]
        temperature: Option<f32>,
        /// Write this session's transcript (JSONL) to PATH instead of the default location
        #[arg(long)]
        transcript: Option<std::path::PathBuf>,
}

const HELP: &str = "\
/help      show this help
/clear     reset the conversation (reloads git status and JIAN.md)
/context   show context-window usage
/exit      quit (or Ctrl-D)
End a line with \\ to continue on the next line.";

/// Point fd 1 at stderr so all UI output goes there, and return the ORIGINAL stdout for the answer.
#[cfg(unix)]
fn split_stdout() -> Box<dyn Write> {
    use std::os::unix::io::FromRawFd;
    // SAFETY: fd 1 is open; dup/dup2 are plain syscalls, and the saved fd is owned only by the File.
    unsafe {
        let saved = libc::dup(1);
        if saved < 0 {
            return Box::new(std::io::stdout());
        }
        libc::dup2(2, 1);
        Box::new(std::fs::File::from_raw_fd(saved))
    }
}

#[cfg(not(unix))]
fn split_stdout() -> Box<dyn Write> {
    Box::new(std::io::stdout())
}

fn read_input(rl: &mut DefaultEditor) -> Result<Option<String>, ReadlineError> {
    let mut input = String::new();
    let mut prompt = "› ";
    loop {
        let line = match rl.readline(prompt) {
            Ok(l) => l,
            Err(ReadlineError::Eof) => {
                return Ok(if input.is_empty() { None } else { Some(input) })
            }
            Err(e) => return Err(e),
        };
        match line.strip_suffix('\\') {
            Some(rest) => {
                input.push_str(rest);
                input.push('\n');
                prompt = "… ";
            }
            None => {
                input.push_str(&line);
                return Ok(Some(input));
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Legacy Windows consoles need ANSI processing switched on. Modern terminals already have it.
    #[cfg(windows)]
    let _ = crossterm::ansi_support::supports_ansi();
    let cli = Cli::parse();
    let mut cfg = config::Config::load()?;
    if let Some(m) = cli.model {
        cfg.model = m;
    }
    if let Some(h) = cli.host {
        cfg.host = config::normalize_host(&h);
    }
    if let Some(t) = cli.think {
            cfg.think = Some(t);
        }
        if let Some(n) = cli.num_ctx {
            cfg.num_ctx = n;
        }
        if let Some(t) = cli.temperature {
            cfg.temperature = Some(t);
        }

    // `jian doctor` runs before anything else touches the terminal or Ollama.
    if cli.command.is_some() {
        return doctor::run(&cfg).await;
    }

    // Non-interactive mode: must run before anything prints.
    let final_out = cli.prompt.as_ref().map(|_| split_stdout());

    let client = Ollama::new(&cfg.host);

    if let Err(e) = client.ensure_ready(&cfg.model).await {
        ui::error(&format!("{e}"));
        std::process::exit(1);
    }

    if let Ok(caps) = client.capabilities(&cfg.model).await {
        if !caps.is_empty() && !caps.iter().any(|c| c == "tools") {
            ui::error(&format!(
                "Warning: '{}' doesn't advertise tool calling, so Jian Code may not work. Run `jian doctor`.",
                cfg.model
            ));
        }
    }

    let ctx = tools::Ctx::new()?;
    let mut history = vec![Message::system(context::system_prompt(&ctx.root))];
    let mut perms = permissions::Permissions::new(cli.yes);
    let mut tracker = context::Tracker::default();

    let mut rec = Transcript::open(&cfg, &ctx.root, cli.transcript);
        rec.log(
            "session_start",
            json!({
                "version": env!("CARGO_PKG_VERSION"),
                "mode": if final_out.is_some() { "one_shot" } else { "interactive" },
                "model": cfg.model,
                "host": cfg.host,
                "num_ctx": cfg.num_ctx,
                "think": cfg.think,
                "temperature": cfg.temperature,
                "cwd": ctx.root.display().to_string(),
            }),
        );
        rec.message(&history[0]);

    // ---------- one-shot mode ----------
    if let (Some(prompt), Some(mut out)) = (cli.prompt, final_out) {
        agent::push(&mut history, &mut rec, Message::user(prompt));
               let result = tokio::select! {
                   r = agent::run_turn(&client, &cfg, &ctx, &mut perms, &mut tracker, &mut rec, &mut history) => Some(r),
                   _ = tokio::signal::ctrl_c() => None,
               };
               log_end(&mut rec, &result);
        match result {
            Some(Ok(())) => {
                let answer = history
                    .last()
                    .filter(|m| m.role == "assistant")
                    .map(|m| m.content.trim().to_string())
                    .unwrap_or_default();
                if answer.is_empty() {
                    ui::error("no final answer produced");
                    std::process::exit(1);
                }
                writeln!(out, "{answer}")?;
                return Ok(());
            }
            Some(Err(e)) => {
                ui::error(&format!("{e:#}"));
                std::process::exit(1);
            }
            None => {
                ui::reset_terminal();
                std::process::exit(130);
            }
        }
    }

    // ---------- interactive mode ----------
    ui::banner(&cfg.model, &ctx.root.display().to_string());
    if let Some(p) = &rec.path {
            ui::info(&format!("transcript: {}", p.display()));
        }
    let mut rl = DefaultEditor::new()?;
    let hist_path = dirs::data_dir().map(|d| d.join("jian").join("history.txt"));
    if let Some(p) = &hist_path {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = rl.load_history(p);
    }

    loop {
        let text = match read_input(&mut rl) {
            Ok(Some(t)) => t,
            Ok(None) => break,
            Err(ReadlineError::Interrupted) => {
                ui::info("(Ctrl-D or /exit to quit)");
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        let input = text.trim();
        if input.is_empty() {
            continue;
        }
        let _ = rl.add_history_entry(input);

        match input {
            "/exit" | "/quit" => break,
            "/help" => {
                ui::info(HELP);
                continue;
            }
            "/clear" => {
                history = vec![Message::system(context::system_prompt(&ctx.root))];
                rec.log("clear", json!({}));
                tracker = context::Tracker::default();
                ui::info("Conversation cleared.");
                continue;
            }
            "/context" => {
                let est = tracker.estimate(history.as_slice());
                ui::info(&format!(
                    "~{est} / {} tokens (~{}%); last measured prompt: {} tokens",
                    cfg.num_ctx,
                    context::pct(est, cfg.num_ctx),
                    tracker.last_prompt
                ));
                continue;
            }
            s if s.starts_with('/') => {
                ui::info("Unknown command. Try /help");
                continue;
            }
            _ => {}
        }

        rec.log("turn_start", json!({ "input_chars": input.len() }));
                rec.ended = false;
                agent::push(&mut history, &mut rec, Message::user(input));

                let result = tokio::select! {
                    r = agent::run_turn(&client, &cfg, &ctx, &mut perms, &mut tracker, &mut rec, &mut history) => Some(r),
                    _ = tokio::signal::ctrl_c() => None,
                };
                log_end(&mut rec, &result);

        let ok = matches!(result, Some(Ok(())));
        match result {
            Some(Ok(())) => {}
            Some(Err(e)) if e.downcast_ref::<agent::Cancelled>().is_some() => {
                ui::info("(cancelled)");
            }
            Some(Err(e)) => ui::error(&format!("{e:#}")),
            None => {
                ui::reset_terminal();
                ui::info("(cancelled)");
            }
        }
        if !ok {
            // Keep what happened (edits may already be applied) but leave history valid.
            agent::repair_history(&mut history);
            if history.last().map_or(false, |m| m.role == "user") {
                history.pop();
            }
        }
        println!();
    }

    if let Some(p) = &hist_path {
        let _ = rl.save_history(p);
    }
    Ok(())
}

/// Record how a turn ended, unless the agent already did (done, step_limit, context_full, ...).
fn log_end(rec: &mut Transcript, result: &Option<Result<()>>) {
    if rec.ended {
        return;
    }
    let ev = match result {
        Some(Ok(())) => json!({ "status": "done" }),
        Some(Err(e)) if e.downcast_ref::<agent::Cancelled>().is_some() => json!({ "status": "cancelled" }),
        Some(Err(e)) => json!({ "status": "error", "error": format!("{e:#}") }),
        None => json!({ "status": "cancelled" }),
    };
    rec.log("end", ev);
}
