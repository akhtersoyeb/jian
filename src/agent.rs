use crate::config::Config;
use crate::context::{self, Tracker};
use crate::llm::ollama::{ChatRequest, Ollama, Options};
use crate::llm::{Event, FunctionCall, Message, ToolCall};
use crate::permissions::{Decision, Permissions};
use crate::{tools, ui};
use anyhow::{bail, Result};
use serde_json::Value;
use std::collections::HashSet;
use std::fmt;

const MAX_STEPS: usize = 25;
const TRIM_AT: u64 = 70; // % of num_ctx: start dropping old tool output
const STOP_AT: u64 = 90; // % of num_ctx: refuse to continue (Ollama would truncate silently)
const KEEP_RECENT: usize = 6; // messages never trimmed
const MAX_NUDGES: usize = 2;
const DENIED: &str =
    "The user denied this action. Do not retry it. Ask the user what they would like instead.";

/// Returned when the user cancels at a permission prompt.
#[derive(Debug)]
pub struct Cancelled;
impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cancelled")
    }
}
impl std::error::Error for Cancelled {}

/// Runs one user request to completion: model → tool calls → results → model … until
/// the model answers without calling a tool. Appends everything to `history`.
pub async fn run_turn(
    client: &Ollama,
    cfg: &Config,
    ctx: &tools::Ctx,
    perms: &mut Permissions,
    tracker: &mut Tracker,
    history: &mut Vec<Message>,
) -> Result<()> {
    let defs = tools::definitions();
    let mut seen: HashSet<String> = HashSet::new(); // duplicate guard, read-only tools only
    let mut nudges = 0usize;

    for _ in 0..MAX_STEPS {
        // ---- context budget ----
        let mut est = tracker.estimate(history.as_slice());
        if context::pct(est, cfg.num_ctx) >= TRIM_AT {
            let freed = context::trim_old_tool_results(history.as_mut_slice(), KEEP_RECENT);
            if freed > 0 {
                est = tracker.estimate(history.as_slice());
                ui::info(&format!("(trimmed ~{} KB of old tool output to save context)", freed / 1024));
            }
        }
        let p = context::pct(est, cfg.num_ctx);
        if p >= STOP_AT {
            bail!(
                "Context is nearly full (~{p}% of {} tokens). Use /clear to start fresh, or raise \
                 num_ctx in ~/.config/jian/config.toml.",
                cfg.num_ctx
            );
        }

        let sent_chars = context::history_chars(history.as_slice());
        let req = ChatRequest {
            model: &cfg.model,
            messages: history.as_slice(),
            stream: true,
            options: Options { num_ctx: cfg.num_ctx },
            think: cfg.think,
            tools: &defs,
            keep_alive: "30m",
        };

        let mut printer = ui::StreamPrinter::default();
        let turn = client
            .chat_stream(&req, |ev| match ev {
                Event::Thinking(s) => printer.thinking(s),
                Event::Text(s) => printer.text(s),
            })
            .await;
        printer.finish();
        let mut turn = turn?;

        if let Some(u) = &turn.usage {
            tracker.observe(u.prompt_tokens, sent_chars);
            ui::usage_line(u, cfg.num_ctx);
        }

        // Rescue: the model wrote a tool call as plain text instead of using the tool channel.
        if turn.message.tool_calls.is_empty() {
            if let Some(call) = rescue_tool_call(&turn.message.content) {
                ui::info("(recovered a tool call from text output)");
                turn.message.content.clear();
                turn.message.tool_calls.push(call);
            }
        }

        let calls = turn.message.tool_calls.clone();
        if turn.message.content.trim().is_empty() && calls.is_empty() {
            nudges += 1;
            if nudges > MAX_NUDGES {
                ui::info("(the model keeps returning empty responses; try rephrasing)");
                return Ok(());
            }
            history.push(Message::user(
                "Your last reply was empty. Continue the task: call a tool, or give your final answer.",
            ));
            continue;
        }
        history.push(turn.message);
        if calls.is_empty() {
            return Ok(());
        }

        for call in calls {
            let name = call.function.name;
            let args = call.function.arguments;

            let out = match tools::plan(ctx, &name, &args) {
                Err(e) => {
                    ui::tool_call(&name, &args);
                    format!("Error: {e}")
                }
                Ok(None) => {
                    ui::tool_call(&name, &args);
                    // Small models love to repeat themselves. Short-circuit exact duplicates.
                    if !seen.insert(format!("{name}{args}")) {
                        "Error: you already made this exact call; its result is earlier in the \
                         conversation. Use it, or try something different."
                            .to_string()
                    } else {
                        tools::execute(ctx, &name, &args).await
                    }
                }
                Ok(Some(plan)) => {
                    ui::preview(&plan);
                    match perms.check(&plan).await {
                        Decision::Abort => return Err(Cancelled.into()),
                        Decision::Deny => DENIED.to_string(),
                        Decision::Allow => {
                            let r = tools::apply(ctx, &plan).await;
                            seen.clear(); // the world changed; re-reading is legitimate now
                            r
                        }
                    }
                }
            };
            ui::tool_result(&name, &out);
            history.push(Message::tool(name, out));
        }
    }

    ui::info("(stopped: step limit reached)");
    Ok(())
}

/// Accepts a bare or fenced JSON object like {"name": "grep", "arguments": {...}}
/// naming a known tool. Anything else is plain prose and is left alone.
fn rescue_tool_call(content: &str) -> Option<ToolCall> {
    let t = content.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    if !t.starts_with('{') {
        return None;
    }
    let v: Value = serde_json::from_str(t).ok()?;
    let name = v.get("name")?.as_str()?;
    if !tools::NAMES.iter().any(|n| *n == name) {
        return None;
    }
    let args = v.get("arguments").or_else(|| v.get("parameters")).cloned().unwrap_or(Value::Null);
    Some(ToolCall { id: None, function: FunctionCall { name: name.to_string(), arguments: args } })
}

/// After a cancel/error, make sure every tool call in the last assistant message has a result,
/// so the history stays valid and the model still knows what already happened.
pub fn repair_history(history: &mut Vec<Message>) {
    let Some(idx) = history
        .iter()
        .rposition(|m| m.role == "assistant" && !m.tool_calls.is_empty())
    else {
        return;
    };
    let answered = history.len() - idx - 1;
    let missing: Vec<String> = history[idx]
        .tool_calls
        .iter()
        .skip(answered)
        .map(|c| c.function.name.clone())
        .collect();
    for name in missing {
        history.push(Message::tool(name, "Cancelled by the user before this ran."));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rescues_text_tool_calls() {
        let fenced = "```json\n{\"name\": \"grep\", \"arguments\": {\"pattern\": \"foo\"}}\n```";
        let c = rescue_tool_call(fenced).unwrap();
        assert_eq!(c.function.name, "grep");
        assert_eq!(c.function.arguments["pattern"], "foo");

        let bare = "{\"name\":\"read_file\",\"parameters\":{\"path\":\"a.rs\"}}";
        assert_eq!(rescue_tool_call(bare).unwrap().function.arguments["path"], "a.rs");

        assert!(rescue_tool_call("{\"name\":\"rm_rf\",\"arguments\":{}}").is_none());
        assert!(rescue_tool_call("Here is some prose, not a call.").is_none());
        assert!(rescue_tool_call("{\"foo\": 1}").is_none());
    }
}
