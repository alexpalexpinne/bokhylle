use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;
use bokhylle_acquisition::{
    model::{AcquisitionMethod, ExpectedBook, ReleaseCandidate, SourceIdentity},
    provider::{DownloadProvider, IndexerError, IndexerProvider, SearchOutcome},
    sabnzbd::{NzbDownloadProvider, NzbStatus, SabnzbdError},
};
use bokhylle_server::{
    AppState, acquisition, acquisition_pipeline, error::AppError, nzb_acquisition,
    providers::ProviderFactory,
};

mod common;

struct FakeSab {
    status: Mutex<NzbStatus>,
    adds: Mutex<usize>,
    finds: Mutex<Option<String>>,
    submitted: Mutex<Vec<(String, String)>>,
    cancels: Mutex<Vec<String>>,
    add_times_out: AtomicBool,
    cancel_fails: AtomicBool,
    add_started: Option<Arc<tokio::sync::Notify>>,
    add_resume: Option<Arc<tokio::sync::Notify>>,
}

impl Default for FakeSab {
    fn default() -> Self {
        Self {
            status: Mutex::new(NzbStatus::Queued { progress: 1.0 }),
            adds: Mutex::new(0),
            finds: Mutex::new(None),
            submitted: Mutex::new(Vec::new()),
            cancels: Mutex::new(Vec::new()),
            add_times_out: AtomicBool::new(false),
            cancel_fails: AtomicBool::new(false),
            add_started: None,
            add_resume: None,
        }
    }
}

#[async_trait]
impl NzbDownloadProvider for FakeSab {
    fn name(&self) -> &'static str {
        "sabnzbd"
    }
    async fn add_url(
        &self,
        url: &str,
        _category: &str,
        job_name: &str,
    ) -> Result<String, SabnzbdError> {
        *self.adds.lock().unwrap() += 1;
        self.submitted
            .lock()
            .unwrap()
            .push((url.into(), job_name.into()));
        if let Some(started) = &self.add_started {
            started.notify_one();
        }
        if let Some(resume) = &self.add_resume {
            resume.notified().await;
        }
        if self.add_times_out.load(Ordering::SeqCst) {
            return Err(SabnzbdError::Request);
        }
        Ok("job-1".into())
    }
    async fn find_by_name(&self, _job_name: &str) -> Result<Option<String>, SabnzbdError> {
        Ok(self.finds.lock().unwrap().clone())
    }
    async fn status(&self, _id: &str) -> Result<NzbStatus, SabnzbdError> {
        Ok(self.status.lock().unwrap().clone())
    }
    async fn cancel(&self, id: &str) -> Result<(), SabnzbdError> {
        self.cancels.lock().unwrap().push(id.into());
        if self.cancel_fails.load(Ordering::SeqCst) {
            return Err(SabnzbdError::Request);
        }
        Ok(())
    }
    async fn test_connection(&self) -> Result<String, SabnzbdError> {
        Ok("4.0".into())
    }
}

struct Factory(Arc<FakeSab>);

#[async_trait]
impl ProviderFactory for Factory {
    async fn indexer(
        &self,
        _state: &AppState,
    ) -> Result<Option<Arc<dyn IndexerProvider>>, AppError> {
        Ok(None)
    }
    async fn downloader(
        &self,
        _state: &AppState,
    ) -> Result<Option<Arc<dyn DownloadProvider>>, AppError> {
        Ok(None)
    }
    async fn nzb_downloader(
        &self,
        _state: &AppState,
    ) -> Result<Option<Arc<dyn NzbDownloadProvider>>, AppError> {
        Ok(Some(self.0.clone()))
    }
}

struct NzbIndexer;

