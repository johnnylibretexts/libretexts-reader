//! Hiding math tokens from a translator that would shred them.
//!
//! Marian has no passthrough for opaque spans, so the sentinel is a bet that
//! SentencePiece keeps it intact and the decoder emits each one exactly once.
//! `restore_math` cannot prevent that bet losing -- it detects it, which is
//! enough, because the caller always has the untranslated sentence to fall
//! back to.

use std::collections::HashMap;

use regex::Regex;

pub(crate) struct Masked {
    pub text: String,
    pub tokens: Vec<String>,
}

fn token_pattern() -> Regex {
    Regex::new(r"\[\[(?:mathml|latex):[A-Za-z0-9+/=]+\]\]").expect("math token regex")
}

fn sentinel_pattern() -> Regex {
    Regex::new(r"⟦(\d+)⟧").expect("sentinel regex")
}

pub(crate) fn mask_math(text: &str) -> Masked {
    let mut tokens = Vec::new();
    let masked = token_pattern().replace_all(text, |caps: &regex::Captures| {
        tokens.push(caps[0].to_string());
        format!("⟦{}⟧", tokens.len() - 1)
    });
    Masked {
        text: masked.into_owned(),
        tokens,
    }
}

pub(crate) fn restore_math(translated: &str, tokens: &[String]) -> Option<String> {
    // The multiset of indices, not merely the count: a duplicated or reordered
    // sentinel is as wrong as a missing one, and counting alone misses both.
    let mut seen: HashMap<usize, usize> = HashMap::new();
    for caps in sentinel_pattern().captures_iter(translated) {
        let index: usize = caps[1].parse().ok()?;
        *seen.entry(index).or_insert(0) += 1;
    }
    if seen.len() != tokens.len() || seen.values().any(|count| *count != 1) {
        return None;
    }

    let mut failed = false;
    let restored =
        sentinel_pattern().replace_all(translated, |caps: &regex::Captures| {
            match caps[1]
                .parse::<usize>()
                .ok()
                .and_then(|index| tokens.get(index))
            {
                Some(token) => token.clone(),
                None => {
                    failed = true;
                    String::new()
                }
            }
        });
    if failed {
        None
    } else {
        Some(restored.into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_each_token_with_its_index_and_restores_it() {
        let masked = mask_math("Given [[latex:eA==]] and [[mathml:eB==]], solve.");
        assert_eq!(masked.text, "Given ⟦0⟧ and ⟦1⟧, solve.");
        assert_eq!(masked.tokens.len(), 2);

        let restored = restore_math("Dado ⟦0⟧ y ⟦1⟧, resuelve.", &masked.tokens).unwrap();
        assert_eq!(restored, "Dado [[latex:eA==]] y [[mathml:eB==]], resuelve.");
    }

    #[test]
    fn refuses_a_translation_that_lost_or_duplicated_a_sentinel() {
        let masked = mask_math("Given [[latex:eA==]] and [[mathml:eB==]], solve.");
        // The decoder dropped one. Restoring would silently delete an equation.
        assert!(restore_math("Dado ⟦0⟧, resuelve.", &masked.tokens).is_none());
        // And emitted one twice. Restoring would duplicate it.
        assert!(restore_math("Dado ⟦0⟧ y ⟦0⟧.", &masked.tokens).is_none());
    }

    #[test]
    fn a_sentence_with_no_math_round_trips_unchanged() {
        let masked = mask_math("Plain sentence.");
        assert_eq!(masked.text, "Plain sentence.");
        assert!(masked.tokens.is_empty());
        assert_eq!(restore_math("Frase simple.", &[]).unwrap(), "Frase simple.");
    }
}
