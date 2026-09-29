use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

pub fn normalize_text(input: &str) -> String {
    let decomposed: String = input
        .nfkd()
        .filter(|character| !is_combining_mark(*character))
        .collect();

    let mut out = String::with_capacity(decomposed.len());
    let mut pending_space = false;

    for character in decomposed.chars() {
        if character.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            out.extend(character.to_lowercase());
            pending_space = false;
        } else {
            pending_space = true;
        }
    }

    out
}

/// Fraction of `expected` tokens that appear as one contiguous phrase inside
/// `candidate` (longest matching run / expected token count).
///
/// This is stricter than set overlap on purpose: "Clean Code" must not match
/// "The Clean Coder", even though both of its tokens appear somewhere.
pub fn phrase_ratio(expected: &str, candidate: &str) -> f32 {
    let expected_tokens: Vec<&str> = expected.split_whitespace().collect();
    if expected_tokens.is_empty() {
        return 0.0;
    }

    let candidate_tokens: Vec<&str> = candidate.split_whitespace().collect();
    if candidate_tokens.is_empty() {
        return 0.0;
    }

    let mut lengths = vec![0usize; expected_tokens.len() + 1];
    let mut best = 0usize;

    for token in &candidate_tokens {
        for index in (1..=expected_tokens.len()).rev() {
            if *token == expected_tokens[index - 1] {
                lengths[index] = lengths[index - 1] + 1;
                best = best.max(lengths[index]);
            } else {
                lengths[index] = 0;
            }
        }
    }

    best as f32 / expected_tokens.len() as f32
}

pub fn parse_isbn(text: &str) -> Option<String> {
    let mut candidate = String::new();

    for character in text.chars().chain(std::iter::once(' ')) {
        if character.is_ascii_digit() || character == 'X' || character == 'x' || character == '-' {
            candidate.push(character);
        } else {
            if let Some(isbn) = normalize_isbn(&candidate) {
                return Some(isbn);
            }
            candidate.clear();
        }
    }

    None
}

pub fn normalize_isbn(input: &str) -> Option<String> {
    let cleaned: String = input
        .chars()
        .filter(|character| character.is_ascii_digit() || *character == 'X' || *character == 'x')
        .collect::<String>()
        .to_ascii_uppercase();

    match cleaned.len() {
        10 if valid_isbn10(&cleaned) => Some(cleaned),
        13 if valid_isbn13(&cleaned) => Some(cleaned),
        _ => None,
    }
}

pub fn valid_isbn10(isbn: &str) -> bool {
    let bytes = isbn.as_bytes();
    if bytes.len() != 10 {
        return false;
    }

    let mut sum: u32 = 0;
    for (index, byte) in bytes.iter().enumerate() {
        let value = match byte {
            b'0'..=b'9' => u32::from(byte - b'0'),
            b'X' if index == 9 => 10,
            _ => return false,
        };
        sum += (10 - index as u32) * value;
    }

    sum.is_multiple_of(11)
}

pub fn valid_isbn13(isbn: &str) -> bool {
    let bytes = isbn.as_bytes();
    if bytes.len() != 13 {
        return false;
    }

    let mut sum: u32 = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if !byte.is_ascii_digit() {
            return false;
        }
        let value = u32::from(byte - b'0');
        let weight = if index.is_multiple_of(2) { 1 } else { 3 };
        sum += weight * value;
    }

    sum.is_multiple_of(10)
}

