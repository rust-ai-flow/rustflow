//! Playground HTTP handlers.
//!
//! Routes:
//!   `POST /playground/agents` — parse a YAML workflow and store it as an agent
//!
//! The playground UI is served separately via `pnpm run dev` (port 5173).

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::json;
use tracing::info;

use rustflow_core::workflow::WorkflowDef;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct PlaygroundCreateRequest {
    pub yaml: String,
}

/// `POST /playground/agents` — parse a YAML workflow, register it, and return its ID.
pub async fn playground_create_agent(
    State(state): State<AppState>,
    Json(req): Json<PlaygroundCreateRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
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
