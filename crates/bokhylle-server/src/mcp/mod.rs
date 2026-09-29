//! The MCP server: a thin interface into Bokhylle at `/mcp`, authenticated
//! with per-profile agent tokens. It calls the same services as the HTTP API.

mod auth;
mod server;
mod types;

use axum::Router;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
};

use crate::AppState;

/// Mounts the stateless Streamable HTTP MCP service behind token auth.
pub fn router(state: AppState) -> Router<AppState> {
    let factory_state = state.clone();
    let service = StreamableHttpService::new(
        move || Ok(server::BokhylleMcp::new(factory_state.clone())),
        NeverSessionManager::default().into(),
        StreamableHttpServerConfig::default().with_json_response(true),
    );
    let router: Router<AppState> = Router::new()
        .nest_service("/mcp", service)
        .layer(axum::middleware::from_fn_with_state(state, auth::authorize));
    router
}
