//! The wire shapes of the Model Context Protocol.
//!
//! Only what this server actually sends: MCP is a large spec and most of it
//! describes client behaviour, sampling and elicitation that a read-only
//! telemetry server has no use for. Everything here is camelCase on the
//! wire and snake_case in Rust.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The revision this server implements.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// Revisions whose shape this server is compatible with. A client asking
/// for one of these gets it echoed back; anything else is answered with
/// [`PROTOCOL_VERSION`] and the client decides whether to proceed.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

pub fn negotiate_version(requested: Option<&str>) -> &'static str {
    requested
        .and_then(|r| SUPPORTED_PROTOCOL_VERSIONS.iter().find(|v| **v == r))
        .copied()
        .unwrap_or(PROTOCOL_VERSION)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    #[serde(default)]
    pub protocol_version: Option<String>,
    #[serde(default)]
    pub client_info: Option<Implementation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Implementation {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: &'static str,
    pub capabilities: ServerCapabilities,
    pub server_info: Implementation,
    /// Shown to the model as system context. This is the one place to say
    /// what the server is holding and how to query it well.
    pub instructions: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    pub tools: ListChanged,
    pub resources: ListChanged,
    pub prompts: ListChanged,
    /// Advertised as present-but-empty: the server accepts
    /// `logging/setLevel` and otherwise says nothing.
    pub logging: Value,
}

impl ServerCapabilities {
    pub fn new() -> Self {
        Self {
            tools: ListChanged::default(),
            resources: ListChanged::default(),
            prompts: ListChanged::default(),
            logging: Value::Object(Default::default()),
        }
    }
}

/// Nothing this server exposes changes while it runs, so every
/// `listChanged` is false and there are no subscriptions.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListChanged {
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: String,
    pub input_schema: Value,
    pub annotations: ToolAnnotations,
}

/// Every tool here reads; none of them write, and none of them reach
/// outside the otelview instance they are pointed at.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    pub title: &'static str,
    pub read_only_hint: bool,
    pub destructive_hint: bool,
    pub idempotent_hint: bool,
    pub open_world_hint: bool,
}

impl ToolAnnotations {
    pub fn read_only(title: &'static str) -> Self {
        Self {
            title,
            read_only_hint: true,
            destructive_hint: false,
            idempotent_hint: true,
            open_world_hint: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Content {
    #[serde(rename_all = "camelCase")]
    Text { text: String },
}

impl Content {
    pub fn text(s: impl Into<String>) -> Self {
        Content::Text { text: s.into() }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    pub content: Vec<Content>,
    /// The same answer as machine-readable JSON. Clients that understand it
    /// hand the model the structure; the rest read the text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    pub is_error: bool,
}

impl CallToolResult {
    pub fn ok(text: impl Into<String>, structured: Value) -> Self {
        Self {
            content: vec![Content::text(text)],
            // The spec requires an object here, so a bare array or scalar
            // is carried under a key rather than sent as-is and rejected.
            structured_content: Some(match structured {
                Value::Object(_) => structured,
                other => serde_json::json!({ "result": other }),
            }),
            is_error: false,
        }
    }

    /// A failure the model should see and can act on — a mistyped query, an
    /// unknown trace id — as opposed to a protocol error, which is a
    /// JSON-RPC error object and never reaches the model.
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![Content::text(text)],
            structured_content: None,
            is_error: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub uri: String,
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub mime_type: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceContents {
    pub uri: String,
    pub mime_type: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub arguments: Vec<PromptArgument>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptArgument {
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPromptResult {
    pub description: String,
    pub messages: Vec<PromptMessage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptMessage {
    pub role: &'static str,
    pub content: Content,
}
