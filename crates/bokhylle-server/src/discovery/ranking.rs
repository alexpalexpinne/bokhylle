//! Provider-independent relevance ranking.
//!
//! Providers normalize their API responses into `MetadataResult`; this module
//! scores only those normalized signals plus ownership. It contains no
//! provider-specific logic and is deterministic: same candidates, same order.
//!
//! Tiers (added, never gates):
//! - title: exact 1000, prefix 700, contains 450, partial 140 × token share
//! - author query match: exact 900, partial 420 × overlap (Author searches)
//! - author evidence: up to 8 strong matches × 20 × the dominant author's
//!   share of strong matches for non-exact titles; duplicate exact titles use
//!   engagement rather than repeated derivative authorship
//! - series: 120 when the candidate is in the query/dominant series
//! - language match with the query text: 60
//! - engagement: log-scaled ratings/popularity/editions, capped at 120 so it
//!   can never erase a title tier
//! - collection/companion markers: −260 / −220, flipped to +160 when the
//!   query itself asks for that kind of material

use std::cmp::Ordering;
use std::collections::HashMap;

use bokhylle_core::identity::{core_title, normalize_text};

use super::SearchKind;
use crate::discovery::{DiscoveryResult, MetadataResult};

/// First-page provider candidates fetched before ranking, so canonical books
/// the provider ranked lower can still surface. Providers clamp to their own
/// caps (Open Library 50, Google Books 40).
pub const FIRST_PAGE_CANDIDATES: usize = 50;

const TITLE_EXACT: i64 = 1000;
const TITLE_CORE_MATCH: i64 = 880;
const TITLE_PREFIX: i64 = 700;
const TITLE_CONTAINS: i64 = 450;
const TITLE_PARTIAL: i64 = 140;
const AUTHOR_QUERY_EXACT: i64 = 900;
const AUTHOR_QUERY_PARTIAL: i64 = 420;
const AUTHOR_EVIDENCE_PER_MATCH: i64 = 20;
const AUTHOR_EVIDENCE_MAX_MATCHES: usize = 8;
const SERIES_MATCH: i64 = 120;
const SERIES_QUERY_MATCH: i64 = 1200;
const LANGUAGE_MATCH: i64 = 60;
const COLLECTION_PENALTY: i64 = 260;
const COMPANION_PENALTY: i64 = 220;
const INTENT_BONUS: i64 = 160;
const ENGAGEMENT_CAP: f64 = 120.0;

const COLLECTION_MARKERS: [&str; 7] = [
    "box set",
    "boxed set",
    "boxset",
    "complete collection",
    "complete series",
    "omnibus",
    "bundle",
];
const COMPANION_MARKERS: [&str; 14] = [
    "unofficial",
    "study guide",
    "summary",
    "quiz",
    "colouring",
    "coloring",
    "analysis",
    "workbook",
    "reading guide",
    "companion",
    "annotated",
    "graphic novel",
    "calendar",
    "roleplaying game",
];

pub(super) fn without_leading_article(value: &str) -> &str {
    value
        .strip_prefix("the ")
        .or_else(|| value.strip_prefix("an "))
        .or_else(|| value.strip_prefix("a "))
        .unwrap_or(value)
}

/// Scores one normalized candidate against the query.
pub fn rank_books(results: &mut Vec<DiscoveryResult>, kind: SearchKind, text: &str) {
    let query = Query::new(kind, text);
    if !query.usable() || results.is_empty() {
        return;
    }
    let candidates: Vec<Candidate<'_>> = results.iter().map(Candidate::from_book).collect();
    reorder(results, rank_order(&candidates, &query));
}

/// The same ranking for provider candidates before ownership resolution.
pub fn rank_metadata(results: &mut Vec<MetadataResult>, kind: SearchKind, text: &str) {
    let query = Query::new(kind, text);
    if !query.usable() || results.is_empty() {
        return;
    }
    let candidates: Vec<Candidate<'_>> = results.iter().map(Candidate::from_metadata).collect();
    reorder(results, rank_order(&candidates, &query));
}

