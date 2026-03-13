use axum::{
    Router,
    routing::{delete, get, post},
};

use crate::handlers;
use crate::state::AppState;

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
        // Agent execution
        .route("/agents/{id}/run", post(handlers::run_agent))
        .with_state(state)
}
