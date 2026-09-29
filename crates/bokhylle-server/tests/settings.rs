use bokhylle_server::db;
use bokhylle_server::settings::{self, Settings};
use serde_json::json;

async fn new_settings() -> (Settings, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::init(&dir.path().join("bokhylle.db")).await.unwrap();
    (Settings::new(pool), dir)
}

#[tokio::test]
async fn defaults_apply_when_unset() {
    let (settings, _dir) = new_settings().await;

    assert_eq!(
        settings
            .get_string(settings::PREFERRED_LANGUAGE, "en")
            .await
            .unwrap(),
        "en"
    );
    assert!(
        settings
            .get_bool(settings::SCAN_ON_STARTUP, true)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn database_value_overrides_default() {
    let (settings, _dir) = new_settings().await;

    settings
        .set(settings::PREFERRED_LANGUAGE, &json!("sv"))
        .await
        .unwrap();

    assert_eq!(
        settings
            .get_string(settings::PREFERRED_LANGUAGE, "en")
            .await
            .unwrap(),
        "sv"
    );
}

#[tokio::test]
async fn environment_overrides_database_and_default() {
    let (settings, _dir) = new_settings().await;
    let env_name = "BOKHYLLE_PREFERRED_FORMAT";

    unsafe { std::env::set_var(env_name, "mobi") };
    assert_eq!(
        settings
            .get_string(settings::PREFERRED_FORMAT, "epub")
            .await
            .unwrap(),
        "mobi"
    );

    settings
        .set(settings::PREFERRED_FORMAT, &json!("pdf"))
        .await
        .unwrap();
    assert_eq!(
        settings
            .get_string(settings::PREFERRED_FORMAT, "epub")
            .await
            .unwrap(),
        "mobi"
    );

    unsafe { std::env::remove_var(env_name) };
    assert_eq!(
        settings
            .get_string(settings::PREFERRED_FORMAT, "epub")
            .await
            .unwrap(),
        "pdf"
    );

    settings.delete(settings::PREFERRED_FORMAT).await.unwrap();
    assert_eq!(
        settings
            .get_string(settings::PREFERRED_FORMAT, "epub")
            .await
            .unwrap(),
        "epub"
    );
}

#[tokio::test]
async fn invalid_type_is_rejected() {
    let (settings, _dir) = new_settings().await;

    settings
        .set(settings::LIBRARY_ROOT, &json!(5))
        .await
        .unwrap();

    let error = settings
        .get_string(settings::LIBRARY_ROOT, "/library")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("must be a string"));
}

#[tokio::test]
async fn invalid_bool_is_rejected() {
    let (settings, _dir) = new_settings().await;

    settings
        .set("test.invalid_bool", &json!("maybe"))
        .await
        .unwrap();

    let error = settings
        .get_bool("test.invalid_bool", true)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("must be a boolean"));
}

#[tokio::test]
async fn fractional_scheduler_hours_round_trip_as_numbers() {
    let (settings, _dir) = new_settings().await;

    settings
        .set(settings::SCAN_INTERVAL_HOURS, &json!(6.5))
        .await
        .unwrap();
    settings
        .set(bokhylle_server::backup::INTERVAL_HOURS, &json!(12))
        .await
        .unwrap();

    assert_eq!(
        settings
            .get_float(settings::SCAN_INTERVAL_HOURS, 0.0)
            .await
            .unwrap(),
        6.5
    );
    assert_eq!(
        settings
            .get_float(bokhylle_server::backup::INTERVAL_HOURS, 24.0)
            .await
            .unwrap(),
        12.0
    );
}

#[tokio::test]
async fn scheduler_hours_validation_accepts_numbers_and_rejects_strings() {
    let (_settings, _dir) = new_settings().await;

    assert!(
        settings::validate(settings::SCAN_INTERVAL_HOURS, &json!(6.5)).is_ok(),
        "fractional hours are a supported scheduler value"
    );
    assert!(settings::validate(settings::SCAN_INTERVAL_HOURS, &json!(0)).is_ok());
    assert!(
        settings::validate(settings::SCAN_INTERVAL_HOURS, &json!("6")).is_err(),
        "a string would be rejected by the numeric reader"
    );
    assert!(settings::validate(settings::BACKUP_INTERVAL_HOURS, &json!(12.5)).is_ok());
}
