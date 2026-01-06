//! CLI message types for the JSON protocol.

use serde::{Deserialize, Serialize};

use super::content::ContentBlock;
use super::events::StreamEventType;
use super::usage::Usage;

/// A message from the Claude CLI JSON output.
///
/// The CLI outputs newline-delimited JSON messages of different types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CliMessage {
    /// System initialization message.
    System(SystemMessage),
    /// Assistant response message.
    Assistant(AssistantMessage),
    /// User message (typically tool results).
    User(UserMessage),
    /// Streaming event.
    StreamEvent(StreamEventMessage),
    /// Final result message.
    Result(ResultMessage),
}

/// System initialization message sent at the start of a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemMessage {
    /// Subtype of system message.
    pub subtype: String,
    /// Current working directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Session identifier.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Available tools.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Model being used.
    #[serde(default)]
    pub model: Option<String>,
    /// Permission mode.
    #[serde(default, rename = "permissionMode")]
    pub permission_mode: Option<String>,
    /// Claude Code version.
    #[serde(default)]
    pub claude_code_version: Option<String>,
}

impl SystemMessage {
    /// Check if this is an init message.
    pub fn is_init(&self) -> bool {
        self.subtype == "init"
    }
}

/// Assistant response message containing model output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantMessage {
    /// The message content.
    pub message: AssistantMessageContent,
    /// Session identifier.
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Content of an assistant message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantMessageContent {
    /// Message ID.
    pub id: String,
    /// Model used.
    pub model: String,
    /// Role (always "assistant").
    pub role: String,
    /// Content blocks.
    pub content: Vec<ContentBlock>,
    /// Reason the message stopped.
    #[serde(default)]
    pub stop_reason: Option<String>,
    /// Stop sequence that triggered completion.
    #[serde(default)]
    pub stop_sequence: Option<String>,
    /// Token usage for this message.
    #[serde(default)]
    pub usage: Option<Usage>,
}

impl AssistantMessageContent {
    /// Get all text content concatenated.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| block.text())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Get all tool use blocks.
    pub fn tool_uses(&self) -> Vec<&super::content::ToolUseBlock> {
        self.content
            .iter()
            .filter_map(|block| block.as_tool_use())
            .collect()
    }

    /// Check if the message ended normally.
    pub fn is_end_turn(&self) -> bool {
        self.stop_reason.as_deref() == Some("end_turn")
    }

    /// Check if the message stopped for tool use.
    pub fn is_tool_use(&self) -> bool {
        self.stop_reason.as_deref() == Some("tool_use")
    }
}

/// User message (typically containing tool results).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserMessage {
    /// The message content.
    pub message: UserMessageContent,
    /// Session identifier.
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Content of a user message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserMessageContent {
    /// Role (always "user").
    pub role: String,
    /// Content blocks (typically tool results).
    pub content: Vec<ContentBlock>,
}

impl UserMessageContent {
    /// Get all tool result blocks.
    pub fn tool_results(&self) -> Vec<&super::content::ToolResultBlock> {
        self.content
            .iter()
            .filter_map(|block| block.as_tool_result())
            .collect()
    }
}

/// Wrapper for streaming events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamEventMessage {
    /// The streaming event.
    pub event: StreamEventType,
    /// Session identifier.
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Final result message with summary statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultMessage {
    /// Result subtype.
    pub subtype: String,
    /// Whether the result indicates an error.
    #[serde(default)]
    pub is_error: bool,
    /// Duration in milliseconds.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Number of conversation turns.
    #[serde(default)]
    pub num_turns: Option<u32>,
    /// Final text result.
    #[serde(default)]
    pub result: Option<String>,
    /// Total cost in USD.
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    /// Total token usage.
    #[serde(default)]
    pub usage: Option<Usage>,
    /// Session identifier.
    #[serde(default)]
    pub session_id: Option<String>,
}

impl ResultMessage {
    /// Check if this is a success result.
    pub fn is_success(&self) -> bool {
        self.subtype == "success" && !self.is_error
    }

    /// Check if this is an error result.
    pub fn is_error(&self) -> bool {
        self.subtype == "error" || self.is_error
    }

    /// Get the duration as a std::time::Duration.
    pub fn duration(&self) -> Option<std::time::Duration> {
        self.duration_ms.map(std::time::Duration::from_millis)
    }
}

impl CliMessage {
    /// Get the session ID if present.
    pub fn session_id(&self) -> Option<&str> {
        match self {
            CliMessage::System(m) => m.session_id.as_deref(),
            CliMessage::Assistant(m) => m.session_id.as_deref(),
            CliMessage::User(m) => m.session_id.as_deref(),
            CliMessage::StreamEvent(m) => m.session_id.as_deref(),
            CliMessage::Result(m) => m.session_id.as_deref(),
        }
    }

    /// Check if this is a system init message.
    pub fn is_system_init(&self) -> bool {
        matches!(self, CliMessage::System(m) if m.is_init())
    }

