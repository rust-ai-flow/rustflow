use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

use rustflow_core::agent::Agent;
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
    /// All events emitted so far (including the terminal event once `done`).
    pub events: Vec<crate::ws::WsEvent>,
    /// Broadcast channel — subscribers receive events emitted after they subscribe.
    pub sender: broadcast::Sender<crate::ws::WsEvent>,
    /// True once `workflow_completed` or `workflow_failed` has been appended.
    pub done: bool,
}

/// Snapshot plus live subscription for an active or recently completed run.
pub struct RunSubscription {
    pub events: Vec<crate::ws::WsEvent>,
    pub done: bool,
    pub receiver: broadcast::Receiver<crate::ws::WsEvent>,
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
        tool_registry.register(HttpTool::new()).ok();
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
            runs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_services(llm_gateway: LlmGateway, tool_registry: ToolRegistry) -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            llm_gateway: Arc::new(llm_gateway),
            tool_registry: Arc::new(tool_registry),
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
        let (tx, _) = broadcast::channel(RUN_BROADCAST_CAPACITY);
        let record = RunRecord {
            events: vec![],
            sender: tx,
            done: false,
        };
        self.runs.write().await.insert(agent_id, record);
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
            let (tx, _) = broadcast::channel(RUN_BROADCAST_CAPACITY);
            store.insert(
                agent_id.clone(),
                RunRecord {
                    events: vec![],
                    sender: tx,
                    done: false,
                },
            );
        }

        let record = store
            .get(&agent_id)
            .expect("run record exists after start_or_observe_run");
        let subscription = RunSubscription {
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
            events: record.events.clone(),
            done: record.done,
            receiver: record.sender.subscribe(),
        })
    }

    /// Append an event to the buffer and broadcast it to all subscribers.
    pub async fn emit_run_event(&self, agent_id: &str, event: crate::ws::WsEvent) {
        let mut store = self.runs.write().await;
        if let Some(record) = store.get_mut(agent_id) {
            let _ = record.sender.send(event.clone());
            record.events.push(event);
        }
    }

    /// Append the terminal event and mark the run as completed.
    pub async fn finish_run(&self, agent_id: &str, terminal: crate::ws::WsEvent) {
        let mut store = self.runs.write().await;
        if let Some(record) = store.get_mut(agent_id) {
            let _ = record.sender.send(terminal.clone());
            record.events.push(terminal);
            record.done = true;
        }
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
        assert!(matches!(
            subscription.events.as_slice(),
            [WsEvent::WorkflowFailed { error }] if error == "boom"
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
                assert!(subscription.events.is_empty());
                assert!(!subscription.done);
            }
            RunStart::Existing(_) => panic!("completed run should be replaced for a new stream"),
        }
    }
}
