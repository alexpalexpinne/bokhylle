use std::process::ExitCode;

use bokhylle_acquisition::evaluator;
use bokhylle_acquisition::model::{ExpectedBook, Selection};
use bokhylle_acquisition::prowlarr::ProwlarrClient;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut book = ExpectedBook::default();
    let mut json = false;

    let mut iter = args.iter();
    while let Some(argument) = iter.next() {
        match argument.as_str() {
            "--title" => book.title = next_value(&mut iter, "--title"),
            "--author" => book.authors.push(next_value(&mut iter, "--author")),
            "--isbn" => book.isbn = Some(next_value(&mut iter, "--isbn")),
            "--language" => book.language = Some(next_value(&mut iter, "--language")),
            "--format" => book.preferred_format = Some(next_value(&mut iter, "--format")),
            "--series-number" => {
                book.series_number = Some(next_value(&mut iter, "--series-number"))
            }
            "--year" => {
                book.year = next_value(&mut iter, "--year").parse().ok();
            }
            "--json" => json = true,
            "--help" | "-h" => {
                usage();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument: {other}");
                usage();
                return ExitCode::from(2);
            }
        }
    }

    if book.title.trim().is_empty() {
        eprintln!("--title is required");
        usage();
        return ExitCode::from(2);
    }

    let base_url =
        std::env::var("PROWLARR_URL").unwrap_or_else(|_| "http://localhost:9696".to_string());
    let Ok(api_key) = std::env::var("PROWLARR_API_KEY") else {
        eprintln!("PROWLARR_API_KEY is not set");
        return ExitCode::from(2);
    };

    let client = match ProwlarrClient::new(&base_url, &api_key) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("failed to create Prowlarr client: {error}");
            return ExitCode::FAILURE;
        }
    };

    match client.test_connection().await {
        Ok(version) => println!("Prowlarr {version} at {base_url}"),
        Err(error) => {
            eprintln!("connection test failed: {error}");
            return ExitCode::FAILURE;
        }
    }

    let outcome = match client.search_book(&book).await {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("search failed: {error}");
            return ExitCode::FAILURE;
        }
    };

    if !json {
        println!();
        for attempt in &outcome.queries {
            println!("query: {attempt}");
        }
        if outcome.candidates.is_empty() {
            println!("no candidates found for any query");
        }
    }

    let evaluated = evaluator::rank(&book, &outcome.candidates);
    let selection = evaluator::select(&evaluated);

    if json {
        let selected = match &selection {
            Selection::Auto { index } => Some(index),
            _ => None,
        };
        let payload = serde_json::json!({
            "book": {
                "title": book.title,
                "authors": book.authors,
                "language": book.language,
                "preferredFormat": book.preferred_format,
            },
            "queries": outcome.queries,
            "selection": selection.describe(),
            "selectedIndex": selected,
            "candidates": evaluated,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
        );
        return ExitCode::SUCCESS;
    }

    println!();
    println!(
        "{:>5}  {:>5}  {:<7} {:<4} {:>10}  {:<8}  title",
        "score", "conf", "format", "lang", "size", "seeds"
    );

    for release in &evaluated {
        let status = if release.rejected() {
            format!(
                "REJECTED: {}",
                release
                    .rejection_reasons
                    .iter()
                    .map(|reason| reason.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        } else {
            String::new()
        };

        println!(
            "{:>5}  {:>5.2}  {:<7} {:<4} {:>10}  {:<8}  {} {}",
            release.score,
            release.confidence,
            release.candidate.detected_format.as_deref().unwrap_or("-"),
            release
                .candidate
                .detected_language
                .as_deref()
                .unwrap_or("-"),
            human_size(release.candidate.size_bytes),
            release
                .candidate
                .seeders
                .map(|seeders| seeders.to_string())
                .unwrap_or_else(|| "-".to_string()),
            release.candidate.title,
            status
        );

        for reason in &release.score_reasons {
            println!("        {:>+4} {}", reason.weight, reason.reason);
        }
    }

    println!();
    match &selection {
        Selection::Auto { index } => {
            println!("SELECTED: {}", evaluated[*index].candidate.title);
            println!("  confidence: {:.2}", evaluated[*index].confidence);
        }
        Selection::NeedsSelection => {
            println!("NEEDS_SELECTION: no candidate reached sufficient confidence");
        }
        Selection::None => {
            println!("NO_RELEASE_FOUND: all candidates were rejected");
        }
    }

    ExitCode::SUCCESS
}

fn next_value<'a>(iter: &mut std::slice::Iter<'a, String>, flag: &str) -> String {
    match iter.next() {
        Some(value) => value.clone(),
        None => {
            eprintln!("{flag} requires a value");
            std::process::exit(2);
        }
    }
}

fn human_size(bytes: i64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn usage() {
    eprintln!(
        "usage: rank-releases --title TITLE [--author AUTHOR ...] [--isbn ISBN]\n\
         \x20                    [--language LANG] [--format FORMAT] [--year YEAR]\n\
         \x20                    [--series-number N] [--json]\n\
         \n\
         environment: PROWLARR_URL (default http://localhost:9696), PROWLARR_API_KEY\n\
         \n\
         This is a development prototype: it only searches and ranks releases."
    );
}
