use bokhylle_acquisition::evaluator;
use bokhylle_acquisition::model::{
    EvaluatedRelease, ExpectedBook, RejectionReason, ReleaseCandidate, Selection,
};

fn candidate(title: &str, size: i64, seeders: i64) -> ReleaseCandidate {
    ReleaseCandidate {
        source: None,
        method: None,
        id: title.to_string(),
        title: title.to_string(),
        indexer: Some("fixture".to_string()),
        size_bytes: size,
        seeders: Some(seeders),
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

fn hail_mary() -> ExpectedBook {
    ExpectedBook {
        title: "Project Hail Mary".to_string(),
        authors: vec!["Andy Weir".to_string()],
        year: Some(2021),
        isbn: Some("9780593135204".to_string()),
        language: Some("en".to_string()),
        languages: Vec::new(),
        preferred_format: Some("epub".to_string()),
        series_number: None,
    }
}

fn dataset() -> Vec<ReleaseCandidate> {
    vec![
        candidate("Andy.Weir.Project.Hail.Mary.RETAIL.EPUB", 2_900_000, 12),
        candidate("Project.Hail.Mary.Andy.Weir.EPUB", 3_100_000, 5),
        candidate("Project Hail Mary PDF", 14_000_000, 7),
        candidate("Andy Weir Complete Collection EPUB MOBI", 840_000_000, 20),
        candidate("Project Hail Mary German EPUB", 2_500_000, 3),
        candidate("Project Hail Mary Audiobook M4B", 300_000_000, 9),
        candidate("Project.Hail.Mary.2021.epub", 2_200_000, 2),
        candidate("Andy.Weir.Project.Hail.Mary.Retail.EN.EPUB", 3_000_000, 15),
        candidate("Project Hail Mary + Artemis Collection EPUB", 5_000_000, 4),
    ]
}

fn find<'a>(evaluated: &'a [EvaluatedRelease], title: &str) -> &'a EvaluatedRelease {
    evaluated
        .iter()
        .find(|release| release.candidate.title == title)
        .expect("candidate present")
}

#[test]
fn rejects_unsuitable_candidates() {
    let book = hail_mary();
    let evaluated = evaluator::rank(&book, &dataset());

    assert_eq!(
        find(&evaluated, "Andy Weir Complete Collection EPUB MOBI").rejection_reasons,
        vec![RejectionReason::UnrelatedTitle]
    );
    assert_eq!(
        find(&evaluated, "Project Hail Mary German EPUB").rejection_reasons,
        vec![RejectionReason::LanguageMismatch]
    );
    assert_eq!(
        find(&evaluated, "Project Hail Mary Audiobook M4B").rejection_reasons,
        vec![RejectionReason::Audiobook]
    );

    assert!(!find(&evaluated, "Project Hail Mary PDF").rejected());
    assert!(!find(&evaluated, "Project Hail Mary + Artemis Collection EPUB").rejected());
}

#[test]
fn format_preference_outranks_language_order() {
    let mut book = hail_mary();
    book.languages = vec!["en".to_string(), "sv".to_string()];
    book.preferred_format = Some("any".to_string());

    let evaluated = evaluator::rank(
        &book,
        &[
            candidate("Project Hail Mary SWEDISH EPUB", 3_000_000, 80),
            candidate("Project Hail Mary ENGLISH EPUB", 3_000_000, 2),
            candidate("Project Hail Mary ENGLISH PDF", 14_000_000, 80),
            candidate("Project Hail Mary SWEDISH PDF", 14_000_000, 80),
        ],
    );

    let order: Vec<&str> = evaluated
        .iter()
        .map(|release| release.candidate.title.as_str())
        .collect();
    assert_eq!(
        order,
        vec![
            "Project Hail Mary ENGLISH EPUB",
            "Project Hail Mary SWEDISH EPUB",
            "Project Hail Mary ENGLISH PDF",
            "Project Hail Mary SWEDISH PDF",
        ],
        "EPUB wins before language order, so a Swedish EPUB beats an English PDF"
    );
    assert!(
        evaluated.iter().all(|release| !release.rejected()),
        "all four are acceptable in any-compatible mode"
    );
}

#[test]
fn zero_seeder_releases_are_a_last_resort() {
    let book = hail_mary();
    let dead = evaluator::evaluate(
        &book,
        &candidate("Project.Hail.Mary.Andy.Weir.EPUB", 3_100_000, 0),
    );
    let seeded = evaluator::evaluate(
        &book,
        &candidate("Andy.Weir.Project.Hail.Mary.Retail.EN.EPUB", 2_900_000, 12),
    );

    let with_alternative = vec![dead, seeded];
    assert!(
        matches!(
            evaluator::select(&with_alternative),
            Selection::Auto { index: 1 }
        ),
        "a seeded release must win over a dead one"
    );

    let lone_dead = vec![evaluator::evaluate(
        &book,
        &candidate("Project.Hail.Mary.Andy.Weir.EPUB", 3_100_000, 0),
    )];
    assert!(
        matches!(evaluator::select(&lone_dead), Selection::Auto { index: 0 }),
        "a lone zero-seeder release is still selectable as a last resort"
    );
}

