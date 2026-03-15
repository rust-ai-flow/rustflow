//! Playground HTTP handlers.
//!
//! Routes:
//!   `GET  /playground`        — serve the embedded playground HTML
//!   `POST /playground/agents` — parse a YAML workflow and store it as an agent

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse};
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::path::Path as StdPath;
use tracing::info;

use rustflow_core::workflow::WorkflowDef;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// The playground HTML, embedded at compile time from the pre-built dist file.
pub static PLAYGROUND_HTML: &str = include_str!("../../../apps/playground/dist/index.html");

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

/// `GET /playground/assets/*path` — serve static assets for the playground.
pub async fn playground_assets(Path(path): Path<String>) -> impl IntoResponse {
    // Build the full path to the asset file
    let asset_path = StdPath::new("apps/playground/dist/assets").join(&path);

    // Check if the file exists and is a file (not a directory)
    if !asset_path.exists() || !asset_path.is_file() {
        return (StatusCode::NOT_FOUND, "File not found").into_response();
    }

    // Read the file content
    match fs::read(&asset_path) {
        Ok(content) => {
            // Set the appropriate Content-Type header based on file extension
            let mut headers = HeaderMap::new();
            if let Some(ext) = asset_path.extension() {
                match ext.to_str() {
                    Some("js") => headers.insert(
                        "Content-Type",
                        HeaderValue::from_static("application/javascript"),
                    ),
                    Some("css") => {
                        headers.insert("Content-Type", HeaderValue::from_static("text/css"))
                    }
                    Some("png") => {
                        headers.insert("Content-Type", HeaderValue::from_static("image/png"))
                    }
                    Some("jpg") | Some("jpeg") => {
                        headers.insert("Content-Type", HeaderValue::from_static("image/jpeg"))
                    }
                    Some("svg") => {
                        headers.insert("Content-Type", HeaderValue::from_static("image/svg+xml"))
                    }
                    _ => None,
                };
            }

            (StatusCode::OK, headers, content).into_response()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read file").into_response(),
    }
}
