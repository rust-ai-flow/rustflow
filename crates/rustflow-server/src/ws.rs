//! WebSocket streaming handler.
//!
//! # Endpoints
//!
//! ## `GET /agents/{id}/stream`
//!
//! Start a new workflow execution and stream its events.
//! If a run is already active for this agent, the client is attached as an
//! observer instead (no duplicate execution is started).
//!
//! ## `GET /agents/{id}/observe`
//!
//! Attach to an existing run as a read-only observer.  Replays all events
//! emitted so far, then streams live events until the workflow finishes.
//! Returns `workflow_failed` immediately if no active run exists.
//!
//! # Protocol
//!
//! 1. Client connects (WebSocket upgrade).
//! 2. Client sends a single JSON "start" frame:
//!    ```json
//!    { "vars": { "key": "value" } }
//!    ```
//!    `vars` is used only by `/stream` when starting a new run; `/observe`
//!    ignores it.
//! 3. Server streams event frames:
//!    ```json
//!    {"type":"step_started",    "step_id":"…","step_name":"…"}
//!    {"type":"step_succeeded",  "step_id":"…","elapsed_ms":820,"output":{…}}
//!    {"type":"step_failed",     "step_id":"…","error":"…","will_retry":true,"attempt":1,"elapsed_ms":12}
//!    {"type":"step_retrying",   "step_id":"…","attempt":2}
//!    {"type":"circuit_breaker_opened","resource":"ollama"}
//!    {"type":"circuit_breaker_closed","resource":"ollama"}
//!    {"type":"workflow_completed","outputs":{…}}
//!    {"type":"workflow_failed","error":"…"}
//!    ```
//! 4. Server closes the connection after `workflow_completed` or
//!    `workflow_failed`.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tracing::{info, warn};

use rustflow_core::agent::Agent;
use rustflow_core::context::Context;
use rustflow_core::types::{AgentId, Value};
use rustflow_orchestrator::{DefaultStepExecutor, Scheduler, SchedulerEvent};

use crate::state::AppState;

// ── WsEvent ───────────────────────────────────────────────────────────────────

/// JSON frame sent from the server to a connected client.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsEvent {
    StepStarted {
        step_id: String,
        step_name: String,
    },
    StepSucceeded {
        step_id: String,
        step_name: String,
        elapsed_ms: u64,
        output: serde_json::Value,
    },
    StepFailed {
        step_id: String,
        step_name: String,
        error: String,
        will_retry: bool,
        attempt: u32,
        elapsed_ms: u64,
    },
    StepRetrying {
        step_id: String,
        step_name: String,
        attempt: u32,
    },
    CircuitBreakerOpened {
        resource: String,
    },
    CircuitBreakerClosed {
        resource: String,
    },
    WorkflowCompleted {
        outputs: serde_json::Value,
    },
    WorkflowFailed {
        error: String,
    },
}

impl From<SchedulerEvent> for WsEvent {
    fn from(event: SchedulerEvent) -> Self {
        match event {
            SchedulerEvent::StepStarted { step_id, step_name } => {
                WsEvent::StepStarted { step_id, step_name }
            }
            SchedulerEvent::StepSucceeded { step_id, step_name, elapsed, output } => {
                WsEvent::StepSucceeded {
                    step_id,
                    step_name,
                    elapsed_ms: elapsed.as_millis() as u64,
                    output,
                }
            }
            SchedulerEvent::StepFailed {
                step_id,
                step_name,
                error,
                will_retry,
                attempt,
                elapsed,
            } => WsEvent::StepFailed {
                step_id,
                step_name,
                error,
                will_retry,
                attempt,
                elapsed_ms: elapsed.as_millis() as u64,
            },
            SchedulerEvent::StepRetrying { step_id, step_name, attempt } => {
                WsEvent::StepRetrying { step_id, step_name, attempt }
            }
            SchedulerEvent::CircuitBreakerOpened { resource } => {
                WsEvent::CircuitBreakerOpened { resource }
            }
            SchedulerEvent::CircuitBreakerClosed { resource } => {
                WsEvent::CircuitBreakerClosed { resource }
            }
        }
    }
}