#[test]
fn ranks_best_candidate_first_and_selects_it() {
    let book = hail_mary();
    let evaluated = evaluator::rank(&book, &dataset());

    assert_eq!(
        evaluated[0].candidate.title,
        "Andy.Weir.Project.Hail.Mary.Retail.EN.EPUB"
    );
    assert_eq!(evaluated[0].score, 145);
    assert!((evaluated[0].confidence - 1.0).abs() < f32::EPSILON);

    assert!(matches!(
        evaluator::select(&evaluated),
        Selection::Auto { index: 0 }
    ));
}

#[test]
fn score_reasons_are_explainable() {
    let book = hail_mary();
    let release = evaluator::evaluate(
        &book,
        &candidate("Andy.Weir.Project.Hail.Mary.Retail.EN.EPUB", 3_000_000, 15),
    );

    let weights: Vec<(i32, &str)> = release
        .score_reasons
        .iter()
        .map(|reason| (reason.weight, reason.reason.as_str()))
        .collect();

    assert!(weights.contains(&(40, "strong title match")));
    assert!(weights.contains(&(30, "author match")));
    assert!(weights.contains(&(25, "preferred format")));
    assert!(weights.contains(&(20, "preferred language")));
    assert!(weights.contains(&(10, "retail indicator")));
    assert!(weights.contains(&(5, "healthy seed count")));
    assert_eq!(release.candidate.detected_format.as_deref(), Some("epub"));
    assert_eq!(release.candidate.detected_language.as_deref(), Some("en"));
    assert!(release.candidate.detected_title.is_some());
    assert!(release.candidate.detected_author.is_some());
}

#[test]
fn partial_title_match_scores_lower() {
    let book = hail_mary();
    let release = evaluator::evaluate(&book, &candidate("Hail Mary EPUB", 1_500_000, 1));

    let weights: Vec<(i32, &str)> = release
        .score_reasons
        .iter()
        .map(|reason| (reason.weight, reason.reason.as_str()))
        .collect();

    assert!(weights.contains(&(20, "partial title match")));
    assert!(!release.rejected());
}

#[test]
fn non_contiguous_title_tokens_without_author_are_rejected() {
    let book = hail_mary();
    let release = evaluator::evaluate(&book, &candidate("Project Mary EPUB", 1_500_000, 1));

    let weights: Vec<(i32, &str)> = release
        .score_reasons
        .iter()
        .map(|reason| (reason.weight, reason.reason.as_str()))
        .collect();

    assert!(!weights.contains(&(40, "strong title match")));
    assert!(!weights.contains(&(20, "partial title match")));
    assert!(release.rejected());
    assert!(
        release
            .rejection_reasons
            .contains(&RejectionReason::UnrelatedTitle)
    );
}

fn dune() -> ExpectedBook {
    ExpectedBook {
        title: "Dune".to_string(),
        authors: vec!["Frank Herbert".to_string()],
        year: None,
        isbn: None,
        language: Some("en".to_string()),
        languages: Vec::new(),
        preferred_format: Some("epub".to_string()),
        series_number: None,
    }
}

#[test]
fn single_word_titles_do_not_match_longer_titles() {
    let book = dune();
    let releases: Vec<EvaluatedRelease> = [
        "Frank Herbert - Dune Messiah [EPUB]",
        "Frank Herbert - Dune Messiah Retail EPUB",
        "Frank Herbert - Children of Dune [EPUB]",
    ]
    .iter()
    .map(|title| evaluator::evaluate(&book, &candidate(title, 2_000_000, 10)))
    .collect();

    for release in &releases {
        assert!(
            release.rejected(),
            "{} should be rejected",
            release.candidate.title
        );
    }
    assert_eq!(evaluator::select(&releases), Selection::None);
}

#[test]
fn single_word_titles_still_match_exact_releases() {
    let book = dune();
    for title in [
        "Frank Herbert - Dune [EPUB]",
        "Frank.Herbert.Dune.Retail.EN.EPUB",
        "Frank Herbert - Dune (Dune #1) [EPUB]",
        "Frank Herbert Dune 1965 EPUB",
    ] {
        let release = evaluator::evaluate(&book, &candidate(title, 2_000_000, 10));
        assert!(
            !release.rejected(),
            "{title} rejected: {:?}",
            release.rejection_reasons
        );
        assert!(
            release
                .score_reasons
                .iter()
                .any(|reason| reason.weight == 40),
            "{title} has no strong title match"
        );
        assert!(
            release.confidence >= 0.85,
            "{title}: {}",
            release.confidence
        );
    }
}

