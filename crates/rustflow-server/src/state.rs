use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::warn;

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

/// Default local run replay store used by the server and playground.
///
/// Override with `RUSTFLOW_RUN_STORE_DIR` when a process needs a different
/// runtime location.
const DEFAULT_RUN_STORE_DIR: &str = ".rustflow/runs";

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

/// Best-effort local-disk store for recently emitted WebSocket run events.
#[derive(Debug)]
pub struct RunStore {
    root: PathBuf,
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
    /// Optional local-disk replay store for run event recovery.
    pub run_store: Option<Arc<RunStore>>,
}

impl AppState {
    pub fn default_run_store_path() -> PathBuf {
        std::env::var_os("RUSTFLOW_RUN_STORE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_RUN_STORE_DIR))
    }

    pub fn new() -> Self {
        Self::with_shell_enabled(false)
    }

    pub fn with_shell_enabled(shell_enabled: bool) -> Self {
        Self::build_with_default_services(shell_enabled, None)
    }

    pub fn with_run_store_path(path: impl Into<PathBuf>) -> Self {
        Self::with_shell_enabled_and_run_store_path(false, path)
    }

    pub fn with_shell_enabled_and_run_store_path(
        shell_enabled: bool,
        path: impl Into<PathBuf>,
    ) -> Self {
        Self::build_with_default_services(shell_enabled, Some(path.into()))
    }

    fn build_with_default_services(shell_enabled: bool, run_store_path: Option<PathBuf>) -> Self {
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

        Self::from_services_and_optional_run_store(
            llm_gateway,
            tool_registry,
            Arc::new(CircuitBreakerRegistry::default()),
            run_store_path,
        )
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
        Self::from_services_and_optional_run_store(
            llm_gateway,
            tool_registry,
            circuit_breakers,
            None,
        )
    }

    fn from_services_and_optional_run_store(
        llm_gateway: LlmGateway,
        tool_registry: ToolRegistry,
        circuit_breakers: Arc<CircuitBreakerRegistry>,
        run_store_path: Option<PathBuf>,
    ) -> Self {
        let run_store = run_store_path.map(|path| Arc::new(RunStore::new(path)));
        let runs = run_store
            .as_ref()
            .map(|store| store.recover_runs())
            .unwrap_or_default();

        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            llm_gateway: Arc::new(llm_gateway),
            tool_registry: Arc::new(tool_registry),
            circuit_breakers,
            runs: Arc::new(RwLock::new(runs)),
            run_store,
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
        let snapshot = {
            let mut store = self.runs.write().await;
            store.insert(agent_id.clone(), new_run_record());
            store
                .get(&agent_id)
                .map(|record| PersistedRunRecord::from_record(&agent_id, record))
        };
        self.persist_run_snapshot(snapshot.as_ref());
    }

    /// Start a new run if none is active, otherwise subscribe to the active run.
    ///
    /// Completed runs are retained for `/observe`, but a new `/stream` request
    /// replaces a completed record so users can rerun the same agent.
    pub async fn start_or_observe_run(&self, agent_id: String) -> RunStart {
        let (start, snapshot) = {
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
            let snapshot = should_start.then(|| PersistedRunRecord::from_record(&agent_id, record));
            let start = if should_start {
                RunStart::Started(subscription)
            } else {
                RunStart::Existing(subscription)
            };
            (start, snapshot)
        };

        self.persist_run_snapshot(snapshot.as_ref());
        start
    }

    /// Atomically snapshot past events and subscribe to future ones.
    ///
    /// Holding the read lock across both operations guarantees no events are
    /// missed between the snapshot and the subscription.
    pub async fn observe_run(&self, agent_id: &str) -> Option<RunSubscription> {
        self.observe_run_since(agent_id, None).await
    }

    /// Atomically snapshot past events after `since_seq` and subscribe to future ones.
    ///
    /// `since_seq` is an event replay cursor only. It does not resume execution.
    pub async fn observe_run_since(
        &self,
        agent_id: &str,
        since_seq: Option<u64>,
    ) -> Option<RunSubscription> {
        let store = self.runs.read().await;
        let record = store.get(agent_id)?;
        let events = match since_seq {
            Some(seq) => record
                .events
                .iter()
                .filter(|event| event.seq > seq)
                .cloned()
                .collect(),
            None => record.events.clone(),
        };
        Some(RunSubscription {
            run_id: record.run_id.clone(),
            events,
            done: record.done,
            receiver: record.sender.subscribe(),
        })
    }

    /// Append an event to the buffer and broadcast it to all subscribers.
    pub async fn emit_run_event(&self, agent_id: &str, event: crate::ws::WsEvent) {
        let snapshot = {
            let mut store = self.runs.write().await;
            store.get_mut(agent_id).map(|record| {
                let envelope =
                    crate::ws::WsEventEnvelope::new(record.run_id.clone(), record.next_seq, event);
                record.next_seq += 1;
                let _ = record.sender.send(envelope.clone());
                record.events.push(envelope);
                PersistedRunRecord::from_record(agent_id, record)
            })
        };
        self.persist_run_snapshot(snapshot.as_ref());
    }

    /// Append the terminal event and mark the run as completed.
    pub async fn finish_run(&self, agent_id: &str, terminal: crate::ws::WsEvent) {
        let snapshot = {
            let mut store = self.runs.write().await;
            store.get_mut(agent_id).map(|record| {
                let envelope = crate::ws::WsEventEnvelope::new(
                    record.run_id.clone(),
                    record.next_seq,
                    terminal,
                );
                record.next_seq += 1;
                let _ = record.sender.send(envelope.clone());
                record.events.push(envelope);
                record.done = true;
                PersistedRunRecord::from_record(agent_id, record)
            })
        };
        self.persist_run_snapshot(snapshot.as_ref());
    }

    fn persist_run_snapshot(&self, snapshot: Option<&PersistedRunRecord>) {
        let (Some(store), Some(snapshot)) = (&self.run_store, snapshot) else {
            return;
        };

        if let Err(e) = store.persist(snapshot) {
            warn!(
                agent_id = %snapshot.agent_id,
                run_id = %snapshot.run_id,
                path = %store.path().display(),
                "failed to persist run events: {e}"
            );
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedRunRecord {
    agent_id: String,
    run_id: String,
    next_seq: u64,
    done: bool,
    events: Vec<crate::ws::WsEventEnvelope>,
}

type RunStoreResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
type RecoveredRunFile = Option<(String, RunRecord, Option<PersistedRunRecord>)>;

impl PersistedRunRecord {
    fn from_record(agent_id: &str, record: &RunRecord) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            run_id: record.run_id.clone(),
            next_seq: record.next_seq,
            done: record.done,
            events: record.events.clone(),
        }
    }
}

impl RunStore {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    fn persist(&self, snapshot: &PersistedRunRecord) -> RunStoreResult<()> {
        fs::create_dir_all(&self.root)?;

        let path = self.path_for_agent(&snapshot.agent_id);
        let tmp_path = path.with_extension(format!("json.tmp-{}", uuid::Uuid::new_v4()));
        let mut data = serde_json::to_vec(snapshot)?;
        data.push(b'\n');

        fs::write(&tmp_path, data)?;
        fs::rename(&tmp_path, path)?;
        Ok(())
    }

    fn recover_runs(&self) -> HashMap<String, RunRecord> {
        let mut runs = HashMap::new();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return runs,
            Err(e) => {
                warn!(
                    path = %self.root.display(),
                    "failed to read run store directory: {e}"
                );
                return runs;
            }
        };

        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if !is_json_file(&path) {
                continue;
            }

            match self.recover_run_file(&path) {
                Ok(Some((agent_id, record, recovered_snapshot))) => {
                    if let Some(snapshot) = recovered_snapshot.as_ref()
                        && let Err(e) = self.persist(snapshot)
                    {
                        warn!(
                            agent_id = %snapshot.agent_id,
                            run_id = %snapshot.run_id,
                            path = %self.root.display(),
                            "failed to persist recovered run terminal event: {e}"
                        );
                    }
                    runs.insert(agent_id, record);
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(
                        path = %path.display(),
                        "failed to recover run events: {e}"
                    );
                }
            }
        }

        runs
    }

