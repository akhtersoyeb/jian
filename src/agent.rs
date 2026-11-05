use crate::config::Config;
use crate::llm::ollama::{ChatRequest, Ollama, Options};
use crate::llm::{Event, Message};
use crate::permissions::{Decision, Permissions};
use crate::{tools, ui};
use anyhow::Result;
use std::collections::HashSet;
use std::fmt;

const MAX_STEPS: usize = 25;
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
    history: &mut Vec<Message>,
) -> Result<()> {
    let defs = tools::definitions();
    let mut seen: HashSet<String> = HashSet::new(); // duplicate guard, read-only tools only

    for _ in 0..MAX_STEPS {
        let req = ChatRequest {
            model: &cfg.model,
            messages: history.as_slice(),
            stream: true,
            options: Options {
                num_ctx: cfg.num_ctx,
            },
            think: cfg.think,
            tools: &defs,
        };

        let mut printer = ui::StreamPrinter::default();
        let turn = client
            .chat_stream(&req, |ev| match ev {
                Event::Thinking(s) => printer.thinking(s),
                Event::Text(s) => printer.text(s),
            })
            .await;
        printer.finish();
        let turn = turn?;

        if let Some(u) = &turn.usage {
            ui::usage_line(u);
        }
        let calls = turn.message.tool_calls.clone();
        let empty = turn.message.content.is_empty() && calls.is_empty();
        history.push(turn.message);

        if empty {
            ui::info("(model returned an empty response)");
            return Ok(());
        }
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
        history.push(Message::tool(
            name,
            "Cancelled by the user before this ran.",
        ));
    }
}
