//! JSON protocol types for Claude CLI communication.
//!
//! This module defines the types for the JSON messages exchanged with the Claude CLI
//! when using `--output-format json`.
//!
//! # Message Types
//!
//! The CLI outputs newline-delimited JSON messages of these types:
//!
//! - [`SystemMessage`]: Initialization info (session ID, tools, model)
//! - [`AssistantMessage`]: Model responses with text and tool use
//! - [`UserMessage`]: Tool results returned to the model
//! - [`StreamEventMessage`]: Real-time streaming events
//! - [`ResultMessage`]: Final summary with cost and usage
//!
//! # Example
//!
//! ```
//! use libclaude::protocol::{CliMessage, ContentBlock};
//!
//! let json = r#"{"type": "assistant", "message": {"id": "msg_01", "model": "claude-opus-4-5-20251101", "role": "assistant", "content": [{"type": "text", "text": "Hello!"}], "stop_reason": "end_turn"}}"#;
//! let msg: CliMessage = serde_json::from_str(json).unwrap();
//!
//! if let Some(assistant) = msg.as_assistant() {
//!     println!("Response: {}", assistant.message.text());
//! }
//! ```

mod content;
mod events;
mod messages;
mod usage;

// Re-export all public types
pub use content::{
    ContentBlock, ImageSource, TextBlock, ThinkingBlock, ToolResultBlock, ToolResultContent,
    ToolResultContentBlock, ToolUseBlock,
};
pub use events::{
    ContentBlockInfo, ContentDelta, MessageDeltaInfo, MessageStart, StreamError, StreamEventType,
};
pub use messages::{
    AssistantMessage, AssistantMessageContent, CliMessage, ResultMessage, StreamEventMessage,
    SystemMessage, UserMessage, UserMessageContent,
};
pub use usage::Usage;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_types_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CliMessage>();
        assert_send_sync::<ContentBlock>();
        assert_send_sync::<StreamEventType>();
        assert_send_sync::<Usage>();
    }

    #[test]
    fn roundtrip_cli_message() {
        let original = CliMessage::Assistant(AssistantMessage {
            message: AssistantMessageContent {
                id: "msg_test".into(),
                model: "claude-opus-4-5-20251101".into(),
                role: "assistant".into(),
                content: vec![ContentBlock::Text(TextBlock {
                    text: "Hello, world!".into(),
                })],
                stop_reason: Some("end_turn".into()),
                stop_sequence: None,
                usage: Some(Usage {
                    input_tokens: 100,
                    output_tokens: 50,
                    ..Default::default()
                }),
            },
            session_id: Some("session-123".into()),
        });

        let json = serde_json::to_string(&original).unwrap();
        let parsed: CliMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }
}
