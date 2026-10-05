use std::collections::HashSet;

use bokhylle_core::identity::{normalize_isbn, normalize_text, parse_isbn, phrase_ratio};

use crate::model::{
    EvaluatedRelease, ExpectedBook, RejectionReason, ReleaseCandidate, ScoreReason, Selection,
};

pub const SUPPORTED_FORMATS: [&str; 3] = ["epub", "pdf", "cbz"];
pub const MAX_RELEASE_BYTES: i64 = 1_000_000_000;

const FORMAT_TOKENS: &[(&str, &str)] = &[
    ("epub", "epub"),
    ("pdf", "pdf"),
    ("mobi", "mobi"),
    ("azw3", "azw3"),
    ("azw", "azw3"),
    ("fb2", "fb2"),
    ("djvu", "djvu"),
    ("cbz", "cbz"),
    ("cbr", "cbr"),
];

const LANGUAGE_NAMES: &[(&str, &str)] = &[
    ("english", "en"),
    ("swedish", "sv"),
    ("german", "de"),
    ("deutsch", "de"),
    ("french", "fr"),
    ("spanish", "es"),
    ("italian", "it"),
    ("portuguese", "pt"),
    ("dutch", "nl"),
    ("danish", "da"),
    ("norwegian", "no"),
    ("finnish", "fi"),
    ("polish", "pl"),
    ("russian", "ru"),
    ("japanese", "ja"),
    ("chinese", "zh"),
];

const LANGUAGE_CODES: &[(&str, &str)] = &[
    ("en", "en"),
    ("eng", "en"),
    ("sv", "sv"),
    ("swe", "sv"),
    ("de", "de"),
    ("ger", "de"),
    ("deu", "de"),
    ("fr", "fr"),
    ("fre", "fr"),
    ("fra", "fr"),
    ("es", "es"),
    ("spa", "es"),
    ("it", "it"),
    ("ita", "it"),
    ("pt", "pt"),
    ("por", "pt"),
    ("nl", "nl"),
    ("dut", "nl"),
    ("nld", "nl"),
    ("da", "da"),
    ("dan", "da"),
    ("no", "no"),
    ("nor", "no"),
    ("fi", "fi"),
    ("fin", "fi"),
    ("pl", "pl"),
    ("pol", "pl"),
    ("ru", "ru"),
    ("rus", "ru"),
    ("ja", "ja"),
    ("jp", "ja"),
    ("jpn", "ja"),
    ("zh", "zh"),
    ("cn", "zh"),
    ("chi", "zh"),
    ("zho", "zh"),
];

const AUDIOBOOK_TOKENS: &[&str] = &["audiobook", "audiobooks", "audible", "m4b", "m4a", "mp3"];
const COLLECTION_TOKENS: &[&str] = &[
    "collection",
    "collections",
    "omnibus",
    "complete",
    "boxed",
    "boxset",
    "bundle",
    "pack",
    "anthology",
    "trilogy",
    "duology",
];
const COMIC_TOKENS: &[&str] = &["comic", "comics", "manga"];
const VOLUME_WORDS: &[&str] = &["book", "vol", "volume", "bkn", "bk", "part"];
const RETAIL_TOKENS: &[&str] = &["retail", "proper", "repack"];

