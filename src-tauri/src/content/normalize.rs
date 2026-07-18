use std::sync::OnceLock;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use regex::{Captures, Regex};

static URL_RE: OnceLock<Regex> = OnceLock::new();
static EMAIL_RE: OnceLock<Regex> = OnceLock::new();
static MATHML_TOKEN_RE: OnceLock<Regex> = OnceLock::new();
static MATHML_RE: OnceLock<Regex> = OnceLock::new();
static LATEX_MATH_RE: OnceLock<Regex> = OnceLock::new();
static LATEX_FRAC_RE: OnceLock<Regex> = OnceLock::new();
static LATEX_SQRT_RE: OnceLock<Regex> = OnceLock::new();
static LATEX_LABEL_RE: OnceLock<Regex> = OnceLock::new();
static LATEX_COMMAND_RE: OnceLock<Regex> = OnceLock::new();
static LATEX_BRACED_SUBSCRIPT_RE: OnceLock<Regex> = OnceLock::new();
static LATEX_BRACED_SUPERSCRIPT_RE: OnceLock<Regex> = OnceLock::new();
static LATEX_SUBSCRIPT_RE: OnceLock<Regex> = OnceLock::new();
static LATEX_SUPERSCRIPT_RE: OnceLock<Regex> = OnceLock::new();
static TAG_RE: OnceLock<Regex> = OnceLock::new();
static CURRENCY_RE: OnceLock<Regex> = OnceLock::new();
static PERCENT_RE: OnceLock<Regex> = OnceLock::new();
static DECIMAL_RE: OnceLock<Regex> = OnceLock::new();
static INTEGER_RE: OnceLock<Regex> = OnceLock::new();

/// Returns text expanded for TTS. Character offsets are intentionally not preserved.
pub fn normalize_for_tts(text: &str) -> String {
    let mut normalized = text.to_string();

    normalized = url_re().replace_all(&normalized, "link ").into_owned();
    normalized = email_re()
        .replace_all(&normalized, "email address ")
        .into_owned();
    normalized = replace_common_abbreviations(&normalized);
    normalized = mathml_token_re()
        .replace_all(&normalized, |captures: &Captures<'_>| {
            decode_mathml_token(&captures[1])
                .map(|markup| mathml_markup_to_speech(&markup))
                .unwrap_or_else(|| "equation".to_string())
        })
        .into_owned();
    normalized = mathml_re()
        .replace_all(&normalized, |captures: &Captures<'_>| {
            mathml_markup_to_speech(&captures[0])
        })
        .into_owned();
    normalized = latex_math_re()
        .replace_all(&normalized, |captures: &Captures<'_>| {
            captures
                .iter()
                .skip(1)
                .flatten()
                .next()
                .map(|value| with_math_pauses(&latex_to_speech(value.as_str())))
                .unwrap_or_else(|| "equation".to_string())
        })
        .into_owned();
    normalized = currency_re()
        .replace_all(&normalized, |captures: &Captures<'_>| {
            currency_to_words(&captures[1], captures.get(2).map(|value| value.as_str()))
        })
        .into_owned();
    normalized = percent_re()
        .replace_all(&normalized, |captures: &Captures<'_>| {
            format!("{} percent", numeric_phrase(&captures[1]))
        })
        .into_owned();
    normalized = decimal_re()
        .replace_all(&normalized, |captures: &Captures<'_>| {
            decimal_to_words(&captures[1], &captures[2])
        })
        .into_owned();
    normalized = integer_re()
        .replace_all(&normalized, |captures: &Captures<'_>| {
            integer_token_to_words(&captures[0])
        })
        .into_owned();

    cleanup_speech_punctuation(&collapse_repeated_punctuation(&normalized))
}

