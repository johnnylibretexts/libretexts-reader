//! Scoring a back-translation against the original.
//!
//! chrF: character n-gram F-score, beta 2 (recall weighted, the standard for
//! chrF). No model, no dependency, no I/O -- which is what keeps the whole
//! quality gate testable in CI without a model download present.

use std::collections::HashMap;

const MAX_NGRAM: usize = 6;
const BETA_SQUARED: f64 = 4.0;
const SAMPLE_FRACTION: f64 = 0.05;
const SAMPLE_FLOOR: usize = 5;
const SAMPLE_CAP: usize = 50;

/// Per-language chrF cutoffs calibrated against deterministic 5% samples from
/// five real textbook chapters. Each value is the midpoint between the lowest
/// healthy chapter p10 and highest deliberately-misaligned chapter p10. Turkish
/// and Vietnamese have overlapping bands; their midpoint is an explicit
/// conservative compromise rather than an invented clean separation.
pub(crate) fn threshold_for(language: &str) -> f64 {
    match language {
        "ko" => 13.08,
        "ja" => 13.92,
        "ar" => 20.85,
        "bg" => 19.32,
        "cs" => 21.25,
        "da" => 29.63,
        "de" => 22.98,
        "el" => 18.39,
        "es" => 23.49,
        "et" => 21.08,
        "fi" => 14.32,
        "fr" => 16.25,
        "hi" => 19.49,
        "hr" => 27.65,
        "hu" => 22.14,
        "id" => 18.01,
        "it" => 24.09,
        "lt" => 11.97,
        "lv" => 21.37,
        "nl" => 20.60,
        "pl" => 18.16,
        "pt" => 15.47,
        "ro" => 22.28,
        "ru" => 23.08,
        "sk" => 17.46,
        "sl" => 20.91,
        "sv" => 25.68,
        "tr" => 13.95,
        "uk" => 18.26,
        "vi" => 11.11,
        // Translation is English-hubbed, so this is only a defensive fallback
        // for corrupt or future settings rather than a releasable pair.
        _ => 20.0,
    }
}

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

pub(crate) fn sample_indices(total: usize) -> Vec<usize> {
    if total == 0 {
        return Vec::new();
    }
    let wanted = ((total as f64 * SAMPLE_FRACTION).ceil() as usize)
        .clamp(SAMPLE_FLOOR, SAMPLE_CAP)
        .min(total);
    let stride = total / wanted;
    (0..wanted).map(|step| step * stride).collect()
}

pub(crate) fn should_escalate(scores: &[f64], threshold: f64) -> bool {
    scores.iter().any(|score| *score < threshold)
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

    #[test]
    fn samples_five_percent_by_stride_with_a_floor_and_a_cap() {
        // Deterministic on purpose: a random 5% makes escalation irreproducible,
        // so the same chapter could pass one run and escalate the next with no
        // test able to pin it. Stride gives identical coverage and a stable
        // bug report.
        assert_eq!(sample_indices(400).len(), 20);
        assert_eq!(sample_indices(400)[0], 0);
        assert_eq!(sample_indices(400)[1], 20);

        // Floor: a short chapter still gets a meaningful check.
        assert_eq!(sample_indices(12).len(), 5);
        // And never asks for more sentences than exist.
        assert_eq!(sample_indices(3).len(), 3);
        assert_eq!(sample_indices(0).len(), 0);

        // Cap: a huge chapter does not pay for 5% of everything.
        assert_eq!(sample_indices(4000).len(), 50);
    }

    #[test]
    fn escalates_only_when_the_sample_looks_bad() {
        assert!(!should_escalate(&[80.0, 75.0, 90.0], 20.0));
        assert!(should_escalate(&[20.0, 15.0, 25.0], 20.0));
        assert!(
            should_escalate(&[100.0, 0.0], 20.0),
            "one catastrophic sampled sentence must escalate the chapter"
        );
        // An empty sample is not evidence of failure.
        assert!(!should_escalate(&[], 20.0));
    }

    #[test]
    fn every_translation_language_has_an_explicit_calibrated_threshold() {
        for language in crate::translate::catalog::TRANSLATION_LANGUAGES {
            assert_ne!(threshold_for(language), 20.0, "missing {language}");
        }
    }
}