    fn recover_run_file(&self, path: &Path) -> RunStoreResult<RecoveredRunFile> {
        let contents = fs::read_to_string(path)?;
        let mut snapshot: PersistedRunRecord = serde_json::from_str(&contents)?;

        if snapshot.agent_id.is_empty() || snapshot.run_id.is_empty() {
            warn!(path = %path.display(), "skipping run record with empty identifiers");
            return Ok(None);
        }

        if !snapshot
            .events
            .iter()
            .all(|event| event.run_id == snapshot.run_id)
        {
            warn!(
                agent_id = %snapshot.agent_id,
                path = %path.display(),
                "skipping run record with mismatched event run_id"
            );
            return Ok(None);
        }

        if !has_zero_based_sequence(&snapshot.events) {
            warn!(
                agent_id = %snapshot.agent_id,
                run_id = %snapshot.run_id,
                path = %path.display(),
                "skipping run record with non-monotonic event sequence"
            );
            return Ok(None);
        }

        let mut repaired_snapshot = None;
        if !snapshot
            .events
            .last()
            .map(|event| is_terminal_event(&event.event))
            .unwrap_or(false)
        {
            snapshot.events.push(crate::ws::WsEventEnvelope::new(
                snapshot.run_id.clone(),
                snapshot.events.len() as u64,
                crate::ws::WsEvent::WorkflowFailed {
                    error: "run interrupted before completion; active execution was not resumed"
                        .to_string(),
                },
            ));
            snapshot.done = true;
            snapshot.next_seq = snapshot.events.len() as u64;
            repaired_snapshot = Some(snapshot.clone());
        } else {
            snapshot.done = true;
            snapshot.next_seq = snapshot.events.len() as u64;
        }

        let (tx, _) = broadcast::channel(RUN_BROADCAST_CAPACITY);
        let record = RunRecord {
            run_id: snapshot.run_id.clone(),
            next_seq: snapshot.next_seq,
            events: snapshot.events,
            sender: tx,
            done: true,
        };

        Ok(Some((snapshot.agent_id, record, repaired_snapshot)))
    }

