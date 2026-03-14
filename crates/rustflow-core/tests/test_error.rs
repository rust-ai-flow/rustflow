use rustflow_core::error::{Result, RustFlowError};

#[test]
fn test_error_constructors_and_display() {
    let cases: Vec<(RustFlowError, &str)> = vec![
        (
            RustFlowError::orchestration("orch fail"),
            "orchestration error: orch fail",
        ),
        (RustFlowError::llm("llm fail"), "LLM error: llm fail"),
        (RustFlowError::tool("tool fail"), "tool error: tool fail"),
        (
            RustFlowError::plugin("plugin fail"),
            "plugin error: plugin fail",
        ),
        (
            RustFlowError::config("config fail"),
            "config error: config fail",
        ),
        (RustFlowError::timeout("timed out"), "timeout: timed out"),
        (
            RustFlowError::circuit_breaker("open"),
            "circuit breaker open: open",
        ),
    ];

    for (err, expected) in cases {
        assert_eq!(format!("{err}"), expected);
    }
}

#[test]
fn test_error_debug() {
    let err = RustFlowError::config("bad config");
    let debug = format!("{err:?}");
    assert!(debug.contains("Config"));
    assert!(debug.contains("bad config"));
}

#[test]
fn test_result_type_alias() {
    let ok: Result<i32> = Ok(42);
    assert_eq!(ok.unwrap(), 42);

    let err: Result<i32> = Err(RustFlowError::timeout("slow"));
    assert!(err.is_err());
}
