use crate::{AppState, routes};
use aide::axum::ApiRouter;

pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new()
        .api_route(
            "/api/catalogues",
            aide::axum::routing::get(routes::catalogues::list)
                .post(routes::catalogues::create_source),
        )
        .api_route(
            "/api/catalogues/{id}",
            aide::axum::routing::delete(routes::catalogues::delete_source),
        )
        .api_route(
            "/api/catalogues/{id}/feed",
            aide::axum::routing::get(routes::catalogues::feed),
        )
        .api_route(
            "/api/catalogues/{id}/acquisitions",
            aide::axum::routing::post(routes::catalogues::acquire),
        )
}
