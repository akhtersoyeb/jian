/// Split text into lowercase words made of letters and digits.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_punctuation() {
        assert_eq!(tokenize("hello, world!"), vec!["hello", "world"]);
    }

    #[test]
    fn lowercases_words() {
        assert_eq!(tokenize("Hello WORLD"), vec!["hello", "world"]);
    }
}
