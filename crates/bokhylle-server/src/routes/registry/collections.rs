use crate::{AppState, routes};
use aide::axum::ApiRouter;

pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new()
        .api_route(
            "/api/collections",
            aide::axum::routing::get(routes::collections::list_collections)
                .post(routes::collections::create_collection),
        )
        .api_route(
            "/api/collections/{id}",
            aide::axum::routing::get(routes::collections::get_collection)
                .delete(routes::collections::delete_collection),
        )
        .api_route(
            "/api/collections/{id}/books",
            aide::axum::routing::post(routes::collections::add_book),
        )
        .api_route(
            "/api/collections/{id}/books/{book_id}",
            aide::axum::routing::delete(routes::collections::remove_book),
        )
        .api_route(
            "/api/books/{book_id}/collections",
            aide::axum::routing::get(routes::collections::book_collections),
        )
}
