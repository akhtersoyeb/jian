use super::{Event, Message, ToolCall, Turn, Usage};
use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub struct Ollama {
    http: reqwest::Client,
    host: String,
}

fn no_tools(v: &&[Value]) -> bool {
    v.is_empty()
}

#[derive(Serialize)]
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub messages: &'a [Message],
    pub stream: bool,
    pub options: Options,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub think: Option<bool>,
    #[serde(skip_serializing_if = "no_tools")]
    pub tools: &'a [Value],
}

#[derive(Serialize)]
pub struct Options {
    pub num_ctx: u32,
}

#[derive(Deserialize)]
struct Chunk {
    message: Option<ChunkMessage>,
    #[serde(default)]
    done: bool,
    error: Option<String>,
    prompt_eval_count: Option<u64>,
    eval_count: Option<u64>,
    eval_duration: Option<u64>, // nanoseconds
}

#[derive(Deserialize, Default)]
struct ChunkMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    thinking: String,
    #[serde(default)]
    tool_calls: Vec<ToolCall>,
}

#[derive(Deserialize)]
struct Tags {
    models: Vec<TagModel>,
}
#[derive(Deserialize)]
struct TagModel {
    name: String,
}

impl Ollama {
    pub fn new(host: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            host: host.trim_end_matches('/').to_string(),
        }
    }

    /// Startup check: server reachable and model pulled.
    pub async fn ensure_ready(&self, model: &str) -> Result<()> {
        let resp = self
            .http
            .get(format!("{}/api/tags", self.host))
            .send()
            .await
            .map_err(|_| {
                anyhow!(
                    "Can't reach Ollama at {}.\n  Start it with `ollama serve` (or open the Ollama app).",
                    self.host
                )
            })?
            .error_for_status()?;
        let tags: Tags = resp.json().await.context("unexpected /api/tags response")?;
        let want = if model.contains(':') {
            model.to_string()
        } else {
            format!("{model}:latest")
        };
        if !tags.models.iter().any(|m| m.name == want) {
            bail!("Model '{model}' isn't installed.\n  Run: ollama pull {model}");
        }
        Ok(())
    }

    /// Stream one assistant turn. Dropping the returned future cancels the HTTP request,
    /// which makes Ollama stop generating.
    pub async fn chat_stream(
        &self,
        req: &ChatRequest<'_>,
        mut on_event: impl FnMut(Event<'_>),
    ) -> Result<Turn> {
        let resp = self
            .http
            .post(format!("{}/api/chat", self.host))
            .json(req)
            .send()
            .await
            .context("request to Ollama failed")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Ollama returned {status}: {body}");
        }

        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        let mut content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut usage = None;

        while let Some(bytes) = stream.next().await {
            buf.extend_from_slice(&bytes.context("stream interrupted")?);
            // NDJSON: split on b'\n' at the byte level so multi-byte UTF-8 is never cut.
            while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                let line = &line[..line.len() - 1];
                if line.iter().all(|b| b.is_ascii_whitespace()) {
                    continue;
                }
                let chunk: Chunk = serde_json::from_slice(line)
                    .with_context(|| format!("bad chunk: {}", String::from_utf8_lossy(line)))?;
                if let Some(err) = chunk.error {
                    bail!("Ollama error: {err}");
                }
                if let Some(m) = chunk.message {
                    if !m.thinking.is_empty() {
                        on_event(Event::Thinking(&m.thinking));
                    }
                    if !m.content.is_empty() {
                        on_event(Event::Text(&m.content));
                        content.push_str(&m.content);
                    }
                    tool_calls.extend(m.tool_calls);
                }
                if chunk.done {
                    if let (Some(p), Some(c)) = (chunk.prompt_eval_count, chunk.eval_count) {
                        let tps = chunk
                            .eval_duration
                            .filter(|&d| d > 0)
                            .map(|d| c as f64 / (d as f64 / 1e9));
                        usage = Some(Usage {
                            prompt_tokens: p,
                            completion_tokens: c,
                            tokens_per_sec: tps,
                        });
                    }
                }
            }
        }

        Ok(Turn {
            message: Message {
                role: "assistant".into(),
                content,
                tool_calls,
                tool_name: None,
            },
            usage,
        })
    }
}
