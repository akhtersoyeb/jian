mod grep;
mod list_dir;
mod read_file;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const MAX_OUTPUT_CHARS: usize = 8000;
pub const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Clone)]
pub struct Ctx {
    pub root: PathBuf,
}

impl Ctx {
    pub fn new() -> Result<Self> {
        Ok(Self { root: std::env::current_dir()?.canonicalize()? })
    }

    /// Resolve a model-supplied path. It must exist and stay inside the project root.
    pub fn resolve(&self, p: &str) -> Result<PathBuf> {
        let canon = self
            .root
            .join(p) // an absolute `p` replaces root; the starts_with check below catches it
            .canonicalize()
            .map_err(|_| anyhow!("path not found: {p}"))?;
        if !canon.starts_with(&self.root) {
            bail!("path is outside the project directory: {p}");
        }
        Ok(canon)
    }

    pub fn display(&self, p: &Path) -> String {
        p.strip_prefix(&self.root).unwrap_or(p).display().to_string()
    }
}

/// Directories we never walk into.
pub fn is_noise(e: &ignore::DirEntry) -> bool {
    e.depth() > 0 && matches!(e.file_name().to_str(), Some("node_modules" | "target" | ".git"))
}

pub fn definitions() -> Vec<Value> {
    vec![
        json!({"type":"function","function":{
            "name":"read_file",
            "description":"Read a text file and return numbered lines. Returns up to 200 lines by default; use offset/limit to page through larger files.",
            "parameters":{"type":"object","properties":{
                "path":{"type":"string","description":"File path relative to the project root"},
                "offset":{"type":"integer","description":"1-based line to start from (default 1)"},
                "limit":{"type":"integer","description":"Number of lines to read (default 200, max 300)"}
            },"required":["path"]}}}),
        json!({"type":"function","function":{
            "name":"list_dir",
            "description":"List files and directories (respects .gitignore). Directories end with '/'.",
            "parameters":{"type":"object","properties":{
                "path":{"type":"string","description":"Directory relative to the project root (default '.')"},
                "depth":{"type":"integer","description":"How many levels to show, 1-3 (default 1)"}
            }}}}),
        json!({"type":"function","function":{
            "name":"grep",
            "description":"Search file contents with a regex. Returns path:line:text. Respects .gitignore. Prefer this over reading whole files to locate code.",
            "parameters":{"type":"object","properties":{
                "pattern":{"type":"string","description":"Regex to search for"},
                "path":{"type":"string","description":"Directory or file to search (default '.')"},
                "glob":{"type":"string","description":"Only search files matching this glob, e.g. '*.rs'"},
                "ignore_case":{"type":"boolean","description":"Case-insensitive search (default false)"}
            },"required":["pattern"]}}}),
    ]
}

/// Run a tool. Never fails: errors become "Error: ..." text the model can react to.
pub async fn execute(ctx: &Ctx, name: &str, args: &Value) -> String {
    let ctx = ctx.clone();
    let name = name.to_string();
    let args = normalize_args(args);
    // spawn_blocking so a slow grep doesn't block Ctrl-C handling.
    let res = tokio::task::spawn_blocking(move || dispatch(&ctx, &name, &args)).await;
    let out = match res {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => format!("Error: {e}"),
        Err(e) => format!("Error: tool crashed: {e}"),
    };
    truncate(out)
}

fn dispatch(ctx: &Ctx, name: &str, args: &Value) -> Result<String> {
    match name {
        "read_file" => read_file::run(ctx, args),
        "list_dir" => list_dir::run(ctx, args),
        "grep" => grep::run(ctx, args),
        other => bail!("unknown tool '{other}'. Available tools: read_file, list_dir, grep"),
    }
}

/// Some models send arguments as a JSON string instead of an object.
fn normalize_args(args: &Value) -> Value {
    match args {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        v => v.clone(),
    }
}

fn truncate(mut s: String) -> String {
    if s.chars().count() <= MAX_OUTPUT_CHARS {
        return s;
    }
    let cut = s.char_indices().nth(MAX_OUTPUT_CHARS).map(|(i, _)| i).unwrap_or(s.len());
    s.truncate(cut);
    s.push_str("\n[output truncated; narrow your request: smaller range, more specific pattern or path]");
    s
}

// ---- argument helpers (small models are sloppy with types, so be forgiving) ----

pub fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing required string parameter '{key}'"))
}

pub fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

pub fn opt_u64(args: &Value, key: &str) -> Option<u64> {
    let v = args.get(key)?;
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

pub fn opt_bool(args: &Value, key: &str) -> Option<bool> {
    let v = args.get(key)?;
    v.as_bool().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}
