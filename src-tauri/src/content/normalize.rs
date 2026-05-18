use std::sync::OnceLock;

use regex::{Captures, Regex};

static URL_RE: OnceLock<Regex> = OnceLock::new();
static EMAIL_RE: OnceLock<Regex> = OnceLock::new();
static MATHML_RE: OnceLock<Regex> = OnceLock::new();
static LATEX_MATH_RE: OnceLock<Regex> = OnceLock::new();
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
    normalized = currency_re()
        .replace_all(&normalized, |captures: &Captures<'_>| {
            currency_to_words(&captures[1], captures.get(2).map(|value| value.as_str()))
        })
        .into_owned();
    normalized = mathml_re()
        .replace_all(&normalized, " equation ")
        .into_owned();
    normalized = latex_math_re()
        .replace_all(&normalized, " equation ")
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

    collapse_repeated_punctuation(&normalized)
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
    let dollar_value = dollars.parse::<u64>().unwrap_or(0);
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
    let number = value.parse::<u64>().unwrap_or(0);
    if (1000..=2099).contains(&number) {
        year_to_words(number)
    } else {
        integer_to_words(number)
    }
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

fn year_to_words(year: u64) -> String {
    if year == 2000 {
        return "two thousand".to_string();
    }

    if (2001..=2009).contains(&year) {
        return format!("two thousand {}", integer_to_words(year - 2000));
    }

    let first = year / 100;
    let last = year % 100;
    let first_words = integer_to_words(first);

    if last == 0 {
        format!("{first_words} hundred")
    } else if last < 10 {
        format!("{first_words} oh {}", integer_to_words(last))
    } else {
        format!("{first_words} {}", integer_to_words(last))
    }
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

fn url_re() -> &'static Regex {
    URL_RE.get_or_init(|| Regex::new(r"https?://\S+").expect("valid URL regex"))
}

fn email_re() -> &'static Regex {
    EMAIL_RE.get_or_init(|| {
        Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b")
            .expect("valid email regex")
    })
}

fn mathml_re() -> &'static Regex {
    MATHML_RE
        .get_or_init(|| Regex::new(r"(?is)<math\b[^>]*>.*?</math>").expect("valid MathML regex"))
}

fn latex_math_re() -> &'static Regex {
    LATEX_MATH_RE.get_or_init(|| Regex::new(r"\$[^$\n]+\$").expect("valid LaTeX math regex"))
}

fn currency_re() -> &'static Regex {
    CURRENCY_RE
        .get_or_init(|| Regex::new(r"\$(\d+)(?:\.(\d{1,2}))?").expect("valid currency regex"))
}

fn percent_re() -> &'static Regex {
    PERCENT_RE.get_or_init(|| Regex::new(r"\b(\d+(?:\.\d+)?)%").expect("valid percent regex"))
}

fn decimal_re() -> &'static Regex {
    DECIMAL_RE.get_or_init(|| Regex::new(r"\b(\d+)\.(\d+)\b").expect("valid decimal regex"))
}

fn integer_re() -> &'static Regex {
    INTEGER_RE.get_or_init(|| Regex::new(r"\b\d+\b").expect("valid integer regex"))
}