pub fn tokens(name: &str) -> Vec<String> {
    // Use the same punctuation and accent rules as the expected identity.
    // Apostrophes in titles and accents in author names must not depend on
    // which spelling the release uploader used.
    normalize_text(name)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

struct Detection {
    format: Option<String>,
    language: Option<String>,
    volume: Option<String>,
    volumes: Vec<String>,
    retail: bool,
    is_collection: bool,
    is_audiobook: bool,
    is_comic: bool,
}

fn detect(
    name: &str,
    tokens: &[String],
    identity_positions: &HashSet<usize>,
    preferred_format: &str,
) -> Detection {
    let mut formats = Vec::new();
    let mut language = None;
    let tagged = tagged_tokens(name);
    let mut is_collection = has_number_range(name);
    let mut is_audiobook = false;
    let mut is_comic = tokens.windows(2).enumerate().any(|(index, pair)| {
        !identity_positions.contains(&index) && pair[0] == "issue" && pair[1].parse::<u16>().is_ok()
    });
    let mut retail = false;

    for (index, token) in tokens.iter().enumerate() {
        // Collection markers stay conservative even in a requested pack's
        // title: matching identity does not prove it contains a single file.
        if COLLECTION_TOKENS.contains(&token.as_str()) {
            is_collection = true;
        }
        // Identity words are not language, format or media tags. Only their
        // matched occurrence is excluded; a later explicit tag still applies.
        if identity_positions.contains(&index) {
            continue;
        }
        if let Some((_, value)) = FORMAT_TOKENS.iter().find(|(key, _)| key == token)
            && !formats.contains(value)
        {
            formats.push(*value);
        }
        if language.is_none() {
            if let Some((_, value)) = LANGUAGE_NAMES
                .iter()
                .find(|(key, _)| *key == token.as_str())
            {
                language = Some(value.to_string());
            } else if tagged.contains(token.as_str())
                && let Some((_, value)) = LANGUAGE_CODES
                    .iter()
                    .find(|(key, _)| *key == token.as_str())
            {
                language = Some(value.to_string());
            }
        }
        if AUDIOBOOK_TOKENS.contains(&token.as_str()) {
            is_audiobook = true;
        }
        if COMIC_TOKENS.contains(&token.as_str()) {
            is_comic = true;
        }
        if RETAIL_TOKENS.contains(&token.as_str()) {
            retail = true;
        }
    }

    // A release may contain several formats. Choose a usable format even
    // when an unsupported one appears first in the title.
    let format = formats
        .iter()
        .copied()
        .find(|format| *format == preferred_format)
        .or_else(|| {
            formats
                .iter()
                .copied()
                .find(|format| SUPPORTED_FORMATS.contains(format))
        })
        .or_else(|| formats.first().copied());

    let volumes = detect_volumes(name);
    Detection {
        format: format.map(str::to_string),
        language,
        volume: volumes.first().cloned(),
        volumes,
        retail,
        is_collection,
        is_audiobook,
        is_comic,
    }
}

const TAG_SEPARATORS: [char; 6] = ['.', '-', '_', '/', '+', ';'];

// Two-letter language codes are only trusted in explicit release syntax:
// bracketed groups ("[en]", "(en)"), language markers ("LANG-EN"), or
// upper-case dot-separated tags (".EN."). Title words like "It" or "No" in
// "Stephen.King.It.EPUB" or "No.Country.for.Old.Men.EPUB" stay untagged.
fn tagged_tokens(name: &str) -> HashSet<String> {
    let mut tagged = HashSet::new();

    for group in bracketed_groups(name) {
        for fragment in group.split(TAG_SEPARATORS) {
            let token = normalize_fragment(fragment);
            if !token.is_empty() {
                tagged.insert(token);
            }
        }
    }

    for chunk in name.split_whitespace() {
        let fragments: Vec<&str> = chunk.split(TAG_SEPARATORS).collect();
        if fragments.len() < 2 {
            continue;
        }

        for (index, fragment) in fragments.iter().enumerate() {
            let marker = normalize_fragment(fragment);
            if matches!(marker.as_str(), "lang" | "language" | "lng")
                && let Some(next) = fragments.get(index + 1)
            {
                let token = normalize_fragment(next);
                if !token.is_empty() {
                    tagged.insert(token);
                }
            }

            // Identity occurrences are ignored by detect, while a repeated
            // explicit tag ("STEPHEN.KING.IT.IT.EPUB") still applies.
            let trimmed = fragment.trim();
            if (2..=3).contains(&trimmed.len())
                && trimmed
                    .chars()
                    .all(|character| character.is_ascii_alphabetic())
                && trimmed == trimmed.to_ascii_uppercase()
            {
                tagged.insert(trimmed.to_ascii_lowercase());
            }
        }
    }

    tagged
}

fn bracketed_groups(name: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let mut stack: Vec<(char, usize)> = Vec::new();

    for (index, character) in name.char_indices() {
        match character {
            '[' => stack.push((']', index + 1)),
            '(' => stack.push((')', index + 1)),
            '{' => stack.push(('}', index + 1)),
            _ => {
                if let Some((expected, start)) = stack.last().copied()
                    && character == expected
                {
                    stack.pop();
                    groups.push(name[start..index].to_string());
                }
            }
        }
    }

    groups
}

fn normalize_fragment(fragment: &str) -> String {
    fragment
        .trim_matches(|character: char| !character.is_alphanumeric())
        .to_ascii_lowercase()
}

fn has_number_range(name: &str) -> bool {
    name.char_indices().any(|(index, character)| {
        if !matches!(character, '-' | '–' | '—') {
            return false;
        }
        let isbn_character =
            |character: char| character.is_ascii_digit() || matches!(character, '-' | '–' | '—' | 'X' | 'x');
        let before = name[..index]
            .rsplit(|character| !isbn_character(character))
            .next()
            .unwrap_or("");
        let after = name[index..]
            .split(|character| !isbn_character(character))
            .next()
            .unwrap_or("");
        if normalize_isbn(&format!("{before}{after}")).is_some() {
            return false;
        }
        let start = name[..index]
            .trim_end()
            .rsplit(|character: char| !character.is_ascii_digit())
            .next()
            .and_then(|number| number.parse::<u32>().ok());
        let end = name[index + character.len_utf8()..]
            .trim_start()
            .split(|character: char| !character.is_ascii_digit())
            .next()
            .and_then(|number| number.parse::<u32>().ok());
        // Small numbered ranges denote volumes; publication date ranges do
        // not turn a standalone book into a collection.
        matches!((start, end), (Some(start), Some(end)) if start != end && start < 1000 && end < 1000)
    })
}

fn without_leading_tags(mut name: &str) -> &str {
    loop {
        name = name.trim();
        let close = match name.chars().next() {
            Some('[') => ']',
            Some('(') => ')',
            Some('{') => '}',
            _ => return name,
        };
        let Some(end) = name.find(close) else {
            return name;
        };
        name = &name[end + close.len_utf8()..];
    }
}

fn release_parts(name: &str) -> Vec<String> {
    // Uploaders also write subtitle separators as "Title- Subtitle".
    name.replace(['—', '–'], "-")
        .replace("- ", " - ")
        .split(" - ")
        .map(|part| part.trim().to_string())
        .collect()
}

fn normalize_volume(number: &str) -> String {
    let number = number.trim();
    let (integer, fraction) = number.split_once('.').unwrap_or((number, ""));
    if integer.is_empty()
        || !integer.chars().all(|ch| ch.is_ascii_digit())
        || !fraction.chars().all(|ch| ch.is_ascii_digit())
    {
        return number.to_string();
    }
    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        integer.to_string()
    } else {
        format!("{integer}.{fraction}")
    }
}

fn detect_volumes(name: &str) -> Vec<String> {
    let lowered = name.to_ascii_lowercase();
    let mut volumes = Vec::new();
    for (index, character) in lowered.char_indices() {
        let rest = &lowered[index..];
        let number = if character == '#' {
            Some(&rest[1..])
        } else if index == 0
            || lowered[..index]
                .chars()
                .next_back()
                .is_some_and(|ch| !ch.is_alphanumeric())
        {
            VOLUME_WORDS.iter().find_map(|word| {
                rest.strip_prefix(word).filter(|suffix| {
                    suffix
                        .chars()
                        .next()
                        .is_some_and(|next| !next.is_alphabetic())
                })
            })
        } else {
            None
        };
        if let Some(number) = number {
            let number = number.trim_start_matches(|character: char| {
                character.is_whitespace() || matches!(character, '.' | '_' | '-' | ':' | '#')
            });
            let bytes = number.as_bytes();
            let mut end = 0;
            while end < bytes.len()
                && (bytes[end].is_ascii_digit()
                    || (bytes[end] == b'.' && bytes.get(end + 1).is_some_and(u8::is_ascii_digit)))
            {
                end += 1;
            }
            if end > 0 && normalize_isbn(&number[..end]).is_none() {
                let volume = normalize_volume(&number[..end]);
                if !volumes.contains(&volume) {
                    volumes.push(volume);
                }
            }
        }
    }
    volumes
}