fn reorder<T>(items: &mut Vec<T>, order: Vec<usize>) {
    let mut slots: Vec<Option<T>> = items.drain(..).map(Some).collect();
    items.extend(
        order
            .into_iter()
            .map(|index| slots[index].take().expect("rank order covers each item")),
    );
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TitleTier {
    Exact,
    /// The core title matches, but the full title carries a subtitle or
    /// edition suffix (e.g. "Dune: The Complete Saga" for "Dune").
    CoreMatch,
    Prefix,
    Contains,
    Partial(f64),
    None,
}

struct Candidate<'a> {
    title: &'a str,
    authors: &'a [String],
    subjects: &'a [String],
    series: Option<&'a str>,
    series_number: Option<&'a str>,
    year: Option<i32>,
    languages: &'a [String],
    isbn10: Option<&'a str>,
    isbn13: Option<&'a str>,
    rating_average: Option<f64>,
    rating_count: Option<i64>,
    edition_count: Option<i64>,
    popularity: Option<i64>,
}

impl<'a> Candidate<'a> {
    fn from_book(book: &'a DiscoveryResult) -> Self {
        Self {
            title: &book.title,
            authors: &book.authors,
            subjects: &book.subjects,
            series: book.series.as_deref(),
            series_number: book.series_number.as_deref(),
            year: book.year,
            languages: &book.languages,
            isbn10: book.isbn10.as_deref(),
            isbn13: book.isbn13.as_deref(),
            rating_average: book.rating_average,
            rating_count: book.rating_count,
            edition_count: book.edition_count,
            popularity: book.popularity,
        }
    }

    fn from_metadata(result: &'a MetadataResult) -> Self {
        Self {
            title: &result.title,
            authors: &result.authors,
            subjects: &result.subjects,
            series: result.series.as_deref(),
            series_number: result.series_number.as_deref(),
            year: result.year,
            languages: &result.languages,
            isbn10: result.isbn10.as_deref(),
            isbn13: result.isbn13.as_deref(),
            rating_average: result.rating_average,
            rating_count: result.rating_count,
            edition_count: result.edition_count,
            popularity: result.popularity,
        }
    }
}

struct Query<'a> {
    kind: SearchKind,
    text: &'a str,
    core: String,
    tokens: Vec<String>,
    collection_intent: bool,
    companion_intent: bool,
}

impl<'a> Query<'a> {
    fn new(kind: SearchKind, text: &'a str) -> Self {
        let core = normalize_text(&core_title(text));
        let tokens: Vec<String> = core
            .split_whitespace()
            .filter(|token| token.len() > 1)
            .map(str::to_string)
            .collect();
        let lower = text.to_lowercase();
        Self {
            kind,
            text,
            collection_intent: has_marker(&lower, &COLLECTION_MARKERS) || has_book_range(&lower),
            companion_intent: has_marker(&lower, &COMPANION_MARKERS),
            core,
            tokens,
        }
    }

    fn usable(&self) -> bool {
        !self.core.is_empty() || matches!(self.kind, SearchKind::Isbn)
    }

    fn title_tier(&self, title: &str) -> TitleTier {
        let full = normalize_text(title);
        if full == self.core
            || without_leading_article(&full) == without_leading_article(&self.core)
        {
            return TitleTier::Exact;
        }
        let title = normalize_text(&core_title(title));
        if title == self.core {
            return TitleTier::CoreMatch;
        }
        if title.starts_with(&self.core) {
            return TitleTier::Prefix;
        }
        if title.contains(&self.core) {
            return TitleTier::Contains;
        }
        if self.tokens.is_empty() {
            return TitleTier::None;
        }
        let matched = self
            .tokens
            .iter()
            .filter(|token| title.contains(token.as_str()))
            .count();
        if matched == 0 {
            TitleTier::None
        } else {
            TitleTier::Partial(matched as f64 / self.tokens.len() as f64)
        }
    }

    /// The query looks like an author name; used by Author searches and for
    /// `Any` queries that read as a person.
    fn author_signal(&self, candidate: &Candidate<'_>) -> Option<f64> {
        let mut best: Option<f64> = None;
        for author in candidate.authors {
            let author = normalize_text(author);
            if author.is_empty() {
                continue;
            }
            let overlap = if author == self.core {
                1.0
            } else if author.contains(&self.core) || self.core.contains(&author) {
                0.9
            } else if self.tokens.is_empty() {
                0.0
            } else {
                let matched = self
                    .tokens
                    .iter()
                    .filter(|token| author.split_whitespace().any(|word| word == token.as_str()))
                    .count();
                matched as f64 / self.tokens.len() as f64
            };
            if overlap > 0.0 && best.is_none_or(|current| overlap > current) {
                best = Some(overlap);
            }
        }
        best
    }