#[async_trait]
impl IndexerProvider for NzbIndexer {
    fn name(&self) -> &'static str {
        "newznab"
    }
    async fn search_book(&self, _book: &ExpectedBook) -> Result<SearchOutcome, IndexerError> {
        Ok(SearchOutcome {
            queries: vec!["Project Hail Mary".into()],
            candidates: vec![ReleaseCandidate {
                id: "guid-1".into(),
                source: Some(SourceIdentity {
                    kind: "newznab".into(),
                    name: "Newznab".into(),
                    key: "guid-1".into(),
                }),
                method: Some(AcquisitionMethod::Nzb {
                    guid: "guid-1".into(),
                }),
                title: "Andy.Weir.Project.Hail.Mary.EN.EPUB".into(),
                indexer: Some("Newznab".into()),
                size_bytes: 3_000_000,
                ..Default::default()
            }],
        })
    }
    async fn fetch_torrent(
        &self,
        _release: &ReleaseCandidate,
    ) -> Result<Arc<Vec<u8>>, IndexerError> {
        Err(IndexerError::NoDownloadLink)
    }
    async fn test_connection(&self) -> Result<String, IndexerError> {
        Ok("Newznab".into())
    }
    fn nzb_url(&self, release: &ReleaseCandidate) -> Result<String, IndexerError> {
        Ok(format!(
            "https://indexer.test/api?t=get&id={}&apikey=private-secret",
            release.id
        ))
    }
}

struct FullFactory(Arc<FakeSab>);

#[async_trait]
impl ProviderFactory for FullFactory {
    async fn indexer(
        &self,
        _state: &AppState,
    ) -> Result<Option<Arc<dyn IndexerProvider>>, AppError> {
        Ok(Some(Arc::new(NzbIndexer)))
    }
    async fn indexer_named(
        &self,
        _state: &AppState,
        kind: &str,
    ) -> Result<Option<Arc<dyn IndexerProvider>>, AppError> {
        Ok((kind == "newznab").then(|| Arc::new(NzbIndexer) as Arc<dyn IndexerProvider>))
    }
    async fn downloader(
        &self,
        _state: &AppState,
    ) -> Result<Option<Arc<dyn DownloadProvider>>, AppError> {
        Ok(None)
    }
    async fn nzb_downloader(
        &self,
        _state: &AppState,
    ) -> Result<Option<Arc<dyn NzbDownloadProvider>>, AppError> {
        Ok(Some(self.0.clone()))
    }
}

