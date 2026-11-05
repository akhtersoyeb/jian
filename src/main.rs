mod agent;
mod config;
mod llm;
mod permissions;
mod tools;
mod ui;

use anyhow::Result;
use clap::Parser;
use llm::ollama::Ollama;
use llm::Message;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

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
}

const HELP: &str = "\
/help     show this help
/clear    reset the conversation
/exit     quit (or Ctrl-D)";

fn system_prompt() -> String {
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    format!(
        "You are Jian Code, a coding agent running in the user's terminal. \
         You help with software tasks in the project at: {cwd}\n\
         OS: {}\n\n\
         Tools: list_dir, grep, read_file (read-only); edit_file, write_file, bash (the user \
         approves each one).\n\
         Rules:\n\
         - Never guess file contents or project structure. Look first: list_dir or grep to \
         find the relevant files, then read_file.\n\
         - Prefer grep to locate code; read files in ranges instead of whole.\n\
         - Always read a file before editing it. To change an existing file use edit_file with \
         an old_str copied exactly from the file and unique within it (include a few \
         surrounding lines). Use write_file only for new files.\n\
         - Make the smallest change that does the job. Don't rewrite code you weren't asked to touch.\n\
         - After changing code, verify it by running the project's tests or build with bash \
         when the command is obvious (cargo test, npm test, pytest, ...).\n\
         - bash has no stdin and a timeout: never run interactive programs or servers that \
         don't exit. Never run destructive commands unless the user explicitly asked.\n\
         - If the user denies an action, don't retry it; ask what they'd prefer.\n\
         - If a tool returns an error, read the message and correct your call.\n\
         - When explaining code, cite locations as path:line. Be concise. Once you have \
         enough information, answer; don't keep exploring.",
        std::env::consts::OS
    )
}

/// Main entry point of the Jian agent.
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
    let mut perms = permissions::Permissions::default();
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

        history.push(Message::user(input));

        let result = tokio::select! {
            r = agent::run_turn(&client, &cfg, &ctx, &mut perms, &mut history) => Some(r),
            _ = tokio::signal::ctrl_c() => None,
        };

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