// ── StartMessage ──────────────────────────────────────────────────────────────

/// The single JSON message the client sends before execution begins.
#[derive(Debug, Deserialize, Default)]
pub struct StartMessage {
    /// Input variables (used only when starting a new run).
    #[serde(default)]
    pub vars: HashMap<String, serde_json::Value>,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// `GET /agents/{id}/stream` — start or join a run and stream events.
pub async fn stream_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = run_socket(socket, state, id, false).await {
            warn!("WebSocket (stream) error: {e}");
        }
    })
}

/// `GET /agents/{id}/observe` — observe an existing run (read-only).
pub async fn observe_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = run_socket(socket, state, id, true).await {
            warn!("WebSocket (observe) error: {e}");
        }
    })
}

// ── Core socket logic ─────────────────────────────────────────────────────────

async fn run_socket(
    mut socket: WebSocket,
    state: AppState,
    id: String,
    observe_only: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!(agent_id = %id, observe_only, "WebSocket session started");

    // 1. Read the start message (vars only matter for new runs).
    let start: StartMessage = match socket.recv().await {
        Some(Ok(Message::Text(text))) => serde_json::from_str(&text).unwrap_or_default(),
        Some(Ok(Message::Close(_))) | None => {
            info!(agent_id = %id, "client disconnected before sending start message");
            return Ok(());
        }
        Some(Ok(_)) => StartMessage::default(),
        Some(Err(e)) => return Err(e.into()),
    };

    // 2. Look up the agent.
    let agent_id = AgentId::new(&id);
    let agent = match state.get_agent(&agent_id).await {
        Some(a) => a,
        None => {
            let msg = serde_json::to_string(&WsEvent::WorkflowFailed {
                error: format!("agent '{id}' not found"),
            })?;
            let _ = socket.send(Message::Text(msg.into())).await;
            return Ok(());
        }
    };

    // 3. Check for an existing run.
    let has_run = state.observe_run(&id).await.is_some();

    if !has_run {
        if observe_only {
            // Observe-only with no active run — tell the client.
            let msg = serde_json::to_string(&WsEvent::WorkflowFailed {
                error: format!("no active run for agent '{id}'"),
            })?;
            let _ = socket.send(Message::Text(msg.into())).await;
            return Ok(());
        }

        // Start a new run in the background.
        state.create_run(id.clone()).await;
        let state_bg = state.clone();
        let id_bg = id.clone();
        let vars = start.vars;
        tokio::spawn(async move {
            run_workflow_bg(state_bg, id_bg, agent, vars).await;
        });
    }

    // 4. Observe the run (works for both new and pre-existing runs).
    //    `observe_run` is safe to call immediately after `create_run` because
    //    the scheduler (spawned above) must acquire a write lock to emit its
    //    first event, while we hold a read lock here — no race.
    let (past, done, rx) = state.observe_run(&id).await.unwrap();
    forward_to_socket(socket, past, done, rx).await?;

    info!(agent_id = %id, "WebSocket session closed");
    Ok(())
}

// ── Background executor ───────────────────────────────────────────────────────

