//! Turning text into the unicode ids the model expects.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use ndarray::Array3;
use regex::Regex;
use unicode_normalization::UnicodeNormalization;

use crate::error::{AppError, AppResult};
use crate::tts::supertonic::voice::is_valid_supertonic_language;

pub(crate) struct UnicodeProcessor {
    indexer: Vec<i64>,
}

impl UnicodeProcessor {
    pub(crate) fn new(unicode_indexer_json_path: &Path) -> AppResult<Self> {
        let file = File::open(unicode_indexer_json_path)?;
        let reader = BufReader::new(file);
        let indexer: Vec<i64> = serde_json::from_reader(reader)?;
        Ok(Self { indexer })
    }

    pub(crate) fn call(
        &self,
        text_list: &[String],
        lang_list: &[String],
    ) -> AppResult<(Vec<Vec<i64>>, Array3<f32>)> {
        let processed_texts = text_list
            .iter()
            .zip(lang_list.iter())
            .map(|(text, language)| preprocess_text(text, language))
            .collect::<AppResult<Vec<_>>>()?;
        let text_ids_lengths = processed_texts
            .iter()
            .map(|text| text.chars().count())
            .collect::<Vec<_>>();
        let max_len = *text_ids_lengths.iter().max().unwrap_or(&0);

        let mut text_ids = Vec::new();
        for text in &processed_texts {
            let mut row = vec![0_i64; max_len];
            for (index, value) in text_to_unicode_values(text).iter().enumerate() {
                row[index] = if *value < self.indexer.len() {
                    self.indexer[*value]
                } else {
                    -1
                };
            }
            text_ids.push(row);
        }

        Ok((text_ids, get_text_mask(&text_ids_lengths)))
    }
}

pub(crate) fn preprocess_text(text: &str, language: &str) -> AppResult<String> {
    if !is_valid_supertonic_language(language) {
        return Err(AppError::InvalidInput(format!(
            "unknown Supertonic language: {language}"
        )));
    }

    let mut text = text.nfkd().collect::<String>();
    let emoji_pattern = Regex::new(r"[\x{1F600}-\x{1F64F}\x{1F300}-\x{1F5FF}\x{1F680}-\x{1F6FF}\x{1F700}-\x{1F77F}\x{1F780}-\x{1F7FF}\x{1F800}-\x{1F8FF}\x{1F900}-\x{1F9FF}\x{1FA00}-\x{1FA6F}\x{1FA70}-\x{1FAFF}\x{2600}-\x{26FF}\x{2700}-\x{27BF}\x{1F1E6}-\x{1F1FF}]+")
        .expect("emoji regex");
    text = emoji_pattern.replace_all(&text, "").to_string();

    for (from, to) in [
        ("–", "-"),
        ("‑", "-"),
        ("—", "-"),
        ("_", " "),
        ("\u{201C}", "\""),
        ("\u{201D}", "\""),
        ("\u{2018}", "'"),
        ("\u{2019}", "'"),
        ("´", "'"),
        ("`", "'"),
        ("[", " "),
        ("]", " "),
        ("|", " "),
        ("/", " "),
        ("#", " "),
        ("→", " "),
        ("←", " "),
        ("@", " at "),
        ("e.g.,", "for example, "),
        ("i.e.,", "that is, "),
    ] {
        text = text.replace(from, to);
    }
    for symbol in ["♥", "☆", "♡", "©", "\\"] {
        text = text.replace(symbol, "");
    }

    for (from, to) in [
        (" ,", ","),
        (" .", "."),
        (" !", "!"),
        (" ?", "?"),
        (" ;", ";"),
        (" :", ":"),
        (" '", "'"),
    ] {
        text = text.replace(from, to);
    }
    while text.contains("\"\"") {
        text = text.replace("\"\"", "\"");
    }
    while text.contains("''") {
        text = text.replace("''", "'");
    }
    while text.contains("``") {
        text = text.replace("``", "`");
    }

    text = Regex::new(r"\s+")
        .expect("space regex")
        .replace_all(&text, " ")
        .trim()
        .to_string();
    if !text.is_empty() {
        let ends_with_punctuation =
            Regex::new(r#"[.!?;:,'"\u{201C}\u{201D}\u{2018}\u{2019})\]}…。」』】〉》›»]$"#)
                .expect("punctuation regex");
        if !ends_with_punctuation.is_match(&text) {
            text.push('.');
        }
    }

    Ok(format!("<{language}>{text}</{language}>"))
}

pub(crate) fn text_to_unicode_values(text: &str) -> Vec<usize> {
    text.chars().map(|character| character as usize).collect()
}

pub(crate) fn length_to_mask(lengths: &[usize], max_len: usize) -> Array3<f32> {
    let mut mask = Array3::<f32>::zeros((lengths.len(), 1, max_len));
    for (batch, length) in lengths.iter().enumerate() {
        for index in 0..(*length).min(max_len) {
            mask[[batch, 0, index]] = 1.0;
        }
    }
    mask
}

pub(crate) fn get_text_mask(text_ids_lengths: &[usize]) -> Array3<f32> {
    let max_len = *text_ids_lengths.iter().max().unwrap_or(&0);
    length_to_mask(text_ids_lengths, max_len)
}
