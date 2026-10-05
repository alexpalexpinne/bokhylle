use bokhylle_acquisition::evaluator;
use bokhylle_acquisition::model::{ExpectedBook, RejectionReason, ReleaseCandidate, Selection};

fn book(title: &str, author: &str) -> ExpectedBook {
    ExpectedBook {
        title: title.into(),
        authors: vec![author.into()],
        languages: vec!["en".into()],
        ..Default::default()
    }
}

fn release(name: &str) -> ReleaseCandidate {
    ReleaseCandidate {
        id: name.into(),
        title: name.into(),
        size_bytes: 3_000_000,
        seeders: Some(50),
        ..Default::default()
    }
}

fn confident(book: &ExpectedBook, name: &str) {
    let evaluated = evaluator::evaluate(book, &release(name));
    assert!(
        !evaluated.rejected(),
        "{name}: {:?}",
        evaluated.rejection_reasons
    );
    assert!(
        evaluator::is_confident_match(&evaluated),
        "{name}: {:?}",
        evaluated.score_reasons
    );
    assert_eq!(
        evaluator::select(&[evaluated]),
        Selection::Auto { index: 0 },
        "{name}"
    );
}

fn review(book: &ExpectedBook, name: &str) {
    let evaluated = evaluator::evaluate(book, &release(name));
    assert!(
        !evaluated.rejected(),
        "{name}: {:?}",
        evaluated.rejection_reasons
    );
    assert!(!evaluator::is_confident_match(&evaluated), "{name}");
    assert_eq!(
        evaluator::select(&[evaluated]),
        Selection::NeedsSelection,
        "{name}"
    );
}

fn rejected(book: &ExpectedBook, name: &str, reason: RejectionReason) {
    let evaluated = evaluator::evaluate(book, &release(name));
    assert!(
        evaluated.rejection_reasons.contains(&reason),
        "{name}: {:?}",
        evaluated.rejection_reasons
    );
    assert_eq!(evaluator::select(&[evaluated]), Selection::None, "{name}");
}

#[test]
fn identity_words_are_not_language_or_audio_tags() {
    for (title, author) in [
        ("Norwegian Wood", "Haruki Murakami"),
        ("The English Patient", "Michael Ondaatje"),
        ("The Audible Past", "Jonathan Sterne"),
    ] {
        let mut expected = book(title, author);
        confident(&expected, &format!("{author} - {title} EPUB"));
        confident(&expected, &format!("{author} - {title} [EN] EPUB"));
        rejected(
            &expected,
            &format!("{author} - {title} [SV] EPUB"),
            RejectionReason::LanguageMismatch,
        );
        expected.languages = vec!["sv".into()];
        confident(&expected, &format!("{author} - {title} [SV] EPUB"));
    }
}

#[test]
fn bracketed_short_titles_do_not_override_explicit_language_tags() {
    let expected = book("It", "Stephen King");
    for name in [
        "Stephen King - [It] EPUB",
        "Stephen King - [IT] [EN] EPUB",
        "Stephen King - (It) LANG-EN EPUB",
        "Stephen King - It [EN] EPUB",
    ] {
        confident(&expected, name);
    }
    for name in [
        "Stephen King - It [IT] EPUB",
        "Stephen King - [It] [IT] EPUB",
        "Stephen King - It LANG-IT EPUB",
    ] {
        rejected(&expected, name, RejectionReason::LanguageMismatch);
    }
}

#[test]
fn volume_padding_does_not_change_the_number() {
    let mut expected = book("Amber Voyage", "Robin Example");
    expected.series_number = Some("3".into());
    for name in [
        "Robin Example - Amber Voyage Book 03 EPUB",
        "Robin Example - Amber Voyage (#003) EPUB",
        "Robin Example - Amber Voyage Book 3 (#003) EPUB",
    ] {
        confident(&expected, name);
    }
    expected.series_number = Some("6.5".into());
    confident(&expected, "Robin Example - Amber Voyage Book 06.50 EPUB");
    rejected(
        &expected,
        "Robin Example - Amber Voyage Book 06 EPUB",
        RejectionReason::WrongVolume,
    );
}

#[test]
fn contradictory_volume_labels_are_never_automatically_selected() {
    let mut expected = book("Amber Voyage", "Robin Example");
    expected.series_number = Some("3".into());
    for name in [
        "Robin Example - Amber Voyage Book 3 (#4) EPUB",
        "Robin Example - Amber Voyage (#3) Volume 4 EPUB",
        "Robin Example - Amber Voyage Book 4 (#3) EPUB",
    ] {
        rejected(&expected, name, RejectionReason::WrongVolume);
    }
}

