//! Streaming event types from the Claude API.

use serde::{Deserialize, Serialize};

use super::usage::Usage;

/// A streaming event from the Claude API.
///
/// These are the raw events wrapped in `stream_event` messages from the CLI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEventType {
    /// Start of a new message.
    MessageStart {
        /// Partial message info.
        message: MessageStart,
    },
    /// Start of a content block.
    ContentBlockStart {
        /// Index of this block in the message.
        index: usize,
        /// The content block being started.
        content_block: ContentBlockInfo,
    },
    /// Delta update to a content block.
    ContentBlockDelta {
        /// Index of the block being updated.
        index: usize,
        /// The delta update.
        delta: ContentDelta,
    },
    /// End of a content block.
    ContentBlockStop {
        /// Index of the completed block.
        index: usize,
    },
    /// Delta update to the message (e.g., stop reason).
    MessageDelta {
        /// The delta update.
        delta: MessageDeltaInfo,
        /// Updated usage statistics.
        #[serde(default)]
        usage: Option<Usage>,
    },
    /// End of the message.
    MessageStop,
    /// Ping event (keepalive).
    Ping,
    /// Error event.
    Error {
        /// Error details.
        error: StreamError,
    },
}

/// Information at the start of a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageStart {
    /// Message ID.
    pub id: String,
    /// Model used.
    pub model: String,
    /// Role (always "assistant").
    pub role: String,
    /// Initial usage statistics.
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// Information about a content block being started.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlockInfo {
    /// Text block starting.
    Text {
        /// Initial text (usually empty).
        #[serde(default)]
        text: String,
    },
    /// Tool use block starting.
    ToolUse {
        /// Tool use ID.
        id: String,
        /// Tool name.
        name: String,
        /// Initial input (usually empty object).
        #[serde(default)]
        input: serde_json::Value,
    },
    /// Thinking block starting.
    Thinking {
        /// Initial thinking text.
        #[serde(default)]
        thinking: String,
    },
}

/// A delta update to a content block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentDelta {
    /// Text delta.
    TextDelta {
        /// The text fragment.
        text: String,
    },
    /// Tool input JSON delta.
    InputJsonDelta {
        /// Partial JSON string.
        partial_json: String,
    },
    /// Thinking delta.
    ThinkingDelta {
        /// The thinking fragment.
        thinking: String,
    },
}

impl ContentDelta {
    /// Get the text if this is a text delta.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentDelta::TextDelta { text } => Some(text),
            _ => None,
        }
    }

    /// Get the partial JSON if this is an input_json_delta.
    pub fn as_input_json(&self) -> Option<&str> {
        match self {
            ContentDelta::InputJsonDelta { partial_json } => Some(partial_json),
            _ => None,
        }
    }
}

/// Delta update to a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageDeltaInfo {
    /// The stop reason if message is complete.
    #[serde(default)]
    pub stop_reason: Option<String>,
    /// The stop sequence that triggered completion.
    #[serde(default)]
    pub stop_sequence: Option<String>,
}

/// Error information in a stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamError {
    /// Error type.
    #[serde(rename = "type")]
    pub error_type: String,
    /// Error message.
    pub message: String,
}

impl StreamEventType {
    /// Check if this is a text delta event.
    pub fn is_text_delta(&self) -> bool {
        matches!(
            self,
            StreamEventType::ContentBlockDelta {
                delta: ContentDelta::TextDelta { .. },
                ..
            }
        )
    }

    /// Extract text from a text delta event.
    pub fn text_delta(&self) -> Option<&str> {
        match self {
            StreamEventType::ContentBlockDelta {
                delta: ContentDelta::TextDelta { text },
                ..
            } => Some(text),
            _ => None,
        }
    }

    /// Check if this is the end of the message.
    pub fn is_message_stop(&self) -> bool {
        matches!(self, StreamEventType::MessageStop)
    }

