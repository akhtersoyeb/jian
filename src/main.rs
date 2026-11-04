mod agent;
mod config;
mod llm;
mod tools;
mod ui;

use anyhow::Result;
use clap::Parser;
use llm::ollama::Ollama;
use llm::Message;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

#[derive(Parser)]
#[command(name = "jian", version, about = "Jian Code: a local coding agent for your terminal")]
struct Cli {
    /// Ollama model to use (overrides config)
    #[arg(long, short)]
    model: Option<String>,
    /// Ollama host URL (overrides config)
    #[arg(long)]
    host: Option<String>,
}

const HELP: &str = "\
/help     show this help
/clear    reset the conversation
/exit     quit (or Ctrl-D)";

fn system_prompt() -> String {
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
    format!(
        "You are Jian Code, a coding agent running in the user's terminal. \
         You help with software tasks in the project at: {cwd}\n\
         OS: {}\n\n\
         You have read-only tools: list_dir, grep, read_file.\n\
         Rules:\n\
         - Never guess file contents or project structure. Look first: use list_dir or grep \
         to find the relevant files, then read_file to read them.\n\
         - Prefer grep to locate code; read files in ranges instead of whole.\n\
         - When explaining code, cite locations as path:line.\n\
         - Be concise. Once you have enough information, answer; don't keep exploring.\n\
         - If a tool returns an error, read the message and correct your call.",
        std::env::consts::OS
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut cfg = config::Config::load()?;
    if let Some(m) = cli.model {
        cfg.model = m;
    }
    if let Some(h) = cli.host {
        cfg.host = h;
    }

    let client = Ollama::new(&cfg.host);
    if let Err(e) = client.ensure_ready(&cfg.model).await {
        ui::error(&format!("{e}"));
        std::process::exit(1);
    }

    let ctx = tools::Ctx::new()?;
    ui::banner(&cfg.model, &ctx.root.display().to_string());

    let mut history = vec![Message::system(system_prompt())];
    let mut rl = DefaultEditor::new()?;
    let hist_path = dirs::data_dir().map(|d| d.join("jian").join("history.txt"));
    if let Some(p) = &hist_path {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = rl.load_history(p);
    }

    loop {
        let line = match rl.readline("› ") {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => {
                ui::info("(Ctrl-D or /exit to quit)");
                continue;
            }
            Err(ReadlineError::Eof) => break,
            Err(e) => return Err(e.into()),
        };
        let input = line.trim();
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
                history.truncate(1);
                ui::info("Conversation cleared.");
                continue;
            }
            s if s.starts_with('/') => {
                ui::info("Unknown command. Try /help");
                continue;
            }
            _ => {}
        }

        let checkpoint = history.len();
        history.push(Message::user(input));

        let result = tokio::select! {
            r = agent::run_turn(&client, &cfg, &ctx, &mut history) => Some(r),
            _ = tokio::signal::ctrl_c() => None,
        };

        match result {
            Some(Ok(())) => {}
            Some(Err(e)) => {
                ui::error(&format!("{e:#}"));
                history.truncate(checkpoint); // drop the half-finished exchange
            }
            None => {
                ui::reset_terminal();
                ui::info("(cancelled)");
                history.truncate(checkpoint);
            }
        }
        println!();
    }

    if let Some(p) = &hist_path {
        let _ = rl.save_history(p);
    }
    Ok(())
}