    /// Check if this is a result message.
    pub fn is_result(&self) -> bool {
        matches!(self, CliMessage::Result(_))
    }

    /// Check if this is an assistant message.
    pub fn is_assistant(&self) -> bool {
        matches!(self, CliMessage::Assistant(_))
    }

    /// Check if this is a streaming event.
    pub fn is_stream_event(&self) -> bool {
        matches!(self, CliMessage::StreamEvent(_))
    }

    /// Get as system message.
    pub fn as_system(&self) -> Option<&SystemMessage> {
        match self {
            CliMessage::System(m) => Some(m),
            _ => None,
        }
    }

    /// Get as assistant message.
    pub fn as_assistant(&self) -> Option<&AssistantMessage> {
        match self {
            CliMessage::Assistant(m) => Some(m),
            _ => None,
        }
    }

    /// Get as user message.
    pub fn as_user(&self) -> Option<&UserMessage> {
        match self {
            CliMessage::User(m) => Some(m),
            _ => None,
        }
    }

    /// Get as stream event message.
    pub fn as_stream_event(&self) -> Option<&StreamEventMessage> {
        match self {
            CliMessage::StreamEvent(m) => Some(m),
            _ => None,
        }
    }

    /// Get as result message.
    pub fn as_result(&self) -> Option<&ResultMessage> {
        match self {
            CliMessage::Result(m) => Some(m),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_system_init() {
        let json = r#"{
            "type": "system",
            "subtype": "init",
            "cwd": "/home/user/project",
            "session_id": "550e8400-e29b-41d4-a716-446655440000",
            "tools": ["Bash", "Read", "Edit"],
            "model": "claude-opus-4-5-20251101",
            "permissionMode": "default",
            "claude_code_version": "2.0.76"
        }"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        assert!(msg.is_system_init());
        let system = msg.as_system().unwrap();
        assert_eq!(system.cwd.as_deref(), Some("/home/user/project"));
        assert_eq!(system.tools, vec!["Bash", "Read", "Edit"]);
        assert_eq!(system.model.as_deref(), Some("claude-opus-4-5-20251101"));
    }

    #[test]
    fn parse_assistant_message() {
        let json = r#"{
            "type": "assistant",
            "message": {
                "id": "msg_01234",
                "model": "claude-opus-4-5-20251101",
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Hello, world!"}
                ],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 100, "output_tokens": 50}
            },
            "session_id": "550e8400-e29b-41d4-a716-446655440000"
        }"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        assert!(msg.is_assistant());
        let assistant = msg.as_assistant().unwrap();
        assert_eq!(assistant.message.text(), "Hello, world!");
        assert!(assistant.message.is_end_turn());
    }

    #[test]
    fn parse_assistant_with_tool_use() {
        let json = r#"{
            "type": "assistant",
            "message": {
                "id": "msg_01234",
                "model": "claude-opus-4-5-20251101",
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Let me check that file."},
                    {"type": "tool_use", "id": "toolu_01234", "name": "Read", "input": {"path": "/tmp/test.txt"}}
                ],
                "stop_reason": "tool_use"
            },
            "session_id": "550e8400-e29b-41d4-a716-446655440000"
        }"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        let assistant = msg.as_assistant().unwrap();
        assert!(assistant.message.is_tool_use());
        let tool_uses = assistant.message.tool_uses();
        assert_eq!(tool_uses.len(), 1);
        assert_eq!(tool_uses[0].name, "Read");
    }

    #[test]
    fn parse_user_message() {
        let json = r#"{
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_01234", "content": "file contents here", "is_error": false}
                ]
            },
            "session_id": "550e8400-e29b-41d4-a716-446655440000"
        }"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        let user = msg.as_user().unwrap();
        let results = user.message.tool_results();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tool_use_id, "toolu_01234");
    }

    #[test]
    fn parse_stream_event() {
        let json = r#"{
            "type": "stream_event",
            "event": {
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "Hello"}
            },
            "session_id": "550e8400-e29b-41d4-a716-446655440000"
        }"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        assert!(msg.is_stream_event());
        let event = msg.as_stream_event().unwrap();
        assert!(event.event.is_text_delta());
        assert_eq!(event.event.text_delta(), Some("Hello"));
    }

    #[test]
    fn parse_result_success() {
        let json = r#"{
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "duration_ms": 1234,
            "num_turns": 1,
            "result": "The answer is 4.",
            "total_cost_usd": 0.01,
            "usage": {"input_tokens": 100, "output_tokens": 50}
        }"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        assert!(msg.is_result());
        let result = msg.as_result().unwrap();
        assert!(result.is_success());
        assert!(!result.is_error());
        assert_eq!(result.result.as_deref(), Some("The answer is 4."));
        assert_eq!(result.total_cost_usd, Some(0.01));
        assert_eq!(result.duration_ms, Some(1234));
    }

    #[test]
    fn parse_result_error() {
        let json = r#"{
            "type": "result",
            "subtype": "error",
            "is_error": true,
            "result": "Authentication failed"
        }"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        let result = msg.as_result().unwrap();
        assert!(result.is_error());
        assert!(!result.is_success());
    }

    #[test]
    fn session_id_extraction() {
        let json = r#"{
            "type": "system",
            "subtype": "init",
            "session_id": "test-session-123"
        }"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.session_id(), Some("test-session-123"));
    }

    #[test]
    fn result_duration() {
        let result = ResultMessage {
            subtype: "success".into(),
            is_error: false,
            duration_ms: Some(5000),
            num_turns: Some(1),
            result: Some("test".into()),
            total_cost_usd: None,
            usage: None,
            session_id: None,
        };
        assert_eq!(
            result.duration(),
            Some(std::time::Duration::from_millis(5000))
        );
    }

    #[test]
    fn result_duration_none() {
        let result = ResultMessage {
            subtype: "success".into(),
            is_error: false,
            duration_ms: None,
            num_turns: None,
            result: None,
            total_cost_usd: None,
            usage: None,
            session_id: None,
        };
        assert!(result.duration().is_none());
    }

    #[test]
    fn cli_message_as_system_returns_none_for_non_system() {
        let json = r#"{
            "type": "result",
            "subtype": "success",
            "is_error": false
        }"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        assert!(msg.as_system().is_none());
    }

    #[test]
    fn cli_message_as_assistant_returns_none_for_non_assistant() {
        let json = r#"{
            "type": "system",
            "subtype": "init"
        }"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        assert!(msg.as_assistant().is_none());
    }

    #[test]
    fn cli_message_as_user_returns_none_for_non_user() {
        let json = r#"{
            "type": "system",
            "subtype": "init"
        }"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        assert!(msg.as_user().is_none());
    }

    #[test]
    fn cli_message_as_stream_event_returns_none_for_non_stream_event() {
        let json = r#"{
            "type": "system",
            "subtype": "init"
        }"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        assert!(msg.as_stream_event().is_none());
    }

    #[test]
    fn cli_message_as_result_returns_none_for_non_result() {
        let json = r#"{
            "type": "system",
            "subtype": "init"
        }"#;
        let msg: CliMessage = serde_json::from_str(json).unwrap();
        assert!(msg.as_result().is_none());
    }

    #[test]
    fn cli_message_session_id_for_all_types() {
        // Test session_id extraction from User message
        let user_json = r#"{
            "type": "user",
            "message": {"role": "user", "content": []},
            "session_id": "user-session"
        }"#;
        let user_msg: CliMessage = serde_json::from_str(user_json).unwrap();
        assert_eq!(user_msg.session_id(), Some("user-session"));

        // Test session_id extraction from Result message
        let result_json = r#"{
            "type": "result",
            "subtype": "success",
            "session_id": "result-session"
        }"#;
        let result_msg: CliMessage = serde_json::from_str(result_json).unwrap();
        assert_eq!(result_msg.session_id(), Some("result-session"));
    }

    #[test]
    fn system_message_is_init_false_for_other_subtypes() {
        let system = SystemMessage {
            subtype: "status".into(),
            cwd: None,
            session_id: None,
            tools: vec![],
            model: None,
            permission_mode: None,
            claude_code_version: None,
        };
        assert!(!system.is_init());
    }

    #[test]
    fn assistant_message_content_methods() {
        let content = AssistantMessageContent {
            id: "msg_123".into(),
            model: "claude".into(),
            role: "assistant".into(),
            content: vec![
                ContentBlock::Text(super::super::content::TextBlock {
                    text: "Hello ".into(),
                }),
                ContentBlock::Text(super::super::content::TextBlock {
                    text: "World".into(),
                }),
            ],
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
            usage: None,
        };

        // text() joins without separator, so "Hello " + "World" = "Hello World"
        assert_eq!(content.text(), "Hello World");
        assert!(content.is_end_turn());
        assert!(!content.is_tool_use());
    }

    #[test]
    fn assistant_message_content_with_no_stop_reason() {
        let content = AssistantMessageContent {
            id: "msg_123".into(),
            model: "claude".into(),
            role: "assistant".into(),
            content: vec![],
            stop_reason: None,
            stop_sequence: None,
            usage: None,
        };

        assert!(!content.is_end_turn());
        assert!(!content.is_tool_use());
    }

    #[test]
    fn roundtrip_cli_message() {
        // Test that a CLI message can be serialized and deserialized
        let original = CliMessage::System(SystemMessage {
            subtype: "init".into(),
            cwd: Some("/home".into()),
            session_id: Some("sess-123".into()),
            tools: vec!["Read".into()],
            model: Some("claude".into()),
            permission_mode: Some("default".into()),
            claude_code_version: Some("2.0.0".into()),
        });

        let json = serde_json::to_string(&original).unwrap();
        let parsed: CliMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }
}
