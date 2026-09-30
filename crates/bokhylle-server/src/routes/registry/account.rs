use crate::{AppState, demo, routes};
use aide::axum::ApiRouter;

pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new()
        .api_route(
            "/api/auth/login",
            aide::axum::routing::post(routes::auth::login),
        )
        .api_route("/api/demo", aide::axum::routing::get(demo::status))
        .api_route("/api/demo/enter", aide::axum::routing::post(demo::enter))
        .api_route("/api/demo/switch", aide::axum::routing::post(demo::switch))
        .api_route("/api/demo/get", aide::axum::routing::post(demo::start_get))
        .api_route("/api/demo/send", aide::axum::routing::post(demo::send))
        .api_route(
            "/api/demo/activity",
            aide::axum::routing::get(demo::activity),
        )
        .api_route(
            "/api/demo/requests/{id}/approve",
            aide::axum::routing::post(demo::approve_request),
        )
        .api_route(
            "/api/demo/requests/{id}/decline",
            aide::axum::routing::post(demo::decline_request),
        )
        .api_route(
            "/api/auth/users",
            aide::axum::routing::get(routes::auth::login_users),
        )
        .api_route(
            "/api/auth/users/{id}/avatar",
            aide::axum::routing::get(routes::auth::login_avatar),
        )
        .api_route(
            "/api/auth/logout-all",
            aide::axum::routing::post(routes::auth::logout_all),
        )
        .api_route(
            "/api/profile/tokens",
            aide::axum::routing::get(routes::auth::list_reader_tokens)
                .post(routes::auth::create_reader_token),
        )
        .api_route(
            "/api/profile/tokens/{id}",
            aide::axum::routing::delete(routes::auth::revoke_reader_token),
        )
        .api_route(
            "/api/profile/agent-tokens",
            aide::axum::routing::get(routes::agent_tokens::list).post(routes::agent_tokens::create),
        )
        .api_route(
            "/api/profile/agent-tokens/{id}",
            aide::axum::routing::delete(routes::agent_tokens::revoke),
        )
        .api_route(
            "/api/auth/logout",
            aide::axum::routing::post(routes::auth::logout),
        )
        .api_route("/api/auth/me", aide::axum::routing::get(routes::auth::me))
        .api_route(
            "/api/profile/avatar",
            aide::axum::routing::get(routes::auth::avatar)
                .put(routes::auth::upload_avatar)
                .delete(routes::auth::delete_avatar),
        )
        .api_route(
            "/api/profile/avatar/preset",
            aide::axum::routing::put(routes::auth::set_avatar_preset),
        )
        .api_route(
            "/api/profile",
            aide::axum::routing::put(routes::auth::update_profile),
        )
        .api_route(
            "/api/profile/onboarding",
            aide::axum::routing::get(routes::auth::onboarding_state),
        )
        .api_route(
            "/api/profile/interests",
            aide::axum::routing::put(routes::auth::update_interests),
        )
        .api_route(
            "/api/profile/onboarded",
            aide::axum::routing::post(routes::auth::complete_onboarding),
        )
        .api_route(
            "/api/profile/credential",
            aide::axum::routing::put(routes::auth::change_credential),
        )
        .api_route(
            "/api/notifications",
            aide::axum::routing::get(routes::notifications::list_notifications),
        )
        .api_route(
            "/api/notifications/read",
            aide::axum::routing::post(routes::notifications::mark_read),
        )
}