#[test]
fn a_named_different_book_by_the_same_author_is_rejected() {
    let book = ExpectedBook {
        title: "Clean Code".to_string(),
        authors: vec!["Robert C. Martin".to_string()],
        year: None,
        isbn: None,
        language: Some("en".to_string()),
        languages: Vec::new(),
        preferred_format: Some("pdf".to_string()),
        series_number: None,
    };
    let release = candidate(
        "Robert C Martin - The Clean Coder- A Code of Conduct for Professional Programmers (pdf)",
        6_176_112,
        3,
    );

    let evaluated = evaluator::evaluate(&book, &release);

    assert!(
        evaluated
            .rejection_reasons
            .contains(&RejectionReason::UnrelatedTitle)
    );
    assert!(
        !evaluated
            .score_reasons
            .iter()
            .any(|reason| reason.weight == 40)
    );
    assert!(evaluated.confidence < 0.85);
    assert_eq!(evaluator::select(&[evaluated]), Selection::None);
}

#[test]
fn historian_search_keeps_the_ebook_and_rejects_unrelated_large_results() {
    let book = ExpectedBook {
        title: "The Historian".to_string(),
        authors: vec!["Elizabeth Kostova".to_string()],
        preferred_format: Some("epub".to_string()),
        ..Default::default()
    };
    let evaluated = evaluator::rank(
        &book,
        &[
            candidate(
                "TheGreatCoursesPlus - A Historian Goes To The Movies Ancient Rome 2019 BOOKWARE-LERNSTUF",
                5_880_000_000,
                5,
            ),
            candidate(
                "Stephen E Ambrose To America Personal Reflections Of An Historian 2002 RETAiL ePub eBook-LiBRiCiDE",
                2_000_000,
                3,
            ),
            candidate(
                "Elizabeth Kostova - The Historian (azw3 epub mobi)",
                3_000_000,
                4,
            ),
        ],
    );

    let exact = &evaluated[0];
    assert_eq!(exact.candidate.detected_format.as_deref(), Some("epub"));
    assert!(!exact.rejected());
    assert_eq!(evaluator::select(&evaluated), Selection::Auto { index: 0 });

    let large = find(
        &evaluated,
        "TheGreatCoursesPlus - A Historian Goes To The Movies Ancient Rome 2019 BOOKWARE-LERNSTUF",
    );
    assert!(
        large
            .rejection_reasons
            .contains(&RejectionReason::OversizedRelease)
    );
    assert!(
        large
            .rejection_reasons
            .contains(&RejectionReason::UnrelatedTitle)
    );
    assert!(
        find(
            &evaluated,
            "Stephen E Ambrose To America Personal Reflections Of An Historian 2002 RETAiL ePub eBook-LiBRiCiDE"
        )
        .rejection_reasons
        .contains(&RejectionReason::UnrelatedTitle)
    );
}

#[test]
fn oversized_release_is_rejected_even_with_exact_title_and_author() {
    let evaluated = evaluator::evaluate(
        &hail_mary(),
        &candidate("Andy Weir Project Hail Mary EPUB", 5_000_000_000, 10),
    );
    assert!(
        evaluated
            .rejection_reasons
            .contains(&RejectionReason::OversizedRelease)
    );
}

#[test]
fn wrong_volume_is_rejected() {
    let mut book = hail_mary();
    book.series_number = Some("2".to_string());

    let release = evaluator::evaluate(
        &book,
        &candidate("Andy.Weir.Project.Hail.Mary.Book.3.EPUB", 3_000_000, 8),
    );

    assert_eq!(release.candidate.detected_volume.as_deref(), Some("3"));
    assert!(
        release
            .rejection_reasons
            .contains(&RejectionReason::WrongVolume)
    );
    assert_eq!(evaluator::select(&[release]), Selection::None);
}

fn alchemy() -> ExpectedBook {
    ExpectedBook {
        title: "Alchemy".into(),
        authors: vec!["Diana Fernando".into()],
        isbn: Some("9780713726688".into()),
        language: Some("en".into()),
        ..Default::default()
    }
}

#[test]
fn same_title_with_another_author_is_excluded() {
    for title in [
        "Rory.Sutherland.Alchemy.EN.EPUB",
        "Rory Sutherland - Alchemy [EPUB]",
        "Alchemy [EPUB] - Rory Sutherland",
        "Alchemy [EPUB] by Rory Sutherland",
    ] {
        let release = evaluator::evaluate(&alchemy(), &candidate(title, 2_000_000, 50));
        assert!(
            release
                .rejection_reasons
                .contains(&RejectionReason::AuthorMismatch),
            "{title}"
        );
        assert!(!evaluator::is_confident_match(&release));
        assert_eq!(evaluator::select(&[release]), Selection::None);
    }
}

