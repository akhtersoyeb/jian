mod bash;
mod edit_file;
mod grep;
mod list_dir;
mod read_file;
mod write_file;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

pub const MAX_OUTPUT_CHARS: usize = 8000;
pub const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
pub const NAMES: [&str; 6] = [
    "read_file",
    "list_dir",
    "grep",
    "edit_file",
    "write_file",
    "bash",
];

#[derive(Clone)]
pub struct Ctx {
    pub root: PathBuf,
}

impl Ctx {
    pub fn new() -> Result<Self> {
        Ok(Self {
            root: std::env::current_dir()?.canonicalize()?,
        })
    }

    /// Resolve an EXISTING path. Must stay inside the project root.
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

    /// Resolve a path that may not exist yet (for write_file).
    pub fn resolve_new(&self, p: &str) -> Result<PathBuf> {
        let joined = self.root.join(p);
        let mut norm = PathBuf::new();
        for c in joined.components() {
            match c {
                Component::ParentDir => {
                    if !norm.pop() {
                        bail!("path is outside the project directory: {p}");
                    }
                }
                Component::CurDir => {}
                other => norm.push(other.as_os_str()),
            }
        }
        // Canonicalize the deepest existing ancestor (defeats symlink escapes), re-append the rest.
        let mut anc = norm.as_path();
        let mut tail = Vec::new();
        while !anc.exists() {
            tail.push(
                anc.file_name()
                    .ok_or_else(|| anyhow!("invalid path: {p}"))?
                    .to_owned(),
            );
            anc = anc.parent().ok_or_else(|| anyhow!("invalid path: {p}"))?;
        }
        let mut canon = anc.canonicalize()?;
        for t in tail.iter().rev() {
            canon.push(t);
        }
        if !canon.starts_with(&self.root) {
            bail!("path is outside the project directory: {p}");
        }
        Ok(canon)
    }

    pub fn check_writable(&self, canon: &Path) -> Result<()> {
        if let Ok(rel) = canon.strip_prefix(&self.root) {
            if rel
                .components()
                .next()
                .map_or(false, |c| c.as_os_str() == ".git")
            {
                bail!("refusing to modify the .git directory");
            }
        }
        Ok(())
    }

    pub fn display(&self, p: &Path) -> String {
        p.strip_prefix(&self.root)
            .unwrap_or(p)
            .display()
            .to_string()
    }
}

/// A validated, side-effect-free description of a mutating action.
pub enum Plan {
    Edit {
        path: PathBuf,
        rel: String,
        old: String,
        new: String,
        count: usize,
    },
    Write {
        path: PathBuf,
        rel: String,
        old: Option<String>,
        new: String,
    },
    Bash {
        command: String,
        timeout_secs: u64,
    },
}

impl Plan {
    /// (permission key, human description of what "always" covers). None = "always" not offered.
    pub fn always_rule(&self) -> Option<(String, String)> {
        match self {
            Plan::Edit { .. } | Plan::Write { .. } => Some(("edit".into(), "file edits".into())),
            Plan::Bash { command, .. } => {
                bash::always_key(command).map(|k| (format!("bash:{k}"), format!("`{k}` commands")))
            }
        }
    }
}

