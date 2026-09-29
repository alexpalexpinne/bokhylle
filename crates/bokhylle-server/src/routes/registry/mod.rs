mod account;
mod acquisitions;
mod admin;
mod catalogues;
mod collections;
mod delivery;
mod discovery;
mod library;
mod system;

use aide::axum::ApiRouter;
use axum::routing::any;

use crate::AppState;

pub fn router() -> ApiRouter<AppState> {
    ApiRouter::new()
        .merge(system::router())
        .merge(account::router())
        .merge(admin::router())
        .merge(collections::router())
        .merge(library::router())
        .merge(acquisitions::router())
        .merge(catalogues::router())
        .merge(discovery::router())
        .merge(delivery::router())
        .route("/api/{*rest}", any(super::fallback))
}