/// Run a workflow to completion, emitting all events into the shared run store.
/// The task is completely independent of any WebSocket connection.
async fn run_workflow_bg(
    state: AppState,
    id: String,
    agent: Agent,
    vars: HashMap<String, serde_json::Value>,
) {
    // Build context.
    let mut ctx = Context::for_agent(agent.id.clone());
    for (key, value) in vars {
        ctx.set_var(key, Value::from(value));
    }

    // Bridge sync scheduler callbacks → async state via an mpsc channel.
    let (tx, mut rx) = mpsc::unbounded_channel::<WsEvent>();

    // Forward task: drains the mpsc and writes to the run store.
    let state_fwd = state.clone();
    let id_fwd = id.clone();
    let fwd_task = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            state_fwd.emit_run_event(&id_fwd, event).await;
        }
    });

    let executor = Arc::new(DefaultStepExecutor::new(
        Arc::clone(&state.llm_gateway),
        Arc::clone(&state.tool_registry),
    ));
    let scheduler = Scheduler::new(executor);

    let result = scheduler
        .run_with_events(&agent.steps, ctx, move |event| {
            let _ = tx.send(WsEvent::from(event));
        })
        .await;

    // Wait for all mid-run events to be flushed before writing the terminal.
    let _ = fwd_task.await;

    let terminal = match result {
        Ok(ctx) => {
            let outputs: serde_json::Map<String, serde_json::Value> = ctx
                .step_outputs
                .iter()
                .map(|(k, v)| (k.clone(), v.inner().clone()))
                .collect();
            WsEvent::WorkflowCompleted { outputs: serde_json::Value::Object(outputs) }
        }
        Err(e) => WsEvent::WorkflowFailed { error: e.to_string() },
    };

    state.finish_run(&id, terminal).await;
}

// ── Event forwarding ──────────────────────────────────────────────────────────

