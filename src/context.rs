use crate::llm::Message;
use crate::tools;
use ignore::WalkBuilder;
use std::path::Path;

const MAX_TREE_ENTRIES: usize = 60;
const MAX_INSTRUCTIONS_CHARS: usize = 4000;
const STUB: &str = "[old tool output removed to save context; call the tool again if you need it]";

const RULES: &str = "\
You are Jian Code, a coding agent running in the user's terminal, helping with software tasks \
in the project described below.\n\n\
Tools: list_dir, grep, read_file (read-only); edit_file, write_file, bash (the user approves \
each one).\n\
Rules:\n\
- Never guess file contents or project structure. Look first: list_dir or grep to find the \
relevant files, then read_file.\n\
- Prefer grep to locate code; read files in ranges instead of whole.\n\
- Always read a file before editing it. To change an existing file use edit_file with an \
old_str copied exactly from the file and unique within it (include a few surrounding lines). \
Use write_file only for new files.\n\
- Make the smallest change that does the job. Don't rewrite code you weren't asked to touch.\n\
- After changing code, verify it by running the project's tests or build with bash when the \
command is obvious (cargo test, npm test, pytest, ...).\n\
- bash has no stdin and a timeout: never run interactive programs or servers that don't exit. \
Never run destructive commands unless the user explicitly asked.\n\
- If the user denies an action, don't retry it; ask what they'd prefer.\n\
- If a tool returns an error, read the message and correct your call.\n\
- When explaining code, cite locations as path:line. Be concise. Once you have enough \
information, answer; don't keep exploring.";

pub fn system_prompt(root: &Path) -> String {
    let mut s = String::from(RULES);
    s.push_str(&format!(
        "\n\n# Environment\nWorking directory: {}\nOS: {}\n",
        root.display(),
        std::env::consts::OS
    ));
    if let Some(branch) = git(root, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        s.push_str(&format!("Git branch: {branch}\n"));
        if let Some(status) = git(root, &["status", "--short"]) {
            if status.is_empty() {
                s.push_str("Git status: clean\n");
            } else {
                let lines: Vec<&str> = status.lines().collect();
                s.push_str("Git status (uncommitted changes):\n");
                for l in lines.iter().take(15) {
                    s.push_str(&format!("  {l}\n"));
                }
                if lines.len() > 15 {
                    s.push_str(&format!("  ... +{} more\n", lines.len() - 15));
                }
            }
        }
    }
    s.push_str("\n# Project layout (depth 2)\n");
    s.push_str(&tree(root));
    if let Some(text) = project_instructions(root) {
        s.push_str("\n# Project instructions (from JIAN.md)\n");
        s.push_str(&text);
        s.push('\n');
    }
    s
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn tree(root: &Path) -> String {
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .max_depth(Some(2))
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(|e| !tools::is_noise(e))
        .build();
    let mut out = String::new();
    let (mut shown, mut more) = (0usize, 0usize);
    for e in walker.flatten() {
        if e.depth() == 0 {
            continue;
        }
        if shown >= MAX_TREE_ENTRIES {
            more += 1;
            continue;
        }
        shown += 1;
        let indent = "  ".repeat(e.depth() - 1);
        let slash = if e.file_type().map_or(false, |t| t.is_dir()) {
            "/"
        } else {
            ""
        };
        out.push_str(&format!(
            "{indent}{}{slash}\n",
            e.file_name().to_string_lossy()
        ));
    }
    if shown == 0 {
        return "(empty)\n".into();
    }
    if more > 0 {
        out.push_str(&format!(
            "... +{more} more entries (use list_dir to explore)\n"
        ));
    }
    out
}

fn project_instructions(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("JIAN.md")).ok()?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if text.chars().count() > MAX_INSTRUCTIONS_CHARS {
        let t: String = text.chars().take(MAX_INSTRUCTIONS_CHARS).collect();
        Some(format!("{t}\n[JIAN.md truncated]"))
    } else {
        Some(text.to_string())
    }
}

// ---------- context budget ----------

pub fn pct(tokens: u64, num_ctx: u32) -> u64 {
    tokens * 100 / num_ctx.max(1) as u64
}

pub fn history_chars(h: &[Message]) -> usize {
    h.iter()
        .map(|m| {
            m.content.len()
                + m.tool_calls
                    .iter()
                    .map(|c| c.function.name.len() + c.function.arguments.to_string().len())
                    .sum::<usize>()
        })
        .sum()
}

/// Estimates prompt size without a tokenizer by learning tokens-per-byte from Ollama's own counts.
pub struct Tracker {
    ratio: f64,
    pub last_prompt: u64,
}

impl Default for Tracker {
    fn default() -> Self {
        Self {
            ratio: 0.33,
            last_prompt: 0,
        }
    }
}

impl Tracker {
    pub fn estimate(&self, h: &[Message]) -> u64 {
        (history_chars(h) as f64 * self.ratio) as u64
    }

    /// `chars` = history_chars at the time the request was sent.
    pub fn observe(&mut self, prompt_tokens: u64, chars: usize) {
        self.last_prompt = prompt_tokens;
        if chars > 2000 && prompt_tokens > 0 {
            self.ratio = prompt_tokens as f64 / chars as f64;
        }
    }
}

/// Replace big tool results older than the last `keep_last` messages with a stub.
/// Returns bytes freed.
pub fn trim_old_tool_results(h: &mut [Message], keep_last: usize) -> usize {
    let cutoff = h.len().saturating_sub(keep_last);
    let mut freed = 0;
    for m in h[..cutoff].iter_mut() {
        if m.role == "tool" && m.content.len() > 200 && m.content != STUB {
            freed += m.content.len() - STUB.len();
            m.content = STUB.into();
        }
    }
    freed
}