#[test]
fn missing_author_needs_review_even_when_it_is_the_only_seeded_result() {
    let release = evaluator::evaluate(&alchemy(), &candidate("Alchemy [EN] EPUB", 2_000_000, 200));
    assert!(!release.rejected());
    assert!(!evaluator::is_confident_match(&release));
    assert_eq!(evaluator::select(&[release]), Selection::NeedsSelection);
}

#[test]
fn title_and_author_or_exact_isbn_can_be_recommended() {
    for title in [
        "Diana Fernando - Alchemy [EN] EPUB",
        "9780713726688 [EN] EPUB",
    ] {
        let release = evaluator::evaluate(&alchemy(), &candidate(title, 2_000_000, 10));
        assert!(
            !release.rejected(),
            "{title}: {:?}",
            release.rejection_reasons
        );
        assert!(evaluator::is_confident_match(&release), "{title}");
        assert_eq!(evaluator::select(&[release]), Selection::Auto { index: 0 });
    }
}

#[test]
fn matching_identity_outranks_a_possible_match_in_a_preferred_format() {
    let ranked = evaluator::rank(
        &alchemy(),
        &[
            candidate("Alchemy [EN] EPUB", 2_000_000, 200),
            candidate("Diana Fernando - Alchemy [EN] PDF", 4_000_000, 5),
        ],
    );
    assert!(evaluator::is_confident_match(&ranked[0]));
    assert_eq!(ranked[0].candidate.detected_format.as_deref(), Some("pdf"));
    assert_eq!(evaluator::select(&ranked), Selection::Auto { index: 0 });
}

#[test]
fn structured_conflicting_author_is_rejected() {
    let mut row = candidate("Alchemy [EN] EPUB", 2_000_000, 30);
    row.detected_author = Some("Rory Sutherland".into());
    assert!(
        evaluator::evaluate(&alchemy(), &row)
            .rejection_reasons
            .contains(&RejectionReason::AuthorMismatch)
    );
}

#[test]
fn abbreviated_authors_remain_possible_matches() {
    for title in ["D Fernando - Alchemy [EPUB]", "D.Fernando.Alchemy.EPUB"] {
        let release = evaluator::evaluate(&alchemy(), &candidate(title, 2_000_000, 30));
        assert!(
            !release.rejected(),
            "{title}: {:?}",
            release.rejection_reasons
        );
        assert!(!evaluator::is_confident_match(&release));
        assert_eq!(evaluator::select(&[release]), Selection::NeedsSelection);
    }
}

#[test]
fn historical_order_does_not_recommend_a_possible_match_ahead_of_known_identity() {
    let releases = [
        evaluator::evaluate(&alchemy(), &candidate("Alchemy EPUB", 2_000_000, 100)),
        evaluator::evaluate(
            &alchemy(),
            &candidate("Diana Fernando - Alchemy EPUB", 2_000_000, 5),
        ),
    ];
    assert_eq!(evaluator::select(&releases), Selection::Auto { index: 1 });
}

#[test]
fn seeded_unknown_identity_does_not_replace_a_known_book() {
    let releases = [
        evaluator::evaluate(&alchemy(), &candidate("Alchemy EPUB", 2_000_000, 100)),
        evaluator::evaluate(
            &alchemy(),
            &candidate("Diana Fernando - Alchemy EPUB", 2_000_000, 0),
        ),
    ];
    assert_eq!(evaluator::select(&releases), Selection::Auto { index: 1 });
}

#[test]
fn decimal_series_numbers_are_preserved_when_comparing_volumes() {
    let mut book = hail_mary();
    book.series_number = Some("6.5".into());
    for title in [
        "Andy Weir - Project Hail Mary Book 6.5 EPUB",
        "Andy Weir - Project Hail Mary (#6.5) EPUB",
    ] {
        let release = evaluator::evaluate(&book, &candidate(title, 2_000_000, 5));
        assert_eq!(release.candidate.detected_volume.as_deref(), Some("6.5"));
        assert!(
            !release
                .rejection_reasons
                .contains(&RejectionReason::WrongVolume)
        );
    }
}

#[test]
fn ambiguous_medium_confidence_needs_selection() {
    let book = hail_mary();
    let candidates = vec![
        candidate("Project.Hail.Mary.2021.epub", 2_200_000, 2),
        candidate("Project.Hail.Mary.2021.epub.release", 2_240_000, 2),
    ];

    let evaluated = evaluator::rank(&book, &candidates);
    for release in &evaluated {
        assert!(!release.rejected());
        assert!((release.confidence - 0.6).abs() < 0.001);
    }

    assert_eq!(evaluator::select(&evaluated), Selection::NeedsSelection);
}