/// Directories we never walk into.
pub fn is_noise(e: &ignore::DirEntry) -> bool {
    e.depth() > 0
        && matches!(
            e.file_name().to_str(),
            Some("node_modules" | "target" | ".git")
        )
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
        json!({"type":"function","function":{
            "name":"edit_file",
            "description":"Edit an existing file by replacing old_str with new_str. old_str must match the file exactly (including whitespace and indentation) and be unique in the file: include a few surrounding lines for context. Read the file first. The user must approve each edit.",
            "parameters":{"type":"object","properties":{
                "path":{"type":"string","description":"File path relative to the project root"},
                "old_str":{"type":"string","description":"Exact text to replace"},
                "new_str":{"type":"string","description":"Replacement text (empty string deletes old_str)"},
                "replace_all":{"type":"boolean","description":"Replace every occurrence instead of requiring a unique match (default false)"}
            },"required":["path","old_str","new_str"]}}}),
        json!({"type":"function","function":{
            "name":"write_file",
            "description":"Create a new file, or completely overwrite an existing one. To change part of an existing file use edit_file instead. Parent directories are created automatically. The user must approve each write.",
            "parameters":{"type":"object","properties":{
                "path":{"type":"string","description":"File path relative to the project root"},
                "content":{"type":"string","description":"Full file content"}
            },"required":["path","content"]}}}),
        json!({"type":"function","function":{
            "name":"bash",
            "description":"Run a shell command in the project root. No stdin, so never run interactive commands or servers that don't exit. Returns exit code, stdout and stderr (long output keeps the END). The user must approve each command.",
            "parameters":{"type":"object","properties":{
                "command":{"type":"string","description":"Command to run, e.g. 'cargo test'"},
                "timeout_secs":{"type":"integer","description":"Kill the command after this many seconds (default 60, max 600)"}
            },"required":["command"]}}}),
    ]
}

/// Validate a call to a mutating tool WITHOUT side effects.
/// Ok(None) means the tool is read-only: run it with `execute`.
pub fn plan(ctx: &Ctx, name: &str, args: &Value) -> Result<Option<Plan>> {
    let args = normalize_args(args);
    match name {
        "edit_file" => edit_file::plan(ctx, &args).map(Some),
        "write_file" => write_file::plan(ctx, &args).map(Some),
        "bash" => bash::plan(&args).map(Some),
        _ => Ok(None),
    }
}

pub async fn apply(ctx: &Ctx, plan: &Plan) -> String {
    let out = match plan {
        Plan::Edit {
            path,
            rel,
            old,
            new,
            count,
        } => {
            if std::fs::read_to_string(path).ok().as_deref() != Some(old.as_str()) {
                format!("Error: {rel} changed on disk after the edit was planned. Re-read the file and try again.")
            } else {
                match std::fs::write(path, new) {
                    Ok(()) => format!("Edited {rel}: replaced {count} occurrence(s)."),
                    Err(e) => format!("Error: could not write {rel}: {e}"),
                }
            }
        }
        Plan::Write {
            path,
            rel,
            old,
            new,
        } => {
            let unchanged = match old {
                Some(o) => std::fs::read_to_string(path).ok().as_deref() == Some(o.as_str()),
                None => !path.exists(),
            };
            if !unchanged {
                format!("Error: {rel} changed on disk after the write was planned. Re-read it and try again.")
            } else {
                let r = match path.parent() {
                    Some(dir) => std::fs::create_dir_all(dir),
                    None => Ok(()),
                }
                .and_then(|_| std::fs::write(path, new));
                match r {
                    Ok(()) => format!(
                        "{} {rel} ({} lines).",
                        if old.is_some() {
                            "Overwrote"
                        } else {
                            "Created"
                        },
                        new.lines().count()
                    ),
                    Err(e) => format!("Error: could not write {rel}: {e}"),
                }
            }
        }
        Plan::Bash {
            command,
            timeout_secs,
        } => bash::run(ctx, command, *timeout_secs).await,
    };
    truncate(out)
}

/// Run a read-only tool. Never fails: errors become "Error: ..." text the model can react to.
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
        other => bail!(
            "unknown tool '{other}'. Available tools: read_file, list_dir, grep, edit_file, write_file, bash"
        ),
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
    let cut = s
        .char_indices()
        .nth(MAX_OUTPUT_CHARS)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    s.truncate(cut);
    s.push_str(
        "\n[output truncated; narrow your request: smaller range, more specific pattern or path]",
    );
    s
}

// ---- argument helpers (small models are sloppy with types, so be forgiving) ----

pub fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing required string parameter '{key}'"))
}

pub fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

pub fn opt_u64(args: &Value, key: &str) -> Option<u64> {
    let v = args.get(key)?;
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

pub fn opt_bool(args: &Value, key: &str) -> Option<bool> {
    let v = args.get(key)?;
    v.as_bool()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}
