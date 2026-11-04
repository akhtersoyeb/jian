pub mod ollama;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String, // system | user | assistant | tool
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

impl Message {
    pub fn system(s: impl Into<String>) -> Self {
        Self::plain("system", s)
    }
    pub fn user(s: impl Into<String>) -> Self {
        Self::plain("user", s)
    }
    fn plain(role: &str, s: impl Into<String>) -> Self {
        Self { role: role.into(), content: s.into(), tool_calls: vec![], tool_name: None }
    }
    pub fn tool(name: impl Into<String>, content: impl Into<String>) -> Self {
            Self {
                role: "tool".into(),
                content: content.into(),
                tool_calls: vec![],
                tool_name: Some(name.into()),
            }
        }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub tokens_per_sec: Option<f64>,
}

/// Result of one full model turn (after the stream ends).
#[derive(Debug)]
pub struct Turn {
    pub message: Message,
    pub usage: Option<Usage>,
}

/// Streaming events surfaced to the UI while a turn is in flight.
pub enum Event<'a> {
    Thinking(&'a str),
    Text(&'a str),
}