    fn score(&self, candidate: &Candidate<'_>, evidence: &Evidence) -> i64 {
        if self.kind == SearchKind::Isbn {
            let needle = digits(self.text);
            let hit = candidate.isbn13.is_some_and(|isbn| digits(isbn) == needle)
                || candidate.isbn10.is_some_and(|isbn| digits(isbn) == needle);
            return if hit { TITLE_EXACT } else { 0 };
        }
        if self.kind == SearchKind::Subject {
            // Topic searches rank actual subjects rather than rewarding a book
            // merely because its title happens to contain the topic's name.
            return candidate
                .subjects
                .iter()
                .map(|subject| {
                    let subject = normalize_text(subject);
                    if crate::library::subjects::concept(&subject)
                        == crate::library::subjects::concept(&self.core)
                    {
                        900
                    } else if self
                        .tokens
                        .iter()
                        .all(|token| subject.split_whitespace().any(|word| word == token))
                    {
                        600
                    } else {
                        0
                    }
                })
                .max()
                .unwrap_or(0);
        }

        let tier = self.title_tier(candidate.title);
        let strong_title = matches!(
            tier,
            TitleTier::Exact | TitleTier::CoreMatch | TitleTier::Prefix | TitleTier::Contains
        );
        let title_score = match tier {
            TitleTier::Exact => TITLE_EXACT,
            TitleTier::CoreMatch => TITLE_CORE_MATCH,
            TitleTier::Prefix => TITLE_PREFIX,
            TitleTier::Contains => TITLE_CONTAINS,
            TitleTier::Partial(share) => (TITLE_PARTIAL as f64 * share) as i64,
            TitleTier::None => 0,
        };
        let author_query = match self.author_signal(candidate) {
            Some(overlap) if overlap >= 1.0 => AUTHOR_QUERY_EXACT,
            Some(overlap) if self.kind == SearchKind::Author => {
                (AUTHOR_QUERY_PARTIAL as f64 * overlap) as i64
            }
            _ => 0,
        };
        let mut score = match self.kind {
            SearchKind::Author => author_query.max(title_score.min(TITLE_PARTIAL)),
            _ => title_score.max(author_query),
        };

        // A mixed title + author query should favor a work whose fields
        // together explain every word ("It Stephen King").
        if matches!(self.kind, SearchKind::Any) {
            let title_words = normalize_text(candidate.title);
            let author_words: Vec<String> = candidate
                .authors
                .iter()
                .map(|name| normalize_text(name))
                .collect();
            let query_words: Vec<&str> = self.core.split_whitespace().collect();
            let title_hit = query_words.iter().any(|word| {
                title_words
                    .split_whitespace()
                    .any(|candidate| candidate == *word)
            });
            let author_hit = query_words.iter().any(|word| {
                author_words.iter().any(|author| {
                    author
                        .split_whitespace()
                        .any(|candidate| candidate == *word)
                })
            });
            if query_words.len() >= 2
                && title_hit
                && author_hit
                && query_words.iter().all(|word| {
                    title_words
                        .split_whitespace()
                        .any(|candidate| candidate == *word)
                        || author_words.iter().any(|author| {
                            author
                                .split_whitespace()
                                .any(|candidate| candidate == *word)
                        })
                })
            {
                score = score.max(980);
            }
        }

        let matches = candidate
            .authors
            .iter()
            .map(|author| {
                evidence
                    .author_strong
                    .get(&normalize_text(author))
                    .copied()
                    .unwrap_or(0)
            })
            .max()
            .unwrap_or(0);
        if matches > 0 && evidence.strong_matches > 0 && !matches!(tier, TitleTier::Exact) {
            let share = matches as f64 / evidence.strong_matches as f64;
            let capped = matches.min(AUTHOR_EVIDENCE_MAX_MATCHES);
            score += (capped as f64 * AUTHOR_EVIDENCE_PER_MATCH as f64 * share) as i64;
        }

        let in_dominant_series = evidence
            .dominant_series
            .as_deref()
            .is_some_and(|series| candidate_series_matches(candidate, series));
        let query_names_series = candidate
            .series
            .is_some_and(|series| self.core.contains(&normalize_text(series)));
        // Providers are inconsistent about series metadata; a strong title
        // match missing the field is treated as likely in the same series.
        let likely_same_series =
            evidence.dominant_series.is_some() && candidate.series.is_none() && strong_title;
        let exact_series = candidate
            .series
            .is_some_and(|series| normalize_text(series) == self.core);
        if exact_series {
            score += SERIES_QUERY_MATCH;
        } else if in_dominant_series || query_names_series || likely_same_series {
            score += SERIES_MATCH;
        }

        if candidate
            .languages
            .iter()
            .any(|language| self.tokens.iter().any(|token| language == token))
        {
            score += LANGUAGE_MATCH;
        }

        let collection = candidate.title.to_lowercase();
        if has_marker(&collection, &COLLECTION_MARKERS) || has_book_range(&collection) {
            score += if self.collection_intent {
                INTENT_BONUS
            } else {
                -COLLECTION_PENALTY
            };
        }
        if has_marker(&collection, &COMPANION_MARKERS) {
            score += if self.companion_intent {
                INTENT_BONUS
            } else {
                -COMPANION_PENALTY
            };
        }
        score
    }
}

