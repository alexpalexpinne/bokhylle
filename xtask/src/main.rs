use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("gen-fixtures") => gen_fixtures(&args[1..]),
        _ => {
            eprintln!("usage: xtask gen-fixtures [--output DIR] [--count N]");
            std::process::exit(2);
        }
    }
}

fn gen_fixtures(args: &[String]) {
    let mut output = PathBuf::from("fixtures/generated");
    let mut count = 100usize;

    let mut iter = args.iter();
    while let Some(argument) = iter.next() {
        match argument.as_str() {
            "--output" => {
                output = PathBuf::from(iter.next().expect("--output requires a value"));
            }
            "--count" => {
                count = iter
                    .next()
                    .expect("--count requires a value")
                    .parse()
                    .expect("--count must be a number");
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }

    match bokhylle_library::fixtures::generate_library(&output, count) {
        Ok(paths) => println!(
            "generated {} fixture files in {}",
            paths.len(),
            output.display()
        ),
        Err(error) => {
            eprintln!("fixture generation failed: {error}");
            std::process::exit(1);
        }
    }
}
