//! High-level streaming events for consumers.
//!
//! This module provides the [`StreamEvent`] enum which represents meaningful
//! events during a Claude response stream. These are higher-level than the
//! raw CLI protocol messages and are designed for convenient consumption.

use crate::config::SessionId;
use crate::protocol::{AssistantMessage, ContentBlock, ResultMessage, Usage, UserMessage};

/// Session initialization information.
///
/// Contains metadata about the session extracted from the system init message.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Session identifier.
    pub session_id: SessionId,
    /// Current working directory.
    pub cwd: Option<String>,
    /// Available tools.
    pub tools: Vec<String>,
    /// Model being used.
    pub model: Option<String>,
    /// Permission mode.
    pub permission_mode: Option<String>,
    /// Claude Code version.
    pub claude_code_version: Option<String>,
}

/// A high-level streaming event for consumers.
///
/// These events represent meaningful moments in a Claude response stream,
/// abstracting away the low-level CLI protocol details.
///
/// # Event Order
///
/// Events are emitted in a predictable order:
/// 1. `SessionInit` - Always first (once per stream)
/// 2. `TextDelta`/`ToolInputDelta` - Incremental content as it arrives
/// 3. `ContentBlockComplete` - When each content block finishes
/// 4. `AssistantMessage` - Complete assistant messages
/// 5. `ToolResult` - Results from tool execution (if tools are used)
/// 6. `UsageUpdate` - Periodic usage statistics
/// 7. `Complete` - Always last on success
///
/// # Example
///
/// ```ignore
/// use futures::StreamExt;
/// use libclaude::stream::StreamEvent;
///
/// let mut stream = client.send("Hello").await?;
/// while let Some(event) = stream.next().await {
///     match event? {
///         StreamEvent::TextDelta { text, .. } => print!("{}", text),
///         StreamEvent::Complete(result) => {
///             println!("\nCost: ${:.4}", result.total_cost_usd.unwrap_or(0.0));
///         }
///         _ => {}
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Session metadata from system/init message.
    ///
    /// Always emitted first. Contains session ID and configuration info.
    SessionInit(SessionInfo),

    /// Token-by-token text as it arrives.
    ///
    /// These events allow displaying text incrementally as Claude generates it.
    TextDelta {
        /// Index of the content block this text belongs to.
        index: usize,
        /// The text fragment.
        text: String,
    },

    /// Partial tool input JSON as it arrives.
    ///
    /// Useful for showing tool call progress. The JSON is incomplete until
    /// the corresponding `ContentBlockComplete` event.
    ToolInputDelta {
        /// Index of the content block this input belongs to.
        index: usize,
        /// Partial JSON string.
        partial_json: String,
    },

    /// Thinking delta (for extended thinking mode).
    ///
    /// Shows Claude's internal reasoning process incrementally.
    ThinkingDelta {
        /// Index of the content block.
        index: usize,
        /// The thinking text fragment.
        thinking: String,
    },

    /// A content block has completed.
    ///
    /// Emitted when a text, tool_use, or thinking block is fully received.
    ContentBlockComplete {
        /// Index of the completed block.
        index: usize,
        /// The complete content block.
        block: ContentBlock,
    },

    /// Full assistant message.
    ///
    /// The final message is always emitted. Intermediate messages during
    /// tool execution are only emitted with `--include-partial-messages`.
    AssistantMessage(AssistantMessage),

    /// Tool execution result from the CLI.
    ///
    /// Emitted when a tool completes execution. The CLI handles tool
    /// execution automatically; this is for observation only.
    ToolResult(UserMessage),

    /// Final result with cost and usage statistics.
    ///
    /// Always emitted last on successful completion.
    Complete(ResultMessage),

    /// Incremental usage update.
    ///
    /// Emitted periodically during streaming with current token counts.
    UsageUpdate(Usage),
}

impl StreamEvent {
    /// Check if this is a text delta event.
    pub fn is_text_delta(&self) -> bool {
        matches!(self, StreamEvent::TextDelta { .. })
    }

    /// Check if this is the completion event.
    pub fn is_complete(&self) -> bool {
        matches!(self, StreamEvent::Complete(_))
    }

    /// Get text from a TextDelta event.
    pub fn text(&self) -> Option<&str> {
        match self {
            StreamEvent::TextDelta { text, .. } => Some(text),
            _ => None,
        }
    }

    /// Get the result if this is a Complete event.
    pub fn as_complete(&self) -> Option<&ResultMessage> {
        match self {
            StreamEvent::Complete(r) => Some(r),
            _ => None,
        }
    }

    /// Get the assistant message if this is an AssistantMessage event.
    pub fn as_assistant_message(&self) -> Option<&AssistantMessage> {
        match self {
            StreamEvent::AssistantMessage(m) => Some(m),
            _ => None,
        }
    }