struct Evidence {
    author_strong: HashMap<String, usize>,
    strong_matches: usize,
    dominant_series: Option<String>,
}

fn evidence(candidates: &[Candidate<'_>], query: &Query<'_>) -> Evidence {
    let mut author_strong: HashMap<String, usize> = HashMap::new();
    let mut series_counts: HashMap<String, usize> = HashMap::new();
    let mut strong_matches = 0usize;
    for candidate in candidates {
        let tier = query.title_tier(candidate.title);
        let strong = matches!(
            tier,
            TitleTier::Exact | TitleTier::CoreMatch | TitleTier::Prefix | TitleTier::Contains
        );
        if !strong {
            continue;
        }
        strong_matches += 1;
        for author in candidate.authors {
            let author = normalize_text(author);
            if !author.is_empty() {
                *author_strong.entry(author).or_insert(0) += 1;
            }
        }
        if let Some(series) = candidate.series {
            *series_counts.entry(normalize_text(series)).or_insert(0) += 1;
        }
    }
    // A series is only evidence when it recurs; one stray record is not.
    let dominant_series = series_counts
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
        .map(|(series, _)| series);
    Evidence {
        author_strong,
        strong_matches,
        dominant_series,
    }
}

struct Scored {
    score: i64,
    index: usize,
    series: Option<String>,
    position: Option<i64>,
    year: Option<i32>,
    engagement: i64,
}

fn rank_order(candidates: &[Candidate<'_>], query: &Query<'_>) -> Vec<usize> {
    let context = evidence(candidates, query);
    let mut scored: Vec<Scored> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| Scored {
            score: query.score(candidate, &context),
            index,
            series: candidate.series.map(normalize_text),
            position: series_position(candidate.series_number),
            year: candidate.year,
            engagement: engagement(candidate),
        })
        .collect();
    scored.sort_by(|left, right| {
        // Score first, then series order for the dominant series, then
        // engagement, then the original provider position (stable tie-break).
        right
            .score
            .cmp(&left.score)
            .then_with(|| series_order(left, right, context.dominant_series.as_deref()))
            .then_with(|| right.engagement.cmp(&left.engagement))
            .then_with(|| left.index.cmp(&right.index))
    });
    scored.into_iter().map(|item| item.index).collect()
}

/// Series position and publication year order equal-scoring candidates of the
/// same series without turning either into a score.
fn series_order(left: &Scored, right: &Scored, dominant: Option<&str>) -> Ordering {
    let Some(dominant) = dominant else {
        return Ordering::Equal;
    };
    let left_in = left.series.as_deref() == Some(dominant);
    let right_in = right.series.as_deref() == Some(dominant);
    // Two different named series are not comparable.
    if left.series.is_some() && right.series.is_some() && !left_in && !right_in {
        return Ordering::Equal;
    }
    // A candidate with no series metadata may still belong to the dominant
    // series (providers are inconsistent), so year order covers it.
    if !left_in && !right_in {
        return Ordering::Equal;
    }
    match (left.position, right.position) {
        // Complete positions are the strongest series signal.
        (Some(left_position), Some(right_position)) if left_position != right_position => {
            left_position.cmp(&right_position)
        }
        // With incomplete positions, publication order is the fallback.
        (_, _) => match (left.year, right.year) {
            (Some(left_year), Some(right_year)) if left_year != right_year => {
                left_year.cmp(&right_year)
            }
            _ => match (left.position, right.position) {
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                _ => Ordering::Equal,
            },
        },
    }
}

fn candidate_series_matches(candidate: &Candidate<'_>, series: &str) -> bool {
    candidate
        .series
        .is_some_and(|value| normalize_text(value) == series)
}

/// Series numbers are ordinal only when they parse cleanly and sanely.
pub fn series_position(value: Option<&str>) -> Option<i64> {
    let value = value?.trim();
    let digits: String = value.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    let position = digits.parse::<i64>().ok()?;
    (1..=500).contains(&position).then_some(position)
}

fn engagement(candidate: &Candidate<'_>) -> i64 {
    let mut score = 0.0;
    if let Some(count) = candidate.rating_count {
        score += (1.0 + count.max(0) as f64).log10() * 14.0;
    }
    if let Some(popularity) = candidate.popularity {
        score += (1.0 + popularity.max(0) as f64).log10() * 12.0;
    }
    if let Some(editions) = candidate.edition_count {
        score += (1.0 + editions.max(0) as f64).log10() * 8.0;
    }
    if let (Some(average), Some(count)) = (candidate.rating_average, candidate.rating_count)
        && count >= 5
    {
        score += (average - 3.0).clamp(0.0, 2.0) * 8.0;
    }
    score.min(ENGAGEMENT_CAP) as i64
}

fn has_marker(title: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| title.contains(marker))
}

