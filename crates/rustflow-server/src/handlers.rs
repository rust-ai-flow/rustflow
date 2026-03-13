use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{info, warn};

use rustflow_core::agent::Agent;
use rustflow_core::context::Context;
use rustflow_core::step::Step;
use rustflow_core::types::{AgentId, Value};
use rustflow_orchestrator::{DefaultStepExecutor, Scheduler};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

// ── Request / response shapes ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateAgentRequest {
    pub name: String,
    pub description: Option<String>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Serialize)]
pub struct AgentSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub step_count: usize,
    pub created_at: String,
}

impl From<&Agent> for AgentSummary {
    fn from(agent: &Agent) -> Self {
        Self {
            id: agent.id.as_str().to_string(),
            name: agent.name.clone(),
            description: agent.description.clone(),
            step_count: agent.steps.len(),
            created_at: agent.created_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RunAgentRequest {
    /// Optional input variables to inject into the execution context.
    #[serde(default)]
    pub vars: HashMap<String, serde_json::Value>,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// POST /agents — Create a new agent.
pub async fn create_agent(
    State(state): State<AppState>,
    Json(req): Json<CreateAgentRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let mut agent = Agent::new(req.name, req.steps);
    if let Some(desc) = req.description {
        agent = agent.with_description(desc);
    }

    let id = agent.id.as_str().to_string();
    info!(agent_id = %id, "creating agent");

    state.upsert_agent(agent).await;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": id,
            "message": "agent created",
        })),
    ))
}

/// GET /agents — List all agents.
pub async fn list_agents(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let agents = state.list_agents().await;
    let summaries: Vec<AgentSummary> = agents.iter().map(AgentSummary::from).collect();
    let count = summaries.len();
    Ok(Json(json!({ "agents": summaries, "count": count })))
}

/// GET /agents/:id — Fetch a single agent.
pub async fn get_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let agent_id = AgentId::new(&id);
    let agent = state
        .get_agent(&agent_id)
        .await
        .ok_or_else(|| ApiError::NotFound(format!("agent '{id}' not found")))?;

    Ok(Json(
        serde_json::to_value(&agent).map_err(|e| ApiError::Internal(e.to_string()))?,
    ))
}

/// DELETE /agents/:id — Delete an agent.
pub async fn delete_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let agent_id = AgentId::new(&id);
    match state.delete_agent(&agent_id).await {
        Some(_) => {
            info!(agent_id = %id, "agent deleted");
            Ok(Json(json!({ "message": format!("agent '{id}' deleted") })))
        }
        None => {
            warn!(agent_id = %id, "attempted to delete non-existent agent");
            Err(ApiError::NotFound(format!("agent '{id}' not found")))
        }
    }
}

/// POST /agents/:id/run — Execute an agent's workflow.
pub async fn run_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<RunAgentRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let agent_id = AgentId::new(&id);
    let agent = state
        .get_agent(&agent_id)
        .await
        .ok_or_else(|| ApiError::NotFound(format!("agent '{id}' not found")))?;

    info!(agent_id = %id, "running agent");

    // Build execution context with input variables.
    let mut ctx = Context::for_agent(agent.id.clone());
    for (key, value) in req.vars {
        ctx.set_var(key, Value::from(value));
    }

    // Create executor and scheduler.
    let executor = Arc::new(DefaultStepExecutor::new(
        Arc::clone(&state.llm_gateway),
        Arc::clone(&state.tool_registry),
    ));
    let scheduler = Scheduler::new(executor);

    // Run the workflow.
    let result_ctx = scheduler
        .run(&agent.steps, ctx)
        .await
        .map_err(|e| ApiError::Internal(format!("execution failed: {e}")))?;

    // Collect outputs.
    let outputs: serde_json::Map<String, serde_json::Value> = result_ctx
        .step_outputs
        .iter()
        .map(|(k, v)| (k.clone(), v.inner().clone()))
        .collect();

    Ok(Json(json!({
        "agent_id": id,
        "status": "completed",
        "outputs": outputs,
    })))
}

/// GET /health — Health check.
pub async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