pub fn isbn10_to_isbn13(isbn10: &str) -> Option<String> {
    if !valid_isbn10(isbn10) {
        return None;
    }

    let mut digits: Vec<u8> = Vec::with_capacity(13);
    digits.extend_from_slice(b"978");
    for byte in &isbn10.as_bytes()[..9] {
        digits.push(*byte);
    }

    let mut sum: u32 = 0;
    for (index, digit) in digits.iter().enumerate() {
        let value = u32::from(digit - b'0');
        let weight = if index.is_multiple_of(2) { 1 } else { 3 };
        sum += weight * value;
    }
    let check = ((10 - (sum % 10)) % 10) as u8;
    digits.push(b'0' + check);

    String::from_utf8(digits).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_titles_conservatively() {
        assert_eq!(normalize_text("Project Hail Mary"), "project hail mary");
        assert_eq!(
            normalize_text("The Hitchhiker's Guide to the Galaxy"),
            "the hitchhiker s guide to the galaxy"
        );
        assert_eq!(
            normalize_text("Brontë—Wuthering  Heights"),
            "bronte wuthering heights"
        );
        assert_eq!(normalize_text("  "), "");
    }

    #[test]
    fn validates_isbn_checksums() {
        assert!(valid_isbn10("0593135202"));
        assert!(!valid_isbn10("0593135201"));
        assert!(valid_isbn13("9780593135204"));
        assert!(!valid_isbn13("9780593135205"));
    }

    #[test]
    fn parses_isbn_from_identifiers() {
        assert_eq!(
            parse_isbn("urn:isbn:9780593135204"),
            Some("9780593135204".to_string())
        );
        assert_eq!(
            parse_isbn("ISBN 978-0-59-313520-4"),
            Some("9780593135204".to_string())
        );
        assert_eq!(parse_isbn("not an identifier"), None);
    }

    #[test]
    fn converts_isbn10_to_isbn13() {
        assert_eq!(
            isbn10_to_isbn13("0593135202"),
            Some("9780593135204".to_string())
        );
    }

    #[test]
    fn phrase_ratio_requires_contiguous_tokens() {
        assert_eq!(
            phrase_ratio(
                "clean code",
                "robert c martin the clean coder a code of conduct"
            ),
            0.5
        );
        assert_eq!(phrase_ratio("clean code", "the clean coder"), 0.5);
        assert_eq!(phrase_ratio("clean code", "clean code"), 1.0);
        assert_eq!(
            phrase_ratio(
                "project hail mary",
                "andy weir project hail mary 2021 retail epub"
            ),
            1.0
        );
        assert_eq!(
            phrase_ratio("project hail mary", "hail mary project"),
            2.0 / 3.0
        );
        assert_eq!(phrase_ratio("", "anything"), 0.0);
        assert_eq!(phrase_ratio("dune", "dune messiah"), 1.0);
    }
}

/// "Last, First" provider forms canonicalised to "First Last" so an author
/// is one entity regardless of how the catalogue spells them.
pub fn canonical_author_name(name: &str) -> String {
    let trimmed = name.trim();
    if let Some((last, first)) = trimmed.split_once(',') {
        let last = last.trim();
        let first = first.trim();
        if !last.is_empty() && !first.is_empty() && !first.contains(',') {
            return format!("{first} {last}");
        }
    }
    trimmed.to_string()
}

pub fn core_title(title: &str) -> String {
    let mut value = title.to_string();

    for (open, close) in [('(', ')'), ('[', ']')] {
        while let (Some(start), Some(end)) = (value.find(open), value.find(close)) {
            if end > start {
                value.replace_range(start..=end, " ");
            } else {
                break;
            }
        }
    }

    if let Some(index) = value.find(':') {
        value.truncate(index);
    }
    if let Some(index) = value.find(" - ") {
        value.truncate(index);
    }

    let cleaned = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        title.trim().to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod core_title_tests {
    use super::core_title;

    #[test]
    fn strips_subtitles_and_parentheticals() {
        assert_eq!(
            core_title("Strange Dogs: An Expanse Novella (The Expanse)"),
            "Strange Dogs"
        );
        assert_eq!(core_title("Project Hail Mary"), "Project Hail Mary");
        assert_eq!(core_title("The Hobbit [Illustrated Edition]"), "The Hobbit");
        assert_eq!(core_title("Dune - Deluxe Edition"), "Dune");
        assert_eq!(core_title("  "), "");
    }
}
