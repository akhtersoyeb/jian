use crate::llm::Usage;
use serde_json::Value;
use std::io::{stdout, Write};

const GREY: &str = "\x1b[38;5;244m";
const THINK: &str = "\x1b[3;38;5;244m"; // italic grey
const BOLD: &str = "\x1b[1m";
const CYAN: &str = "\x1b[36m";
const RED: &str = "\x1b[31m";
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

pub fn usage_line(u: &Usage) {
    let tps = u.tokens_per_sec.map(|t| format!(" · {t:.1} tok/s")).unwrap_or_default();
    println!("{GREY}[{} prompt · {} generated{tps}]{RESET}", u.prompt_tokens, u.completion_tokens);
}

pub fn tool_call(name: &str, args: &Value) {
    let a = match args.as_object() {
        Some(o) => o.iter().map(|(k, v)| format!("{k}={}", compact(v))).collect::<Vec<_>>().join(", "),
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

pub fn tool_result(out: &str) {
    if let Some(msg) = out.strip_prefix("Error:") {
        println!("  {RED}⎿ error:{msg}{RESET}");
    } else {
        println!("  {GREY}⎿ {} lines{RESET}", out.lines().count());
    }
}

pub fn info(s: &str) {
    println!("{GREY}{s}{RESET}");
}

pub fn error(s: &str) {
    eprintln!("{RED}{s}{RESET}");
}

pub fn banner(model: &str, cwd: &str) {
    println!("{BOLD}Jian Code{RESET} {GREY}v{} · {model} · {cwd}{RESET}", env!("CARGO_PKG_VERSION"));
    println!("{GREY}/help for commands · Ctrl-C cancels a response · Ctrl-D exits{RESET}\n");
}
