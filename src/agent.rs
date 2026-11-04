use crate::config::Config;
use crate::llm::ollama::{ChatRequest, Ollama, Options};
use crate::llm::{Event, Message};
use crate::{tools, ui};
use anyhow::Result;
use std::collections::HashSet;

const MAX_STEPS: usize = 25;

/// Runs one user request to completion: model → tool calls → results → model … until
/// the model answers without calling a tool. Appends everything to `history`.
/// On Err (or if this future is dropped on Ctrl-C) the caller rolls `history` back.
pub async fn run_turn(
    client: &Ollama,
    cfg: &Config,
    ctx: &tools::Ctx,
    history: &mut Vec<Message>,
) -> Result<()> {
    let defs = tools::definitions();
    let mut seen: HashSet<String> = HashSet::new();

    for _ in 0..MAX_STEPS {
        let req = ChatRequest {
            model: &cfg.model,
            messages: history.as_slice(),
            stream: true,
            options: Options { num_ctx: cfg.num_ctx },
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
            ui::tool_call(&name, &args);

            // Small models love to repeat themselves. Short-circuit exact duplicates.
            let out = if !seen.insert(format!("{name}{args}")) {
                "Error: you already made this exact call; its result is earlier in the conversation. \
                 Use it, or try something different."
                    .to_string()
            } else {
                tools::execute(ctx, &name, &args).await
            };
            ui::tool_result(&out);
            history.push(Message::tool(name, out));
        }
    }

    ui::info("(stopped: step limit reached)");
    Ok(())
}
