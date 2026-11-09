use std::collections::BTreeMap;

/// Count how many times each word appears.
pub fn word_freq(words: &[String]) -> BTreeMap<String, usize> {
    let mut map = BTreeMap::new();
    for w in words {
        *map.entry(w.clone()).or_insert(0) += 1;
    }
    map
}

/// The longest word; ties go to the first one seen.
pub fn longest_word(words: &[String]) -> Option<&String> {
    let mut best: Option<&String> = None;
    for w in words {
        if best.map_or(true, |b| w.len() > b.len()) {
            best = Some(w);
        }
    }
    best
}

/// Average word length in characters.
pub fn average_length(words: &[String]) -> f64 {
    if words.is_empty() {
        return 0.0;
    }
    let total: usize = words.iter().map(|w| w.chars().count()).sum();
    total as f64 / words.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn counts_words() {
        let f = word_freq(&words(&["a", "b", "a"]));
        assert_eq!(f["a"], 2);
        assert_eq!(f["b"], 1);
    }

    #[test]
    fn finds_longest() {
        let w = words(&["hi", "there", "yo"]);
        assert_eq!(longest_word(&w), Some(&"there".to_string()));
    }

    #[test]
    fn averages_length() {
        assert_eq!(average_length(&words(&["ab", "abcd"])), 3.0);
    }
}
