use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

static ABBREVIATIONS: OnceLock<HashSet<&'static str>> = OnceLock::new();
static BLANK_LINE_RE: OnceLock<Regex> = OnceLock::new();

/// Returns byte offsets `[start, end)` for each sentence in `text`.
pub fn sentence_boundaries(text: &str) -> Vec<(usize, usize)> {
    let mut boundaries = Vec::new();

    for (paragraph_start, paragraph_end) in paragraph_ranges(text) {
        tokenize_paragraph(text, paragraph_start, paragraph_end, &mut boundaries);
    }

    boundaries
}

fn tokenize_paragraph(
    text: &str,
    paragraph_start: usize,
    paragraph_end: usize,
    boundaries: &mut Vec<(usize, usize)>,
) {
    let chars: Vec<(usize, char)> = text[paragraph_start..paragraph_end]
        .char_indices()
        .map(|(offset, character)| (paragraph_start + offset, character))
        .collect();

    if chars.is_empty() {
        return;
    }

    let mut sentence_start =
        first_non_whitespace(text, paragraph_start, paragraph_end).unwrap_or(paragraph_start);
    let mut i = 0;

    while i < chars.len() {
        let (byte_index, character) = chars[i];
        if !is_terminator(character) {
            i += 1;
            continue;
        }

        let run_end_index = terminator_run_end(&chars, i);
        let run_end_byte = char_end_byte(&chars, run_end_index - 1);

        if is_sentence_boundary(
            text,
            &chars,
            i,
            run_end_index,
            paragraph_start,
            paragraph_end,
        ) {
            let boundary_end = closing_punctuation_end(text, run_end_byte, paragraph_end);
            if sentence_start < boundary_end {
                boundaries.push((sentence_start, boundary_end));
            }

            sentence_start =
                first_non_whitespace(text, boundary_end, paragraph_end).unwrap_or(paragraph_end);
        }

        if run_end_index <= i {
            i += 1;
        } else {
            i = run_end_index;
        }

        if byte_index >= paragraph_end {
            break;
        }
    }

    let final_end = trim_end(text, sentence_start, paragraph_end);
    if sentence_start < final_end {
        boundaries.push((sentence_start, final_end));
    }
}

fn paragraph_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0;

    for separator in blank_line_re().find_iter(text) {
        push_trimmed_range(text, start, separator.start(), &mut ranges);
        start = separator.end();
    }

    push_trimmed_range(text, start, text.len(), &mut ranges);
    ranges
}

fn push_trimmed_range(text: &str, start: usize, end: usize, ranges: &mut Vec<(usize, usize)>) {
    let Some(trimmed_start) = first_non_whitespace(text, start, end) else {
        return;
    };
    let trimmed_end = trim_end(text, trimmed_start, end);
    if trimmed_start < trimmed_end {
        ranges.push((trimmed_start, trimmed_end));
    }
}

fn is_sentence_boundary(
    text: &str,
    chars: &[(usize, char)],
    run_start_index: usize,
    run_end_index: usize,
    paragraph_start: usize,
    paragraph_end: usize,
) -> bool {
    let (_, first_terminator) = chars[run_start_index];

    if first_terminator == '.' && run_end_index == run_start_index + 1 {
        let period_index = chars[run_start_index].0;

        if is_decimal_period(text, period_index) || is_acronym_period(text, period_index) {
            return false;
        }

        if is_abbreviation_period(text, period_index, paragraph_start)
            && !is_sentence_ending_abbreviation(text, period_index, paragraph_start)
        {
            return false;
        }
    }

    if let Some(next) =
        next_non_space_same_line(text, char_end_byte(chars, run_end_index - 1), paragraph_end)
    {
        return !next.is_lowercase() && !next.is_ascii_digit();
    }

    true
}

fn is_decimal_period(text: &str, period_index: usize) -> bool {
    previous_char(text, period_index).is_some_and(|character| character.is_ascii_digit())
        && next_char(text, period_index + 1).is_some_and(|character| character.is_ascii_digit())
}

fn is_abbreviation_period(text: &str, period_index: usize, paragraph_start: usize) -> bool {
    let token = token_before_period(text, period_index, paragraph_start);
    if token.is_empty() {
        return false;
    }

    let normalized = token.trim_matches('.').to_ascii_lowercase();
    abbreviations().contains(normalized.as_str())
}

/// Abbreviations that commonly end a sentence (unlike titles such as "Dr." that
/// always precede a word). For these, the boundary is decided by the lookahead
/// rather than always suppressed, so "..., etc. The next" splits correctly while
/// "..., etc. and more" does not.
///
/// Note: "al" (as in "et al.") is intentionally excluded — it is frequently
/// followed by a capitalized citation like "(2020)", which the lookahead would
/// misread as a new sentence.
fn is_sentence_ending_abbreviation(
    text: &str,
    period_index: usize,
    paragraph_start: usize,
) -> bool {
    let token = token_before_period(text, period_index, paragraph_start);
    let normalized = token.trim_matches('.').to_ascii_lowercase();
    matches!(normalized.as_str(), "etc")
}

fn is_acronym_period(text: &str, period_index: usize) -> bool {
    let (start, end) = dotted_token_range(text, period_index);
    let token = &text[start..end];
    let mut letters = 0;
    let mut periods = 0;

    for character in token.chars() {
        if character == '.' {
            periods += 1;
        } else if character.is_ascii_uppercase() {
            letters += 1;
        } else {
            return false;
        }
    }

    letters >= 2 && periods >= 1
}

