use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

use rustflow_core::agent::Agent;
use rustflow_core::circuit_breaker::CircuitBreakerRegistry;
use rustflow_core::types::AgentId;
use rustflow_llm::{LlmGateway, providers::glm::GlmProvider, providers::ollama::OllamaProvider};
use rustflow_tools::security::{SecurityPolicy, ShellPolicy};
use rustflow_tools::{
    EnvTool, FileReadTool, FileWriteTool, HttpTool, JsonExtractTool, ShellTool, SleepTool,
    ToolRegistry,
};

/// Capacity of the broadcast channel per run.
/// At 1024 we can buffer a lot of step events before any subscriber falls behind.
const RUN_BROADCAST_CAPACITY: usize = 1024;

/// In-memory record of an active or completed workflow run.
/// Stored in `AppState::runs`, keyed by agent ID.
pub struct RunRecord {
    /// Stable identifier for this execution. Changes when a completed agent run
    /// is replaced by a new `/stream` request.
    pub run_id: String,
    /// Next zero-based sequence number to assign within this run.
    pub next_seq: u64,
    /// All events emitted so far (including the terminal event once `done`).
    pub events: Vec<crate::ws::WsEventEnvelope>,
    /// Broadcast channel — subscribers receive events emitted after they subscribe.
    pub sender: broadcast::Sender<crate::ws::WsEventEnvelope>,
    /// True once `workflow_completed` or `workflow_failed` has been appended.
    pub done: bool,
}

/// Snapshot plus live subscription for an active or recently completed run.
pub struct RunSubscription {
    pub run_id: String,
    pub events: Vec<crate::ws::WsEventEnvelope>,
    pub done: bool,
    pub receiver: broadcast::Receiver<crate::ws::WsEventEnvelope>,
}

/// Result of asking `/stream` to start execution.
pub enum RunStart {
    /// A new run record was created and the caller should spawn execution.
    Started(RunSubscription),
    /// An active run already exists and the caller should observe it.
    Existing(RunSubscription),
}

