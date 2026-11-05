use super::{opt_u64, str_arg, Ctx, Plan};
use anyhow::{bail, Result};
use serde_json::Value;
use std::process::{Output, Stdio};
use std::time::Duration;
use tokio::process::Command;

const TAIL_CHARS: usize = 3500; // per stream; keeps the total under MAX_OUTPUT_CHARS

pub fn plan(args: &Value) -> Result<Plan> {
    let command = str_arg(args, "command")?.trim().to_string();
    if command.is_empty() {
        bail!("command is empty");
    }
    let timeout_secs = opt_u64(args, "timeout_secs").unwrap_or(60).clamp(1, 600);
    Ok(Plan::Bash {
        command,
        timeout_secs,
    })
}

pub async fn run(ctx: &Ctx, command: &str, timeout_secs: u64) -> String {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };
    cmd.current_dir(&ctx.root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true); // timeout or Ctrl-C drops the future, which kills the child

    match tokio::time::timeout(Duration::from_secs(timeout_secs), cmd.output()).await {
        Err(_) => format!(
            "Error: command timed out after {timeout_secs}s and was killed. If it is expected to \
             take long, narrow its scope or pass a larger timeout_secs."
        ),
        Ok(Err(e)) => format!("Error: failed to run command: {e}"),
        Ok(Ok(out)) => format_output(out),
    }
}

fn format_output(out: Output) -> String {
    let code = out
        .status
        .code()
        .map_or("killed by signal".to_string(), |c| c.to_string());
    let stdout = tail(&String::from_utf8_lossy(&out.stdout));
    let stderr = tail(&String::from_utf8_lossy(&out.stderr));
    let mut s = format!("exit code: {code}\n");
    if !stdout.trim().is_empty() {
        s.push_str(&format!("--- stdout ---\n{}\n", stdout.trim_end()));
    }
    if !stderr.trim().is_empty() {
        s.push_str(&format!("--- stderr ---\n{}\n", stderr.trim_end()));
    }
    if stdout.trim().is_empty() && stderr.trim().is_empty() {
        s.push_str("(no output)");
    }
    s
}

/// Keep the END of long output: errors and test summaries live there.
fn tail(s: &str) -> String {
    let total = s.chars().count();
    if total <= TAIL_CHARS {
        return s.to_string();
    }
    let skip = total - TAIL_CHARS;
    let idx = s.char_indices().nth(skip).map(|(i, _)| i).unwrap_or(0);
    format!("[... {skip} chars omitted ...]\n{}", &s[idx..])
}

/// What "always allow" would cover for this command. None = never offer it.
pub fn always_key(command: &str) -> Option<String> {
    const DENY: &[&str] = &[
        "rm", "sudo", "dd", "mv", "chmod", "chown", "sh", "bash", "zsh", "eval", "curl", "wget",
        "kill", "pkill", "mkfs", "shutdown", "reboot",
    ];
    if command.chars().any(|c| ";|&<>`$\n\\".contains(c)) {
        return None; // compound command: always ask
    }
    let mut it = command.split_whitespace();
    let first = it.next()?;
    if DENY.contains(&first) {
        return None;
    }
    Some(match it.next() {
        Some(second) if !second.starts_with('-') => format!("{first} {second}"),
        _ => first.to_string(),
    })
}