    /// Check if this is an error event.
    pub fn is_error(&self) -> bool {
        matches!(self, StreamEventType::Error { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_message_start() {
        let json = r#"{
            "type": "message_start",
            "message": {
                "id": "msg_01234",
                "model": "claude-opus-4-5-20251101",
                "role": "assistant",
                "usage": {"input_tokens": 100, "output_tokens": 0}
            }
        }"#;
        let event: StreamEventType = serde_json::from_str(json).unwrap();
        match event {
            StreamEventType::MessageStart { message } => {
                assert_eq!(message.id, "msg_01234");
                assert_eq!(message.model, "claude-opus-4-5-20251101");
                assert_eq!(message.role, "assistant");
            }
            _ => panic!("Expected MessageStart"),
        }
    }

    #[test]
    fn parse_content_block_start_text() {
        let json = r#"{
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "text", "text": ""}
        }"#;
        let event: StreamEventType = serde_json::from_str(json).unwrap();
        match event {
            StreamEventType::ContentBlockStart {
                index,
                content_block,
            } => {
                assert_eq!(index, 0);
                assert!(matches!(content_block, ContentBlockInfo::Text { .. }));
            }
            _ => panic!("Expected ContentBlockStart"),
        }
    }

    #[test]
    fn parse_content_block_start_tool_use() {
        let json = r#"{
            "type": "content_block_start",
            "index": 1,
            "content_block": {
                "type": "tool_use",
                "id": "toolu_01234",
                "name": "Bash",
                "input": {}
            }
        }"#;
        let event: StreamEventType = serde_json::from_str(json).unwrap();
        match event {
            StreamEventType::ContentBlockStart {
                index,
                content_block: ContentBlockInfo::ToolUse { id, name, .. },
            } => {
                assert_eq!(index, 1);
                assert_eq!(id, "toolu_01234");
                assert_eq!(name, "Bash");
            }
            _ => panic!("Expected ContentBlockStart with ToolUse"),
        }
    }

    #[test]
    fn parse_text_delta() {
        let json = r#"{
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "Hello"}
        }"#;
        let event: StreamEventType = serde_json::from_str(json).unwrap();
        assert!(event.is_text_delta());
        assert_eq!(event.text_delta(), Some("Hello"));
    }

    #[test]
    fn parse_input_json_delta() {
        let json = r#"{
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": "{\"command\":"}
        }"#;
        let event: StreamEventType = serde_json::from_str(json).unwrap();
        match event {
            StreamEventType::ContentBlockDelta { index, delta } => {
                assert_eq!(index, 1);
                assert_eq!(delta.as_input_json(), Some("{\"command\":"));
            }
            _ => panic!("Expected ContentBlockDelta"),
        }
    }

    #[test]
    fn parse_content_block_stop() {
        let json = r#"{"type": "content_block_stop", "index": 0}"#;
        let event: StreamEventType = serde_json::from_str(json).unwrap();
        match event {
            StreamEventType::ContentBlockStop { index } => assert_eq!(index, 0),
            _ => panic!("Expected ContentBlockStop"),
        }
    }

    #[test]
    fn parse_message_delta() {
        let json = r#"{
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": 50}
        }"#;
        let event: StreamEventType = serde_json::from_str(json).unwrap();
        match event {
            StreamEventType::MessageDelta { delta, usage } => {
                assert_eq!(delta.stop_reason, Some("end_turn".into()));
                assert!(usage.is_some());
            }
            _ => panic!("Expected MessageDelta"),
        }
    }

    #[test]
    fn parse_message_stop() {
        let json = r#"{"type": "message_stop"}"#;
        let event: StreamEventType = serde_json::from_str(json).unwrap();
        assert!(event.is_message_stop());
    }

    #[test]
    fn parse_ping() {
        let json = r#"{"type": "ping"}"#;
        let event: StreamEventType = serde_json::from_str(json).unwrap();
        assert!(matches!(event, StreamEventType::Ping));
    }

    #[test]
    fn parse_error() {
        let json = r#"{
            "type": "error",
            "error": {"type": "overloaded_error", "message": "Server overloaded"}
        }"#;
        let event: StreamEventType = serde_json::from_str(json).unwrap();
        assert!(event.is_error());
        match event {
            StreamEventType::Error { error } => {
                assert_eq!(error.error_type, "overloaded_error");
                assert_eq!(error.message, "Server overloaded");
            }
            _ => panic!("Expected Error"),
        }
    }
}