/// Shared application state injected into every request handler via axum's
/// `State` extractor.
#[derive(Clone)]
pub struct AppState {
    /// In-memory agent store.
    pub agents: Arc<RwLock<HashMap<String, Agent>>>,
    /// LLM gateway for executing LLM steps.
    pub llm_gateway: Arc<LlmGateway>,
    /// Tool registry for executing tool steps.
    pub tool_registry: Arc<ToolRegistry>,
    /// Circuit breakers shared by REST and WebSocket workflow execution.
    pub circuit_breakers: Arc<CircuitBreakerRegistry>,
    /// Active and recently completed run records, keyed by agent ID.
    pub runs: Arc<RwLock<HashMap<String, RunRecord>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self::with_shell_enabled(false)
    }

    pub fn with_shell_enabled(shell_enabled: bool) -> Self {
        let policy = Arc::new(SecurityPolicy {
            shell: ShellPolicy {
                enabled: shell_enabled,
                ..Default::default()
            },
            ..Default::default()
        });

        let mut tool_registry = ToolRegistry::new();
        tool_registry
            .register(HttpTool::with_policy(Arc::clone(&policy)))
            .ok();
        tool_registry
            .register(FileReadTool::with_policy(Arc::clone(&policy)))
            .ok();
        tool_registry
            .register(FileWriteTool::with_policy(Arc::clone(&policy)))
            .ok();
        tool_registry
            .register(ShellTool::with_policy(Arc::clone(&policy)))
            .ok();
        tool_registry.register(JsonExtractTool::new()).ok();
        tool_registry
            .register(EnvTool::with_policy(Arc::clone(&policy)))
            .ok();
        tool_registry.register(SleepTool::new()).ok();

        let mut llm_gateway = LlmGateway::new();
        llm_gateway.register(OllamaProvider::new());
        if std::env::var("GLM_API_KEY").is_ok() {
            llm_gateway.register(GlmProvider::from_env());
        }

        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            llm_gateway: Arc::new(llm_gateway),
            tool_registry: Arc::new(tool_registry),
            circuit_breakers: Arc::new(CircuitBreakerRegistry::default()),
            runs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_services(llm_gateway: LlmGateway, tool_registry: ToolRegistry) -> Self {
        Self::with_services_and_circuit_breakers(
            llm_gateway,
            tool_registry,
            Arc::new(CircuitBreakerRegistry::default()),
        )
    }

    pub fn with_services_and_circuit_breakers(
        llm_gateway: LlmGateway,
        tool_registry: ToolRegistry,
        circuit_breakers: Arc<CircuitBreakerRegistry>,
    ) -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            llm_gateway: Arc::new(llm_gateway),
            tool_registry: Arc::new(tool_registry),
            circuit_breakers,
            runs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // ── Agent store ───────────────────────────────────────────────────────────

    pub async fn upsert_agent(&self, agent: Agent) {
        let mut store = self.agents.write().await;
        store.insert(agent.id.as_str().to_string(), agent);
    }

    pub async fn get_agent(&self, id: &AgentId) -> Option<Agent> {
        let store = self.agents.read().await;
        store.get(id.as_str()).cloned()
    }

    pub async fn delete_agent(&self, id: &AgentId) -> Option<Agent> {
        let mut store = self.agents.write().await;
        store.remove(id.as_str())
    }

    pub async fn list_agents(&self) -> Vec<Agent> {
        let store = self.agents.read().await;
        store.values().cloned().collect()
    }

    // ── Run store ─────────────────────────────────────────────────────────────

    /// Create a fresh `RunRecord` for the given agent, replacing any prior one.
    /// Must be called before the scheduler starts emitting events.
    pub async fn create_run(&self, agent_id: String) {
        self.runs.write().await.insert(agent_id, new_run_record());
    }

    /// Start a new run if none is active, otherwise subscribe to the active run.
    ///
    /// Completed runs are retained for `/observe`, but a new `/stream` request
    /// replaces a completed record so users can rerun the same agent.
    pub async fn start_or_observe_run(&self, agent_id: String) -> RunStart {
        let mut store = self.runs.write().await;
        let should_start = store
            .get(&agent_id)
            .map(|record| record.done)
            .unwrap_or(true);

        if should_start {
            store.insert(agent_id.clone(), new_run_record());
        }

        let record = store
            .get(&agent_id)
            .expect("run record exists after start_or_observe_run");
        let subscription = RunSubscription {
            run_id: record.run_id.clone(),
            events: record.events.clone(),
            done: record.done,
            receiver: record.sender.subscribe(),
        };

        if should_start {
            RunStart::Started(subscription)
        } else {
            RunStart::Existing(subscription)
        }
    }

    /// Atomically snapshot past events and subscribe to future ones.
    ///
    /// Holding the read lock across both operations guarantees no events are
    /// missed between the snapshot and the subscription.
    pub async fn observe_run(&self, agent_id: &str) -> Option<RunSubscription> {
        let store = self.runs.read().await;
        let record = store.get(agent_id)?;
        Some(RunSubscription {
            run_id: record.run_id.clone(),
            events: record.events.clone(),
            done: record.done,
            receiver: record.sender.subscribe(),
        })
    }

    /// Append an event to the buffer and broadcast it to all subscribers.
    pub async fn emit_run_event(&self, agent_id: &str, event: crate::ws::WsEvent) {
        let mut store = self.runs.write().await;
        if let Some(record) = store.get_mut(agent_id) {
            let envelope =
                crate::ws::WsEventEnvelope::new(record.run_id.clone(), record.next_seq, event);
            record.next_seq += 1;
            let _ = record.sender.send(envelope.clone());
            record.events.push(envelope);
        }
    }

    /// Append the terminal event and mark the run as completed.
    pub async fn finish_run(&self, agent_id: &str, terminal: crate::ws::WsEvent) {
        let mut store = self.runs.write().await;
        if let Some(record) = store.get_mut(agent_id) {
            let envelope =
                crate::ws::WsEventEnvelope::new(record.run_id.clone(), record.next_seq, terminal);
            record.next_seq += 1;
            let _ = record.sender.send(envelope.clone());
            record.events.push(envelope);
            record.done = true;
        }
    }
}

