use crate::{AppState, routes};
use aide::axum::ApiRouter;
use axum::routing::{get, post, put};

pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new()
        .route("/healthz", get(routes::health::liveness))
        .api_route(
            "/api/health",
            aide::axum::routing::get(routes::health::readiness),
        )
        .route(
            "/opds",
            get(routes::opds::root).layer(axum::middleware::from_fn(routes::opds::challenge)),
        )
        .route(
            "/opds/all",
            get(routes::opds::all).layer(axum::middleware::from_fn(routes::opds::challenge)),
        )
        .route(
            "/opds/shelf",
            get(routes::opds::shelf).layer(axum::middleware::from_fn(routes::opds::challenge)),
        )
        .route(
            "/opds/recent",
            get(routes::opds::recent).layer(axum::middleware::from_fn(routes::opds::challenge)),
        )
        .route(
            "/opds/authors",
            get(routes::opds::authors).layer(axum::middleware::from_fn(routes::opds::challenge)),
        )
        .route(
            "/opds/authors/{id}",
            get(routes::opds::author_feed)
                .layer(axum::middleware::from_fn(routes::opds::challenge)),
        )
        .route(
            "/opds/books/{id}/download",
            get(routes::opds::download).layer(axum::middleware::from_fn(routes::opds::challenge)),
        )
        .route("/healthcheck", get(routes::kosync::healthcheck))
        .route("/users/create", post(routes::kosync::create_user))
        .route("/users/auth", get(routes::kosync::auth))
        .route("/syncs/progress", put(routes::kosync::put_progress))
        .route(
            "/syncs/progress/{document}",
            get(routes::kosync::get_progress),
        )
}