fn single_token_title_present(
    token: &str,
    release_tokens: &[String],
    excluded: &HashSet<String>,
) -> bool {
    release_tokens.iter().enumerate().any(|(index, candidate)| {
        candidate == token
            && neighbor_ignorable(release_tokens.get(index.wrapping_sub(1)), token, excluded)
            && neighbor_ignorable(release_tokens.get(index + 1), token, excluded)
    })
}

fn neighbor_ignorable(neighbor: Option<&String>, token: &str, excluded: &HashSet<String>) -> bool {
    let Some(neighbor) = neighbor else {
        return true;
    };

    neighbor == token
        || excluded.contains(neighbor)
        || neighbor.chars().all(|character| character.is_ascii_digit())
        || FORMAT_TOKENS.iter().any(|(key, _)| key == neighbor)
        || RETAIL_TOKENS.contains(&neighbor.as_str())
        || release_metadata(neighbor)
}

fn release_metadata(token: &str) -> bool {
    FORMAT_TOKENS.iter().any(|(key, _)| *key == token)
        || LANGUAGE_NAMES.iter().any(|(key, _)| *key == token)
        || LANGUAGE_CODES.iter().any(|(key, _)| *key == token)
        || RETAIL_TOKENS.contains(&token)
        || matches!(
            token,
            "ebook"
                | "ebooks"
                | "digital"
                | "edition"
                | "illustrated"
                | "lang"
                | "language"
                | "lng"
        )
        || token.chars().all(|character| character.is_ascii_digit())
}

fn name_fragment(tokens: &[&str]) -> Option<String> {
    let words: Vec<_> = tokens
        .iter()
        .copied()
        .filter(|word| !release_metadata(word) || LANGUAGE_CODES.iter().any(|(key, _)| key == word))
        .filter(|word| !matches!(*word, "lang" | "language"))
        .collect();
    // Infer an author only from a short name beside an exact title. Longer
    // phrases and subtitle syntax are not evidence of a conflicting author.
    ((2..=6).contains(&words.len())
        && words
            .iter()
            .all(|word| word.chars().all(char::is_alphabetic))
        && !words.iter().any(|word| {
            matches!(
                *word,
                "the"
                    | "a"
                    | "an"
                    | "and"
                    | "of"
                    | "for"
                    | "with"
                    | "complete"
                    | "collection"
                    | "volume"
                    | "book"
                    | "part"
                    | "series"
                    | "by"
            )
        }))
    .then(|| words.join(" "))
}

fn filename_author(name: &str, variants: &[String]) -> Option<String> {
    let parts = release_parts(name);
    if let Some(first) = parts.first()
        && parts.len() > 1
        && !title_variants(without_leading_tags(first))
            .iter()
            .any(|title| variants.contains(title))
        && variants
            .iter()
            .any(|title| phrase_ratio(title, &normalize_text(&parts[1..].join(" "))) >= 0.6)
        && let Some(author) = name_fragment(
            &normalize_text(without_leading_tags(first))
                .split_whitespace()
                .collect::<Vec<_>>(),
        )
    {
        return Some(author);
    }
    // An author after a title/series reference is still authoritative when
    // the expected title is only partially present in that reference.
    for (index, part) in parts.iter().enumerate().rev() {
        let core = bokhylle_core::identity::core_title(without_leading_tags(part));
        let normalized = normalize_text(&core);
        if index > 0
            && !variants.contains(&normalized)
            && variants
                .iter()
                .any(|title| phrase_ratio(title, &normalize_text(&parts[..index].join(" "))) >= 0.6)
            && let Some(author) = name_fragment(&normalized.split_whitespace().collect::<Vec<_>>())
        {
            return Some(author);
        }
    }
    let lowered = name.to_ascii_lowercase();
    if let Some((before, after)) = lowered.rsplit_once(" by ")
        && variants
            .iter()
            .any(|variant| phrase_ratio(variant, &normalize_text(before)) >= 1.0)
    {
        let normalized = normalize_text(after);
        if let Some(author) = name_fragment(&normalized.split_whitespace().collect::<Vec<_>>()) {
            return Some(author);
        }
    }
    let normalized = normalize_text(name);
    let words: Vec<_> = normalized.split_whitespace().collect();
    for variant in variants {
        let title: Vec<_> = variant.split_whitespace().collect();
        if title.is_empty() || title.len() > words.len() {
            continue;
        }
        for (index, window) in words.windows(title.len()).enumerate() {
            if window != title {
                continue;
            }
            if let Some(author) = name_fragment(&words[..index]) {
                return Some(author);
            }
            // A trailing name needs explicit syntax; an arbitrary subtitle is
            // not an author. Dot-separated author-first releases work above.
            if name.contains(" - ")
                || name.contains(" — ")
                || name.contains(" – ")
                || name.contains(" by ")
            {
                let suffix = &words[index + title.len()..];
                let suffix = suffix.strip_prefix(&["by"]).unwrap_or(suffix);
                if let Some(author) = name_fragment(suffix) {
                    return Some(author);
                }
            }
        }
    }
    None
}

