use rustflow_core::agent::Agent;
use rustflow_core::step::Step;
use rustflow_core::types::StepId;

fn sample_steps() -> Vec<Step> {
    vec![
        Step::new_tool("s1", "Step 1", "http", serde_json::json!({})),
        Step::new_llm("s2", "Step 2", "openai", "gpt-4", "hello"),
    ]
}

#[test]
fn test_agent_new() {
    let agent = Agent::new("test-agent", sample_steps());
    assert_eq!(agent.name, "test-agent");
    assert_eq!(agent.steps.len(), 2);
    assert!(agent.description.is_none());
    assert!(!agent.id.as_str().is_empty());
}

#[test]
fn test_agent_with_id() {
    let agent = Agent::with_id("custom-id", "agent", vec![]);
    assert_eq!(agent.id.as_str(), "custom-id");
    assert_eq!(agent.name, "agent");
}

#[test]
fn test_agent_with_description() {
    let agent = Agent::new("a", vec![]).with_description("does things");
    assert_eq!(agent.description, Some("does things".to_string()));
}

#[test]
fn test_agent_get_step_found() {
    let agent = Agent::new("a", sample_steps());
    let sid = StepId::new("s1");
    let step = agent.get_step(&sid);
    assert!(step.is_some());
    assert_eq!(step.unwrap().name, "Step 1");
}

#[test]
fn test_agent_get_step_not_found() {
    let agent = Agent::new("a", sample_steps());
    let sid = StepId::new("missing");
    assert!(agent.get_step(&sid).is_none());
}

#[test]
fn test_agent_new_generates_unique_ids() {
    let a1 = Agent::new("a", vec![]);
    let a2 = Agent::new("a", vec![]);
    assert_ne!(a1.id, a2.id);
}

#[test]
fn test_agent_serde_roundtrip() {
    let agent = Agent::with_id("id-1", "agent-1", sample_steps()).with_description("test");
    let json = serde_json::to_string(&agent).unwrap();
    let deserialized: Agent = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, agent.id);
    assert_eq!(deserialized.name, agent.name);
    assert_eq!(deserialized.description, agent.description);
    assert_eq!(deserialized.steps.len(), 2);
}
