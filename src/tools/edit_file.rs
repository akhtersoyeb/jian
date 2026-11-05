use super::{opt_bool, str_arg, Ctx, Plan};
use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use similar::TextDiff;

pub fn plan(ctx: &Ctx, args: &Value) -> Result<Plan> {
    let path = ctx.resolve(str_arg(args, "path")?)?;
    ctx.check_writable(&path)?;
    let rel = ctx.display(&path);
    if path.is_dir() {
        bail!("'{rel}' is a directory");
    }
    let old_str = str_arg(args, "old_str")?;
    let new_str = args.get("new_str").and_then(Value::as_str).ok_or_else(|| {
        anyhow!("missing parameter 'new_str' (use an empty string to delete the text)")
    })?;
    if old_str.is_empty() {
        bail!("old_str must not be empty. To create a new file use write_file.");
    }
    if old_str == new_str {
        bail!("old_str and new_str are identical; nothing to change");
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|_| anyhow!("'{rel}' is not a readable UTF-8 text file"))?;

    // The model almost always emits \n; match the file's own line endings.
    let (old_m, new_m) = if content.contains("\r\n") {
        (to_crlf(old_str), to_crlf(new_str))
    } else {
        (old_str.to_string(), new_str.to_string())
    };

    let count = content.matches(old_m.as_str()).count();
    if count == 0 {
        bail!(
            "old_str was not found in {rel}.{}",
            closest_hint(&content, old_str)
        );
    }
    let replace_all = opt_bool(args, "replace_all").unwrap_or(false);
    if count > 1 && !replace_all {
        bail!(
            "old_str matches {count} places in {rel} (lines {}). Include more surrounding lines \
             so it is unique, or set replace_all=true to change every occurrence.",
            match_lines(&content, &old_m)
        );
    }
    let new = if replace_all {
        content.replace(old_m.as_str(), &new_m)
    } else {
        content.replacen(old_m.as_str(), &new_m, 1)
    };
    Ok(Plan::Edit {
        path,
        rel,
        old: content,
        new,
        count: if replace_all { count } else { 1 },
    })
}

fn to_crlf(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\n', "\r\n")
}

fn match_lines(content: &str, needle: &str) -> String {
    content
        .match_indices(needle)
        .take(5)
        .map(|(i, _)| (content[..i].matches('\n').count() + 1).to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// When old_str isn't found, point at the most similar line so the model can self-correct.
fn closest_hint(content: &str, old: &str) -> String {
    const GENERIC: &str =
        " Re-read the file with read_file and copy the text exactly, including indentation.";
    let Some(probe) = old.lines().map(str::trim).find(|l| l.len() >= 3) else {
        return GENERIC.into();
    };
    let probe: String = probe.chars().take(300).collect();
    let best = content
        .lines()
        .enumerate()
        .filter(|(_, l)| l.len() <= 500)
        .take(20_000)
        .map(|(i, l)| {
            (
                i + 1,
                TextDiff::from_chars(probe.as_str(), l.trim()).ratio(),
                l,
            )
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    match best {
        Some((n, r, l)) if r > 0.6 => format!(
            " The closest line is {n}: `{}`. Re-read that region with read_file and copy the \
             text exactly, including indentation.",
            l.trim()
        ),
        _ => GENERIC.into(),
    }
}