fn decode_mathml_token(value: &str) -> Option<String> {
    BASE64_STANDARD
        .decode(value.as_bytes())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

fn mathml_markup_to_speech(markup: &str) -> String {
    let text = tag_re().replace_all(markup, " ");
    with_math_pauses(&plain_math_to_speech(&decode_basic_entities(&text)))
}

fn latex_to_speech(source: &str) -> String {
    let mut text = latex_label_re().replace_all(source, " ").into_owned();

    loop {
        let replaced = latex_frac_re()
            .replace_all(&text, |captures: &Captures<'_>| {
                format!(
                    " {}, over, {} ",
                    latex_to_speech(&captures[1]),
                    latex_to_speech(&captures[2])
                )
            })
            .into_owned();
        if replaced == text {
            break;
        }
        text = replaced;
    }

    loop {
        let replaced = latex_sqrt_re()
            .replace_all(&text, |captures: &Captures<'_>| {
                format!(" square root of, {} ", latex_to_speech(&captures[1]))
            })
            .into_owned();
        if replaced == text {
            break;
        }
        text = replaced;
    }

    text = latex_braced_subscript_re()
        .replace_all(&text, |captures: &Captures<'_>| {
            format!(
                "{} sub {},",
                &captures[1],
                plain_math_to_speech(&captures[2])
            )
        })
        .into_owned();
    text = latex_braced_superscript_re()
        .replace_all(&text, |captures: &Captures<'_>| {
            format!(
                "{} {},",
                &captures[1],
                exponent_to_speech(&latex_to_speech(&captures[2]))
            )
        })
        .into_owned();
    text = latex_subscript_re()
        .replace_all(&text, |captures: &Captures<'_>| {
            format!(
                "{} sub {},",
                &captures[1],
                plain_math_to_speech(&captures[2])
            )
        })
        .into_owned();
    text = latex_superscript_re()
        .replace_all(&text, |captures: &Captures<'_>| {
            format!(
                "{} {},",
                &captures[1],
                exponent_to_speech(&latex_to_speech(&captures[2]))
            )
        })
        .into_owned();
    text = latex_command_re()
        .replace_all(&text, |captures: &Captures<'_>| {
            latex_command_to_speech(&captures[1]).to_string()
        })
        .into_owned();

    plain_math_to_speech(&text)
}

fn plain_math_to_speech(text: &str) -> String {
    let mut words = Vec::new();
    let mut token = String::new();

    for character in text.chars() {
        if character.is_alphanumeric() {
            token.push(character);
            continue;
        }

        push_math_token(&mut words, &mut token);
        match character {
            '=' => words.push(", equals,".to_string()),
            '+' => words.push(", plus,".to_string()),
            '-' => words.push("minus".to_string()),
            '*' => words.push(", times,".to_string()),
            '/' => words.push("over".to_string()),
            '<' => words.push(", less than,".to_string()),
            '>' => words.push(", greater than,".to_string()),
            '\'' => words.push("prime,".to_string()),
            ',' => words.push(",".to_string()),
            '.' => words.push("point".to_string()),
            '(' => words.push("open parenthesis".to_string()),
            ')' => words.push("close parenthesis".to_string()),
            '[' => words.push("open bracket".to_string()),
            ']' => words.push("close bracket".to_string()),
            _ if character.is_whitespace() => {}
            _ => words.push(character.to_string()),
        }
    }

    push_math_token(&mut words, &mut token);
    words.join(" ")
}

fn push_math_token(words: &mut Vec<String>, token: &mut String) {
    if token.is_empty() {
        return;
    }
    let value = std::mem::take(token);
    if value.len() > 1
        && value
            .chars()
            .all(|character| character.is_ascii_uppercase())
    {
        words.push(
            value
                .chars()
                .map(|character| character.to_string())
                .collect::<Vec<_>>()
                .join(" "),
        );
    } else {
        words.push(value);
    }
}

fn with_math_pauses(text: &str) -> String {
    let text = normalize_speech_punctuation(text);
    if text.is_empty() {
        String::new()
    } else {
        format!(", {text},")
    }
}

fn normalize_speech_punctuation(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" ,", ",")
        .replace(",,", ",")
        .trim_start_matches(',')
        .trim()
        .to_string()
}

fn exponent_to_speech(value: &str) -> String {
    let value = value.trim();
    match value {
        "2" | "two" => "squared".to_string(),
        "3" | "three" => "cubed".to_string(),
        _ if value.starts_with("minus ") => {
            format!(
                "to the negative {} power",
                value.trim_start_matches("minus ")
            )
        }
        _ => format!("to the {value} power"),
    }
}