fn token_before_period(text: &str, period_index: usize, paragraph_start: usize) -> String {
    let mut token_start = period_index;

    while token_start > paragraph_start {
        let Some((previous_index, character)) = text[..token_start].char_indices().next_back()
        else {
            break;
        };

        if character.is_ascii_alphabetic() || character == '.' {
            token_start = previous_index;
        } else {
            break;
        }
    }

    text[token_start..period_index].to_string()
}

fn dotted_token_range(text: &str, period_index: usize) -> (usize, usize) {
    let mut start = period_index;
    while start > 0 {
        let Some((previous_index, character)) = text[..start].char_indices().next_back() else {
            break;
        };

        if character.is_ascii_alphabetic() || character == '.' {
            start = previous_index;
        } else {
            break;
        }
    }

    let mut end = period_index + 1;
    while end < text.len() {
        let Some(character) = next_char(text, end) else {
            break;
        };

        if character.is_ascii_alphabetic() || character == '.' {
            end += character.len_utf8();
        } else {
            break;
        }
    }

    (start, end)
}

fn terminator_run_end(chars: &[(usize, char)], run_start_index: usize) -> usize {
    let mut index = run_start_index;
    while index < chars.len() && is_terminator(chars[index].1) {
        index += 1;
    }
    index
}

fn closing_punctuation_end(text: &str, start: usize, paragraph_end: usize) -> usize {
    let mut end = start;
    while end < paragraph_end {
        let Some(character) = next_char(text, end) else {
            break;
        };

        if matches!(character, '"' | '\'' | ')' | ']' | '}' | '”' | '’') {
            end += character.len_utf8();
        } else {
            break;
        }
    }

    end
}

fn next_non_space_same_line(text: &str, start: usize, end: usize) -> Option<char> {
    let mut index = start;
    while index < end {
        let character = next_char(text, index)?;
        if character == '\n' {
            return None;
        }
        if !matches!(character, ' ' | '\t' | '\r') {
            return Some(character);
        }
        index += character.len_utf8();
    }

    None
}

fn first_non_whitespace(text: &str, start: usize, end: usize) -> Option<usize> {
    text[start..end]
        .char_indices()
        .find(|(_, character)| !character.is_whitespace())
        .map(|(offset, _)| start + offset)
}

fn trim_end(text: &str, start: usize, end: usize) -> usize {
    let mut trimmed_end = end;
    while trimmed_end > start {
        let Some((previous_index, character)) = text[..trimmed_end].char_indices().next_back()
        else {
            break;
        };

        if character.is_whitespace() {
            trimmed_end = previous_index;
        } else {
            break;
        }
    }

    trimmed_end
}

fn previous_char(text: &str, index: usize) -> Option<char> {
    text[..index].chars().next_back()
}

fn next_char(text: &str, index: usize) -> Option<char> {
    text[index..].chars().next()
}

fn char_end_byte(chars: &[(usize, char)], char_index: usize) -> usize {
    let (byte_index, character) = chars[char_index];
    byte_index + character.len_utf8()
}

fn is_terminator(character: char) -> bool {
    matches!(character, '.' | '!' | '?')
}

fn blank_line_re() -> &'static Regex {
    BLANK_LINE_RE.get_or_init(|| Regex::new(r"\n[ \t\r]*\n+").expect("valid blank-line regex"))
}

fn abbreviations() -> &'static HashSet<&'static str> {
    ABBREVIATIONS.get_or_init(|| {
        [
            "mr", "mrs", "ms", "mx", "dr", "prof", "sr", "jr", "st", "vs", "etc", "e.g", "i.e",
            "inc", "ltd", "corp", "co", "dept", "univ", "assn", "fig", "eq", "ch", "sec", "no",
            "vol", "ed", "rev", "est", "misc", "approx", "appt", "apt", "ave", "blvd", "rd", "jan",
            "feb", "mar", "apr", "jun", "jul", "aug", "sep", "sept", "oct", "nov", "dec", "mon",
            "tue", "tues", "wed", "thu", "thur", "thurs", "fri", "sat", "sun", "al", "ala", "ariz",
            "ark", "calif", "colo", "conn", "del", "fla", "ga", "ill", "ind", "kan", "kans", "ky",
            "la", "mass", "md", "mich", "minn", "miss", "mo", "mont", "neb", "nebr", "nev", "okla",
            "ore", "pa", "tenn", "tex", "va", "vt", "wash", "wis", "wisc", "wyo", "u.s", "u.s.a",
            "u.k", "p.m", "a.m",
        ]
        .into_iter()
        .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::sentence_boundaries;

    fn sentences(text: &str) -> Vec<String> {
        sentence_boundaries(text)
            .into_iter()
            .map(|(start, end)| text[start..end].to_string())
            .collect()
    }

    #[test]
    fn clause_ending_abbreviation_can_end_a_sentence() {
        let result = sentences("Add eggs, milk, etc. The cake is ready.");
        assert_eq!(result.len(), 2, "{result:?}");
        assert_eq!(result[0], "Add eggs, milk, etc.");
        assert_eq!(result[1], "The cake is ready.");
    }

    #[test]
    fn clause_ending_abbreviation_mid_sentence_does_not_split() {
        let result = sentences("Bring eggs, milk, etc. and sugar too.");
        assert_eq!(result.len(), 1, "{result:?}");
    }

    #[test]
    fn title_abbreviation_does_not_split_before_a_name() {
        let result = sentences("Dr. Smith arrived early.");
        assert_eq!(result.len(), 1, "{result:?}");
    }

    #[test]
    fn et_al_does_not_split_before_a_citation() {
        let result = sentences("Smith et al. (2020) found a strong effect.");
        assert_eq!(result.len(), 1, "{result:?}");
    }
}
