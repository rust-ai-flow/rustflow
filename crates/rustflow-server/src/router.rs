use axum::{
    Router,
    routing::{delete, get, post},
};

use crate::handlers;
use crate::playground;
use crate::state::AppState;
use crate::ws;

/// Build and return the main axum router with all API routes attached.
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Health
        .route("/health", get(handlers::health))
        // Agents CRUD
        .route("/agents", post(handlers::create_agent))
        .route("/agents", get(handlers::list_agents))
        .route("/agents/{id}", get(handlers::get_agent))
        .route("/agents/{id}", delete(handlers::delete_agent))
        // Agent execution (REST)
        .route("/agents/{id}/run", post(handlers::run_agent))
        // Agent execution (WebSocket streaming)
        .route("/agents/{id}/stream", get(ws::stream_agent))
        // Playground UI
        .route("/playground", get(playground::playground_index))
        .route("/playground/agents", post(playground::playground_create_agent))
        .with_state(state)
}