fn latex_command_to_speech(command: &str) -> &str {
    match command {
        "alpha" => "alpha",
        "beta" => "beta",
        "gamma" => "gamma",
        "delta" => "delta",
        "epsilon" => "epsilon",
        "theta" => "theta",
        "lambda" => "lambda",
        "mu" => "mu",
        "pi" => "pi",
        "rho" => "rho",
        "sigma" => "sigma",
        "phi" => "phi",
        "omega" => "omega",
        "Delta" => "capital delta",
        "Gamma" => "capital gamma",
        "Theta" => "capital theta",
        "Omega" => "capital omega",
        "cdot" | "times" => ", times,",
        "div" => "divided by",
        "ge" | "geq" => ", greater than or equal to,",
        "le" | "leq" => ", less than or equal to,",
        "infty" => "infinity",
        "neq" => ", not equal to,",
        "pm" => ", plus or minus,",
        "sin" => "sine",
        "cos" => "cosine",
        "tan" => "tangent",
        "ln" => "natural log",
        "log" => "log",
        "left" | "right" => "",
        other => other,
    }
}

fn decode_basic_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn replace_common_abbreviations(text: &str) -> String {
    let replacements = [
        (r"(?i)\be\.g\.", "for example"),
        (r"(?i)\bi\.e\.", "that is"),
        (r"(?i)\bdr\.", "Doctor"),
        (r"(?i)\bmr\.", "Mister"),
        (r"(?i)\bmrs\.", "Misses"),
        (r"(?i)\bms\.", "Miss"),
        (r"(?i)\bprof\.", "Professor"),
        (r"(?i)\bvs\.", "versus"),
        (r"(?i)\betc\.", "et cetera"),
    ];

    replacements
        .iter()
        .fold(text.to_string(), |current, (pattern, replacement)| {
            Regex::new(pattern)
                .expect("valid abbreviation regex")
                .replace_all(&current, *replacement)
                .into_owned()
        })
}

fn currency_to_words(dollars: &str, cents: Option<&str>) -> String {
    let dollar_value = dollars.replace(',', "").parse::<u64>().unwrap_or(0);
    let dollar_unit = if dollar_value == 1 {
        "dollar"
    } else {
        "dollars"
    };
    let mut phrase = format!("{} {dollar_unit}", integer_to_words(dollar_value));

    if let Some(cents) = cents {
        let cent_value = match cents.len() {
            0 => 0,
            1 => cents.parse::<u64>().unwrap_or(0) * 10,
            _ => cents[..2].parse::<u64>().unwrap_or(0),
        };

        if cent_value > 0 {
            let cent_unit = if cent_value == 1 { "cent" } else { "cents" };
            phrase.push_str(&format!(
                " and {} {cent_unit}",
                integer_to_words(cent_value)
            ));
        }
    }

    phrase
}

fn numeric_phrase(value: &str) -> String {
    if let Some((whole, fractional)) = value.split_once('.') {
        decimal_to_words(whole, fractional)
    } else {
        integer_token_to_words(value)
    }
}

fn integer_token_to_words(value: &str) -> String {
    // Strip digit-group separators ("1,200" -> 1200) and read as a plain
    // cardinal. We intentionally do not guess years here: without explicit date
    // context, "1024" must read as a number, not "ten twenty-four".
    let digits = value.replace(',', "");
    integer_to_words(digits.parse::<u64>().unwrap_or(0))
}

fn decimal_to_words(whole: &str, fractional: &str) -> String {
    let whole = whole.parse::<u64>().unwrap_or(0);
    let digits = fractional
        .chars()
        .filter_map(|digit| digit.to_digit(10))
        .map(|digit| digit_word(digit as u8))
        .collect::<Vec<_>>()
        .join(" ");

    format!("{} point {digits}", integer_to_words(whole))
}

fn integer_to_words(number: u64) -> String {
    match number {
        0..=19 => small_number_word(number).to_string(),
        20..=99 => {
            let tens = number / 10;
            let ones = number % 10;
            if ones == 0 {
                tens_word(tens).to_string()
            } else {
                format!("{}-{}", tens_word(tens), small_number_word(ones))
            }
        }
        100..=999 => {
            let hundreds = number / 100;
            let remainder = number % 100;
            if remainder == 0 {
                format!("{} hundred", small_number_word(hundreds))
            } else {
                format!(
                    "{} hundred {}",
                    small_number_word(hundreds),
                    integer_to_words(remainder)
                )
            }
        }
        1_000..=999_999 => join_scale(number, 1_000, "thousand"),
        1_000_000..=999_999_999 => join_scale(number, 1_000_000, "million"),
        1_000_000_000..=999_999_999_999 => join_scale(number, 1_000_000_000, "billion"),
        _ => number.to_string(),
    }
}

