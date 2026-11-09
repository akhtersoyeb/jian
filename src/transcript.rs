use crate::config::Config;
use crate::llm::Message;
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Append-only JSONL log of a session. One event per line, flushed immediately, so a killed
/// process still leaves a readable file. Logging must never break a session: all errors are ignored.
pub struct Transcript {
    file: Option<File>,
    pub path: Option<PathBuf>,
    /// Set once an "end" event is logged for the current turn.
    pub ended: bool,
}

impl Transcript {
    pub fn disabled() -> Self {
        Self { file: None, path: None, ended: false }
    }

    pub fn open(cfg: &Config, root: &Path, explicit: Option<PathBuf>) -> Self {
        let path = match explicit {
            Some(p) => p,
            None if cfg.transcripts => match default_path(root) {
                Some(p) => p,
                None => return Self::disabled(),
            },
            None => return Self::disabled(),
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(f) => Self { file: Some(f), path: Some(path), ended: false },
            Err(_) => Self::disabled(),
        }
    }

    pub fn log(&mut self, kind: &str, data: Value) {
        if kind == "end" {
            self.ended = true;
        }
        let Some(f) = self.file.as_mut() else { return };
        let mut obj = serde_json::Map::new();
        obj.insert("type".into(), kind.into());
        obj.insert("ts".into(), now_ms().into());
        if let Value::Object(m) = data {
            obj.extend(m);
        }
        let mut line = Value::Object(obj).to_string();
        line.push('\n');
        let _ = f.write_all(line.as_bytes());
    }

    pub fn message(&mut self, m: &Message) {
        self.log("message", json!({ "message": m }));
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// <data dir>/jian/sessions/<project-name>-<stable hash of path>/<timestamp>-<pid>.jsonl
/// FNV-1a rather than DefaultHasher: the path must stay stable across Rust versions (V3 resume).
fn default_path(root: &Path) -> Option<PathBuf> {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "root".into());
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let dir = dirs::data_dir()?
        .join("jian")
        .join("sessions")
        .join(format!("{slug}-{:08x}", fnv1a(root.to_string_lossy().as_bytes()) as u32));
    Some(dir.join(format!("{}-{}.jsonl", now_ms(), std::process::id())))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
