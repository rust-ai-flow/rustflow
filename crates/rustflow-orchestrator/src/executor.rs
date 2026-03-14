use std::sync::Arc;

use async_trait::async_trait;
use tracing::{debug, info};

use rustflow_core::context::Context;
use rustflow_core::step::{Step, StepKind};
use rustflow_core::types::Value;
use rustflow_llm::gateway::LlmGateway;
use rustflow_llm::types::{LlmRequest, Message};
use rustflow_tools::registry::ToolRegistry;

use crate::scheduler::StepExecutor;

/// A concrete `StepExecutor` that routes LLM steps to the `LlmGateway`
/// and tool steps to the `ToolRegistry`.
pub struct DefaultStepExecutor {
    llm_gateway: Arc<LlmGateway>,
    tool_registry: Arc<ToolRegistry>,
}

impl DefaultStepExecutor {
    pub fn new(llm_gateway: Arc<LlmGateway>, tool_registry: Arc<ToolRegistry>) -> Self {
        Self {
            llm_gateway,
            tool_registry,
        }
    }
}

#[async_trait]
impl StepExecutor for DefaultStepExecutor {
    async fn execute(&self, step: &Step, ctx: &Context) -> Result<Value, String> {
        match &step.kind {
            StepKind::Llm(config) => {
                info!(step_id = %step.id, provider = %config.provider, model = %config.model, "executing LLM step");

                // Interpolate prompt: replace {{steps.<id>.output}} with actual values.
                let prompt = interpolate_prompt(&config.prompt, ctx);
                debug!(prompt = %prompt, "interpolated prompt");

                let mut request = LlmRequest::new(&config.model, vec![Message::user(prompt)]);
                if let Some(t) = config.temperature {
                    request = request.with_temperature(t);
                }
                if let Some(n) = config.max_tokens {
                    request = request.with_max_tokens(n);
                }

                let response = self
                    .llm_gateway
                    .complete(&config.provider, &request)
                    .await
                    .map_err(|e| format!("LLM error: {e}"))?;

                Ok(Value::from(serde_json::json!({
                    "content": response.content,
                    "model": response.model,
                    "usage": response.usage,
                    "stop_reason": response.stop_reason,
                })))
            }
            StepKind::Tool(config) => {
                info!(step_id = %step.id, tool = %config.tool, "executing tool step");

                // Interpolate tool input: replace string values containing {{steps.<id>.output}}
                let input = interpolate_json(&config.input, ctx);
                debug!(input = %input, "interpolated tool input");

                let tool = self
                    .tool_registry
                    .get(&config.tool)
                    .map_err(|e| format!("tool error: {e}"))?;

                let result = tool
                    .execute(input, ctx)
                    .await
                    .map_err(|e| format!("tool execution error: {e}"))?;

                Ok(result)
            }
        }
    }
}

/// Replace `{{steps.<step_id>.output}}` placeholders in a prompt string
/// with the JSON-serialised output of the referenced step.
fn interpolate_prompt(template: &str, ctx: &Context) -> String {
    let mut result = template.to_string();
    for (step_id, value) in &ctx.step_outputs {
        let placeholder = format!("{{{{steps.{step_id}.output}}}}");
        if result.contains(&placeholder) {
            let replacement = match value.inner() {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            result = result.replace(&placeholder, &replacement);
        }
    }
    // Also replace {{vars.<key>}} with context variables.
    for (key, value) in &ctx.vars {
        let placeholder = format!("{{{{vars.{key}}}}}");
        if result.contains(&placeholder) {
            let replacement = match value.inner() {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            result = result.replace(&placeholder, &replacement);
        }
    }
    result
}

/// Recursively walk a JSON value and interpolate any string that contains
/// `{{steps.<id>.output}}` or `{{vars.<key>}}` placeholders.
fn interpolate_json(value: &serde_json::Value, ctx: &Context) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            let interpolated = interpolate_prompt(s, ctx);
            serde_json::Value::String(interpolated)
        }
        serde_json::Value::Object(map) => {
            let new_map: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), interpolate_json(v, ctx)))
                .collect();
            serde_json::Value::Object(new_map)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(|v| interpolate_json(v, ctx)).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflow_core::types::StepId;

    #[test]
    fn test_interpolate_prompt() {
        let mut ctx = Context::new();
        ctx.set_step_output(
            &StepId::new("fetch"),
            Value::from(serde_json::json!("Hello World")),
        );
        ctx.set_var("language", Value::from(serde_json::json!("English")));

        let template = "Summarise in {{vars.language}}: {{steps.fetch.output}}";
        let result = interpolate_prompt(template, &ctx);
        assert_eq!(result, "Summarise in English: Hello World");
    }

    #[test]
    fn test_interpolate_json() {
        let mut ctx = Context::new();
        ctx.set_step_output(
            &StepId::new("fetch"),
            Value::from(serde_json::json!("https://example.com")),
        );

        let input = serde_json::json!({
            "url": "{{steps.fetch.output}}",
            "method": "GET"
        });
        let result = interpolate_json(&input, &ctx);
        assert_eq!(result["url"], "https://example.com");
        assert_eq!(result["method"], "GET");
    }

    #[test]
    fn test_interpolate_prompt_no_placeholders() {
        let ctx = Context::new();
        let result = interpolate_prompt("no placeholders here", &ctx);
        assert_eq!(result, "no placeholders here");
    }

    #[test]
    fn test_interpolate_prompt_non_string_value() {
        let mut ctx = Context::new();
        ctx.set_step_output(
            &StepId::new("calc"),
            Value::from(serde_json::json!({"count": 42})),
        );
        let result = interpolate_prompt("Result: {{steps.calc.output}}", &ctx);
        assert_eq!(result, "Result: {\"count\":42}");
    }

    #[test]
    fn test_interpolate_prompt_multiple_placeholders() {
        let mut ctx = Context::new();
        ctx.set_step_output(&StepId::new("a"), Value::from(serde_json::json!("AAA")));
        ctx.set_step_output(&StepId::new("b"), Value::from(serde_json::json!("BBB")));
        let result = interpolate_prompt("{{steps.a.output}} and {{steps.b.output}}", &ctx);
        assert_eq!(result, "AAA and BBB");
    }

    #[test]
    fn test_interpolate_prompt_missing_step_leaves_placeholder() {
        let ctx = Context::new();
        let result = interpolate_prompt("{{steps.missing.output}}", &ctx);
        assert_eq!(result, "{{steps.missing.output}}");
    }

    #[test]
    fn test_interpolate_json_nested_array() {
        let mut ctx = Context::new();
        ctx.set_step_output(&StepId::new("s1"), Value::from(serde_json::json!("val")));

        let input = serde_json::json!({
            "items": ["{{steps.s1.output}}", "static"]
        });
        let result = interpolate_json(&input, &ctx);
        assert_eq!(result["items"][0], "val");
        assert_eq!(result["items"][1], "static");
    }

    #[test]
    fn test_interpolate_json_preserves_non_string_types() {
        let ctx = Context::new();
        let input = serde_json::json!({
            "count": 42,
            "flag": true,
            "nothing": null
        });
        let result = interpolate_json(&input, &ctx);
        assert_eq!(result["count"], 42);
        assert_eq!(result["flag"], true);
        assert!(result["nothing"].is_null());
    }
}