fn join_scale(number: u64, scale: u64, scale_name: &str) -> String {
    let high = number / scale;
    let remainder = number % scale;
    if remainder == 0 {
        format!("{} {scale_name}", integer_to_words(high))
    } else {
        format!(
            "{} {scale_name} {}",
            integer_to_words(high),
            integer_to_words(remainder)
        )
    }
}

fn small_number_word(number: u64) -> &'static str {
    match number {
        0 => "zero",
        1 => "one",
        2 => "two",
        3 => "three",
        4 => "four",
        5 => "five",
        6 => "six",
        7 => "seven",
        8 => "eight",
        9 => "nine",
        10 => "ten",
        11 => "eleven",
        12 => "twelve",
        13 => "thirteen",
        14 => "fourteen",
        15 => "fifteen",
        16 => "sixteen",
        17 => "seventeen",
        18 => "eighteen",
        19 => "nineteen",
        _ => unreachable!("small number out of range"),
    }
}

fn tens_word(number: u64) -> &'static str {
    match number {
        2 => "twenty",
        3 => "thirty",
        4 => "forty",
        5 => "fifty",
        6 => "sixty",
        7 => "seventy",
        8 => "eighty",
        9 => "ninety",
        _ => unreachable!("tens number out of range"),
    }
}

fn digit_word(number: u8) -> &'static str {
    match number {
        0 => "zero",
        1 => "one",
        2 => "two",
        3 => "three",
        4 => "four",
        5 => "five",
        6 => "six",
        7 => "seven",
        8 => "eight",
        9 => "nine",
        _ => unreachable!("digit out of range"),
    }
}

fn collapse_repeated_punctuation(text: &str) -> String {
    let mut collapsed = String::with_capacity(text.len());
    let mut previous = None;

    for character in text.chars() {
        if matches!(character, '.' | '!' | '?') && previous == Some(character) {
            continue;
        }

        collapsed.push(character);
        previous = Some(character);
    }

    collapsed
}

fn cleanup_speech_punctuation(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" ,", ",")
        .replace(",,", ",")
        .replace(",.", ".")
        .replace(",!", "!")
        .replace(",?", "?")
        .trim_start_matches(',')
        .trim()
        .to_string()
}

fn url_re() -> &'static Regex {
    URL_RE.get_or_init(|| Regex::new(r"https?://\S+").expect("valid URL regex"))
}

fn email_re() -> &'static Regex {
    EMAIL_RE.get_or_init(|| {
        Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b")
            .expect("valid email regex")
    })
}

fn mathml_token_re() -> &'static Regex {
    MATHML_TOKEN_RE.get_or_init(|| {
        Regex::new(r"\[\[mathml:([A-Za-z0-9+/=]+)\]\]").expect("valid MathML token regex")
    })
}

fn mathml_re() -> &'static Regex {
    MATHML_RE
        .get_or_init(|| Regex::new(r"(?is)<math\b[^>]*>.*?</math>").expect("valid MathML regex"))
}

fn latex_math_re() -> &'static Regex {
    LATEX_MATH_RE.get_or_init(|| {
        Regex::new(r"(?s)\\\[(.*?)\\\]|\\\((.*?)\\\)|\$\$(.*?)\$\$|\$([^$\n]+)\$")
            .expect("valid LaTeX math regex")
    })
}

fn latex_frac_re() -> &'static Regex {
    LATEX_FRAC_RE.get_or_init(|| {
        Regex::new(r"\\frac\s*\{([^{}]+)\}\s*\{([^{}]+)\}").expect("valid frac regex")
    })
}

fn latex_sqrt_re() -> &'static Regex {
    LATEX_SQRT_RE.get_or_init(|| Regex::new(r"\\sqrt\s*\{([^{}]+)\}").expect("valid sqrt regex"))
}

fn latex_label_re() -> &'static Regex {
    LATEX_LABEL_RE.get_or_init(|| Regex::new(r"\\label\s*\{[^{}]*\}").expect("valid label regex"))
}