#[test]
fn isbn_punctuation_preserves_identity_without_creating_a_pack() {
    let mut expected = book("Amber Voyage", "Robin Example");
    expected.isbn = Some("9781250871992".into());
    for isbn in [
        "978-1-250-87199-2",
        "978–1–250–87199–2",
        "978—1—250—87199—2",
    ] {
        let name = format!("Robin Example - Amber Voyage [ISBN {isbn}] EPUB");
        let evaluated = evaluator::evaluate(&expected, &release(&name));
        assert!(!evaluated.candidate.is_collection, "{name}");
        confident(&expected, &name);
        confident(&expected, &format!("{isbn} EPUB"));
    }
    rejected(
        &expected,
        "978-1-250-87199-3 EPUB",
        RejectionReason::UnrelatedTitle,
    );
}

#[test]
fn zero_based_and_reverse_ranges_are_still_packs() {
    let expected = book("Amber Voyage", "Robin Example");
    for name in [
        "Robin Example - [Amber Voyage 00-03] (epub)",
        "Robin Example - [Amber Voyage 03–01] (epub)",
        "Robin Example - [Amber Voyage 01—03] (epub)",
    ] {
        let evaluated = evaluator::evaluate(&expected, &release(name));
        assert!(evaluated.candidate.is_collection, "{name}");
        review(&expected, name);
    }
}

#[test]
fn a_requested_collection_still_requires_review() {
    let expected = book("Amber Voyage Complete Collection", "Robin Example");
    review(
        &expected,
        "Robin Example - Amber Voyage Complete Collection EPUB",
    );
}

#[test]
fn study_guides_and_summaries_are_different_works() {
    let expected = book("Amber Voyage", "Robin Example");
    for name in [
        "Robin Example - Amber Voyage - A Study Guide EPUB",
        "Robin Example - Amber Voyage - Summary EPUB",
        "Robin Example - Amber Voyage - Workbook EPUB",
    ] {
        rejected(&expected, name, RejectionReason::UnrelatedTitle);
    }
    confident(
        &book("Amber Voyage Study Guide", "Robin Example"),
        "Robin Example - Amber Voyage Study Guide EPUB",
    );
    rejected(
        &book("Amber Voyage: A Study Guide", "Robin Example"),
        "Robin Example - Amber Voyage EPUB",
        RejectionReason::UnrelatedTitle,
    );
}

#[test]
fn initials_require_review_and_a_different_full_name_is_rejected() {
    let expected = book("Amber Voyage", "Robin S. Example");
    for name in [
        "R S Example - Amber Voyage EPUB",
        "Robin Example - Amber Voyage EPUB",
    ] {
        review(&expected, name);
    }
    rejected(
        &expected,
        "Robert Example - Amber Voyage EPUB",
        RejectionReason::AuthorMismatch,
    );
    confident(&expected, "Robin S Example - Amber Voyage EPUB");
}

#[test]
fn format_words_in_the_title_are_not_evidence_of_the_file_type() {
    let expected = book("EPUB Secrets", "Robin Example");
    review(&expected, "Robin Example - EPUB Secrets");
    confident(&expected, "Robin Example - EPUB Secrets PDF");
    rejected(
        &expected,
        "Robin Example - EPUB Secrets MOBI",
        RejectionReason::UnsupportedFormat,
    );
}

#[test]
fn unsupported_formats_and_audiobooks_cannot_win_on_seeders() {
    let expected = book("Amber Voyage", "Robin Example");
    for format in ["MOBI", "AZW3", "FB2", "DJVU", "CBR"] {
        rejected(
            &expected,
            &format!("Robin Example - Amber Voyage {format}"),
            RejectionReason::UnsupportedFormat,
        );
    }
    for suffix in ["Audiobook M4B", "EPUB MP3", "Audible EPUB"] {
        rejected(
            &expected,
            &format!("Robin Example - Amber Voyage {suffix}"),
            RejectionReason::Audiobook,
        );
    }
    for formats in ["MOBI EPUB", "AZW3 PDF EPUB"] {
        confident(
            &expected,
            &format!("Robin Example - Amber Voyage ({formats})"),
        );
    }
}

#[test]
fn matching_isbn_cannot_override_structured_identity_conflicts() {
    let mut expected = book("Amber Voyage", "Robin Example");
    expected.isbn = Some("9781250871992".into());
    for (author, title, reason) in [
        (
            Some("Robert Example"),
            None,
            RejectionReason::AuthorMismatch,
        ),
        (
            None,
            Some("A Different Novel"),
            RejectionReason::UnrelatedTitle,
        ),
    ] {
        let mut row = release("Robin Example - Amber Voyage 9781250871992 EPUB");
        row.detected_author = author.map(str::to_string);
        row.detected_title = title.map(str::to_string);
        let evaluated = evaluator::evaluate(&expected, &row);
        assert!(
            evaluated.rejection_reasons.contains(&reason),
            "{:?}",
            evaluated.rejection_reasons
        );
        assert_eq!(evaluator::select(&[evaluated]), Selection::None);
    }
}
