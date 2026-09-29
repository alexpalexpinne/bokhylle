use crate::{AppState, routes};
use aide::axum::ApiRouter;

pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new()
        .api_route(
            "/api/delivery-targets",
            aide::axum::routing::get(routes::delivery::list_targets)
                .post(routes::delivery::create_target),
        )
        .api_route(
            "/api/delivery-targets/presets",
            aide::axum::routing::get(routes::delivery::presets),
        )
        .api_route(
            "/api/delivery-targets/default",
            aide::axum::routing::get(routes::delivery::default_target),
        )
        .api_route(
            "/api/delivery-targets/{id}",
            aide::axum::routing::put(routes::delivery::update_target)
                .delete(routes::delivery::delete_target),
        )
        .api_route(
            "/api/delivery-targets/{id}/default",
            aide::axum::routing::post(routes::delivery::set_default),
        )
        .api_route(
            "/api/books/{book_id}/files/{file_id}/deliver",
            aide::axum::routing::post(routes::delivery::deliver),
        )
        .api_route(
            "/api/deliveries",
            aide::axum::routing::get(routes::delivery::list_deliveries),
        )
        .api_route(
            "/api/deliveries/{id}/retry",
            aide::axum::routing::post(routes::delivery::retry),
        )
        .api_route(
            "/api/admin/integrations/smtp/test",
            aide::axum::routing::post(routes::delivery::test_smtp),
        )
}
