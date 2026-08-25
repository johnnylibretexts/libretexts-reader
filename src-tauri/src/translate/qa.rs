//! Scoring a back-translation against the original.
//!
//! chrF: character n-gram F-score, beta 2 (recall weighted, the standard for
//! chrF). No model, no dependency, no I/O -- which is what keeps the whole
//! quality gate testable in CI without a 155MB download present.

use std::collections::HashMap;

const MAX_NGRAM: usize = 6;
const BETA_SQUARED: f64 = 4.0;

fn ngram_counts(text: &str, n: usize) -> HashMap<String, usize> {
    let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    let mut counts = HashMap::new();
    if chars.len() < n {
        return counts;
    }
    for window in chars.windows(n) {
        *counts.entry(window.iter().collect::<String>()).or_insert(0) += 1;
    }
    counts
}

pub(crate) fn chrf(hypothesis: &str, reference: &str) -> f64 {
    if hypothesis.trim().is_empty() || reference.trim().is_empty() {
        return 0.0;
    }

    let mut precisions = Vec::new();
    let mut recalls = Vec::new();
    for n in 1..=MAX_NGRAM {
        let hypothesis_grams = ngram_counts(hypothesis, n);
        let reference_grams = ngram_counts(reference, n);
        if hypothesis_grams.is_empty() || reference_grams.is_empty() {
            continue;
        }
        let overlap: usize = hypothesis_grams
            .iter()
            .map(|(gram, count)| *count.min(reference_grams.get(gram).unwrap_or(&0)))
            .sum();
        let hypothesis_total: usize = hypothesis_grams.values().sum();
        let reference_total: usize = reference_grams.values().sum();
        precisions.push(overlap as f64 / hypothesis_total as f64);
        recalls.push(overlap as f64 / reference_total as f64);
    }

    if precisions.is_empty() {
        return 0.0;
    }
    let precision = precisions.iter().sum::<f64>() / precisions.len() as f64;
    let recall = recalls.iter().sum::<f64>() / recalls.len() as f64;
    if precision + recall == 0.0 {
        return 0.0;
    }
    100.0 * (1.0 + BETA_SQUARED) * precision * recall / (BETA_SQUARED * precision + recall)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_strings_score_one_hundred() {
        assert!((chrf("the cell divides", "the cell divides") - 100.0).abs() < 1e-6);
    }

    #[test]
    fn unrelated_strings_score_near_zero() {
        assert!(chrf("xyz qqq", "the cell divides") < 10.0);
    }

    #[test]
    fn a_close_back_translation_outscores_a_mangled_one() {
        let reference = "The cell divides into two daughter cells.";
        let close = chrf("The cell divides in two daughter cells.", reference);
        let mangled = chrf("Banana.", reference);
        assert!(close > mangled);
        assert!(close > 70.0, "close paraphrase scored {close}");
    }

    #[test]
    fn empty_input_scores_zero_rather_than_dividing_by_zero() {
        assert_eq!(chrf("", "anything"), 0.0);
        assert_eq!(chrf("anything", ""), 0.0);
        assert_eq!(chrf("", ""), 0.0);
    }
}