fn filename_title(name: &str, authors: &[String], variants: &[String]) -> Option<String> {
    let parts = release_parts(name);
    let exact_author = |part: &str| {
        let part = without_leading_tags(part);
        authors
            .iter()
            .any(|author| author_matches(author, part) && author_matches(part, author))
    };
    let title_part = |part: &str| {
        let part = without_leading_tags(part);
        let normalized = normalize_text(part);
        !part.is_empty()
            && (variants.contains(&normalized)
                || normalized
                    .split_whitespace()
                    .any(|word| !release_metadata(word)))
    };
    let title = if let Some(author_index) = parts.iter().position(|part| exact_author(part)) {
        // An author may follow the title and precede a separate format list.
        // Bracketed series labels between author and title are decorations.
        let before = parts[..author_index]
            .iter()
            .rev()
            .find(|part| title_part(part));
        let after = parts[author_index + 1..]
            .iter()
            .find(|part| title_part(part));
        if before.is_some_and(|part| {
            title_variants(without_leading_tags(part))
                .iter()
                .any(|title| variants.contains(title))
        }) {
            before
        } else {
            after.or(before)
        }?
        .as_str()
    } else {
        let (title, author) = name.rsplit_once(" by ")?;
        if !exact_author(author) {
            return None;
        }
        title
    };
    let title = without_leading_tags(title);
    // A group attached to an explicit format/ebook tag is release metadata,
    // not a continuation of the book's title. Keep arbitrary title suffixes
    // so sequels and conflicting identities are still rejected.
    let title = match title.rsplit_once('-') {
        Some((prefix, group))
            if !group.is_empty()
                && group.chars().all(|ch| ch.is_alphanumeric() || ch == '_')
                && normalize_text(prefix)
                    .split_whitespace()
                    .next_back()
                    .is_some_and(|tag| {
                        matches!(tag, "ebook" | "ebooks")
                            || FORMAT_TOKENS.iter().any(|(format, _)| *format == tag)
                    }) =>
        {
            prefix
        }
        _ => title,
    };
    let core = normalize_text(&bokhylle_core::identity::core_title(title));
    let words: Vec<_> = core.split_whitespace().collect();
    let volume_start = words.windows(2).position(|pair| {
        VOLUME_WORDS.contains(&pair[0])
            && pair[1].chars().all(|character| character.is_ascii_digit())
    });
    let end = volume_start
        .or_else(|| {
            words
                .iter()
                .rposition(|word| !release_metadata(word))
                .map(|index| index + 1)
        })
        .or_else(|| variants.contains(&core).then_some(words.len()))?;
    let title = words[..end].join(" ");
    if variants.contains(&title) {
        Some(title)
    } else {
        // "A Novel" is a publishing descriptor, not a different book.
        Some(title.strip_suffix(" a novel").unwrap_or(&title).to_string())
    }
}

fn author_matches(expected: &str, actual: &str) -> bool {
    let expected = normalize_text(expected);
    let actual = normalize_text(actual);
    let actual: HashSet<_> = actual.split_whitespace().collect();
    !expected.is_empty()
        && expected
            .split_whitespace()
            .all(|word| actual.contains(word))
}

fn numbered_series_suffix(name: &str, title: &str, variants: &[String]) -> bool {
    let normalized = normalize_text(name);
    variants.iter().any(|variant| {
        variant.split_whitespace().count() >= 3
            && title
                .strip_prefix(&format!("{variant} "))
                .is_some_and(|suffix| {
                    suffix.split_whitespace().count() >= 3
                        && normalized
                            .split_once(&format!("{title} "))
                            .is_some_and(|(_, rest)| {
                                rest.split_whitespace()
                                    .next()
                                    .and_then(|number| number.parse::<u16>().ok())
                                    .is_some_and(|number| (1..100).contains(&number))
                            })
                })
    })
}

fn author_compatible(expected: &str, actual: &str) -> bool {
    if author_matches(expected, actual) {
        return true;
    }
    let expected = normalize_text(expected);
    let actual = normalize_text(actual);
    let expected: Vec<_> = expected.split_whitespace().collect();
    let actual: Vec<_> = actual.split_whitespace().collect();
    let Some(surname) = expected.last() else {
        return false;
    };
    if !actual.contains(surname) {
        return false;
    }
    // An omitted middle initial or an abbreviated first name is missing
    // evidence, rather than proof that this is a different author.
    actual.iter().filter(|word| *word != surname).all(|word| {
        expected[..expected.len() - 1].iter().any(|expected| {
            word == expected
                || ((word.len() == 1 || expected.len() == 1)
                    && word.chars().next() == expected.chars().next())
        })
    })
}

fn exact_title_present(variant: &str, release: &[String], excluded: &HashSet<String>) -> bool {
    let title: Vec<_> = variant.split_whitespace().collect();
    !title.is_empty()
        && title.len() <= release.len()
        && release
            .windows(title.len())
            .enumerate()
            .any(|(index, window)| {
                window.iter().map(String::as_str).eq(title.iter().copied())
                    && neighbor_ignorable(release.get(index.wrapping_sub(1)), title[0], excluded)
                    && neighbor_ignorable(
                        release.get(index + title.len()),
                        title[title.len() - 1],
                        excluded,
                    )
            })
}

pub fn title_variants(title: &str) -> Vec<String> {
    let mut variants = vec![normalize_text(title)];
    let core = normalize_text(&bokhylle_core::identity::core_title(title));
    if !core.is_empty() && !variants.contains(&core) {
        variants.push(core);
    }
    variants.retain(|variant| !variant.is_empty());
    variants
}

fn phrase_positions(phrases: &[String], words: &[String]) -> HashSet<usize> {
    let mut positions = HashSet::new();
    for phrase in phrases {
        let phrase: Vec<_> = phrase.split_whitespace().collect();
        if phrase.is_empty() || phrase.len() > words.len() {
            continue;
        }
        if let Some(start) = words
            .windows(phrase.len())
            .position(|window| window.iter().map(String::as_str).eq(phrase.iter().copied()))
        {
            positions.extend(start..start + phrase.len());
        }
    }
    positions
}

fn derivative_kinds(words: &[&str]) -> HashSet<&'static str> {
    let mut kinds = HashSet::new();
    if words.windows(2).any(|pair| pair == ["study", "guide"]) {
        kinds.insert("study guide");
    }
    if words.contains(&"workbook") {
        kinds.insert("workbook");
    }
    if words
        .iter()
        .any(|word| matches!(*word, "summary" | "summaries"))
    {
        kinds.insert("summary");
    }
    kinds
}