fn latex_command_re() -> &'static Regex {
    LATEX_COMMAND_RE.get_or_init(|| Regex::new(r"\\([A-Za-z]+)").expect("valid command regex"))
}

fn latex_braced_subscript_re() -> &'static Regex {
    LATEX_BRACED_SUBSCRIPT_RE.get_or_init(|| {
        Regex::new(r"([A-Za-z0-9])_\s*\{([^{}]+)\}").expect("valid braced subscript regex")
    })
}

fn latex_braced_superscript_re() -> &'static Regex {
    LATEX_BRACED_SUPERSCRIPT_RE.get_or_init(|| {
        Regex::new(r"([A-Za-z0-9])\^\s*\{([^{}]+)\}").expect("valid braced superscript regex")
    })
}

fn latex_subscript_re() -> &'static Regex {
    LATEX_SUBSCRIPT_RE
        .get_or_init(|| Regex::new(r"([A-Za-z0-9])_\s*([^{}\s]+)").expect("valid subscript regex"))
}

fn latex_superscript_re() -> &'static Regex {
    LATEX_SUPERSCRIPT_RE.get_or_init(|| {
        Regex::new(r"([A-Za-z0-9])\^\s*([^{}\s]+)").expect("valid superscript regex")
    })
}

fn tag_re() -> &'static Regex {
    TAG_RE.get_or_init(|| Regex::new(r"(?is)<[^>]+>").expect("valid tag regex"))
}

fn currency_re() -> &'static Regex {
    CURRENCY_RE.get_or_init(|| {
        Regex::new(r"\$(\d{1,3}(?:,\d{3})+|\d+)(?:\.(\d{1,2}))?").expect("valid currency regex")
    })
}

fn percent_re() -> &'static Regex {
    PERCENT_RE.get_or_init(|| Regex::new(r"\b(\d+(?:\.\d+)?)%").expect("valid percent regex"))
}

fn decimal_re() -> &'static Regex {
    DECIMAL_RE.get_or_init(|| Regex::new(r"\b(\d+)\.(\d+)\b").expect("valid decimal regex"))
}

fn integer_re() -> &'static Regex {
    INTEGER_RE
        .get_or_init(|| Regex::new(r"\b\d{1,3}(?:,\d{3})+\b|\b\d+\b").expect("valid integer regex"))
}

#[cfg(test)]
mod tests {
    use super::normalize_for_tts;

    #[test]
    fn normalizes_latex_math_for_tts() {
        let text = r"\[R_{ads} = A e^{-E_a / RT} P^ x\]";

        let normalized = normalize_for_tts(text);

        assert!(
            normalized.contains("R sub ads, equals, A e"),
            "{normalized}"
        );
        assert!(
            normalized.contains("to the negative E sub a, over R T power"),
            "{normalized}"
        );
        assert!(normalized.contains("P to the x power"), "{normalized}");
        assert!(!normalized.contains("\\["), "{normalized}");
    }

    #[test]
    fn normalizes_openstax_mathml_tokens_for_tts() {
        let text = "Given [[mathml:PG1hdGg+PG1yb3c+PG1pPng8L21pPjxtbz49PC9tbz48bW4+MjwvbW4+PC9tcm93PjwvbWF0aD4=]].";

        let normalized = normalize_for_tts(text);

        assert!(normalized.contains("Given, x, equals, two"), "{normalized}");
        assert!(!normalized.contains("[[mathml:"), "{normalized}");
    }

    #[test]
    fn reads_four_digit_numbers_as_cardinals_not_years() {
        let normalized = normalize_for_tts("A buffer holds 1024 bytes.");
        assert!(
            normalized.contains("one thousand twenty-four"),
            "{normalized}"
        );
        assert!(!normalized.contains("ten twenty"), "{normalized}");
    }

    #[test]
    fn reads_comma_grouped_numbers_as_one_value() {
        let normalized = normalize_for_tts("It cost $1,200 for 1,200 items.");
        assert!(
            normalized.contains("one thousand two hundred dollars"),
            "{normalized}"
        );
        // The bare grouped number must not split into "one" and "two hundred".
        assert!(
            normalized.matches("one thousand two hundred").count() >= 2,
            "{normalized}"
        );
    }
}
