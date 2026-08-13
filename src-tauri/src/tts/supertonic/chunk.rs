//! Splitting text into pieces the engine can synthesize in one pass.
//!
//! A Chunk is sized for the engine, not for the reader: it is not a Sentence.
//! Pure — no model, no filesystem, no network.

use regex::Regex;

pub(crate) fn chunk_text_for_language(text: &str, language: &str) -> Vec<String> {
    let max_len = if language == "ko" || language == "ja" {
        120
    } else {
        300
    };
    chunk_text(text, max_len)
}

fn chunk_text(text: &str, max_len: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let paragraph_re = Regex::new(r"\n\s*\n").expect("paragraph regex");
    for paragraph in paragraph_re.split(text) {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        if char_count(paragraph) <= max_len {
            chunks.push(paragraph.to_string());
            continue;
        }

        let mut current = String::new();
        for sentence in split_sentences(paragraph) {
            let sentence = sentence.trim();
            if sentence.is_empty() {
                continue;
            }
            if char_count(sentence) > max_len {
                push_current_chunk(&mut chunks, &mut current);
                for piece in split_long_piece(sentence, max_len) {
                    push_bounded_piece(&mut chunks, &mut current, &piece, max_len, " ");
                }
            } else {
                push_bounded_piece(&mut chunks, &mut current, sentence, max_len, " ");
            }
        }
        push_current_chunk(&mut chunks, &mut current);
    }

    chunks
}

fn split_sentences(text: &str) -> Vec<String> {
    let boundary = Regex::new(r"([.!?])\s+").expect("sentence regex");
    let matches = boundary.find_iter(text).collect::<Vec<_>>();
    if matches.is_empty() {
        return vec![text.to_string()];
    }

    let mut sentences = Vec::new();
    let mut last_end = 0;
    for match_ in matches {
        let end = match_.start() + 1;
        let before_punctuation = &text[last_end..match_.start()];
        let candidate = format!(
            "{}{}",
            before_punctuation.trim(),
            &text[match_.start()..end]
        );
        if is_abbreviation(&candidate) {
            continue;
        }
        sentences.push(text[last_end..match_.end()].to_string());
        last_end = match_.end();
    }

    if last_end < text.len() {
        sentences.push(text[last_end..].to_string());
    }
    if sentences.is_empty() {
        vec![text.to_string()]
    } else {
        sentences
    }
}

fn split_long_piece(text: &str, max_len: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    for part in text.split_inclusive(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if char_count(part) > max_len {
            push_current_chunk(&mut pieces, &mut current);
            for word_piece in split_by_words(part, max_len) {
                pieces.push(word_piece);
            }
        } else {
            push_bounded_piece(&mut pieces, &mut current, part, max_len, " ");
        }
    }
    push_current_chunk(&mut pieces, &mut current);
    pieces
}

fn split_by_words(text: &str, max_len: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if char_count(word) > max_len {
            push_current_chunk(&mut chunks, &mut current);
            chunks.extend(split_by_chars(word, max_len));
        } else {
            push_bounded_piece(&mut chunks, &mut current, word, max_len, " ");
        }
    }
    push_current_chunk(&mut chunks, &mut current);
    chunks
}

fn split_by_chars(text: &str, max_len: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if char_count(&current) >= max_len {
            chunks.push(current);
            current = String::new();
        }
        current.push(character);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn push_bounded_piece(
    chunks: &mut Vec<String>,
    current: &mut String,
    piece: &str,
    max_len: usize,
    separator: &str,
) {
    if piece.trim().is_empty() {
        return;
    }
    if current.is_empty() {
        current.push_str(piece.trim());
        return;
    }

    let next_len = char_count(current) + char_count(separator) + char_count(piece);
    if next_len > max_len {
        push_current_chunk(chunks, current);
    }
    if !current.is_empty() {
        current.push_str(separator);
    }
    current.push_str(piece.trim());
}

fn push_current_chunk(chunks: &mut Vec<String>, current: &mut String) {
    let chunk = current.trim();
    if !chunk.is_empty() {
        chunks.push(chunk.to_string());
    }
    current.clear();
}

fn is_abbreviation(value: &str) -> bool {
    const ABBREVIATIONS: &[&str] = &[
        "Dr.", "Mr.", "Mrs.", "Ms.", "Prof.", "Sr.", "Jr.", "St.", "Ave.", "Rd.", "Blvd.", "Dept.",
        "Inc.", "Ltd.", "Co.", "Corp.", "etc.", "vs.", "i.e.", "e.g.", "Ph.D.",
    ];
    ABBREVIATIONS
        .iter()
        .any(|abbreviation| value.ends_with(abbreviation))
}

fn char_count(value: &str) -> usize {
    value.chars().count()
}

pub(crate) fn count_words(text: &str) -> usize {
    let word_count = text
        .split_whitespace()
        .filter(|word| !word.trim().is_empty())
        .count();
    if word_count <= 1 && text.chars().any(is_cjk) {
        text.chars()
            .filter(|character| !character.is_whitespace())
            .count()
    } else {
        word_count
    }
}

fn is_cjk(character: char) -> bool {
    matches!(
        character as u32,
        0x3040..=0x30ff | 0x3400..=0x9fff | 0xac00..=0xd7af
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_short_paragraph_whole() {
        let chunks = chunk_text_for_language("A short sentence. And another.", "en");

        assert_eq!(chunks, vec!["A short sentence. And another."]);
    }

    #[test]
    fn splits_cjk_into_shorter_chunks_than_english() {
        // Korean and Japanese pack far more meaning per character, so the same
        // byte budget would produce audio chunks that are much too long.
        let text = "가나다라마바사아자차카타파하".repeat(30);

        let korean = chunk_text_for_language(&text, "ko");
        let english = chunk_text_for_language(&text, "en");

        assert!(
            korean.len() > english.len(),
            "ko={} en={}",
            korean.len(),
            english.len()
        );
        assert!(korean.iter().all(|chunk| char_count(chunk) <= 120));
    }

    #[test]
    fn does_not_end_a_sentence_on_an_abbreviation() {
        let sentences = split_sentences("Dr. Chen wrote it. Prof. Ada agreed.");

        assert_eq!(sentences.len(), 2, "{sentences:?}");
        assert!(sentences[0].starts_with("Dr. Chen"), "{sentences:?}");
    }

    #[test]
    fn breaks_a_sentence_that_is_longer_than_the_budget() {
        let text = "word ".repeat(200);

        let chunks = chunk_text(&text, 100);

        assert!(chunks.len() > 1);
        assert!(
            chunks.iter().all(|chunk| char_count(chunk) <= 100),
            "{chunks:?}"
        );
    }

    #[test]
    fn breaks_a_single_unspaced_run_by_characters() {
        // No whitespace to split on, so the character fallback has to fire or
        // the engine would be handed one oversized chunk.
        let chunks = chunk_text(&"a".repeat(250), 100);

        assert!(chunks.len() >= 3, "{chunks:?}");
        assert!(chunks.iter().all(|chunk| char_count(chunk) <= 100));
    }

    #[test]
    fn counts_cjk_characters_as_words_for_estimates() {
        // CJK has no spaces, so a word count would read as 1 and the duration
        // estimate would be wildly short.
        assert_eq!(count_words("hello there friend"), 3);
        assert_eq!(count_words("안녕하세요"), 5);
    }
}