pub fn evaluate(book: &ExpectedBook, candidate: &ReleaseCandidate) -> EvaluatedRelease {
    let configured_format = book
        .preferred_format
        .as_deref()
        .map(|format| format.to_ascii_lowercase())
        .filter(|format| !format.is_empty())
        .unwrap_or_else(|| "epub".to_string());
    // "any" accepts EPUB and PDF, while still preferring EPUB.
    let any_compatible = matches!(configured_format.as_str(), "any" | "all");
    let preferred_format = if any_compatible {
        "epub".to_string()
    } else {
        configured_format
    };

    let release_tokens = tokens(&candidate.title);
    let token_set: HashSet<&str> = release_tokens.iter().map(String::as_str).collect();

    let mut excluded: HashSet<String> = normalize_text(&book.title)
        .split_whitespace()
        .map(str::to_string)
        .collect();
    for author in &book.authors {
        excluded.extend(
            normalize_text(author)
                .split_whitespace()
                .map(str::to_string),
        );
    }
    if book.series_number.is_some() {
        excluded.extend(VOLUME_WORDS.iter().map(|word| word.to_string()));
    }

    let normalized_release = normalize_text(&candidate.title);
    let title_variants = title_variants(&book.title);
    let mut author_variants: Vec<_> = book
        .authors
        .iter()
        .map(|author| normalize_text(author))
        .collect();
    let inferred_author = filename_author(&candidate.title, &title_variants);
    if let Some(author) = inferred_author.as_ref()
        && book
            .authors
            .iter()
            .any(|expected| author_compatible(expected, author))
    {
        author_variants.push(normalize_text(author));
    }
    let author_positions = phrase_positions(&author_variants, &release_tokens);
    let mut identity_positions = phrase_positions(&title_variants, &release_tokens);
    identity_positions.extend(&author_positions);
    let detection = detect(
        &candidate.title,
        &release_tokens,
        &identity_positions,
        &preferred_format,
    );
    let expected_title = normalize_text(&book.title);
    let source_words: Vec<_> = release_tokens
        .iter()
        .enumerate()
        .filter(|(index, _)| !author_positions.contains(index))
        .map(|(_, word)| word.as_str())
        .collect();
    let different_work = derivative_kinds(&source_words)
        != derivative_kinds(&expected_title.split_whitespace().collect::<Vec<_>>());

    let mut title_ratio: f32 = 0.0;
    let mut contains_phrase = false;
    let mut has_title_tokens = false;

    for variant in &title_variants {
        let tokens: Vec<&str> = variant.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }
        has_title_tokens = true;

        if tokens.len() == 1 {
            continue;
        }

        let ratio = phrase_ratio(variant, &normalized_release);
        title_ratio = title_ratio.max(ratio);

        if ratio >= 1.0 {
            contains_phrase = true;
        }
    }

    // Single-word titles need a whole-token match: "Dune" must not match
    // "Dune Messiah" just because the token appears contiguously.
    let single_token_variants: Vec<&String> = title_variants
        .iter()
        .filter(|variant| variant.split_whitespace().count() == 1)
        .collect();
    let single_token_satisfied = if single_token_variants.is_empty() {
        None
    } else {
        Some(
            single_token_variants
                .iter()
                .any(|variant| single_token_title_present(variant, &release_tokens, &excluded)),
        )
    };
    match single_token_satisfied {
        Some(true) => title_ratio = 1.0,
        Some(false) if !contains_phrase => title_ratio = title_ratio.max(0.5),
        _ => {}
    }

    if !has_title_tokens {
        title_ratio = 1.0;
    }

    let author_match = book.authors.iter().any(|author| {
        let author_tokens: Vec<String> = normalize_text(author)
            .split_whitespace()
            .map(str::to_string)
            .collect();
        !author_tokens.is_empty()
            && author_tokens
                .iter()
                .all(|token| token_set.contains(token.as_str()))
    }) || candidate.detected_author.as_deref().is_some_and(|actual| {
        book.authors
            .iter()
            .any(|author| author_matches(author, actual))
    });
    let stated_author = candidate
        .detected_author
        .clone()
        .filter(|author| !author.trim().is_empty())
        .or(inferred_author);
    let author_conflict = !book.authors.is_empty()
        && stated_author.as_deref().is_some_and(|actual| {
            !book
                .authors
                .iter()
                .any(|author| author_compatible(author, actual))
        });
    let isbn_match = book
        .isbn
        .as_deref()
        .and_then(normalize_isbn)
        .is_some_and(|isbn| parse_isbn(&candidate.title).as_deref() == Some(isbn.as_str()));
    let stated_title = filename_title(&candidate.title, &book.authors, &title_variants);
    let stated_title_match = stated_title
        .as_ref()
        .is_some_and(|title| title_variants.contains(title));
    let uncertain_series_title = stated_title
        .as_ref()
        .is_some_and(|title| numbered_series_suffix(&candidate.title, title, &title_variants));
    let strong_title = !uncertain_series_title
        && ((stated_title_match && single_token_satisfied != Some(false))
            || title_variants
                .iter()
                .any(|variant| exact_title_present(variant, &release_tokens, &excluded)));
    let title_conflict = !detection.is_collection
        && (stated_title
            .is_some_and(|title| !title_variants.contains(&title) && !uncertain_series_title)
            || candidate.detected_title.as_deref().is_some_and(|title| {
                !self::title_variants(title)
                    .iter()
                    .any(|variant| title_variants.contains(variant))
            }));

    let expected_languages: Vec<String> = if book.languages.is_empty() {
        book.language
            .as_deref()
            .map(normalize_text)
            .filter(|language| !language.is_empty())
            .into_iter()
            .collect()
    } else {
        book.languages
            .iter()
            .map(|language| normalize_text(language))
            .filter(|language| !language.is_empty())
            .collect()
    };
    let language_index = match detection.language.as_deref() {
        Some(detected) => expected_languages
            .iter()
            .position(|expected| expected == detected)
            .unwrap_or(usize::MAX),
        None => usize::MAX,
    };
    let language_match = language_index != usize::MAX;

    let format_match = detection.format.as_deref() == Some(preferred_format.as_str());

    let mut rejection_reasons = Vec::new();

    if author_conflict {
        rejection_reasons.push(RejectionReason::AuthorMismatch);
    }
    if title_conflict {
        rejection_reasons.push(RejectionReason::UnrelatedTitle);
    }
    if different_work && !rejection_reasons.contains(&RejectionReason::UnrelatedTitle) {
        rejection_reasons.push(RejectionReason::UnrelatedTitle);
    }
    let expected_volume = book.series_number.as_deref().map(normalize_volume);
    if expected_volume.as_ref().is_some_and(|expected| {
        detection
            .volumes
            .iter()
            .any(|detected| detected != expected)
    }) {
        rejection_reasons.push(RejectionReason::WrongVolume);
    }

    if let Some(detected) = detection.language.as_deref()
        && !expected_languages.is_empty()
        && !expected_languages
            .iter()
            .any(|expected| expected == detected)
    {
        rejection_reasons.push(RejectionReason::LanguageMismatch);
    }
    if detection.is_audiobook {
        rejection_reasons.push(RejectionReason::Audiobook);
    }
    if detection.is_comic && detection.format.as_deref() != Some("cbz") {
        rejection_reasons.push(RejectionReason::ComicOrManga);
    }
    if let Some(format) = detection.format.as_deref()
        && !SUPPORTED_FORMATS.contains(&format)
        && !detection.is_comic
    {
        rejection_reasons.push(RejectionReason::UnsupportedFormat);
    }
    // A weak partial title without the expected author is too easy to
    // confuse with a different book in a broad indexer search.
    let minimum_title_ratio = if author_match { 0.25 } else { 0.6 };
    if has_title_tokens && title_ratio < minimum_title_ratio && !contains_phrase && !isbn_match {
        rejection_reasons.push(RejectionReason::UnrelatedTitle);
    }
    if single_token_satisfied == Some(false)
        && !contains_phrase
        && !isbn_match
        && !stated_title_match
    {
        rejection_reasons.push(RejectionReason::UnrelatedTitle);
    }
    if has_title_tokens
        && title_variants
            .iter()
            .all(|title| title.chars().all(|character| character.is_ascii_digit()))
        && !author_match
        && !isbn_match
        && !tokens(without_leading_tags(&candidate.title))
            .first()
            .is_some_and(|first| title_variants.contains(first))
    {
        // A year in another book's release name is not identity evidence.
        rejection_reasons.push(RejectionReason::UnrelatedTitle);
    }
    if candidate.size_bytes > MAX_RELEASE_BYTES {
        rejection_reasons.push(RejectionReason::OversizedRelease);
    }

    // Product rule: the preferred format is evaluated before language
    // order, so a Swedish EPUB beats an English PDF in compatible mode.
    let format_tier = if format_match {
        0
    } else if any_compatible && matches!(detection.format.as_deref(), Some("pdf" | "cbz")) {
        1
    } else {
        2
    };

    let mut score = 0;
    let mut reasons: Vec<ScoreReason> = Vec::new();
    let add = |weight: i32, reason: &str, score: &mut i32, reasons: &mut Vec<ScoreReason>| {
        *score += weight;
        reasons.push(ScoreReason {
            weight,
            reason: reason.to_string(),
        });
    };

    if rejection_reasons.is_empty() {
        if strong_title {
            add(40, "strong title match", &mut score, &mut reasons);
        } else if title_ratio >= 0.6 {
            add(20, "partial title match", &mut score, &mut reasons);
        }

        if author_match {
            add(30, "author match", &mut score, &mut reasons);
        }
        if isbn_match {
            add(50, "isbn match", &mut score, &mut reasons);
        }
        if format_match {
            add(25, "preferred format", &mut score, &mut reasons);
        }
        if language_match {
            add(20, "preferred language", &mut score, &mut reasons);
        }
        if detection.retail {
            add(10, "retail indicator", &mut score, &mut reasons);
        }
        if !detection.is_collection {
            add(10, "standalone book", &mut score, &mut reasons);
        }
        if candidate.seeders.unwrap_or(0) >= 3 {
            add(5, "healthy seed count", &mut score, &mut reasons);
        }
        if plausible_size(detection.format.as_deref(), candidate.size_bytes) {
            add(5, "reasonable file size", &mut score, &mut reasons);
        }

        if preferred_format == "epub" && detection.format.as_deref() == Some("pdf") {
            let penalty = if any_compatible { -12 } else { -20 };
            add(
                penalty,
                if any_compatible {
                    "pdf while epub preferred (compatible fallback allowed)"
                } else {
                    "pdf while epub preferred"
                },
                &mut score,
                &mut reasons,
            );
        }
        if detection.is_collection {
            add(-30, "collection or pack", &mut score, &mut reasons);
        }

        match (detection.volume.as_deref(), expected_volume.as_deref()) {
            (Some(detected), Some(expected)) if detected != expected => {
                add(-40, "possible wrong volume", &mut score, &mut reasons);
            }
            (Some(_), None) => {
                add(-40, "possible wrong volume", &mut score, &mut reasons);
            }
            _ => {}
        }
    }

    let confidence = if rejection_reasons.is_empty() {
        confidence(title_ratio, author_match, format_match, language_match).max(if isbn_match {
            0.9
        } else {
            0.0
        })
    } else {
        0.0
    };

    let mut evaluated_candidate = candidate.clone();
    evaluated_candidate.detected_format = detection.format;
    evaluated_candidate.detected_language = detection.language;
    evaluated_candidate.detected_volume = detection.volume;
    evaluated_candidate.is_collection = detection.is_collection;
    evaluated_candidate.is_audiobook = detection.is_audiobook;
    evaluated_candidate.is_comic = detection.is_comic;

    if evaluated_candidate.detected_title.is_none() && (title_ratio >= 0.6 || contains_phrase) {
        evaluated_candidate.detected_title = Some(book.title.clone());
    }
    if evaluated_candidate.detected_author.is_none() && author_match {
        evaluated_candidate.detected_author = book.authors.first().cloned();
    }

    EvaluatedRelease {
        candidate: evaluated_candidate,
        score,
        confidence,
        format_tier,
        language_index,
        score_reasons: reasons,
        rejection_reasons,
    }
}

