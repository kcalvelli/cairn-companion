//! OpenAI-compatible request and response types for the chat completions gateway.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    #[serde(default)]
    pub model: Option<String>,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub stream: Option<bool>,
    // Accepted but ignored — companion doesn't expose these knobs.
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub top_p: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
}

// ---------------------------------------------------------------------------
// Non-streaming response
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ChatCompletion {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatCompletionChoice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionChoice {
    pub index: u32,
    pub message: ChatCompletionMessage,
    pub finish_reason: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionMessage {
    pub role: &'static str,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl ChatCompletion {
    pub fn new(id: String, model: String, content: String, created: u64) -> Self {
        Self {
            id,
            object: "chat.completion",
            created,
            model,
            choices: vec![ChatCompletionChoice {
                index: 0,
                message: ChatCompletionMessage {
                    role: "assistant",
                    content,
                },
                finish_reason: "stop",
            }],
            usage: Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Streaming response
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

#[derive(Debug, Serialize)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: ChunkDelta,
    pub finish_reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct ChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

impl ChatCompletionChunk {
    /// First chunk — includes role, content.
    pub fn first(id: &str, model: &str, content: String, created: u64) -> Self {
        Self {
            id: id.to_string(),
            object: "chat.completion.chunk",
            created,
            model: model.to_string(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: Some("assistant"),
                    content: Some(content),
                },
                finish_reason: None,
            }],
        }
    }

    /// Subsequent chunk — content only.
    pub fn content(id: &str, model: &str, content: String, created: u64) -> Self {
        Self {
            id: id.to_string(),
            object: "chat.completion.chunk",
            created,
            model: model.to_string(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: None,
                    content: Some(content),
                },
                finish_reason: None,
            }],
        }
    }

    /// Final chunk — stop signal, no content.
    pub fn stop(id: &str, model: &str, created: u64) -> Self {
        Self {
            id: id.to_string(),
            object: "chat.completion.chunk",
            created,
            model: model.to_string(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: None,
                    content: None,
                },
                finish_reason: Some("stop"),
            }],
        }
    }
}

// ---------------------------------------------------------------------------
// Error envelope
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct OpenAIErrorEnvelope {
    pub error: OpenAIError,
}

#[derive(Debug, Serialize)]
pub struct OpenAIError {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    pub code: String,
}

impl OpenAIErrorEnvelope {
    pub fn new(message: impl Into<String>, error_type: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            error: OpenAIError {
                message: message.into(),
                error_type: error_type.into(),
                code: code.into(),
            },
        }
    }

    pub fn invalid_json(detail: impl Into<String>) -> Self {
        Self::new(detail, "invalid_request_error", "invalid_json")
    }

    pub fn invalid_messages(detail: impl Into<String>) -> Self {
        Self::new(detail, "invalid_request_error", "invalid_messages")
    }

    pub fn no_user_message() -> Self {
        Self::new(
            "No message with role 'user' found in messages array",
            "invalid_request_error",
            "no_user_message",
        )
    }

    pub fn companion_error(detail: impl Into<String>) -> Self {
        Self::new(detail, "server_error", "companion_error")
    }

    pub fn not_found() -> Self {
        Self::new("Not found", "invalid_request_error", "not_found")
    }
}

// ---------------------------------------------------------------------------
// Models response
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ModelsResponse {
    pub object: &'static str,
    pub data: Vec<ModelEntry>,
}

#[derive(Debug, Serialize)]
pub struct ModelEntry {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub owned_by: &'static str,
}

impl ModelsResponse {
    pub fn new(model_name: String, created: u64) -> Self {
        Self {
            object: "list",
            data: vec![ModelEntry {
                id: model_name,
                object: "model",
                created,
                owned_by: "axios-companion",
            }],
        }
    }
}

// ---------------------------------------------------------------------------
// Gateway config
// ---------------------------------------------------------------------------

/// Session policy for the gateway.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionPolicy {
    PerConversationId,
    SingleSession,
    Ephemeral,
}

