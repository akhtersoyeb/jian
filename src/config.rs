use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub model: String,
    pub host: String,
    pub num_ctx: u32,
    /// None = let the model decide. Some(false) disables thinking, Some(true) forces it.
    pub think: Option<bool>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: "gemma4:e4b".into(),
            // Priority: --host flag > config file > OLLAMA_HOST > localhost default
            host: std::env::var("OLLAMA_HOST")
                .ok()
                .filter(|h| !h.trim().is_empty())
                .map(|h| normalize_host(&h))
                .unwrap_or_else(|| "http://localhost:11434".into()),
            num_ctx: 16384,
            think: None,
        }
    }
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("jian").join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let Some(path) = Self::path() else {
            return Ok(Self::default());
        };
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let mut cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        cfg.host = normalize_host(&cfg.host);
        Ok(cfg)
    }
}

/// Accepts what OLLAMA_HOST users actually write: "127.0.0.1", "0.0.0.0:8080", "http://host:1234".
pub fn normalize_host(raw: &str) -> String {
    let raw = raw.trim().trim_end_matches('/');
    let had_scheme = raw.contains("://");
    let (scheme, rest) = match raw.split_once("://") {
        Some((s, r)) => (s.to_string(), r.to_string()),
        None => ("http".to_string(), raw.to_string()),
    };
    let mut rest = rest.replace("0.0.0.0", "127.0.0.1"); // a bind address, not a destination
    if !had_scheme && !rest.contains(':') {
        rest.push_str(":11434");
    }
    format!("{scheme}://{rest}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_normalization() {
        assert_eq!(normalize_host("127.0.0.1"), "http://127.0.0.1:11434");
        assert_eq!(normalize_host("localhost:8080"), "http://localhost:8080");
        assert_eq!(normalize_host("0.0.0.0:11434"), "http://127.0.0.1:11434");
        assert_eq!(
            normalize_host("http://box.lan:1234/"),
            "http://box.lan:1234"
        );
        assert_eq!(normalize_host("https://x.example"), "https://x.example");
    }
}
