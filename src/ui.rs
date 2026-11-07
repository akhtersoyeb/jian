use crate::llm::Usage;
use crate::tools::Plan;
use serde_json::Value;
use similar::{ChangeTag, TextDiff};
use std::io::{stdout, Write};

const GREY: &str = "\x1b[38;5;244m";
const THINK: &str = "\x1b[3;38;5;244m"; // italic grey
const BOLD: &str = "\x1b[1m";
const CYAN: &str = "\x1b[36m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str = "\x1b[0m";

/// Renders a streamed turn: thinking in italic grey, answer in normal text.
#[derive(Default)]
pub struct StreamPrinter {
    in_thinking: bool,
}

impl StreamPrinter {
    pub fn thinking(&mut self, s: &str) {
        if !self.in_thinking {
            print!("{THINK}");
            self.in_thinking = true;
        }
        print!("{s}");
        let _ = stdout().flush();
    }

    pub fn text(&mut self, s: &str) {
        if self.in_thinking {
            print!("{RESET}\n\n");
            self.in_thinking = false;
        }
        print!("{s}");
        let _ = stdout().flush();
    }

    pub fn finish(&mut self) {
        if self.in_thinking {
            print!("{RESET}");
            self.in_thinking = false;
        }
        println!();
        let _ = stdout().flush();
    }
}

/// After a cancelled turn the printer may not have finished; restore the terminal.
pub fn reset_terminal() {
    println!("{RESET}");
}

pub fn usage_line(u: &Usage, num_ctx: u32) {
    let tps = u
        .tokens_per_sec
        .map(|t| format!(" · {t:.1} tok/s"))
        .unwrap_or_default();
    let pct = u.prompt_tokens * 100 / num_ctx.max(1) as u64;
    let color = if pct >= 80 { YELLOW } else { GREY };
    println!(
        "{color}[{} prompt · {} generated{tps} · ctx {pct}%]{RESET}",
        u.prompt_tokens, u.completion_tokens
    );
}

// ---------- tool calls ----------

pub fn tool_call(name: &str, args: &Value) {
    let a = match args.as_object() {
        Some(o) => o
            .iter()
            .map(|(k, v)| format!("{k}={}", compact(v)))
            .collect::<Vec<_>>()
            .join(", "),
        None => args.to_string(),
    };
    println!("{CYAN}●{RESET} {BOLD}{name}{RESET}{GREY}({a}){RESET}");
}

fn compact(v: &Value) -> String {
    match v {
        Value::String(s) => {
            let t: String = s.chars().take(60).collect();
            format!("\"{t}\"")
        }
        other => other.to_string(),
    }
}

pub fn tool_result(name: &str, out: &str) {
    if let Some(msg) = out.strip_prefix("Error:") {
        println!("  {RED}⎿ error:{msg}{RESET}");
        return;
    }
    match name {
        "bash" => {
            let lines: Vec<&str> = out.lines().collect();
            for (i, l) in lines.iter().take(15).enumerate() {
                let lead = if i == 0 { "⎿" } else { " " };
                println!("  {GREY}{lead} {l}{RESET}");
            }
            if lines.len() > 15 {
                println!("  {GREY}  … +{} more lines{RESET}", lines.len() - 15);
            }
        }
        "edit_file" | "write_file" => println!("  {GREY}⎿ {out}{RESET}"),
        _ => println!("  {GREY}⎿ {} lines{RESET}", out.lines().count()),
    }
}

// ---------- previews for actions that need approval ----------

pub fn preview(plan: &Plan) {
    match plan {
        Plan::Edit {
            rel,
            old,
            new,
            count,
            ..
        } => {
            let note = if *count > 1 {
                format!(", {count} occurrences")
            } else {
                String::new()
            };
            println!("{CYAN}●{RESET} {BOLD}edit_file{RESET}{GREY}({rel}{note}){RESET}");
            diff(old, new);
        }
        Plan::Write {
            rel,
            old: Some(old),
            new,
            ..
        } => {
            println!("{CYAN}●{RESET} {BOLD}write_file{RESET}{GREY}({rel}, overwrite){RESET}");
            diff(old, new);
        }
        Plan::Write {
            rel,
            old: None,
            new,
            ..
        } => {
            println!(
                "{CYAN}●{RESET} {BOLD}write_file{RESET}{GREY}({rel}, new file, {} lines){RESET}",
                new.lines().count()
            );
            for (i, l) in new.lines().take(40).enumerate() {
                println!("{GREEN}{:>4} + {l}{RESET}", i + 1);
            }
            let more = new.lines().count().saturating_sub(40);
            if more > 0 {
                println!("{GREY}      … +{more} more lines{RESET}");
            }
        }
        Plan::Bash {
            command,
            timeout_secs,
        } => {
            let t = if *timeout_secs != 60 {
                format!("timeout {timeout_secs}s")
            } else {
                String::new()
            };
            println!("{CYAN}●{RESET} {BOLD}bash{RESET}{GREY}({t}){RESET}");
            for l in command.lines() {
                println!("  {YELLOW}${RESET} {l}");
            }
        }
    }
}

fn diff(old: &str, new: &str) {
    const MAX_LINES: usize = 80;
    let d = TextDiff::from_lines(old, new);
    let mut printed = 0usize;
    for (gi, group) in d.grouped_ops(2).iter().enumerate() {
        if gi > 0 {
            println!("{GREY}      ⋮{RESET}");
        }
        for op in group {
            for ch in d.iter_changes(op) {
                let (sign, color, idx) = match ch.tag() {
                    ChangeTag::Delete => ('-', RED, ch.old_index()),
                    ChangeTag::Insert => ('+', GREEN, ch.new_index()),
                    ChangeTag::Equal => (' ', GREY, ch.new_index()),
                };
                let line = ch.value().trim_end_matches('\n');
                println!(
                    "{color}{:>4} {sign} {line}{RESET}",
                    idx.map_or(0, |i| i + 1)
                );
                printed += 1;
                if printed >= MAX_LINES {
                    println!("{GREY}      … diff truncated{RESET}");
                    return;
                }
            }
        }
    }
}

// ---------- approval prompt ----------

pub enum Choice {
    Yes,
    No,
    Always,
    Abort,
}

pub async fn ask(always: Option<&str>) -> Choice {
    let mut p = format!("{BOLD}Allow?{RESET} {GREY}[y]es · [n]o");
    if let Some(desc) = always {
        p.push_str(&format!(" · [a]lways allow {desc} this session"));
    }
    p.push_str(&format!(" · Ctrl-C cancels{RESET} "));
    print!("{p}");
    let _ = stdout().flush();

    let allow_always = always.is_some();
    let c = tokio::task::spawn_blocking(move || read_choice(allow_always))
        .await
        .unwrap_or(Choice::Abort);
    println!(
        "{}",
        match c {
            Choice::Yes => "yes",
            Choice::No => "no",
            Choice::Always => "always",
            Choice::Abort => "cancelled",
        }
    );
    c
}

fn read_choice(allow_always: bool) -> Choice {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use crossterm::terminal;
    use std::time::Duration;

    // Not a terminal (piped stdin): deny by default.
    if terminal::enable_raw_mode().is_err() {
        return Choice::No;
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = terminal::disable_raw_mode();
        }
    }
    let _guard = Guard;

    // Discard keystrokes typed while the model was working so a stale 'y' can't approve anything.
    while event::poll(Duration::ZERO).unwrap_or(false) {
        let _ = event::read();
    }

    loop {
        let key = match event::read() {
            Ok(Event::Key(k)) => k,
            Ok(_) => continue,
            Err(_) => return Choice::Abort,
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => return Choice::Abort,
            (KeyCode::Char('y' | 'Y'), _) => return Choice::Yes,
            (KeyCode::Char('n' | 'N'), _) | (KeyCode::Esc, _) => return Choice::No,
            (KeyCode::Char('a' | 'A'), _) if allow_always => return Choice::Always,
            _ => {}
        }
    }
}

// ---------- misc ----------

pub fn info(s: &str) {
    println!("{GREY}{s}{RESET}");
}

pub fn error(s: &str) {
    eprintln!("{RED}{s}{RESET}");
}

pub fn banner(model: &str, cwd: &str) {
    println!(
        "{BOLD}Jian Code{RESET} {GREY}v{} · {model} · {cwd}{RESET}",
        env!("CARGO_PKG_VERSION")
    );
    println!("{GREY}/help for commands · Ctrl-C cancels a response · Ctrl-D exits{RESET}\n");
}

pub fn check_ok(s: &str) {
    println!("{GREEN}✓{RESET} {s}");
}

pub fn check_warn(s: &str) {
    println!("{YELLOW}!{RESET} {s}");
}

pub fn check_fail(s: &str) {
    println!("{RED}✗{RESET} {s}");
}
