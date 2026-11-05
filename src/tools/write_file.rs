use super::{str_arg, Ctx, Plan};
use anyhow::{anyhow, bail, Result};
use serde_json::Value;

pub fn plan(ctx: &Ctx, args: &Value) -> Result<Plan> {
    let p = str_arg(args, "path")?;
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing required string parameter 'content'"))?;
    let path = ctx.resolve_new(p)?;
    ctx.check_writable(&path)?;
    let rel = ctx.display(&path);
    if path.is_dir() {
        bail!("'{rel}' is a directory");
    }
    let old = if path.exists() {
        Some(
            std::fs::read_to_string(&path)
                .map_err(|_| anyhow!("refusing to overwrite '{rel}': not a UTF-8 text file"))?,
        )
    } else {
        None
    };
    if old.as_deref() == Some(content) {
        bail!("'{rel}' already has exactly this content");
    }
    Ok(Plan::Write {
        path,
        rel,
        old,
        new: content.to_string(),
    })
}
