use std::collections::HashSet;

use rustflow_core::step::Step;
use rustflow_core::types::StepId;
use rustflow_orchestrator::{DagParser, OrchestratorError};

fn tool_step(id: &str, deps: Vec<&str>) -> Step {
    let mut s = Step::new_tool(id, id, "noop", serde_json::Value::Null);
    s.depends_on = deps.into_iter().map(StepId::from).collect();
    s
}

#[test]
fn test_linear_dag() {
    let steps = vec![
        tool_step("a", vec![]),
        tool_step("b", vec!["a"]),
        tool_step("c", vec!["b"]),
    ];
    let order = DagParser::parse(&steps).unwrap();
    assert_eq!(
        order,
        vec![StepId::from("a"), StepId::from("b"), StepId::from("c")]
    );
}

#[test]
fn test_cycle_detection() {
    let steps = vec![
        tool_step("a", vec!["c"]),
        tool_step("b", vec!["a"]),
        tool_step("c", vec!["b"]),
    ];
    let err = DagParser::parse(&steps).unwrap_err();
    assert!(matches!(err, OrchestratorError::CycleDetected { .. }));
}

#[test]
fn test_duplicate_step_id() {
    let steps = vec![tool_step("a", vec![]), tool_step("a", vec![])];
    let err = DagParser::parse(&steps).unwrap_err();
    assert!(matches!(err, OrchestratorError::DuplicateStepId { .. }));
}

#[test]
fn test_unknown_dependency() {
    let steps = vec![tool_step("a", vec!["missing"])];
    let err = DagParser::parse(&steps).unwrap_err();
    assert!(matches!(err, OrchestratorError::UnknownDependency { .. }));
}

#[test]
fn test_empty_dag() {
    let steps: Vec<Step> = vec![];
    let order = DagParser::parse(&steps).unwrap();
    assert!(order.is_empty());
}

#[test]
fn test_single_step() {
    let steps = vec![tool_step("only", vec![])];
    let order = DagParser::parse(&steps).unwrap();
    assert_eq!(order, vec![StepId::from("only")]);
}

#[test]
fn test_diamond_dag() {
    let steps = vec![
        tool_step("a", vec![]),
        tool_step("b", vec!["a"]),
        tool_step("c", vec!["a"]),
        tool_step("d", vec!["b", "c"]),
    ];
    let order = DagParser::parse(&steps).unwrap();
    assert_eq!(order[0], StepId::from("a"));
    assert_eq!(order[3], StepId::from("d"));
    let mid: Vec<&StepId> = order[1..3].iter().collect();
    assert!(mid.contains(&&StepId::from("b")));
    assert!(mid.contains(&&StepId::from("c")));
}

#[test]
fn test_parallel_independent_steps() {
    let steps = vec![
        tool_step("a", vec![]),
        tool_step("b", vec![]),
        tool_step("c", vec![]),
    ];
    let order = DagParser::parse(&steps).unwrap();
    assert_eq!(order.len(), 3);
}

#[test]
fn test_self_cycle() {
    let steps = vec![tool_step("a", vec!["a"])];
    let err = DagParser::parse(&steps).unwrap_err();
    assert!(matches!(err, OrchestratorError::CycleDetected { .. }));
}

#[test]
fn test_build_dependency_map() {
    let steps = vec![
        tool_step("a", vec![]),
        tool_step("b", vec!["a"]),
        tool_step("c", vec!["a", "b"]),
    ];
    let map = DagParser::build_dependency_map(&steps);
    assert!(map["a"].is_empty());
    assert_eq!(map["b"], HashSet::from(["a".to_string()]));
    assert_eq!(
        map["c"],
        HashSet::from(["a".to_string(), "b".to_string()])
    );
}
