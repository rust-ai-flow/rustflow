use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

use rustflow_core::agent::Agent;
use rustflow_core::types::AgentId;
use rustflow_llm::{LlmGateway, providers::glm::GlmProvider, providers::ollama::OllamaProvider};
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
        let mut tool_registry = ToolRegistry::new();
        tool_registry.register(HttpTool::new()).ok();
        tool_registry.register(FileReadTool::new()).ok();
        tool_registry.register(FileWriteTool::new()).ok();
        tool_registry.register(ShellTool::new()).ok();
        tool_registry.register(JsonExtractTool::new()).ok();
        tool_registry.register(EnvTool::new()).ok();
        tool_registry.register(SleepTool::new()).ok();

        let mut llm_gateway = LlmGateway::new();
        llm_gateway.register(OllamaProvider::new());
        llm_gateway.register(GlmProvider::from_env());

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

    /// Atomically snapshot past events and subscribe to future ones.
    ///
    /// Holding the read lock across both operations guarantees no events are
    /// missed between the snapshot and the subscription.
    pub async fn observe_run(
        &self,
        agent_id: &str,
    ) -> Option<(
        Vec<crate::ws::WsEvent>,
        bool,
        broadcast::Receiver<crate::ws::WsEvent>,
    )> {
        let store = self.runs.read().await;
        let record = store.get(agent_id)?;
        let events = record.events.clone();
        let done = record.done;
        let rx = record.sender.subscribe();
        Some((events, done, rx))
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
