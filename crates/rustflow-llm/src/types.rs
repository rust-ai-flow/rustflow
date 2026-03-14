use serde::{Deserialize, Serialize};

/// The role of a message participant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::System => write!(f, "system"),
            Role::User => write!(f, "user"),
            Role::Assistant => write!(f, "assistant"),
        }
    }
}

/// A single chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// A request to an LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    /// The model to use, e.g. "claude-3-5-sonnet-20241022".
    pub model: String,
    /// The conversation history / prompt messages.
    pub messages: Vec<Message>,
    /// Sampling temperature (0.0–2.0 depending on provider).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Whether the caller wants a streaming response.
    #[serde(default)]
    pub stream: bool,
    /// Provider-specific extra parameters.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl LlmRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: None,
            max_tokens: None,
            stream: false,
            extra: serde_json::Map::new(),
        }
    }

    pub fn with_temperature(mut self, t: f32) -> Self {
        self.temperature = Some(t);
        self
    }

    pub fn with_max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = Some(n);
        self
    }

    pub fn with_stream(mut self) -> Self {
        self.stream = true;
        self
    }
}

/// The response from an LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    /// The generated text content.
    pub content: String,
    /// The model that generated the response.
    pub model: String,
    /// Token usage information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
    /// The stop reason (e.g. "end_turn", "max_tokens").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

/// Token usage reported by the provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Role ─────────────────────────────────────────────────────────────

    #[test]
    fn test_role_display() {
        assert_eq!(format!("{}", Role::System), "system");
        assert_eq!(format!("{}", Role::User), "user");
        assert_eq!(format!("{}", Role::Assistant), "assistant");
    }

    #[test]
    fn test_role_serde_roundtrip() {
        for role in [Role::System, Role::User, Role::Assistant] {
            let json = serde_json::to_string(&role).unwrap();
            let deserialized: Role = serde_json::from_str(&json).unwrap();
            assert_eq!(role, deserialized);
        }
    }

    #[test]
    fn test_role_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Role::System).unwrap(), "\"system\"");
        assert_eq!(serde_json::to_string(&Role::User).unwrap(), "\"user\"");
    }

    // ── Message ──────────────────────────────────────────────────────────

    #[test]
    fn test_message_system() {
        let msg = Message::system("You are helpful");
        assert_eq!(msg.role, Role::System);
        assert_eq!(msg.content, "You are helpful");
    }

    #[test]
    fn test_message_user() {
        let msg = Message::user("Hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "Hello");
    }

    #[test]
    fn test_message_assistant() {
        let msg = Message::assistant("Hi there");
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content, "Hi there");
    }

    #[test]
    fn test_message_serde_roundtrip() {
        let msg = Message::user("test");
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.role, Role::User);
        assert_eq!(deserialized.content, "test");
    }

    // ── LlmRequest ───────────────────────────────────────────────────────

    #[test]
    fn test_request_new_defaults() {
        let req = LlmRequest::new("gpt-4", vec![Message::user("hi")]);
        assert_eq!(req.model, "gpt-4");
        assert_eq!(req.messages.len(), 1);
        assert!(req.temperature.is_none());
        assert!(req.max_tokens.is_none());
        assert!(!req.stream);
        assert!(req.extra.is_empty());
    }

    #[test]
    fn test_request_with_temperature() {
        let req = LlmRequest::new("gpt-4", vec![]).with_temperature(0.7);
        assert_eq!(req.temperature, Some(0.7));
    }

    #[test]
    fn test_request_with_max_tokens() {
        let req = LlmRequest::new("gpt-4", vec![]).with_max_tokens(1024);
        assert_eq!(req.max_tokens, Some(1024));
    }

    #[test]
    fn test_request_with_stream() {
        let req = LlmRequest::new("gpt-4", vec![]).with_stream();
        assert!(req.stream);
    }

    #[test]
    fn test_request_builder_chaining() {
        let req = LlmRequest::new("claude-3", vec![Message::user("hello")])
            .with_temperature(0.5)
            .with_max_tokens(2048)
            .with_stream();
        assert_eq!(req.temperature, Some(0.5));
        assert_eq!(req.max_tokens, Some(2048));
        assert!(req.stream);
    }

    #[test]
    fn test_request_serde_skips_none_fields() {
        let req = LlmRequest::new("gpt-4", vec![]);
        let json = serde_json::to_value(&req).unwrap();
        assert!(!json.as_object().unwrap().contains_key("temperature"));
        assert!(!json.as_object().unwrap().contains_key("max_tokens"));
        assert!(!json.as_object().unwrap().contains_key("extra"));
    }

    // ── LlmResponse ──────────────────────────────────────────────────────

    #[test]
    fn test_response_serde_roundtrip() {
        let resp = LlmResponse {
            content: "Hello!".to_string(),
            model: "gpt-4".to_string(),
            usage: Some(TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
            }),
            stop_reason: Some("end_turn".to_string()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: LlmResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.content, "Hello!");
        assert_eq!(deserialized.usage.unwrap().input_tokens, 10);
    }

    #[test]
    fn test_response_optional_fields() {
        let resp = LlmResponse {
            content: "Hi".to_string(),
            model: "gpt-4".to_string(),
            usage: None,
            stop_reason: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(!json.as_object().unwrap().contains_key("usage"));
        assert!(!json.as_object().unwrap().contains_key("stop_reason"));
    }

    // ── TokenUsage ───────────────────────────────────────────────────────

    #[test]
    fn test_token_usage_serde() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let deserialized: TokenUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.input_tokens, 100);
        assert_eq!(deserialized.output_tokens, 50);
    }
}