/// Configuration for the OpenAI gateway, read from environment variables.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub port: u16,
    pub bind_address: String,
    pub model_name: String,
    pub session_policy: SessionPolicy,
}

impl GatewayConfig {
    /// Read gateway configuration from environment variables.
    /// Returns `None` if the gateway is disabled (COMPANION_GATEWAY_ENABLE != "1").
    pub fn from_env() -> Option<Self> {
        let enabled = std::env::var("COMPANION_GATEWAY_ENABLE").unwrap_or_default();
        if enabled != "1" {
            return None;
        }

        let port = std::env::var("COMPANION_GATEWAY_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(18789);

        let bind_address = std::env::var("COMPANION_GATEWAY_BIND")
            .unwrap_or_else(|_| "0.0.0.0".into());

        let model_name = std::env::var("COMPANION_GATEWAY_MODEL")
            .unwrap_or_else(|_| "companion".into());

        let session_policy = match std::env::var("COMPANION_GATEWAY_SESSION_POLICY")
            .unwrap_or_default()
            .as_str()
        {
            "single-session" => SessionPolicy::SingleSession,
            "ephemeral" => SessionPolicy::Ephemeral,
            _ => SessionPolicy::PerConversationId,
        };

        Some(Self {
            port,
            bind_address,
            model_name,
            session_policy,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_completion_serializes() {
        let resp = ChatCompletion::new(
            "chatcmpl-abc".into(),
            "companion".into(),
            "Hello there.".into(),
            1700000000,
        );
        let json = serde_json::to_string(&resp).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["object"], "chat.completion");
        assert_eq!(v["choices"][0]["message"]["content"], "Hello there.");
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
        assert_eq!(v["usage"]["total_tokens"], 0);
    }

    #[test]
    fn chunk_first_includes_role() {
        let chunk = ChatCompletionChunk::first("id-1", "companion", "Hi".into(), 1700000000);
        let json = serde_json::to_string(&chunk).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["choices"][0]["delta"]["role"], "assistant");
        assert_eq!(v["choices"][0]["delta"]["content"], "Hi");
        assert!(v["choices"][0]["finish_reason"].is_null());
    }

    #[test]
    fn chunk_content_omits_role() {
        let chunk = ChatCompletionChunk::content("id-1", "companion", "more".into(), 1700000000);
        let json = serde_json::to_string(&chunk).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["choices"][0]["delta"].get("role").is_none());
        assert_eq!(v["choices"][0]["delta"]["content"], "more");
    }

    #[test]
    fn chunk_stop_has_finish_reason() {
        let chunk = ChatCompletionChunk::stop("id-1", "companion", 1700000000);
        let json = serde_json::to_string(&chunk).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
        assert!(v["choices"][0]["delta"].get("content").is_none());
    }

    #[test]
    fn error_envelope_serializes() {
        let err = OpenAIErrorEnvelope::no_user_message();
        let json = serde_json::to_string(&err).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["error"]["type"], "invalid_request_error");
        assert_eq!(v["error"]["code"], "no_user_message");
    }

    #[test]
    fn models_response_serializes() {
        let resp = ModelsResponse::new("sid".into(), 1700000000);
        let json = serde_json::to_string(&resp).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["object"], "list");
        assert_eq!(v["data"][0]["id"], "sid");
        assert_eq!(v["data"][0]["owned_by"], "axios-companion");
    }

    #[test]
    fn request_deserializes_minimal() {
        let json = r#"{"messages":[{"role":"user","content":"hello"}]}"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert_eq!(req.messages[0].content.as_deref(), Some("hello"));
        assert!(req.stream.is_none());
        assert!(req.model.is_none());
    }

    #[test]
    fn request_deserializes_with_extras() {
        let json = r#"{
            "model": "gpt-4",
            "messages": [{"role":"user","content":"hi"}],
            "stream": true,
            "temperature": 0.7,
            "max_tokens": 100,
            "frequency_penalty": 0.5
        }"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.model.as_deref(), Some("gpt-4"));
        assert_eq!(req.stream, Some(true));
        assert_eq!(req.temperature, Some(0.7));
    }
}
