use bokhylle_server::db;
use bokhylle_server::paths::Paths;
use bokhylle_server::settings::Settings;

#[tokio::test]
async fn resolves_defaults_creates_directories_and_applies_environment() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("config");
    let pool = db::init(&config_dir.join("bokhylle.db")).await.unwrap();
    let settings = Settings::new(pool);

    let paths = Paths::resolve(&settings, config_dir.clone()).await.unwrap();
    assert_eq!(paths.library_root, config_dir.join("library"));
    assert_eq!(paths.downloads_dir, config_dir.join("downloads"));
    assert!(paths.library_root.is_dir());
    assert!(paths.downloads_dir.is_dir());

    let overridden = dir.path().join("elsewhere");
    unsafe { std::env::set_var("BOKHYLLE_DOWNLOADS_DIR", &overridden) };
    let paths = Paths::resolve(&settings, config_dir).await.unwrap();
    unsafe { std::env::remove_var("BOKHYLLE_DOWNLOADS_DIR") };

    assert_eq!(paths.downloads_dir, overridden);
    assert!(paths.downloads_dir.is_dir());
}

#[tokio::test]
async fn rejects_file_in_place_of_directory() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("config");
    let pool = db::init(&config_dir.join("bokhylle.db")).await.unwrap();
    let settings = Settings::new(pool);

    std::fs::write(config_dir.join("library"), "not a directory").unwrap();

    let error = Paths::resolve(&settings, config_dir).await.unwrap_err();
    assert!(error.to_string().contains("not a directory"));
}
