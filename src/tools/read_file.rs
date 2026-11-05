use super::{opt_u64, str_arg, Ctx, MAX_FILE_BYTES};
use anyhow::{bail, Result};
use serde_json::Value;

const DEFAULT_LIMIT: u64 = 200;
const MAX_LIMIT: usize = 300;
const MAX_LINE_CHARS: usize = 400;

pub fn run(ctx: &Ctx, args: &Value) -> Result<String> {
    let path = ctx.resolve(str_arg(args, "path")?)?;
    let meta = std::fs::metadata(&path)?;
    if meta.is_dir() {
        bail!(
            "'{}' is a directory; use list_dir instead",
            ctx.display(&path)
        );
    }
    if meta.len() > MAX_FILE_BYTES {
        bail!(
            "file is too large to read ({} bytes); use grep to find the part you need",
            meta.len()
        );
    }
    let bytes = std::fs::read(&path)?;
    if bytes[..bytes.len().min(8192)].contains(&0u8) {
        bail!("'{}' looks like a binary file", ctx.display(&path));
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();
    if total == 0 {
        return Ok("(empty file)".into());
    }

    let offset = opt_u64(args, "offset").unwrap_or(1).max(1) as usize;
    let limit = (opt_u64(args, "limit").unwrap_or(DEFAULT_LIMIT) as usize).clamp(1, MAX_LIMIT);
    if offset > total {
        bail!("offset {offset} is past the end of the file ({total} lines)");
    }
    let end = (offset - 1 + limit).min(total);

    let mut out = String::new();
    for (i, line) in lines[offset - 1..end].iter().enumerate() {
        let n = offset + i;
        if line.chars().count() > MAX_LINE_CHARS {
            let t: String = line.chars().take(MAX_LINE_CHARS).collect();
            out.push_str(&format!("{n}\t{t}… [line truncated]\n"));
        } else {
            out.push_str(&format!("{n}\t{line}\n"));
        }
    }
    if end < total {
        out.push_str(&format!(
            "[showing lines {offset}-{end} of {total}; call read_file again with offset={} to continue]",
            end + 1
        ));
    } else {
        out.push_str(&format!("[end of file: {total} lines]"));
    }
    Ok(out)
}
