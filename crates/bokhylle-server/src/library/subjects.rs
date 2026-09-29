use bokhylle_core::identity::normalize_text;

const NOISE_EXACT: &[&str] = &[
    "fiction",
    "novel",
    "novels",
    "literature",
    "book",
    "books",
    "prose",
    "reading",
    "general",
    "miscellaneous",
    "accessible book",
    "protected daisy",
    "large type books",
    "large print",
    "bestseller",
    "bestsellers",
    "new york times bestseller",
    "translations",
    "american literature",
    "english literature",
    "juvenile literature",
    "in library",
    "overdrive",
    "fiction in english",
    "american fiction",
    "american science fiction",
    "english fiction",
    "general fiction",
    "romans nouvelles",
];

const NOISE_PREFIXES: &[&str] = &[
    "translations into",
    "large type",
    "protected daisy",
    "accessible book",
    "new york times bestseller",
    "reading level",
    "electronic books",
    "internet archive",
    "fiction ",
    "nonfiction ",
    "romans ",
    "nouvelles ",
];

const BROAD: &[&str] = &[
    "science fiction",
    "fantasy",
    "thriller",
    "mystery",
    "romance",
    "historical fiction",
    "fantasy fiction",
    "horror",
    "biography",
    "autobiography",
    "adventure",
    "crime",
    "suspense",
    "young adult fiction",
    "juvenile fiction",
    "children's fiction",
    "humor",
    "poetry",
    "drama",
    "history",
    "politics",
    "philosophy",
    "religion",
    "psychology",
    "business",
    "self-help",
    "sociology",
    "science",
    "nature",
    "travel",
    "cooking",
    "health",
    "art",
    "music",
    "film",
    "sports",
    "education",
    "reference",
    "essays",
    "short stories",
    "classics",
    "contemporary",
    "paranormal",
    "dystopia",
    "supernatural",
    "detective and mystery stories",
    "detective and mystery fiction",
];

pub fn normalized(name: &str) -> String {
    normalize_text(name)
}

fn is_noise(normalized: &str) -> bool {
    NOISE_EXACT.contains(&normalized)
        || NOISE_PREFIXES
            .iter()
            .any(|prefix| normalized.starts_with(prefix))
}

/// Similarity weight for a shared subject: `None` for catalog noise,
/// 1 for broad genres, 3 for plain subjects and 5 for specific ones.
pub fn similarity_weight(normalized: &str) -> Option<u8> {
    if normalized.is_empty() || is_noise(normalized) {
        return None;
    }
    let base = strip_qualifiers(normalized);
    if BROAD.contains(&base) {
        return Some(1);
    }
    if base.split_whitespace().count() >= 2 {
        Some(5)
    } else {
        Some(3)
    }
}

/// "Fantasy fiction, American" and "Fiction, fantasy, general" are the same
/// concepts as their base forms for similarity purposes.
fn strip_qualifiers(normalized: &str) -> &str {
    let mut base = normalized;
    loop {
        let mut stripped = base;
        for suffix in [" american", " english", " general", " in english"] {
            if let Some(rest) = base.strip_suffix(suffix) {
                stripped = rest;
                break;
            }
        }
        if stripped == base || stripped.is_empty() {
            return base;
        }
        base = stripped;
    }
}

/// Subjects worth showing as chips/facets (noise filtered out).
pub fn is_displayable(normalized: &str) -> bool {
    !normalized.is_empty() && !is_noise(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_catalog_noise_broad_and_specific_subjects() {
        assert_eq!(similarity_weight("fiction"), None);
        assert_eq!(similarity_weight("fiction fantasy general"), None);
        assert_eq!(similarity_weight("fiction science fiction general"), None);
        assert_eq!(similarity_weight("fiction in english"), None);
        assert_eq!(similarity_weight("romans nouvelles"), None);
        assert_eq!(similarity_weight("large type books"), None);

        assert_eq!(similarity_weight("fantasy"), Some(1));
        assert_eq!(similarity_weight("fantasy fiction"), Some(1));
        assert_eq!(similarity_weight("fantasy fiction american"), Some(1));
        assert_eq!(similarity_weight("science fiction"), Some(1));

        assert_eq!(similarity_weight("magic"), Some(3));
        assert_eq!(similarity_weight("space opera"), Some(5));
        assert_eq!(similarity_weight("good and evil"), Some(5));
    }

    #[test]
    fn display_filter_hides_noise_only() {
        assert!(!is_displayable("fiction fantasy general"));
        assert!(!is_displayable("romans nouvelles"));
        assert!(is_displayable("fantasy fiction american"));
    }
}