fn confidence(
    title_ratio: f32,
    author_match: bool,
    format_match: bool,
    language_match: bool,
) -> f32 {
    let mut value = 0.0;
    value += title_ratio.clamp(0.0, 1.0) * 0.5;
    if author_match {
        value += 0.3;
    }
    if format_match {
        value += 0.1;
    }
    if language_match {
        value += 0.1;
    }
    value.clamp(0.0, 1.0)
}

fn plausible_size(format: Option<&str>, bytes: i64) -> bool {
    match format {
        Some("pdf") | Some("cbz") => (100_000..=500_000_000).contains(&bytes),
        _ => (50_000..=100_000_000).contains(&bytes),
    }
}

pub fn rank(book: &ExpectedBook, candidates: &[ReleaseCandidate]) -> Vec<EvaluatedRelease> {
    let mut evaluated: Vec<EvaluatedRelease> = candidates
        .iter()
        .map(|candidate| evaluate(book, candidate))
        .collect();

    evaluated.sort_by(|left, right| {
        left.rejected()
            .cmp(&right.rejected())
            .then(is_confident_match(right).cmp(&is_confident_match(left)))
            .then(left.format_tier.cmp(&right.format_tier))
            // Within a format, earlier entries in the language list win
            // before any quality score can flip the order.
            .then(left.language_index.cmp(&right.language_index))
            .then(right.score.cmp(&left.score))
            .then(
                right
                    .confidence
                    .partial_cmp(&left.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    evaluated
}

/// Book identity is decided before format, language and source quality.
/// Missing authors remain possible matches; seed counts cannot make them
/// safe to recommend. This also works with historical serialized candidates.
pub fn is_confident_match(release: &EvaluatedRelease) -> bool {
    let has_reason = |reason: &str| {
        release
            .score_reasons
            .iter()
            .any(|item| item.reason == reason)
    };
    !release.rejected()
        && !release
            .candidate
            .method
            .as_ref()
            .is_some_and(|method| method.kind() == "http")
        && !release.candidate.is_collection
        && release
            .candidate
            .detected_format
            .as_deref()
            .is_some_and(|format| SUPPORTED_FORMATS.contains(&format))
        && !has_reason("possible wrong volume")
        && (has_reason("isbn match")
            || (has_reason("strong title match") && has_reason("author match")))
}

pub fn select(evaluated: &[EvaluatedRelease]) -> Selection {
    let mut viable: Vec<(usize, &EvaluatedRelease)> = evaluated
        .iter()
        .enumerate()
        .filter(|(_, release)| {
            !release.rejected()
                && !release
                    .candidate
                    .method
                    .as_ref()
                    .is_some_and(|method| method.kind() == "http")
        })
        .collect();

    if viable.is_empty() {
        return Selection::None;
    }
    viable.retain(|(_, release)| is_confident_match(release));
    if viable.is_empty() {
        return Selection::NeedsSelection;
    }
    // Releases with zero seeders are a last resort: when anything with
    // seeders (or unknown availability) is viable, ignore the dead ones.
    let any_available = viable
        .iter()
        .any(|(_, release)| release.candidate.seeders != Some(0));
    if any_available {
        viable.retain(|(_, release)| release.candidate.seeders != Some(0));
    }

    Selection::Auto { index: viable[0].0 }
}

#[cfg(test)]
mod core_title_matching_tests {
    use super::*;

    #[test]
    fn matches_releases_that_use_the_core_title() {
        let book = ExpectedBook {
            title: "Strange Dogs: An Expanse Novella (The Expanse)".to_string(),
            authors: vec!["James S. A. Corey".to_string()],
            language: Some("en".to_string()),
            preferred_format: Some("epub".to_string()),
            ..Default::default()
        };
        let release = ReleaseCandidate {
            source: None,
            method: None,
            id: "1".to_string(),
            title: "James S. A. Corey - Strange Dogs (The Expanse #6.5) [EPUB]".to_string(),
            indexer: Some("IPTorrents".to_string()),
            size_bytes: 1_500_000,
            seeders: Some(5),
            leechers: Some(1),
            download_url: None,
            magnet_url: None,
            info_url: None,
            detected_title: None,
            detected_author: None,
            detected_format: None,
            detected_language: None,
            detected_volume: None,
            is_collection: false,
            is_audiobook: false,
            is_comic: false,
        };

        let evaluated = evaluate(&book, &release);
        assert!(!evaluated.rejected(), "release should not be rejected");
        assert!(
            evaluated
                .score_reasons
                .iter()
                .any(|reason| reason.weight == 40)
        );
        assert!(
            evaluated
                .score_reasons
                .iter()
                .any(|reason| reason.weight == 30)
        );
        assert!(evaluated.confidence >= 0.85);
    }

    #[test]
    fn title_variants_include_core_and_full() {
        let variants = title_variants("Strange Dogs: An Expanse Novella (The Expanse)");
        assert!(variants.contains(&"strange dogs".to_string()));
        assert!(variants.contains(&"strange dogs an expanse novella the expanse".to_string()));
    }
}

#[cfg(test)]
mod language_tag_tests {
    use super::*;

    fn book(title: &str, language: Option<&str>) -> ExpectedBook {
        ExpectedBook {
            title: title.to_string(),
            authors: vec!["Stephen King".to_string()],
            language: language.map(str::to_string),
            preferred_format: Some("epub".to_string()),
            ..Default::default()
        }
    }

    fn release(title: &str) -> ReleaseCandidate {
        ReleaseCandidate {
            source: None,
            method: None,
            id: title.to_string(),
            title: title.to_string(),
            indexer: Some("test".to_string()),
            size_bytes: 2_000_000,
            seeders: Some(2),
            leechers: Some(0),
            download_url: None,
            magnet_url: None,
            info_url: None,
            detected_title: None,
            detected_author: None,
            detected_format: None,
            detected_language: None,
            detected_volume: None,
            is_collection: false,
            is_audiobook: false,
            is_comic: false,
        }
    }

    #[test]
    fn title_words_are_not_language_tags() {
        for title in [
            "Stephen King - It (1990) EPUB",
            "Stephen.King.It.EPUB",
            "No Country for Old Men EPUB",
            "No.Country.for.Old.Men.EPUB",
        ] {
            let expected = if title.contains("It") {
                book("It", Some("en"))
            } else {
                book("No Country for Old Men", Some("en"))
            };
            let evaluated = evaluate(&expected, &release(title));
            assert!(
                evaluated.candidate.detected_language.is_none(),
                "{title} must not be read as a language tag"
            );
            assert!(!evaluated.rejected(), "{title} must not be rejected");
        }
    }

    #[test]
    fn explicit_language_syntax_is_recognised() {
        for release_name in [
            "Stephen.King.It.[EN].EPUB",
            "Stephen King - It (EN) EPUB",
            "Stephen King - It LANG-EN EPUB",
            "Stephen.King.It.EN.EPUB",
        ] {
            let evaluated = evaluate(&book("It", Some("en")), &release(release_name));
            assert_eq!(
                evaluated.candidate.detected_language.as_deref(),
                Some("en"),
                "{release_name} should be recognised as English"
            );
        }
    }

    #[test]
    fn author_names_are_not_language_codes() {
        let dan_brown = ExpectedBook {
            title: "Inferno".to_string(),
            authors: vec!["Dan Brown".to_string()],
            language: Some("en".to_string()),
            preferred_format: Some("epub".to_string()),
            ..Default::default()
        };

        let plain = evaluate(&dan_brown, &release("Dan.Brown.Inferno.EPUB"));
        assert!(
            plain.candidate.detected_language.is_none(),
            "'Dan' must not read as Danish"
        );
        assert!(
            !plain.rejected(),
            "a Dan Brown release must not be rejected"
        );

        let tagged = evaluate(&dan_brown, &release("Dan.Brown.Inferno.ENG.EPUB"));
        assert_eq!(tagged.candidate.detected_language.as_deref(), Some("en"));
    }

    #[test]
    fn upper_case_titles_are_not_language_codes() {
        let it = book("It", Some("en"));

        let plain = evaluate(&it, &release("STEPHEN.KING.IT.EPUB"));
        assert!(
            plain.candidate.detected_language.is_none(),
            "'IT' must not read as Italian when it is the title"
        );
        assert!(!plain.rejected());

        let explicit = evaluate(&it, &release("Stephen.King.It.[IT].EPUB"));
        assert_eq!(explicit.candidate.detected_language.as_deref(), Some("it"));
    }

    #[test]
    fn full_language_names_are_recognised_broadly() {
        let evaluated = evaluate(
            &book("Der Prozess", Some("de")),
            &release("Franz Kafka - Der Prozess.GERMAN.EPUB"),
        );
        assert_eq!(evaluated.candidate.detected_language.as_deref(), Some("de"));
    }

    #[test]
    fn unsupported_formats_are_hard_rejected() {
        for title in [
            "Project Hail Mary.mobi",
            "Project Hail Mary AZW3",
            "Project Hail Mary FB2",
            "Project Hail Mary DJVU",
        ] {
            let evaluated = evaluate(
                &ExpectedBook {
                    title: "Project Hail Mary".to_string(),
                    authors: vec!["Andy Weir".to_string()],
                    preferred_format: Some("epub".to_string()),
                    ..Default::default()
                },
                &release(title),
            );
            assert!(
                evaluated
                    .rejection_reasons
                    .contains(&RejectionReason::UnsupportedFormat),
                "{title} should be rejected as unsupported"
            );
        }
    }
}

/// Stable fingerprint for a release: the indexer's guid when available, else
/// the normalized title, always scoped by indexer so two indexers cannot
/// block each other's releases.
pub fn release_key(candidate: &ReleaseCandidate) -> String {
    let indexer = candidate
        .indexer
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let id = candidate.id.trim();
    let key = if id.is_empty() {
        bokhylle_core::identity::normalize_text(&candidate.title)
    } else {
        id.to_ascii_lowercase()
    };
    format!("{indexer}::{key}")
}

#[cfg(test)]
mod release_key_tests {
    use super::*;

    fn candidate(id: &str, indexer: Option<&str>, title: &str) -> ReleaseCandidate {
        ReleaseCandidate {
            source: None,
            method: None,
            id: id.to_string(),
            title: title.to_string(),
            indexer: indexer.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn key_prefers_guid_and_is_indexer_scoped() {
        assert_eq!(
            release_key(&candidate("guid-1", Some("Indexer A"), "Dune EPUB")),
            "indexer a::guid-1"
        );
        assert_eq!(
            release_key(&candidate("", None, "Dune EPUB")),
            "::dune epub"
        );
        assert_ne!(
            release_key(&candidate("same", Some("one"), "x")),
            release_key(&candidate("same", Some("two"), "x"))
        );
    }
}

#[cfg(test)]
mod language_set_tests {
    use super::*;
    use crate::model::{ExpectedBook, ReleaseCandidate};

    fn candidate(title: &str) -> ReleaseCandidate {
        ReleaseCandidate {
            source: None,
            method: None,
            id: "x".to_string(),
            title: title.to_string(),
            indexer: Some("test".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn any_compatible_prefers_epub_but_allows_pdf() {
        let book = ExpectedBook {
            title: "Some Book".to_string(),
            authors: Vec::new(),
            year: None,
            isbn: None,
            language: None,
            languages: Vec::new(),
            preferred_format: Some("any".to_string()),
            series_number: None,
        };
        let epub = evaluate(&book, &candidate("Some Book EPUB"));
        let pdf = evaluate(&book, &candidate("Some Book PDF"));

        assert!(epub.score > pdf.score, "epub must rank above pdf");
        assert!(
            pdf.rejection_reasons.is_empty(),
            "pdf is an allowed fallback in any-compatible mode"
        );
        assert!(epub.rejection_reasons.is_empty());
    }

    #[test]
    fn accepts_any_profile_language() {
        let book = ExpectedBook {
            title: "Some Book".to_string(),
            authors: Vec::new(),
            year: None,
            isbn: None,
            language: Some("en".to_string()),
            languages: vec!["en".to_string(), "sv".to_string()],
            preferred_format: Some("epub".to_string()),
            series_number: None,
        };
        let evaluated = evaluate(&book, &candidate("Some Book [SV] EPUB"));
        assert!(
            !evaluated
                .rejection_reasons
                .contains(&RejectionReason::LanguageMismatch),
            "swedish must be accepted when it is in the profile set"
        );

        let rejected = evaluate(&book, &candidate("Some Book [DE] EPUB"));
        assert!(
            rejected
                .rejection_reasons
                .contains(&RejectionReason::LanguageMismatch)
        );
    }
}
