//! Test utilities for libclaude integration tests.

use std::collections::VecDeque;

use libclaude::protocol::{
    AssistantMessage, AssistantMessageContent, CliMessage, ContentBlock, ContentBlockInfo,
    ContentDelta, MessageDeltaInfo, MessageStart, ResultMessage, StreamEventMessage,
    StreamEventType, SystemMessage, TextBlock, Usage,
};
use libclaude::process::MessageReader;
use libclaude::{Error, Result};

/// A mock message reader that returns pre-defined messages.
///
/// Messages are returned in order, then `Ok(None)` is returned to signal EOF.
pub struct MockReader {
    messages: VecDeque<Result<CliMessage>>,
}

impl MockReader {
    /// Create a new mock reader with the given messages.
    pub fn new(messages: Vec<CliMessage>) -> Self {
        Self {
            messages: messages.into_iter().map(Ok).collect(),
        }
    }

    /// Create a mock reader that will return an error.
    pub fn with_error(mut messages: Vec<CliMessage>, error: Error) -> Self {
        let mut queue: VecDeque<Result<CliMessage>> = messages.drain(..).map(Ok).collect();
        queue.push_back(Err(error));
        Self { messages: queue }
    }
}

impl MessageReader for MockReader {
    async fn read_message(&mut self) -> Result<Option<CliMessage>> {
        match self.messages.pop_front() {
            Some(Ok(msg)) => Ok(Some(msg)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }
}

/// Builder for creating realistic message sequences.
pub struct ScenarioBuilder {
    messages: Vec<CliMessage>,
    session_id: String,
}

impl ScenarioBuilder {
    /// Create a new scenario builder.
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            session_id: "test-session-123".to_string(),
        }
    }

    /// Set the session ID.
    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = id.into();
        self
    }

    /// Add a system init message.
    pub fn system_init(mut self) -> Self {
        self.messages.push(CliMessage::System(SystemMessage {
            subtype: "init".to_string(),
            cwd: Some("/tmp".to_string()),
            session_id: Some(self.session_id.clone()),
            tools: vec!["Read".to_string(), "Write".to_string(), "Bash".to_string()],
            model: Some("claude-sonnet-4-20250514".to_string()),
            permission_mode: Some("default".to_string()),
            claude_code_version: Some("2.0.0".to_string()),
        }));
        self
    }

    /// Add streaming events for a text response.
    pub fn text_response(mut self, text: &str) -> Self {
        // MessageStart
        self.messages
            .push(CliMessage::StreamEvent(StreamEventMessage {
                event: StreamEventType::MessageStart {
                    message: MessageStart {
                        id: "msg_123".to_string(),
                        model: "claude-sonnet-4-20250514".to_string(),
                        role: "assistant".to_string(),
                        usage: None,
                    },
                },
                session_id: Some(self.session_id.clone()),
            }));

        // ContentBlockStart
        self.messages
            .push(CliMessage::StreamEvent(StreamEventMessage {
                event: StreamEventType::ContentBlockStart {
                    index: 0,
                    content_block: ContentBlockInfo::Text {
                        text: String::new(),
                    },
                },
                session_id: Some(self.session_id.clone()),
            }));

        // Stream text in chunks (respecting UTF-8 boundaries)
        let chars: Vec<char> = text.chars().collect();
        for chunk in chars.chunks(10) {
            let chunk_text: String = chunk.iter().collect();
            self.messages
                .push(CliMessage::StreamEvent(StreamEventMessage {
                    event: StreamEventType::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::TextDelta { text: chunk_text },
                    },
                    session_id: Some(self.session_id.clone()),
                }));
        }

        // ContentBlockStop
        self.messages
            .push(CliMessage::StreamEvent(StreamEventMessage {
                event: StreamEventType::ContentBlockStop { index: 0 },
                session_id: Some(self.session_id.clone()),
            }));

        // MessageDelta with usage
        self.messages
            .push(CliMessage::StreamEvent(StreamEventMessage {
                event: StreamEventType::MessageDelta {
                    delta: MessageDeltaInfo {
                        stop_reason: Some("end_turn".to_string()),
                        stop_sequence: None,
                    },
                    usage: Some(Usage {
                        input_tokens: 100,
                        output_tokens: text.len() as u64 / 4,
                        ..Default::default()
                    }),
                },
                session_id: Some(self.session_id.clone()),
            }));

        // MessageStop
        self.messages
            .push(CliMessage::StreamEvent(StreamEventMessage {
                event: StreamEventType::MessageStop,
                session_id: Some(self.session_id.clone()),
            }));

        // Full assistant message
        self.messages.push(CliMessage::Assistant(AssistantMessage {
            message: AssistantMessageContent {
                id: "msg_123".to_string(),
                model: "claude-sonnet-4-20250514".to_string(),
                role: "assistant".to_string(),
                content: vec![ContentBlock::Text(TextBlock {
                    text: text.to_string(),
                })],
                stop_reason: Some("end_turn".to_string()),
                stop_sequence: None,
                usage: Some(Usage {
                    input_tokens: 100,
                    output_tokens: text.len() as u64 / 4,
                    ..Default::default()
                }),
            },
            session_id: Some(self.session_id.clone()),
        }));

        self
    }

    /// Add a success result message.
    pub fn success_result(mut self, result_text: Option<&str>) -> Self {
        self.messages.push(CliMessage::Result(ResultMessage {
            subtype: "success".to_string(),
            is_error: false,
            duration_ms: Some(1000),
            num_turns: Some(1),
            result: result_text.map(|s| s.to_string()),
            total_cost_usd: Some(0.001),
            usage: Some(Usage {
                input_tokens: 100,
                output_tokens: 25,
                ..Default::default()
            }),
            session_id: Some(self.session_id.clone()),
        }));
        self
    }

    /// Add an error result message.
    pub fn error_result(mut self, error_message: &str) -> Self {
        self.messages.push(CliMessage::Result(ResultMessage {
            subtype: "error".to_string(),
            is_error: true,
            duration_ms: Some(100),
            num_turns: Some(0),
            result: Some(error_message.to_string()),
            total_cost_usd: None,
            usage: None,
            session_id: Some(self.session_id.clone()),
        }));
        self
    }

    /// Add a ping event.
    pub fn ping(mut self) -> Self {
        self.messages
            .push(CliMessage::StreamEvent(StreamEventMessage {
                event: StreamEventType::Ping,
                session_id: Some(self.session_id.clone()),
            }));
        self
    }

    /// Build the mock reader.
    pub fn build(self) -> MockReader {
        MockReader::new(self.messages)
    }

}

impl Default for ScenarioBuilder {
    fn default() -> Self {
        Self::new()
    }
}