#[tokio::test]
async fn selected_newznab_result_queues_sab_without_journaling_the_key() {
    let sab = Arc::new(FakeSab {
        status: Mutex::new(NzbStatus::Queued { progress: 1.0 }),
        adds: Mutex::new(0),
        finds: Mutex::new(None),
        ..Default::default()
    });
    let library = tempfile::tempdir().unwrap();
    let app = common::test_app_with_factory(
        library.path().to_path_buf(),
        Arc::new(FullFactory(sab.clone())),
    )
    .await;
    let book_id: i64 = sqlx::query_scalar(
        "INSERT INTO books (title, normalized_title) VALUES ('Project Hail Mary', 'project hail mary') RETURNING id",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    let author_id: i64 = sqlx::query_scalar(
        "INSERT INTO authors (name, normalized_name) VALUES ('Andy Weir', 'andy weir') RETURNING id",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO book_authors (book_id, author_id) VALUES (?, ?)")
        .bind(book_id)
        .bind(author_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let (request, _) = acquisition::create(
        &app.state.db,
        book_id,
        None,
        Some("epub".into()),
        Some("en".into()),
        false,
        false,
    )
    .await
    .unwrap();
    acquisition_pipeline::run(&app.state, &request.id)
        .await
        .unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM acquisitions WHERE id = ?")
        .bind(&request.id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let detail: Option<String> = sqlx::query_scalar("SELECT detail FROM acquisition_events WHERE acquisition_id = ? AND event = 'acquisition.candidates.evaluated' ORDER BY id DESC LIMIT 1")
        .bind(&request.id).fetch_optional(&app.state.db).await.unwrap();
    assert_eq!(
        *sab.adds.lock().unwrap(),
        1,
        "status={status}; detail={detail:?}"
    );
    let provider: String =
        sqlx::query_scalar("SELECT download_provider FROM acquisitions WHERE id = ?")
            .bind(&request.id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(provider, "sabnzbd");
    let input: String = sqlx::query_scalar("SELECT url FROM nzb_inputs WHERE acquisition_id = ?")
        .bind(&request.id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert!(input.contains("private-secret"));
    let events: Vec<String> = sqlx::query_scalar(
        "SELECT COALESCE(detail, '') FROM acquisition_events WHERE acquisition_id = ?",
    )
    .bind(&request.id)
    .fetch_all(&app.state.db)
    .await
    .unwrap();
    assert!(
        events
            .iter()
            .all(|detail| !detail.contains("private-secret"))
    );
}

#[tokio::test]
async fn rejects_sab_completed_path_outside_downloads() {
    let outside = tempfile::tempdir().unwrap();
    let file = outside.path().join("book.epub");
    std::fs::write(&file, b"outside").unwrap();
    let sab = Arc::new(FakeSab {
        status: Mutex::new(NzbStatus::Completed {
            path: file.to_string_lossy().into_owned(),
        }),
        adds: Mutex::new(0),
        finds: Mutex::new(None),
        ..Default::default()
    });
    let library = tempfile::tempdir().unwrap();
    let app =
        common::test_app_with_factory(library.path().to_path_buf(), Arc::new(Factory(sab.clone())))
            .await;
    let book_id: i64 = sqlx::query_scalar("INSERT INTO books (title, normalized_title) VALUES ('Test Book', 'test book') RETURNING id")
        .fetch_one(&app.state.db).await.unwrap();
    sqlx::query("INSERT INTO acquisitions (id, book_id, status, download_provider) VALUES ('nzb-1', ?, 'QUEUED', 'sabnzbd')")
        .bind(book_id).execute(&app.state.db).await.unwrap();
    sqlx::query("INSERT INTO nzb_inputs (acquisition_id, url, job_name) VALUES ('nzb-1', 'https://example.test/api?t=get&id=1', 'bokhylle-nzb-1')")
        .execute(&app.state.db).await.unwrap();
    nzb_acquisition::run(&app.state, "nzb-1").await.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM acquisitions WHERE id = 'nzb-1'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(status, "DOWNLOAD_FAILED");
    assert_eq!(*sab.adds.lock().unwrap(), 1);
    assert!(file.exists());
}

#[tokio::test]
async fn restart_uses_recorded_sab_job_instead_of_adding_again() {
    let sab = Arc::new(FakeSab {
        status: Mutex::new(NzbStatus::Queued { progress: 44.0 }),
        adds: Mutex::new(0),
        finds: Mutex::new(None),
        ..Default::default()
    });
    let library = tempfile::tempdir().unwrap();
    let app =
        common::test_app_with_factory(library.path().to_path_buf(), Arc::new(Factory(sab.clone())))
            .await;
    let book_id: i64 = sqlx::query_scalar("INSERT INTO books (title, normalized_title) VALUES ('Test Book', 'test book') RETURNING id")
        .fetch_one(&app.state.db).await.unwrap();
    sqlx::query("INSERT INTO acquisitions (id, book_id, status, download_provider, provider_download_id) VALUES ('nzb-2', ?, 'QUEUED', 'sabnzbd', 'job-1')")
        .bind(book_id).execute(&app.state.db).await.unwrap();
    sqlx::query("INSERT INTO nzb_inputs (acquisition_id, url, job_name) VALUES ('nzb-2', 'https://example.test/api?t=get&id=2', 'bokhylle-nzb-2')")
        .execute(&app.state.db).await.unwrap();
    nzb_acquisition::tick(&app.state).await.unwrap();
    assert_eq!(*sab.adds.lock().unwrap(), 0);
    let status: String = sqlx::query_scalar("SELECT status FROM acquisitions WHERE id = 'nzb-2'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(status, "DOWNLOADING");
    // Diagnostics must ask the NZB client even when no torrent client exists.
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use tower::ServiceExt;
    app.state
        .auth
        .create_user("admin", "password123", bokhylle_server::auth::Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&app, "admin", "password123").await;
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/acquisitions/nzb-2")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 128 * 1024).await.unwrap()).unwrap();
    assert_eq!(body["providerState"]["state"], "queued");
    assert_eq!(body["providerState"]["progress"], 44.0);
}

#[tokio::test]
async fn recovers_an_added_job_before_its_id_was_saved() {
    let sab = Arc::new(FakeSab {
        status: Mutex::new(NzbStatus::Queued { progress: 12.0 }),
        adds: Mutex::new(0),
        finds: Mutex::new(Some("job-1".into())),
        ..Default::default()
    });
    let library = tempfile::tempdir().unwrap();
    let app =
        common::test_app_with_factory(library.path().to_path_buf(), Arc::new(Factory(sab.clone())))
            .await;
    let book_id: i64 = sqlx::query_scalar("INSERT INTO books (title, normalized_title) VALUES ('Test Book', 'test book') RETURNING id")
        .fetch_one(&app.state.db).await.unwrap();
    sqlx::query("INSERT INTO acquisitions (id, book_id, status, download_provider) VALUES ('nzb-3', ?, 'QUEUED', 'sabnzbd')")
        .bind(book_id).execute(&app.state.db).await.unwrap();
    sqlx::query("INSERT INTO nzb_inputs (acquisition_id, url, job_name) VALUES ('nzb-3', 'https://example.test/get', 'bokhylle-nzb-3')")
        .execute(&app.state.db).await.unwrap();
    nzb_acquisition::run(&app.state, "nzb-3").await.unwrap();
    assert_eq!(*sab.adds.lock().unwrap(), 0);
    let id: String =
        sqlx::query_scalar("SELECT provider_download_id FROM acquisitions WHERE id = 'nzb-3'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(id, "job-1");
}

async fn seeded_request(app: &common::TestApp, id: &str, status: &str) {
    let book_id: i64 = sqlx::query_scalar("INSERT INTO books (title, normalized_title) VALUES ('Project Hail Mary', 'project hail mary') RETURNING id")
        .fetch_one(&app.state.db).await.unwrap();
    let author_id: i64 = sqlx::query_scalar("INSERT INTO authors (name, normalized_name) VALUES ('Andy Weir', 'andy weir') RETURNING id")
        .fetch_one(&app.state.db).await.unwrap();
    sqlx::query("INSERT INTO book_authors (book_id, author_id) VALUES (?, ?)")
        .bind(book_id)
        .bind(author_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO acquisitions (id, book_id, status, download_provider, preferred_format, preferred_language) VALUES (?, ?, ?, 'sabnzbd', 'epub', 'en')")
        .bind(id).bind(book_id).bind(status).execute(&app.state.db).await.unwrap();
    sqlx::query("INSERT INTO nzb_inputs (acquisition_id, url, job_name) VALUES (?, 'https://indexer.test/old', ?)")
        .bind(id).bind(format!("bokhylle-{id}")).execute(&app.state.db).await.unwrap();
}

#[tokio::test]
async fn retry_replaces_input_and_attempt_name_without_overwriting_active_work() {
    use bokhylle_acquisition::{model::EvaluatedRelease, state::AcquisitionStatus};
    let sab = Arc::new(FakeSab::default());
    let library = tempfile::tempdir().unwrap();
    let app =
        common::test_app_with_factory(library.path().into(), Arc::new(FullFactory(sab.clone())))
            .await;
    seeded_request(&app, "retry-nzb", "EVALUATING").await;
    let mut release = EvaluatedRelease {
        candidate: ReleaseCandidate {
            id: "first-guid".into(),
            title: "Project Hail Mary EN EPUB".into(),
            method: Some(AcquisitionMethod::Nzb {
                guid: "first-guid".into(),
            }),
            ..Default::default()
        },
        score: 100,
        confidence: 1.0,
        format_tier: 0,
        language_index: 0,
        score_reasons: Vec::new(),
        rejection_reasons: Vec::new(),
    };
    nzb_acquisition::queue(&app.state, "retry-nzb", &release)
        .await
        .unwrap();
    let first: (String, String) =
        sqlx::query_as("SELECT url, job_name FROM nzb_inputs WHERE acquisition_id = 'retry-nzb'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert!(matches!(
        nzb_acquisition::queue(&app.state, "retry-nzb", &release).await,
        Err(AppError::Conflict(_))
    ));
    let unchanged: (String, String) =
        sqlx::query_as("SELECT url, job_name FROM nzb_inputs WHERE acquisition_id = 'retry-nzb'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(first, unchanged);
    acquisition::fail(
        &app.state.db,
        "retry-nzb",
        "download_failed",
        "test failure",
    )
    .await
    .unwrap();
    acquisition::retry(&app.state.db, "retry-nzb")
        .await
        .unwrap();
    acquisition::transition(
        &app.state.db,
        "retry-nzb",
        AcquisitionStatus::Searching,
        None,
    )
    .await
    .unwrap();
    acquisition::transition(
        &app.state.db,
        "retry-nzb",
        AcquisitionStatus::Evaluating,
        None,
    )
    .await
    .unwrap();
    release.candidate.id = "second-guid".into();
    release.candidate.method = Some(AcquisitionMethod::Nzb {
        guid: "second-guid".into(),
    });
    nzb_acquisition::queue(&app.state, "retry-nzb", &release)
        .await
        .unwrap();
    let second: (String, String) =
        sqlx::query_as("SELECT url, job_name FROM nzb_inputs WHERE acquisition_id = 'retry-nzb'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_ne!(first.1, second.1);
    assert!(first.0.contains("first-guid"));
    assert!(second.0.contains("second-guid"));
    assert_eq!(*sab.adds.lock().unwrap(), 2);
    assert_eq!(sab.submitted.lock().unwrap()[1], second);
}

#[tokio::test]
async fn ambiguous_submission_is_reconciled_without_adding_again() {
    let sab = Arc::new(FakeSab {
        add_times_out: AtomicBool::new(true),
        ..Default::default()
    });
    let library = tempfile::tempdir().unwrap();
    let app =
        common::test_app_with_factory(library.path().into(), Arc::new(Factory(sab.clone()))).await;
    seeded_request(&app, "timeout-nzb", "QUEUED").await;
    assert!(
        nzb_acquisition::run(&app.state, "timeout-nzb")
            .await
            .is_err()
    );
    nzb_acquisition::run(&app.state, "timeout-nzb")
        .await
        .unwrap();
    assert_eq!(*sab.adds.lock().unwrap(), 1);
    *sab.finds.lock().unwrap() = Some("job-1".into());
    nzb_acquisition::run(&app.state, "timeout-nzb")
        .await
        .unwrap();
    assert_eq!(*sab.adds.lock().unwrap(), 1);
    assert_eq!(
        acquisition::get(&app.state.db, "timeout-nzb")
            .await
            .unwrap()
            .unwrap()
            .provider_download_id
            .as_deref(),
        Some("job-1")
    );
}

#[tokio::test]
async fn a_redacted_backup_input_is_not_submitted_to_sab() {
    let sab = Arc::new(FakeSab::default());
    let library = tempfile::tempdir().unwrap();
    let app =
        common::test_app_with_factory(library.path().into(), Arc::new(Factory(sab.clone()))).await;
    seeded_request(&app, "redacted-nzb", "QUEUED").await;
    sqlx::query("UPDATE nzb_inputs SET url = '' WHERE acquisition_id = 'redacted-nzb'")
        .execute(&app.state.db)
        .await
        .unwrap();
    nzb_acquisition::run(&app.state, "redacted-nzb")
        .await
        .unwrap();
    let record = acquisition::get(&app.state.db, "redacted-nzb")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.status, "DOWNLOAD_FAILED");
    assert_eq!(
        record.error_code.as_deref(),
        Some("download_input_redacted")
    );
    assert_eq!(*sab.adds.lock().unwrap(), 0);
}

#[tokio::test]
async fn cancelled_submission_recovers_its_job_before_id_was_saved() {
    let sab = Arc::new(FakeSab {
        finds: Mutex::new(Some("job-1".into())),
        cancel_fails: AtomicBool::new(true),
        ..Default::default()
    });
    let library = tempfile::tempdir().unwrap();
    let app =
        common::test_app_with_factory(library.path().into(), Arc::new(Factory(sab.clone()))).await;
    seeded_request(&app, "cancel-nzb", "QUEUED").await;
    acquisition::cancel(&app.state.db, "cancel-nzb")
        .await
        .unwrap();
    nzb_acquisition::tick(&app.state).await.unwrap();
    let pending: (i64, Option<String>) = sqlx::query_as(
        "SELECT cancel_pending, provider_download_id FROM acquisitions WHERE id = 'cancel-nzb'",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(pending, (1, Some("job-1".into())));
    sab.cancel_fails.store(false, Ordering::SeqCst);
    nzb_acquisition::tick(&app.state).await.unwrap();
    let pending: i64 =
        sqlx::query_scalar("SELECT cancel_pending FROM acquisitions WHERE id = 'cancel-nzb'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(pending, 0);
    assert_eq!(*sab.adds.lock().unwrap(), 0);
    assert_eq!(sab.cancels.lock().unwrap().as_slice(), ["job-1", "job-1"]);
}

#[tokio::test]
async fn cancellation_during_add_keeps_the_id_for_failed_cleanup() {
    let started = Arc::new(tokio::sync::Notify::new());
    let resume = Arc::new(tokio::sync::Notify::new());
    let sab = Arc::new(FakeSab {
        add_started: Some(started.clone()),
        add_resume: Some(resume.clone()),
        cancel_fails: AtomicBool::new(true),
        ..Default::default()
    });
    let library = tempfile::tempdir().unwrap();
    let app =
        common::test_app_with_factory(library.path().into(), Arc::new(Factory(sab.clone()))).await;
    seeded_request(&app, "race-nzb", "QUEUED").await;
    let state = app.state.clone();
    let task = tokio::spawn(async move { nzb_acquisition::run(&state, "race-nzb").await });
    tokio::time::timeout(std::time::Duration::from_secs(5), started.notified())
        .await
        .unwrap();
    acquisition::cancel(&app.state.db, "race-nzb")
        .await
        .unwrap();
    nzb_acquisition::reconcile_cancel(&app.state, "race-nzb")
        .await
        .unwrap();
    resume.notify_one();
    task.await.unwrap().unwrap();
    let record: (String, i64, Option<String>) = sqlx::query_as("SELECT status, cancel_pending, provider_download_id FROM acquisitions WHERE id = 'race-nzb'")
        .fetch_one(&app.state.db).await.unwrap();
    assert_eq!(record, ("CANCELLED".into(), 1, Some("job-1".into())));
    sab.cancel_fails.store(false, Ordering::SeqCst);
    nzb_acquisition::tick(&app.state).await.unwrap();
    assert_eq!(sab.cancels.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn completed_sab_job_uses_the_shared_import_pipeline() {
    let sab = Arc::new(FakeSab::default());
    let library = tempfile::tempdir().unwrap();
    let app =
        common::test_app_with_factory(library.path().into(), Arc::new(Factory(sab.clone()))).await;
    seeded_request(&app, "import-nzb", "QUEUED").await;
    let fixtures = tempfile::tempdir().unwrap();
    let fixture = bokhylle_library::fixtures::generate_library(fixtures.path(), 1)
        .unwrap()
        .into_iter()
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("Project Hail Mary")
        })
        .unwrap();
    let completed = app.state.paths.downloads_dir.join("completed");
    std::fs::create_dir_all(&completed).unwrap();
    let file = completed.join("Project Hail Mary.epub");
    std::fs::copy(fixture, &file).unwrap();
    *sab.status.lock().unwrap() = NzbStatus::Completed {
        path: completed.to_string_lossy().into_owned(),
    };
    nzb_acquisition::run(&app.state, "import-nzb")
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let record = acquisition::get(&app.state.db, "import-nzb")
                .await
                .unwrap()
                .unwrap();
            if record.status().unwrap().is_terminal() || record.status == "NEEDS_REVIEW" {
                assert_eq!(record.status, "READY", "{record:?}");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM book_files")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert!(file.exists(), "SAB owns the completed download");
}