/// Replay past events to `socket`, then stream live ones until the workflow
/// terminates or the client disconnects.
async fn forward_to_socket(
    mut socket: WebSocket,
    past: Vec<WsEvent>,
    done: bool,
    mut rx: broadcast::Receiver<WsEvent>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Replay buffered events.
    for event in &past {
        let msg = serde_json::to_string(event)?;
        if socket.send(Message::Text(msg.into())).await.is_err() {
            return Ok(()); // client disconnected mid-replay
        }
    }

    if done {
        // All events (including terminal) already replayed.
        let _ = socket.send(Message::Close(None)).await;
        return Ok(());
    }

    // Stream live events.
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        let is_terminal = matches!(
                            event,
                            WsEvent::WorkflowCompleted { .. } | WsEvent::WorkflowFailed { .. }
                        );
                        let msg = serde_json::to_string(&event)?;
                        if socket.send(Message::Text(msg.into())).await.is_err() {
                            break; // client disconnected — scheduler keeps running
                        }
                        if is_terminal {
                            let _ = socket.send(Message::Close(None)).await;
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Observer lagged by {n} messages — disconnecting");
                        break;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        // Sender dropped before terminal arrived — shouldn't happen normally.
                        break;
                    }
                }
            }
            client_msg = socket.recv() => {
                match client_msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        let _ = socket.send(Message::Pong(data)).await;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_step_succeeded() -> SchedulerEvent {
        SchedulerEvent::StepSucceeded {
            step_id: "step-1".to_string(),
            step_name: "My Step".to_string(),
            elapsed: Duration::from_millis(820),
            output: serde_json::json!({"result": "ok"}),
        }
    }

    #[test]
    fn test_ws_event_step_started_serializes() {
        let event = WsEvent::StepStarted {
            step_id: "fetch".to_string(),
            step_name: "Fetch Data".to_string(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "step_started");
        assert_eq!(json["step_id"], "fetch");
        assert_eq!(json["step_name"], "Fetch Data");
    }

    #[test]
    fn test_ws_event_step_succeeded_serializes() {
        let event = WsEvent::StepSucceeded {
            step_id: "s1".to_string(),
            step_name: "Step 1".to_string(),
            elapsed_ms: 820,
            output: serde_json::json!({"content": "hello"}),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "step_succeeded");
        assert_eq!(json["elapsed_ms"], 820);
        assert_eq!(json["output"]["content"], "hello");
    }

    #[test]
    fn test_ws_event_step_failed_serializes() {
        let event = WsEvent::StepFailed {
            step_id: "s2".to_string(),
            step_name: "Step 2".to_string(),
            error: "timeout".to_string(),
            will_retry: true,
            attempt: 1,
            elapsed_ms: 5000,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "step_failed");
        assert_eq!(json["will_retry"], true);
        assert_eq!(json["attempt"], 1);
    }

    #[test]
    fn test_ws_event_workflow_completed_serializes() {
        let outputs = serde_json::json!({"step-1": "result"});
        let event = WsEvent::WorkflowCompleted { outputs };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "workflow_completed");
        assert_eq!(json["outputs"]["step-1"], "result");
    }

    #[test]
    fn test_ws_event_workflow_failed_serializes() {
        let event = WsEvent::WorkflowFailed {
            error: "step exhausted retries".to_string(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "workflow_failed");
        assert_eq!(json["error"], "step exhausted retries");
    }

    #[test]
    fn test_ws_event_circuit_breaker_opened_serializes() {
        let event = WsEvent::CircuitBreakerOpened {
            resource: "ollama".to_string(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "circuit_breaker_opened");
        assert_eq!(json["resource"], "ollama");
    }

    #[test]
    fn test_from_scheduler_event_step_started() {
        let se = SchedulerEvent::StepStarted {
            step_id: "a".to_string(),
            step_name: "A".to_string(),
        };
        let ws = WsEvent::from(se);
        assert!(matches!(ws, WsEvent::StepStarted { .. }));
        let json = serde_json::to_value(&ws).unwrap();
        assert_eq!(json["type"], "step_started");
    }

    #[test]
    fn test_from_scheduler_event_step_succeeded() {
        let ws = WsEvent::from(make_step_succeeded());
        assert!(matches!(ws, WsEvent::StepSucceeded { elapsed_ms: 820, .. }));
        let json = serde_json::to_value(&ws).unwrap();
        assert_eq!(json["elapsed_ms"], 820);
        assert_eq!(json["output"]["result"], "ok");
    }

    #[test]
    fn test_from_scheduler_event_step_failed() {
        let se = SchedulerEvent::StepFailed {
            step_id: "b".to_string(),
            step_name: "B".to_string(),
            error: "boom".to_string(),
            will_retry: false,
            attempt: 3,
            elapsed: Duration::from_secs(1),
        };
        let ws = WsEvent::from(se);
        assert!(matches!(ws, WsEvent::StepFailed { will_retry: false, attempt: 3, .. }));
    }

    #[test]
    fn test_from_scheduler_event_step_retrying() {
        let se = SchedulerEvent::StepRetrying {
            step_id: "c".to_string(),
            step_name: "C".to_string(),
            attempt: 2,
        };
        let ws = WsEvent::from(se);
        assert!(matches!(ws, WsEvent::StepRetrying { attempt: 2, .. }));
    }

    #[test]
    fn test_from_scheduler_event_cb_opened() {
        let se = SchedulerEvent::CircuitBreakerOpened {
            resource: "http".to_string(),
        };
        let ws = WsEvent::from(se);
        let json = serde_json::to_value(&ws).unwrap();
        assert_eq!(json["type"], "circuit_breaker_opened");
        assert_eq!(json["resource"], "http");
    }

    #[test]
    fn test_from_scheduler_event_cb_closed() {
        let se = SchedulerEvent::CircuitBreakerClosed {
            resource: "http".to_string(),
        };
        let ws = WsEvent::from(se);
        let json = serde_json::to_value(&ws).unwrap();
        assert_eq!(json["type"], "circuit_breaker_closed");
    }

    #[test]
    fn test_start_message_deserializes_with_vars() {
        let json = r#"{"vars": {"topic": "Rust", "lang": "en"}}"#;
        let msg: StartMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.vars.len(), 2);
        assert_eq!(msg.vars["topic"], "Rust");
    }

    #[test]
    fn test_start_message_deserializes_empty() {
        let msg: StartMessage = serde_json::from_str("{}").unwrap();
        assert!(msg.vars.is_empty());
    }

    #[test]
    fn test_start_message_default() {
        let msg = StartMessage::default();
        assert!(msg.vars.is_empty());
    }
}