    fn path_for_agent(&self, agent_id: &str) -> PathBuf {
        self.root
            .join(format!("agent-{}.json", hex_agent_id(agent_id)))
    }
}

fn is_terminal_event(event: &crate::ws::WsEvent) -> bool {
    matches!(
        event,
        crate::ws::WsEvent::WorkflowCompleted { .. } | crate::ws::WsEvent::WorkflowFailed { .. }
    )
}

fn has_zero_based_sequence(events: &[crate::ws::WsEventEnvelope]) -> bool {
    events
        .iter()
        .enumerate()
        .all(|(idx, event)| event.seq == idx as u64)
}

fn is_json_file(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("json")
}

fn hex_agent_id(agent_id: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = agent_id.as_bytes();
    let mut encoded = String::with_capacity(bytes.len() * 2);

    for &byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }

    encoded
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

    fn temp_run_store_path() -> PathBuf {
        std::env::temp_dir().join(format!("rustflow-run-store-test-{}", uuid::Uuid::new_v4()))
    }

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
    async fn test_observe_run_since_without_cursor_replays_full_history() {
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
        state
            .finish_run(
                "agent-1",
                WsEvent::WorkflowCompleted {
                    outputs: serde_json::json!({ "a": "done" }),
                },
            )
            .await;

        let replay = state.observe_run_since("agent-1", None).await.unwrap();

        assert!(replay.done);
        assert_eq!(replay.events.len(), 2);
        assert_eq!(replay.events[0].seq, 0);
        assert_eq!(replay.events[1].seq, 1);
    }

    #[tokio::test]
    async fn test_observe_run_since_replays_after_middle_sequence() {
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
        state
            .emit_run_event(
                "agent-1",
                WsEvent::StepStarted {
                    step_id: "b".to_string(),
                    step_name: "B".to_string(),
                },
            )
            .await;
        state
            .finish_run(
                "agent-1",
                WsEvent::WorkflowCompleted {
                    outputs: serde_json::json!({ "b": "done" }),
                },
            )
            .await;

        let replay = state.observe_run_since("agent-1", Some(0)).await.unwrap();

        assert!(replay.done);
        assert_eq!(replay.events.len(), 2);
        assert_eq!(replay.events[0].seq, 1);
        assert_eq!(replay.events[1].seq, 2);
    }

    #[tokio::test]
    async fn test_observe_run_since_out_of_range_starts_empty_and_receives_live_events() {
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

        let mut replay = state.observe_run_since("agent-1", Some(99)).await.unwrap();

        assert!(!replay.done);
        assert!(replay.events.is_empty());

        state
            .emit_run_event(
                "agent-1",
                WsEvent::StepStarted {
                    step_id: "b".to_string(),
                    step_name: "B".to_string(),
                },
            )
            .await;

        let live = replay.receiver.recv().await.unwrap();
        assert_eq!(live.seq, 1);
        assert!(matches!(
            &live.event,
            WsEvent::StepStarted { step_id, .. } if step_id == "b"
        ));
    }

    #[tokio::test]
    async fn test_observe_run_since_replays_completed_run_after_cursor() {
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
        state
            .finish_run(
                "agent-1",
                WsEvent::WorkflowCompleted {
                    outputs: serde_json::json!({ "a": "done" }),
                },
            )
            .await;

        let replay = state.observe_run_since("agent-1", Some(0)).await.unwrap();

        assert!(replay.done);
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.events[0].seq, 1);
        assert!(matches!(
            &replay.events[0].event,
            WsEvent::WorkflowCompleted { outputs } if outputs["a"] == "done"
        ));
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

    #[tokio::test]
    async fn test_persisted_run_events_recover_into_fresh_state() {
        let store_path = temp_run_store_path();
        let state = AppState::with_run_store_path(store_path.clone());
        let first_run_id = match state.start_or_observe_run("agent-1".to_string()).await {
            RunStart::Started(subscription) => subscription.run_id,
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

        let recovered_state = AppState::with_run_store_path(store_path.clone());
        let recovered = recovered_state.observe_run("agent-1").await.unwrap();

        assert!(recovered.done);
        assert_eq!(recovered.run_id, first_run_id);
        assert_eq!(recovered.events.len(), 2);
        assert_eq!(recovered.events[0].run_id, first_run_id);
        assert_eq!(recovered.events[0].seq, 0);
        assert_eq!(recovered.events[1].run_id, first_run_id);
        assert_eq!(recovered.events[1].seq, 1);
        assert!(matches!(
            &recovered.events[1].event,
            WsEvent::WorkflowCompleted { outputs } if outputs["a"] == "done"
        ));

        let _ = fs::remove_dir_all(store_path);
    }

    #[tokio::test]
    async fn test_observe_run_since_replays_recovered_completed_run_after_cursor() {
        let store_path = temp_run_store_path();
        let state = AppState::with_run_store_path(store_path.clone());
        let first_run_id = match state.start_or_observe_run("agent-1".to_string()).await {
            RunStart::Started(subscription) => subscription.run_id,
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

        let recovered_state = AppState::with_run_store_path(store_path.clone());
        let recovered = recovered_state
            .observe_run_since("agent-1", Some(0))
            .await
            .unwrap();

        assert!(recovered.done);
        assert_eq!(recovered.run_id, first_run_id);
        assert_eq!(recovered.events.len(), 1);
        assert_eq!(recovered.events[0].run_id, first_run_id);
        assert_eq!(recovered.events[0].seq, 1);
        assert!(matches!(
            &recovered.events[0].event,
            WsEvent::WorkflowCompleted { outputs } if outputs["a"] == "done"
        ));

        let _ = fs::remove_dir_all(store_path);
    }

    #[tokio::test]
    async fn test_new_stream_after_recovered_completed_run_replaces_run() {
        let store_path = temp_run_store_path();
        let state = AppState::with_run_store_path(store_path.clone());
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

        let recovered_state = AppState::with_run_store_path(store_path.clone());
        let second_run_id = match recovered_state
            .start_or_observe_run("agent-1".to_string())
            .await
        {
            RunStart::Started(subscription) => subscription.run_id,
            RunStart::Existing(_) => panic!("recovered completed run should be replaced"),
        };
        recovered_state
            .emit_run_event(
                "agent-1",
                WsEvent::StepStarted {
                    step_id: "b".to_string(),
                    step_name: "B".to_string(),
                },
            )
            .await;

        let replay = recovered_state.observe_run("agent-1").await.unwrap();
        assert_ne!(first_run_id, second_run_id);
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.events[0].run_id, second_run_id);
        assert_eq!(replay.events[0].seq, 0);

        let _ = fs::remove_dir_all(store_path);
    }
}