/// "books 1-7", "books 1–7", "1-7": a range of volumes reads as a collection.
fn has_book_range(title: &str) -> bool {
    let characters: Vec<char> = title.chars().collect();
    let mut index = 0;
    while index < characters.len() {
        if !characters[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < characters.len() && characters[index].is_ascii_digit() {
            index += 1;
        }
        let end = index;
        let mut cursor = index;
        while cursor < characters.len() && characters[cursor].is_whitespace() {
            cursor += 1;
        }
        if cursor < characters.len()
            && (characters[cursor] == '-' || characters[cursor] == '\u{2013}')
        {
            cursor += 1;
            while cursor < characters.len() && characters[cursor].is_whitespace() {
                cursor += 1;
            }
            let digits_start = cursor;
            while cursor < characters.len() && characters[cursor].is_ascii_digit() {
                cursor += 1;
            }
            if cursor > digits_start && end > start {
                return true;
            }
        }
    }
    false
}

fn digits(value: &str) -> String {
    value.chars().filter(char::is_ascii_digit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(
        title: &str,
        authors: &[&str],
        series: Option<&str>,
        series_number: Option<&str>,
        year: Option<i32>,
        popularity: Option<i64>,
    ) -> DiscoveryResult {
        DiscoveryResult {
            provider: "fake".to_string(),
            provider_key: title.to_lowercase().replace(' ', "-"),
            title: title.to_string(),
            authors: authors.iter().map(|author| author.to_string()).collect(),
            year,
            languages: Vec::new(),
            series: series.map(str::to_string),
            series_number: series_number.map(str::to_string),
            popularity,
            rating_average: None,
            rating_count: None,
            edition_count: None,
            ..Default::default()
        }
    }

    fn titles(results: &[DiscoveryResult]) -> Vec<String> {
        results.iter().map(|book| book.title.clone()).collect()
    }

    #[test]
    fn named_author_queries_keep_authors_distinct() {
        let candidates = vec![
            book(
                "The Final Empire",
                &["Brandon Sanderson"],
                None,
                None,
                Some(2006),
                Some(500),
            ),
            book(
                "Patient-First Revolution",
                &["Brian Sanderson"],
                None,
                None,
                Some(2022),
                Some(5),
            ),
            book(
                "Harry Potter and the Philosopher's Stone",
                &["J. K. Rowling"],
                None,
                None,
                Some(1997),
                Some(1000),
            ),
            book("It", &["Stephen King"], None, None, Some(1986), Some(900)),
        ];
        for (query, expected) in [
            ("j k rowling", "Harry Potter and the Philosopher's Stone"),
            ("brandon sanderson", "The Final Empire"),
            ("brian sanderson", "Patient-First Revolution"),
            ("stephen king", "It"),
        ] {
            let mut results = candidates.clone();
            rank_books(&mut results, SearchKind::Author, query);
            assert_eq!(results[0].title, expected, "query {query}");
        }
    }

    #[test]
    fn short_title_with_author_beats_matching_derivative_titles() {
        let mut results = vec![
            book(
                "It Stephen King Explained",
                &["Someone Else"],
                None,
                None,
                Some(2020),
                Some(1000),
            ),
            book("It", &["Stephen King"], None, None, Some(1986), Some(900)),
            book("It", &["Another Author"], None, None, Some(2019), Some(20)),
        ];
        rank_books(&mut results, SearchKind::Any, "it stephen king");
        assert_eq!(results[0].title, "It");
        assert_eq!(results[0].authors, vec!["Stephen King"]);
    }

    #[test]
    fn canonical_novels_lead_a_noisy_mixed_page() {
        // Realistic provider noise: collections, companions, missing series
        // metadata and uneven popularity.
        let mut results = vec![
            book(
                "Harry Potter Complete Collection",
                &["J. K. Rowling"],
                None,
                None,
                Some(2020),
                Some(9_000),
            ),
            book(
                "Harry Potter Box Set 1-7",
                &["J. K. Rowling"],
                None,
                None,
                Some(2019),
                Some(4_000),
            ),
            book(
                "Harry Potter Quiz Book",
                &["Quiz Author"],
                None,
                None,
                Some(2021),
                Some(300),
            ),
            book(
                "Harry Potter Study Guide",
                &["Study Author"],
                None,
                None,
                Some(2018),
                Some(120),
            ),
            book(
                "Harry Potter and the Philosopher's Stone",
                &["J. K. Rowling"],
                Some("Harry Potter"),
                Some("1"),
                Some(1997),
                Some(1_200),
            ),
            book(
                "Harry Potter and the Chamber of Secrets",
                &["J. K. Rowling"],
                None,
                None,
                Some(1998),
                None,
            ),
            book(
                "Harry Potter and the Prisoner of Azkaban",
                &["J. K. Rowling"],
                Some("Harry Potter"),
                Some("3"),
                Some(1999),
                Some(900),
            ),
            book(
                "Harry Potter and the Goblet of Fire",
                &["J.K. Rowling"],
                None,
                None,
                Some(2000),
                Some(800),
            ),
            book(
                "Harry Potter and the Order of the Phoenix",
                &["J. K. Rowling"],
                Some("Harry Potter"),
                Some("5"),
                Some(2003),
                Some(700),
            ),
            book(
                "Harry Potter and the Half-Blood Prince",
                &["J. K. Rowling"],
                None,
                None,
                Some(2005),
                None,
            ),
            book(
                "Harry Potter and the Deathly Hallows",
                &["J. K. Rowling"],
                Some("Harry Potter"),
                Some("7"),
                Some(2007),
                Some(650),
            ),
            book(
                "Fantastic Beasts and Where to Find Them",
                &["J. K. Rowling"],
                None,
                None,
                Some(2001),
                Some(400),
            ),
        ];

        rank_books(&mut results, SearchKind::Any, "harry potter");

        let titles = titles(&results);
        let canonical_end = titles
            .iter()
            .position(|title| title.contains("Deathly Hallows"))
            .expect("canonical novel present");
        for collection in [
            "Harry Potter Complete Collection",
            "Harry Potter Box Set 1-7",
        ] {
            let position = titles.iter().position(|title| title == collection).unwrap();
            assert!(
                position > canonical_end,
                "{collection} must follow the canonical novels: {titles:?}"
            );
        }
        for companion in ["Harry Potter Quiz Book", "Harry Potter Study Guide"] {
            let position = titles.iter().position(|title| title == companion).unwrap();
            assert!(
                position > canonical_end,
                "{companion} must follow the canonical novels: {titles:?}"
            );
        }
        // The novels share the top block; series metadata is incomplete, so
        // year is the fallback for the ones without positions.
        let top: Vec<&str> = titles[0..8].iter().map(String::as_str).collect();
        assert!(top.contains(&"Harry Potter and the Philosopher's Stone"));
        assert!(top.contains(&"Harry Potter and the Chamber of Secrets"));
        assert!(top.contains(&"Harry Potter and the Half-Blood Prince"));
    }

    #[test]
    fn collection_query_reinstates_collections() {
        let mut results = vec![
            book(
                "Harry Potter Complete Collection",
                &["J. K. Rowling"],
                None,
                None,
                Some(2020),
                Some(9_000),
            ),
            book(
                "Harry Potter and the Philosopher's Stone",
                &["J. K. Rowling"],
                Some("Harry Potter"),
                Some("1"),
                Some(1997),
                Some(1_200),
            ),
        ];
        rank_books(&mut results, SearchKind::Any, "harry potter box set");
        assert_eq!(titles(&results)[0], "Harry Potter Complete Collection");
    }

    #[test]
    fn ambiguous_queries_are_not_reshaped_by_weak_author_evidence() {
        let mut results = vec![
            book("Home", &["Author A"], None, None, None, None),
            book("Homecoming", &["Author B"], None, None, None, None),
            book("At Home", &["Author A"], None, None, None, None),
            book("Home Again", &["Author C"], None, None, None, None),
            book("Long Way Home", &["Author A"], None, None, None, None),
        ];
        rank_books(&mut results, SearchKind::Any, "home");
        // Exact title wins; the rest keep a sensible relative order.
        assert_eq!(titles(&results)[0], "Home");
    }

    #[test]
    fn exact_title_beats_a_title_that_merely_contains_it() {
        let mut results = vec![
            book(
                "The Courage to Be Disliked",
                &["Ichiro Kishimi"],
                None,
                None,
                None,
                None,
            ),
            book(
                "Complete Courage to Be Disliked Duology Boxed Set",
                &["Ichiro Kishimi"],
                None,
                None,
                None,
                None,
            ),
        ];
        rank_books(
            &mut results,
            SearchKind::Title,
            "the courage to be disliked",
        );
        assert_eq!(titles(&results)[0], "The Courage to Be Disliked");
    }

    #[test]
    fn author_search_leads_with_that_authors_works() {
        let mut results = vec![
            book("Some Other Book", &["Someone Else"], None, None, None, None),
            book(
                "The Expanse",
                &["James S. A. Corey"],
                None,
                None,
                None,
                None,
            ),
            book(
                "Leviathan Wakes",
                &["James S. A. Corey"],
                None,
                None,
                None,
                None,
            ),
        ];
        rank_books(&mut results, SearchKind::Author, "james s a corey");
        assert_eq!(titles(&results)[0], "The Expanse");
        assert_eq!(titles(&results)[1], "Leviathan Wakes");
    }

    #[test]
    fn missing_popularity_never_buries_an_exact_match() {
        let mut results = vec![
            book("Dune", &["Frank Herbert"], None, None, Some(1965), None),
            book(
                "Dune: The Complete Saga",
                &["Frank Herbert"],
                None,
                None,
                Some(2019),
                Some(50_000),
            ),
        ];
        rank_books(&mut results, SearchKind::Title, "dune");
        assert_eq!(titles(&results)[0], "Dune");
    }

    #[test]
    fn popular_original_wins_over_repeated_same_title_spin_offs() {
        let mut results = vec![
            book(
                "Dune",
                &["Brian Herbert", "Kevin J. Anderson"],
                None,
                None,
                Some(2001),
                Some(37),
            ),
            book("Dune", &["Brian Herbert"], None, None, Some(2004), Some(19)),
            book("Dune", &["Brian Herbert"], None, None, Some(2008), Some(15)),
            book("Dune", &["Brian Herbert"], None, None, Some(2010), Some(8)),
            book(
                "Dune",
                &["Frank Herbert"],
                None,
                None,
                Some(1965),
                Some(4_402),
            ),
        ];
        rank_books(&mut results, SearchKind::Any, "dune");
        assert_eq!(results[0].authors, vec!["Frank Herbert"]);
    }

    #[test]
    fn optional_leading_article_does_not_bury_original_title() {
        let mut results = vec![
            book(
                "Game of Thrones",
                &["Book Of Thrones"],
                None,
                None,
                None,
                Some(259),
            ),
            book(
                "A Game of Thrones",
                &["George R. R. Martin"],
                None,
                None,
                Some(1996),
                Some(13_459),
            ),
        ];
        rank_books(&mut results, SearchKind::Any, "game of thrones");
        assert_eq!(results[0].title, "A Game of Thrones");
    }

    #[test]
    fn multilingual_results_rank_on_title_not_language() {
        let mut results = vec![
            book(
                "Harry Potter och de vises sten",
                &["J. K. Rowling"],
                None,
                None,
                Some(1997),
                None,
            ),
            book(
                "Harry Potter and the Philosopher's Stone",
                &["J. K. Rowling"],
                None,
                None,
                Some(1997),
                None,
            ),
        ];
        results[0].languages = vec!["sv".to_string()];
        results[1].languages = vec!["en".to_string()];
        rank_books(&mut results, SearchKind::Any, "harry potter");
        // Both are prefix matches by the same author; provider order holds.
        assert_eq!(titles(&results)[0], "Harry Potter och de vises sten");
        rank_books(&mut results, SearchKind::Any, "harry potter svenska");
        assert_eq!(titles(&results)[0], "Harry Potter och de vises sten");
    }

    #[test]
    fn position_parsing_is_conservative() {
        assert_eq!(series_position(Some("3")), Some(3));
        assert_eq!(series_position(Some("3.5")), Some(3));
        assert_eq!(series_position(Some("Book 2")), None);
        assert_eq!(series_position(Some("0")), None);
        assert_eq!(series_position(Some("9999")), None);
        assert_eq!(series_position(None), None);
    }

    #[test]
    fn book_ranges_read_as_collections() {
        assert!(has_book_range("harry potter books 1-7"));
        assert!(has_book_range("complete novels 1\u{2013}5"));
        assert!(!has_book_range("harry potter and the half blood prince"));
        assert!(!has_book_range("room 101"));
    }

    #[test]
    fn provider_order_is_the_final_tie_breaker() {
        let mut results = vec![
            book("Love and War", &["Author A"], None, None, None, None),
            book("Love and Peace", &["Author B"], None, None, None, None),
            book("Love and Time", &["Author A"], None, None, None, None),
            book("Love and Space", &["Author B"], None, None, None, None),
        ];
        let before = titles(&results);
        rank_books(&mut results, SearchKind::Any, "love");
        // All are equal prefix tokens with no author evidence; provider order
        // is preserved.
        assert_eq!(titles(&results), before);
    }

    #[test]
    fn metadata_and_book_ranking_agree() {
        let metadata = vec![
            MetadataResult {
                title: "Harry Potter Quiz Book".to_string(),
                authors: vec!["Quiz Author".to_string()],
                ..Default::default()
            },
            MetadataResult {
                title: "Harry Potter and the Philosopher's Stone".to_string(),
                authors: vec!["J. K. Rowling".to_string()],
                series: Some("Harry Potter".to_string()),
                series_number: Some("1".to_string()),
                ..Default::default()
            },
            MetadataResult {
                title: "Harry Potter and the Chamber of Secrets".to_string(),
                authors: vec!["J. K. Rowling".to_string()],
                series: Some("Harry Potter".to_string()),
                series_number: Some("2".to_string()),
                ..Default::default()
            },
        ];
        let mut metadata = metadata;
        rank_metadata(&mut metadata, SearchKind::Any, "harry potter");
        let names: Vec<&str> = metadata.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Harry Potter and the Philosopher's Stone",
                "Harry Potter and the Chamber of Secrets",
                "Harry Potter Quiz Book",
            ]
        );
    }

    #[test]
    fn series_positions_order_equal_scores() {
        let mut results = vec![
            book(
                "Series Book Three",
                &["Author"],
                Some("The Series"),
                Some("3"),
                Some(2010),
                None,
            ),
            book(
                "Series Book One",
                &["Author"],
                Some("The Series"),
                Some("1"),
                Some(2001),
                None,
            ),
            book(
                "Series Book Two",
                &["Author"],
                Some("The Series"),
                Some("2"),
                Some(2005),
                None,
            ),
        ];
        rank_books(&mut results, SearchKind::Any, "series book");
        assert_eq!(
            titles(&results),
            vec!["Series Book One", "Series Book Two", "Series Book Three"]
        );
    }
}
