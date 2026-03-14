//! Playground HTTP handlers.
//!
//! Routes:
//!   `GET  /playground`        — serve the embedded playground HTML
//!   `POST /playground/agents` — parse a YAML workflow and store it as an agent

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use serde::Deserialize;
use serde_json::json;
use tracing::info;

use rustflow_core::workflow::WorkflowDef;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// The playground HTML, embedded at compile time from the pre-built dist file.
pub static PLAYGROUND_HTML: &str =
    include_str!("../../../apps/playground/dist/index.html");

// ── Request / response ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PlaygroundCreateRequest {
    pub yaml: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `GET /playground` — serve the single-page playground application.
pub async fn playground_index() -> impl IntoResponse {
    Html(PLAYGROUND_HTML)
}

/// `POST /playground/agents` — parse a YAML workflow, register it, and return its ID.
pub async fn playground_create_agent(
    State(state): State<AppState>,
    Json(req): Json<PlaygroundCreateRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    // Parse the YAML into a WorkflowDef and then into an Agent.
    let workflow_def = WorkflowDef::from_yaml(&req.yaml)
        .map_err(|e| ApiError::BadRequest(format!("invalid workflow YAML: {e}")))?;

    let agent = workflow_def
        .into_agent()
        .map_err(|e| ApiError::BadRequest(format!("workflow validation failed: {e}")))?
        .with_yaml(req.yaml);

    let id = agent.id.as_str().to_string();
    info!(agent_id = %id, name = %agent.name, "playground agent created");

    state.upsert_agent(agent).await;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": id,
            "message": "agent created",
        })),
    ))
}
