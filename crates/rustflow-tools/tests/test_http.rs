use rustflow_core::context::Context;
use rustflow_tools::error::ToolError;
use rustflow_tools::http::HttpTool;
use rustflow_tools::tool::Tool;

#[test]
fn test_http_tool_name() {
    let tool = HttpTool::new();
    assert_eq!(tool.name(), "http");
}

#[test]
fn test_http_tool_description() {
    let tool = HttpTool::new();
    assert!(!tool.description().is_empty());
}

#[test]
fn test_http_tool_parameters_schema() {
    let tool = HttpTool::new();
    let params = tool.parameters();
    assert_eq!(params["type"], "object");
    let required = params["required"].as_array().unwrap();
    assert!(required.contains(&serde_json::json!("url")));
    assert!(params["properties"]["url"].is_object());
    assert!(params["properties"]["method"].is_object());
    assert!(params["properties"]["headers"].is_object());
    assert!(params["properties"]["timeout_secs"].is_object());
}

#[test]
fn test_http_tool_default() {
    let tool = HttpTool::default();
    assert_eq!(tool.name(), "http");
}

#[tokio::test]
async fn test_http_tool_invalid_input() {
    let tool = HttpTool::new();
    let ctx = Context::new();
    // Missing required "url" field
    let input = serde_json::json!({"method": "GET"});
    let err = tool.execute(input, &ctx).await.unwrap_err();
    assert!(matches!(err, ToolError::InvalidInput { .. }));
}

#[tokio::test]
async fn test_http_tool_invalid_method() {
    let tool = HttpTool::new();
    let ctx = Context::new();
    // Method bytes containing a space are invalid per HTTP spec
    let input = serde_json::json!({
        "url": "http://localhost:1",
        "method": "BAD METHOD"
    });
    let err = tool.execute(input, &ctx).await.unwrap_err();
    assert!(matches!(err, ToolError::InvalidInput { .. }));
}

#[tokio::test]
async fn test_http_tool_connection_refused() {
    let tool = HttpTool::new();
    let ctx = Context::new();
    // Use a port that's almost certainly not listening
    let input = serde_json::json!({
        "url": "http://127.0.0.1:19999",
        "timeout_secs": 1
    });
    let err = tool.execute(input, &ctx).await.unwrap_err();
    assert!(matches!(err, ToolError::Http(_)));
}
