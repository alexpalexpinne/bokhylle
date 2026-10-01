use std::{env, process::Command, time::SystemTime};

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn main() {
    for key in [
        "BOKHYLLE_BUILD_SHA",
        "BOKHYLLE_BUILD_DIRTY",
        "BOKHYLLE_BUILD_TIME",
        "BOKHYLLE_BUILD_INSTALLATION",
        "SOURCE_DATE_EPOCH",
    ] {
        println!("cargo::rerun-if-env-changed={key}");
    }
    for path in [
        "build.rs",
        "../../crates",
        "../../frontend/src",
        "../../migrations",
    ] {
        println!("cargo::rerun-if-changed={path}");
    }
    for file in ["HEAD", "index"] {
        if let Some(path) = git(&["rev-parse", "--git-path", file]) {
            println!("cargo::rerun-if-changed={path}");
        }
    }
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"])
        && let Some(path) = git(&["rev-parse", "--git-path", &reference])
    {
        println!("cargo::rerun-if-changed={path}");
    }
    let sha = env::var("BOKHYLLE_BUILD_SHA")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| git(&["rev-parse", "HEAD"]));
    if let Some(sha) = sha.filter(|value| {
        matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        println!("cargo::rustc-env=BOKHYLLE_BUILD_SHA={}", sha.to_lowercase());
    }
    let dirty = env::var("BOKHYLLE_BUILD_DIRTY")
        .ok()
        .filter(|value| matches!(value.as_str(), "true" | "false"))
        .or_else(|| git(&["status", "--porcelain"]).map(|status| (!status.is_empty()).to_string()));
    if let Some(dirty) = dirty {
        println!("cargo::rustc-env=BOKHYLLE_BUILD_DIRTY={dirty}");
    }
    let stamp = env::var("BOKHYLLE_BUILD_TIME")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| env::var("SOURCE_DATE_EPOCH").ok())
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| {
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .ok()
                .map(|time| time.as_secs())
        });
    if let Some(stamp) = stamp {
        println!("cargo::rustc-env=BOKHYLLE_BUILD_TIME={stamp}");
    }
    let installation = env::var("BOKHYLLE_BUILD_INSTALLATION").unwrap_or_else(|_| "source".into());
    println!(
        "cargo::rustc-env=BOKHYLLE_BUILD_INSTALLATION={}",
        if installation == "docker" {
            "docker"
        } else {
            "source"
        }
    );
}
