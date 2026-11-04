use super::{is_noise, opt_bool, opt_str, str_arg, Ctx, MAX_FILE_BYTES};
use anyhow::{anyhow, Result};
use ignore::{overrides::OverrideBuilder, WalkBuilder};
use regex::RegexBuilder;
use serde_json::Value;

const MAX_MATCHES: usize = 100;
const MAX_LINE_CHARS: usize = 300;

pub fn run(ctx: &Ctx, args: &Value) -> Result<String> {
    let pattern = str_arg(args, "pattern")?;
    let root = ctx.resolve(opt_str(args, "path").unwrap_or("."))?;
    let re = RegexBuilder::new(pattern)
        .case_insensitive(opt_bool(args, "ignore_case").unwrap_or(false))
        .build()
        .map_err(|e| anyhow!("invalid regex: {e}"))?;

    let mut wb = WalkBuilder::new(&root);
    wb.hidden(false).filter_entry(|e| !is_noise(e));
    if let Some(g) = opt_str(args, "glob") {
        let mut ob = OverrideBuilder::new(&root);
        ob.add(g).map_err(|e| anyhow!("invalid glob: {e}"))?;
        wb.overrides(ob.build()?);
    }

    let mut out = String::new();
    let mut matches = 0usize;
    let mut files = 0usize;
    let mut truncated = false;

    'walk: for entry in wb.build().flatten() {
        if !entry.file_type().map_or(false, |t| t.is_file()) {
            continue;
        }
        if entry.metadata().map_or(true, |m| m.len() > MAX_FILE_BYTES) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else { continue }; // skips non-UTF8
        if text.contains('\0') {
            continue;
        }
        let rel = ctx.display(entry.path());
        let mut file_hit = false;
        for (i, line) in text.lines().enumerate() {
            if !re.is_match(line) {
                continue;
            }
            if !file_hit {
                file_hit = true;
                files += 1;
            }
            let t: String = line.trim().chars().take(MAX_LINE_CHARS).collect();
            out.push_str(&format!("{rel}:{}:{t}\n", i + 1));
            matches += 1;
            if matches >= MAX_MATCHES {
                truncated = true;
                break 'walk;
            }
        }
    }

    if matches == 0 {
        return Ok("No matches found.".into());
    }
    out.push_str(&format!("[{matches} matches in {files} files"));
    if truncated {
        out.push_str(&format!("; stopped at {MAX_MATCHES}, refine your pattern, path or glob"));
    }
    out.push(']');
    Ok(out)
}
