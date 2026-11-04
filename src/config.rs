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
            host: "http://localhost:11434".into(),
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
        let Some(path) = Self::path() else { return Ok(Self::default()) };
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}
