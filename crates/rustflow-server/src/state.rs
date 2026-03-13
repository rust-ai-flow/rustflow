use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use rustflow_core::agent::Agent;
use rustflow_core::types::AgentId;
use rustflow_llm::LlmGateway;
use rustflow_tools::ToolRegistry;

/// Shared application state injected into every request handler via axum's
/// `State` extractor.
#[derive(Clone)]
pub struct AppState {
    /// In-memory agent store (will be replaced with a database later).
    pub agents: Arc<RwLock<HashMap<String, Agent>>>,
    /// LLM gateway for executing LLM steps.
    pub llm_gateway: Arc<LlmGateway>,
    /// Tool registry for executing tool steps.
    pub tool_registry: Arc<ToolRegistry>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            llm_gateway: Arc::new(LlmGateway::new()),
            tool_registry: Arc::new(ToolRegistry::new()),
        }
    }

    /// Create with pre-configured LLM gateway and tool registry.
    pub fn with_services(llm_gateway: LlmGateway, tool_registry: ToolRegistry) -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            llm_gateway: Arc::new(llm_gateway),
            tool_registry: Arc::new(tool_registry),
        }
    }

    /// Insert an agent, replacing any existing entry with the same ID.
    pub async fn upsert_agent(&self, agent: Agent) {
        let mut store = self.agents.write().await;
        store.insert(agent.id.as_str().to_string(), agent);
    }

    /// Retrieve a clone of an agent by ID.
    pub async fn get_agent(&self, id: &AgentId) -> Option<Agent> {
        let store = self.agents.read().await;
        store.get(id.as_str()).cloned()
    }

    /// Delete an agent, returning the removed value if it existed.
    pub async fn delete_agent(&self, id: &AgentId) -> Option<Agent> {
        let mut store = self.agents.write().await;
        store.remove(id.as_str())
    }

    /// List all agents.
    pub async fn list_agents(&self) -> Vec<Agent> {
        let store = self.agents.read().await;
        store.values().cloned().collect()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
