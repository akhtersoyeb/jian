use std::collections::BTreeMap;

/// One line per word, most frequent first (ties alphabetical), at most `top` lines.
pub fn render(freq: &BTreeMap<String, usize>, top: usize) -> String {
    let mut items: Vec<(&String, &usize)> = freq.iter().collect();
    items.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    items
        .into_iter()
        .take(top)
        .map(|(w, c)| format!("{w} {c}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_by_count_then_word() {
        let mut f = BTreeMap::new();
        f.insert("b".to_string(), 2);
        f.insert("a".to_string(), 2);
        f.insert("c".to_string(), 5);
        assert_eq!(render(&f, 2), "c 5\na 2");
    }
}