    /// Get session info if this is a SessionInit event.
    pub fn as_session_init(&self) -> Option<&SessionInfo> {
        match self {
            StreamEvent::SessionInit(info) => Some(info),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{AssistantMessageContent, TextBlock, ToolUseBlock};

    #[test]
    fn stream_event_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<StreamEvent>();
        assert_send_sync::<SessionInfo>();
    }

    #[test]
    fn text_delta_accessors() {
        let event = StreamEvent::TextDelta {
            index: 0,
            text: "Hello".to_string(),
        };
        assert!(event.is_text_delta());
        assert_eq!(event.text(), Some("Hello"));
        assert!(!event.is_complete());
    }

    #[test]
    fn session_info_creation() {
        let info = SessionInfo {
            session_id: SessionId::new("test-123"),
            cwd: Some("/home/user".to_string()),
            tools: vec!["Read".to_string(), "Write".to_string()],
            model: Some("claude-opus".to_string()),
            permission_mode: Some("default".to_string()),
            claude_code_version: Some("2.0.76".to_string()),
        };
        assert_eq!(info.session_id.as_str(), "test-123");
        assert_eq!(info.tools.len(), 2);
    }

    #[test]
    fn is_complete_true_for_complete_event() {
        let result = ResultMessage {
            subtype: "success".to_string(),
            is_error: false,
            duration_ms: Some(100),
            num_turns: Some(1),
            result: Some("done".to_string()),
            total_cost_usd: Some(0.01),
            usage: None,
            session_id: None,
        };
        let event = StreamEvent::Complete(result);
        assert!(event.is_complete());
        assert!(!event.is_text_delta());
    }

    #[test]
    fn text_returns_none_for_non_text_events() {
        let result = ResultMessage {
            subtype: "success".to_string(),
            is_error: false,
            duration_ms: None,
            num_turns: None,
            result: None,
            total_cost_usd: None,
            usage: None,
            session_id: None,
        };
        let event = StreamEvent::Complete(result);
        assert!(event.text().is_none());
    }

    #[test]
    fn as_complete_returns_result() {
        let result = ResultMessage {
            subtype: "success".to_string(),
            is_error: false,
            duration_ms: Some(500),
            num_turns: Some(2),
            result: Some("final".to_string()),
            total_cost_usd: Some(0.05),
            usage: None,
            session_id: Some("session-1".to_string()),
        };
        let event = StreamEvent::Complete(result.clone());
        let extracted = event.as_complete().unwrap();
        assert_eq!(extracted.subtype, "success");
        assert_eq!(extracted.duration_ms, Some(500));
    }

    #[test]
    fn as_complete_returns_none_for_other_events() {
        let event = StreamEvent::TextDelta {
            index: 0,
            text: "text".to_string(),
        };
        assert!(event.as_complete().is_none());
    }

    #[test]
    fn as_assistant_message_returns_message() {
        let msg = AssistantMessage {
            message: AssistantMessageContent {
                id: "msg_123".to_string(),
                model: "claude".to_string(),
                role: "assistant".to_string(),
                content: vec![ContentBlock::Text(TextBlock {
                    text: "Hello".to_string(),
                })],
                stop_reason: Some("end_turn".to_string()),
                stop_sequence: None,
                usage: None,
            },
            session_id: None,
        };
        let event = StreamEvent::AssistantMessage(msg);
        let extracted = event.as_assistant_message().unwrap();
        assert_eq!(extracted.message.id, "msg_123");
        assert_eq!(extracted.message.text(), "Hello");
    }

    #[test]
    fn as_assistant_message_returns_none_for_other_events() {
        let event = StreamEvent::UsageUpdate(Usage::default());
        assert!(event.as_assistant_message().is_none());
    }

    #[test]
    fn as_session_init_returns_info() {
        let info = SessionInfo {
            session_id: SessionId::new("sess-abc"),
            cwd: Some("/tmp".to_string()),
            tools: vec!["Bash".to_string()],
            model: Some("opus".to_string()),
            permission_mode: None,
            claude_code_version: None,
        };
        let event = StreamEvent::SessionInit(info);
        let extracted = event.as_session_init().unwrap();
        assert_eq!(extracted.session_id.as_str(), "sess-abc");
        assert_eq!(extracted.cwd, Some("/tmp".to_string()));
    }

    #[test]
    fn as_session_init_returns_none_for_other_events() {
        let event = StreamEvent::ThinkingDelta {
            index: 0,
            thinking: "hmm".to_string(),
        };
        assert!(event.as_session_init().is_none());
    }

    #[test]
    fn tool_input_delta_event() {
        let event = StreamEvent::ToolInputDelta {
            index: 1,
            partial_json: r#"{"path":"#.to_string(),
        };
        assert!(!event.is_text_delta());
        assert!(!event.is_complete());
        assert!(event.text().is_none());
    }

    #[test]
    fn thinking_delta_event() {
        let event = StreamEvent::ThinkingDelta {
            index: 0,
            thinking: "Let me think...".to_string(),
        };
        assert!(!event.is_text_delta());
        assert!(!event.is_complete());
    }

    #[test]
    fn content_block_complete_event() {
        let block = ContentBlock::ToolUse(ToolUseBlock {
            id: "tool_1".to_string(),
            name: "Read".to_string(),
            input: serde_json::json!({"path": "/tmp"}),
        });
        let event = StreamEvent::ContentBlockComplete { index: 0, block };
        assert!(!event.is_text_delta());
        assert!(!event.is_complete());
    }

    #[test]
    fn usage_update_event() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        let event = StreamEvent::UsageUpdate(usage);
        assert!(!event.is_text_delta());
        assert!(!event.is_complete());
    }

    #[test]
    fn session_info_with_minimal_fields() {
        let info = SessionInfo {
            session_id: SessionId::new("min"),
            cwd: None,
            tools: vec![],
            model: None,
            permission_mode: None,
            claude_code_version: None,
        };
        assert_eq!(info.session_id.as_str(), "min");
        assert!(info.cwd.is_none());
        assert!(info.tools.is_empty());
        assert!(info.model.is_none());
    }
}
