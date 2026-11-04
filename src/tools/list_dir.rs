use super::{is_noise, opt_str, opt_u64, Ctx};
use anyhow::{bail, Result};
use ignore::WalkBuilder;
use serde_json::Value;

const MAX_ENTRIES: usize = 200;

pub fn run(ctx: &Ctx, args: &Value) -> Result<String> {
    let path = ctx.resolve(opt_str(args, "path").unwrap_or("."))?;
    if !path.is_dir() {
        bail!("'{}' is not a directory; use read_file for files", ctx.display(&path));
    }
    let depth = opt_u64(args, "depth").unwrap_or(1).clamp(1, 3) as usize;

    let walker = WalkBuilder::new(&path)
        .hidden(false) // show dotfiles like .gitignore; .git itself is filtered below
        .max_depth(Some(depth))
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(|e| !is_noise(e))
        .build();

    let mut out = String::new();
    let mut shown = 0usize;
    let mut hidden_count = 0usize;
    for entry in walker.flatten() {
        if entry.depth() == 0 {
            continue;
        }
        if shown >= MAX_ENTRIES {
            hidden_count += 1;
            continue;
        }
        shown += 1;
        let indent = "  ".repeat(entry.depth() - 1);
        let name = entry.file_name().to_string_lossy();
        let slash = if entry.file_type().map_or(false, |t| t.is_dir()) { "/" } else { "" };
        out.push_str(&format!("{indent}{name}{slash}\n"));
    }
    if shown == 0 {
        return Ok("(empty directory)".into());
    }
    if hidden_count > 0 {
        out.push_str(&format!("... and {hidden_count} more entries (list a subdirectory or lower the depth)\n"));
    }
    Ok(out)
}