fn new_run_record() -> RunRecord {
    let (tx, _) = broadcast::channel(RUN_BROADCAST_CAPACITY);
    RunRecord {
        run_id: uuid::Uuid::new_v4().to_string(),
        next_seq: 0,
        events: vec![],
        sender: tx,
        done: false,
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ws::WsEvent;

    #[tokio::test]
    async fn test_start_or_observe_run_creates_missing_run() {
        let state = AppState::new();

        let result = state.start_or_observe_run("agent-1".to_string()).await;

        match result {
            RunStart::Started(subscription) => {
                assert!(!subscription.run_id.is_empty());
                assert!(subscription.events.is_empty());
                assert!(!subscription.done);
            }
            RunStart::Existing(_) => panic!("missing run should be started"),
        }
    }

    #[tokio::test]
    async fn test_start_or_observe_run_observes_active_run() {
        let state = AppState::new();
        let _ = state.start_or_observe_run("agent-1".to_string()).await;
        state
            .emit_run_event(
                "agent-1",
                WsEvent::StepStarted {
                    step_id: "a".to_string(),
                    step_name: "A".to_string(),
                },
            )
            .await;

        let result = state.start_or_observe_run("agent-1".to_string()).await;

        match result {
            RunStart::Existing(subscription) => {
                assert_eq!(subscription.events.len(), 1);
                assert_eq!(subscription.events[0].run_id, subscription.run_id);
                assert_eq!(subscription.events[0].seq, 0);
                assert!(!subscription.done);
            }
            RunStart::Started(_) => panic!("active run should be observed"),
        }
    }

    #[tokio::test]
    async fn test_observe_run_replays_completed_terminal_event() {
        let state = AppState::new();
        let _ = state.start_or_observe_run("agent-1".to_string()).await;
        state
            .finish_run(
                "agent-1",
                WsEvent::WorkflowFailed {
                    error: "boom".to_string(),
                },
            )
            .await;

        let subscription = state.observe_run("agent-1").await.unwrap();

        assert!(subscription.done);
        assert_eq!(subscription.events.len(), 1);
        let event = &subscription.events[0];
        assert_eq!(event.run_id, subscription.run_id);
        assert_eq!(event.seq, 0);
        assert!(matches!(
            &event.event,
            WsEvent::WorkflowFailed { error } if error == "boom"
        ));
    }

    #[tokio::test]
    async fn test_start_or_observe_run_replaces_completed_run() {
        let state = AppState::new();
        let _ = state.start_or_observe_run("agent-1".to_string()).await;
        state
            .finish_run(
                "agent-1",
                WsEvent::WorkflowCompleted {
                    outputs: serde_json::json!({ "a": "done" }),
                },
            )
            .await;

        let result = state.start_or_observe_run("agent-1".to_string()).await;

        match result {
            RunStart::Started(subscription) => {
                assert!(!subscription.run_id.is_empty());
                assert!(subscription.events.is_empty());
                assert!(!subscription.done);
            }
            RunStart::Existing(_) => panic!("completed run should be replaced for a new stream"),
        }
    }

    #[tokio::test]
    async fn test_run_events_keep_run_id_and_monotonic_sequence_for_replay_and_live() {
        let state = AppState::new();
        let result = state.start_or_observe_run("agent-1".to_string()).await;
        let mut live_receiver = match result {
            RunStart::Started(subscription) => subscription.receiver,
            RunStart::Existing(_) => panic!("missing run should be started"),
        };

        state
            .emit_run_event(
                "agent-1",
                WsEvent::StepStarted {
                    step_id: "a".to_string(),
                    step_name: "A".to_string(),
                },
            )
            .await;
        state
            .finish_run(
                "agent-1",
                WsEvent::WorkflowCompleted {
                    outputs: serde_json::json!({ "a": "done" }),
                },
            )
            .await;

        let first_live = live_receiver.recv().await.unwrap();
        let second_live = live_receiver.recv().await.unwrap();
        assert_eq!(first_live.seq, 0);
        assert_eq!(second_live.seq, 1);
        assert_eq!(first_live.run_id, second_live.run_id);

        let replay = state.observe_run("agent-1").await.unwrap();
        assert!(replay.done);
        assert_eq!(replay.events.len(), 2);
        assert_eq!(replay.events[0].run_id, first_live.run_id);
        assert_eq!(replay.events[0].seq, 0);
        assert_eq!(replay.events[1].run_id, first_live.run_id);
        assert_eq!(replay.events[1].seq, 1);
    }

    #[tokio::test]
    async fn test_new_stream_after_completed_run_gets_new_run_id_and_resets_sequence() {
        let state = AppState::new();
        let first_run_id = match state.start_or_observe_run("agent-1".to_string()).await {
            RunStart::Started(subscription) => subscription.run_id,
            RunStart::Existing(_) => panic!("missing run should be started"),
        };
        state
            .finish_run(
                "agent-1",
                WsEvent::WorkflowCompleted {
                    outputs: serde_json::json!({ "a": "done" }),
                },
            )
            .await;

        let second_run_id = match state.start_or_observe_run("agent-1".to_string()).await {
            RunStart::Started(subscription) => subscription.run_id,
            RunStart::Existing(_) => panic!("completed run should be replaced"),
        };
        state
            .emit_run_event(
                "agent-1",
                WsEvent::StepStarted {
                    step_id: "b".to_string(),
                    step_name: "B".to_string(),
                },
            )
            .await;

        let replay = state.observe_run("agent-1").await.unwrap();
        assert_ne!(first_run_id, second_run_id);
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.events[0].run_id, second_run_id);
        assert_eq!(replay.events[0].seq, 0);
    }
}
