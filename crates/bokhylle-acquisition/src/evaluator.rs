use std::collections::HashSet;

use bokhylle_core::identity::{normalize_text, phrase_ratio};

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
];
const COMIC_TOKENS: &[&str] = &["comic", "comics", "manga"];
const VOLUME_WORDS: &[&str] = &["book", "vol", "volume", "bkn", "bk", "part"];
const RETAIL_TOKENS: &[&str] = &["retail", "proper", "repack"];

pub fn tokens(name: &str) -> Vec<String> {
    name.replace(
        [
            '.', '_', '-', '+', ',', ';', ':', '(', ')', '[', ']', '{', '}',
        ],
        " ",
    )
    .split_whitespace()
    .map(|part| {
        part.trim_matches(|character: char| !character.is_alphanumeric() && character != '#')
            .to_ascii_lowercase()
    })
    .filter(|token| !token.is_empty())
    .collect()
}

struct Detection {
    format: Option<String>,
    language: Option<String>,
    volume: Option<String>,
    retail: bool,
    is_collection: bool,
    is_audiobook: bool,
    is_comic: bool,
}

fn detect(
    name: &str,
    tokens: &[String],
    excluded: &HashSet<String>,
    preferred_format: &str,
) -> Detection {
    let mut formats = Vec::new();
    let mut language = None;
    let tagged = tagged_tokens(name, excluded);
    let mut is_collection = false;
    let mut is_audiobook = false;
    let mut is_comic = false;
    let mut retail = false;

    for token in tokens {
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
        if COLLECTION_TOKENS.contains(&token.as_str()) {
            is_collection = true;
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

    let lowered = name.to_ascii_lowercase();
    if !is_collection && (lowered.contains(" omnibus") || lowered.contains(" complete ")) {
        is_collection = true;
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

    Detection {
        format: format.map(str::to_string),
        language,
        volume: detect_volume(tokens),
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
fn tagged_tokens(name: &str, excluded: &HashSet<String>) -> HashSet<String> {
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

            // Release tags such as ".EN."/"ENG" are upper-case short fragments
            // that are not part of the expected title or author, so "IT" in
            // "STEPHEN.KING.IT.EPUB" stays untagged for the book "It".
            let trimmed = fragment.trim();
            if (2..=3).contains(&trimmed.len())
                && trimmed
                    .chars()
                    .all(|character| character.is_ascii_alphabetic())
                && trimmed == trimmed.to_ascii_uppercase()
            {
                let token = trimmed.to_ascii_lowercase();
                if !excluded.contains(&token) {
                    tagged.insert(token);
                }
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

fn detect_volume(tokens: &[String]) -> Option<String> {
    for (index, token) in tokens.iter().enumerate() {
        let word = token.trim_start_matches('#');
        if VOLUME_WORDS.contains(&word) {
            if let Some(next) = tokens.get(index + 1)
                && next.chars().all(|character| character.is_ascii_digit())
            {
                return Some(next.clone());
            }
            let trailing: String = word.chars().skip_while(|c| !c.is_ascii_digit()).collect();
            if !trailing.is_empty() {
                return Some(trailing);
            }
        }
        if let Some(number) = word.strip_prefix('#')
            && number.chars().all(|character| character.is_ascii_digit())
        {
            return Some(number.to_string());
        }
    }
    None
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

    let detection = detect(
        &candidate.title,
        &release_tokens,
        &excluded,
        &preferred_format,
    );

    let normalized_release = normalize_text(&candidate.title);
    let title_variants = title_variants(&book.title);

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
    });

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
    if has_title_tokens && title_ratio < minimum_title_ratio && !contains_phrase {
        rejection_reasons.push(RejectionReason::UnrelatedTitle);
    }
    if single_token_satisfied == Some(false) && !contains_phrase {
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
        if title_ratio >= 1.0 || contains_phrase {
            add(40, "strong title match", &mut score, &mut reasons);
        } else if title_ratio >= 0.6 {
            add(20, "partial title match", &mut score, &mut reasons);
        }

        if author_match {
            add(30, "author match", &mut score, &mut reasons);
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

        let expected_volume = book.series_number.as_deref().map(normalize_text);
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
        confidence(title_ratio, author_match, format_match, language_match)
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

pub fn select(evaluated: &[EvaluatedRelease]) -> Selection {
    let mut viable: Vec<(usize, &EvaluatedRelease)> = evaluated
        .iter()
        .enumerate()
        .filter(|(_, release)| !release.rejected())
        .collect();

    // Releases with zero seeders are a last resort: when anything with
    // seeders (or unknown availability) is viable, ignore the dead ones.
    let any_available = viable
        .iter()
        .any(|(_, release)| release.candidate.seeders != Some(0));
    if any_available {
        viable.retain(|(_, release)| release.candidate.seeders != Some(0));
    }

    let Some((index, best)) = viable.first().copied() else {
        return Selection::None;
    };

    // A partial title can be offered for review, but must not start a
    // download automatically even when it is the only candidate.
    if !best
        .score_reasons
        .iter()
        .any(|reason| reason.reason == "strong title match")
    {
        return Selection::NeedsSelection;
    }

    if best.confidence >= 0.85 {
        return Selection::Auto { index };
    }

    if best.confidence >= 0.6 {
        let gap = viable
            .get(1)
            .map(|(_, runner_up)| best.score - runner_up.score)
            .unwrap_or(i32::MAX);
        if gap >= 15 {
            return Selection::Auto { index };
        }
    }

    Selection::NeedsSelection
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
