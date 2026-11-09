use std::io::Read;
use wordstat::{report, stats, tokenize};

fn main() {
    let path = std::env::args().nth(1);
    let mut text = String::new();
    match path {
        Some(p) => text = std::fs::read_to_string(&p).expect("could not read file"),
        None => {
            std::io::stdin()
                .read_to_string(&mut text)
                .expect("could not read stdin");
        }
    }
    let words = tokenize::tokenize(&text);
    let freq = stats::word_freq(&words);
    println!("{}", report::render(&freq, 5));
    println!("total: {}", words.len());
}
