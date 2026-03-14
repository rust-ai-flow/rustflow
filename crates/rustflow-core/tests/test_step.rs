use rustflow_core::retry::RetryPolicy;
use rustflow_core::step::{Step, StepKind, StepState};
use rustflow_core::types::StepId;

#[test]
fn test_step_state_default() {
    assert_eq!(StepState::default(), StepState::Pending);
}

#[test]
fn test_step_state_display() {
    assert_eq!(format!("{}", StepState::Pending), "pending");
    assert_eq!(format!("{}", StepState::Running), "running");
    assert_eq!(format!("{}", StepState::Success), "success");
    assert_eq!(format!("{}", StepState::Failed), "failed");
    assert_eq!(format!("{}", StepState::Retrying), "retrying");
}

#[test]
fn test_step_state_serde_roundtrip() {
    for state in [
        StepState::Pending,
        StepState::Running,
        StepState::Success,
        StepState::Failed,
        StepState::Retrying,
    ] {
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: StepState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, deserialized);
    }
}

#[test]
fn test_new_tool_step() {
    let step = Step::new_tool(
        "fetch",
        "Fetch Data",
        "http",
        serde_json::json!({"url": "https://example.com"}),
    );
    assert_eq!(step.id.as_str(), "fetch");
    assert_eq!(step.name, "Fetch Data");
    assert!(matches!(step.kind, StepKind::Tool(_)));
    assert!(step.depends_on.is_empty());
    assert_eq!(step.retry_policy, RetryPolicy::None);
    assert!(step.timeout_ms.is_none());

    if let StepKind::Tool(config) = &step.kind {
        assert_eq!(config.tool, "http");
        assert_eq!(config.input["url"], "https://example.com");
    }
}

#[test]
fn test_new_llm_step() {
    let step = Step::new_llm("summarise", "Summarise", "openai", "gpt-4", "Hello");
    assert_eq!(step.id.as_str(), "summarise");
    assert!(matches!(step.kind, StepKind::Llm(_)));

    if let StepKind::Llm(config) = &step.kind {
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4");
        assert_eq!(config.prompt, "Hello");
        assert!(config.temperature.is_none());
        assert!(config.max_tokens.is_none());
    }
}

#[test]
fn test_with_depends_on() {
    let step = Step::new_tool("s1", "S1", "http", serde_json::json!(null))
        .with_depends_on(vec![StepId::new("s0")]);
    assert_eq!(step.depends_on.len(), 1);
    assert_eq!(step.depends_on[0].as_str(), "s0");
}

#[test]
fn test_with_retry() {
    let policy = RetryPolicy::Fixed {
        max_retries: 3,
        interval_ms: 1000,
    };
    let step = Step::new_tool("s1", "S1", "http", serde_json::json!(null))
        .with_retry(policy.clone());
    assert_eq!(step.retry_policy, policy);
}

#[test]
fn test_with_timeout_ms() {
    let step =
        Step::new_tool("s1", "S1", "http", serde_json::json!(null)).with_timeout_ms(5000);
    assert_eq!(step.timeout_ms, Some(5000));
}

#[test]
fn test_builder_chaining() {
    let step = Step::new_llm("s1", "S1", "anthropic", "claude", "prompt")
        .with_depends_on(vec![StepId::new("s0")])
        .with_retry(RetryPolicy::Fixed {
            max_retries: 2,
            interval_ms: 500,
        })
        .with_timeout_ms(30000);

    assert_eq!(step.depends_on.len(), 1);
    assert_eq!(step.retry_policy.max_retries(), 2);
    assert_eq!(step.timeout_ms, Some(30000));
}

#[test]
fn test_step_serde_roundtrip() {
    let step = Step::new_tool(
        "fetch",
        "Fetch",
        "http",
        serde_json::json!({"url": "https://example.com"}),
    )
    .with_depends_on(vec![StepId::new("init")])
    .with_retry(RetryPolicy::Exponential {
        max_retries: 3,
        initial_interval_ms: 100,
        multiplier: 2.0,
        max_interval_ms: 5000,
    })
    .with_timeout_ms(10000);

    let json = serde_json::to_string(&step).unwrap();
    let deserialized: Step = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, step.id);
    assert_eq!(deserialized.name, step.name);
    assert_eq!(deserialized.depends_on.len(), 1);
    assert_eq!(deserialized.timeout_ms, Some(10000));
}
