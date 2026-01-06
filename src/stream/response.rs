//! Response stream implementation.
//!
//! This module provides [`ResponseStream`], which implements [`futures::Stream`]
//! to yield [`StreamEvent`]s from a Claude CLI process.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::Stream;
use tokio::sync::mpsc;
use tokio::time::timeout as tokio_timeout;

use super::events::{SessionInfo, StreamEvent};
use crate::config::SessionId;
use crate::process::ClaudeProcess;
use crate::protocol::{
    AssistantMessage, CliMessage, ContentBlock, ContentBlockInfo, ContentDelta, ResultMessage,
    StreamEventType, SystemMessage, TextBlock, ThinkingBlock, ToolUseBlock, Usage,
};
use crate::tools::ToolObserver;
use crate::{Error, Result};

/// Shared state for cancellation detection.
struct SharedState {
    /// The child process, kept alive while streaming.
    process: ClaudeProcess,
}

/// A stream of events from a Claude CLI response.
///
/// This stream yields [`StreamEvent`]s as Claude generates its response.
/// It implements [`futures::Stream`] for use with async combinators.
///
/// # Cancellation
///
/// Dropping a `ResponseStream` will:
/// 1. Stop the background reader task
/// 2. Kill the subprocess
///
/// # Example
///
/// ```ignore
/// use futures::StreamExt;
///
/// let mut stream = client.send("Hello").await?;
/// while let Some(event) = stream.next().await {
///     match event? {
///         StreamEvent::TextDelta { text, .. } => print!("{}", text),
///         StreamEvent::Complete(_) => break,
///         _ => {}
///     }
/// }
/// ```
pub struct ResponseStream {
    rx: mpsc::Receiver<Result<StreamEvent>>,
    /// Held to keep the process alive until the stream is dropped.
    #[allow(dead_code)]
    state: Option<Arc<tokio::sync::Mutex<SharedState>>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    session_id: Option<SessionId>,
    total_usage: Usage,
}

impl ResponseStream {
    /// Create a new response stream from a Claude process.
    ///
    /// This spawns a background task to read from the process and
    /// transform messages into stream events.
    pub fn new(process: ClaudeProcess) -> Self {
        Self::with_observer(process, None)
    }

    /// Create a new response stream with an optional tool observer.
    ///
    /// The observer will be called for tool use and tool result events.
    pub fn with_observer(
        mut process: ClaudeProcess,
        observer: Option<Arc<dyn ToolObserver>>,
    ) -> Self {
        let (tx, rx) = mpsc::channel(64);

        let reader = process
            .take_reader()
            .expect("process reader should be available");

        let state = Arc::new(tokio::sync::Mutex::new(SharedState { process }));
        let state_clone = Arc::clone(&state);

        // Spawn background reader task
        let task_handle = tokio::spawn(async move {
            let result = Self::read_loop(reader, tx.clone(), observer).await;
            if let Err(e) = result {
                // Try to send the error, ignore if receiver is gone
                let _ = tx.send(Err(e)).await;
            }
            // Clean up process
            let mut guard = state_clone.lock().await;
            let _ = guard.process.start_kill();
        });

        Self {
            rx,
            state: Some(state),
            task_handle: Some(task_handle),
            session_id: None,
            total_usage: Usage::default(),
        }
    }

    /// Background loop that reads from the process and sends events.
    async fn read_loop(
        mut reader: crate::process::ProcessReader,
        tx: mpsc::Sender<Result<StreamEvent>>,
        observer: Option<Arc<dyn ToolObserver>>,
    ) -> Result<()> {
        // Track content blocks being built
        let mut content_blocks: HashMap<usize, ContentBlockBuilder> = HashMap::new();

        loop {
            // Check if receiver is still interested
            if tx.is_closed() {
                return Err(Error::Cancelled);
            }

            match reader.read_message().await? {
                None => {
                    // EOF - stream complete
                    return Ok(());
                }
                Some(msg) => {
                    let events = Self::transform_message(msg, &mut content_blocks)?;
                    for event in &events {
                        // Dispatch to observer if present
                        if let Some(ref obs) = observer {
                            Self::dispatch_to_observer(obs.as_ref(), event);
                        }
                    }
                    for event in events {
                        if tx.send(Ok(event)).await.is_err() {
                            // Receiver dropped
                            return Err(Error::Cancelled);
                        }
                    }
                }
            }
        }
    }

    /// Dispatch events to the tool observer.
    fn dispatch_to_observer(observer: &dyn ToolObserver, event: &StreamEvent) {
        match event {
            StreamEvent::ContentBlockComplete { block, .. } => {
                if let ContentBlock::ToolUse(tool_use) = block {
                    observer.on_tool_use(&tool_use.id, &tool_use.name, &tool_use.input);
                }
            }
            StreamEvent::ToolResult(user_msg) => {
                for result in user_msg.message.tool_results() {
                    observer.on_tool_result(
                        &result.tool_use_id,
                        &result.content.as_text(),
                        result.is_error,
                    );
                }
            }
            _ => {}
        }
    }

    /// Transform a CLI message into zero or more stream events.
    fn transform_message(
        msg: CliMessage,
        content_blocks: &mut HashMap<usize, ContentBlockBuilder>,
    ) -> Result<Vec<StreamEvent>> {
        let mut events = Vec::new();

        match msg {
            CliMessage::System(system) => {
                if system.is_init() {
                    events.push(StreamEvent::SessionInit(Self::session_info_from_system(
                        &system,
                    )));
                }
            }

            CliMessage::Assistant(assistant) => {
                events.push(StreamEvent::AssistantMessage(assistant));
            }

            CliMessage::User(user) => {
                events.push(StreamEvent::ToolResult(user));
            }

            CliMessage::StreamEvent(stream_event) => {
                match stream_event.event {
                    StreamEventType::MessageStart { .. } => {
                        // Start of a new message - no event needed
                    }

                    StreamEventType::ContentBlockStart {
                        index,
                        content_block,
                    } => {
                        content_blocks.insert(index, ContentBlockBuilder::new(content_block));
                    }

                    StreamEventType::ContentBlockDelta { index, delta } => {
                        if let Some(builder) = content_blocks.get_mut(&index) {
                            builder.apply_delta(&delta);
                        }

                        // Emit delta events
                        match delta {
                            ContentDelta::TextDelta { text } => {
                                events.push(StreamEvent::TextDelta { index, text });
                            }
                            ContentDelta::InputJsonDelta { partial_json } => {
                                events.push(StreamEvent::ToolInputDelta {
                                    index,
                                    partial_json,
                                });
                            }
                            ContentDelta::ThinkingDelta { thinking } => {
                                events.push(StreamEvent::ThinkingDelta {
                                    index,
                                    thinking,
                                });
                            }
                        }
                    }

                    StreamEventType::ContentBlockStop { index } => {
                        if let Some(builder) = content_blocks.remove(&index) {
                            if let Some(block) = builder.build() {
                                events.push(StreamEvent::ContentBlockComplete { index, block });
                            }
                        }
                    }

                    StreamEventType::MessageDelta { usage, .. } => {
                        if let Some(usage) = usage {
                            events.push(StreamEvent::UsageUpdate(usage));
                        }
                    }

                    StreamEventType::MessageStop => {
                        // Message complete - no event needed
                    }

                    StreamEventType::Ping => {
                        // Keepalive - no event needed
                    }

                    StreamEventType::Error { error } => {
                        return Err(Error::CliError {
                            message: error.message,
                            is_auth_error: error.error_type.contains("auth"),
                        });
                    }
                }
            }

            CliMessage::Result(result) => {
                events.push(StreamEvent::Complete(result));
            }
        }

        Ok(events)
    }

    /// Extract session info from a system message.
    fn session_info_from_system(system: &SystemMessage) -> SessionInfo {
        SessionInfo {
            session_id: SessionId::new(
                system
                    .session_id
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
            ),
            cwd: system.cwd.clone(),
            tools: system.tools.clone(),
            model: system.model.clone(),
            permission_mode: system.permission_mode.clone(),
            claude_code_version: system.claude_code_version.clone(),
        }
    }

    /// Get the session ID if it has been received.
    pub fn session_id(&self) -> Option<&SessionId> {
        self.session_id.as_ref()
    }

    /// Get the accumulated usage so far.
    pub fn total_usage(&self) -> &Usage {
        &self.total_usage
    }

    /// Collect all text from the stream, ignoring other events.
    ///
    /// This is a convenience method for simple use cases where you just
    /// want the final text output.
    pub async fn collect_text(mut self) -> Result<String> {
        use futures::StreamExt;

        let mut text = String::new();

        while let Some(event) = self.next().await {
            match event? {
                StreamEvent::TextDelta { text: t, .. } => {
                    text.push_str(&t);
                }
                StreamEvent::Complete(result) => {
                    if result.is_error() {
                        return Err(Error::CliError {
                            message: result.result.unwrap_or_else(|| "unknown error".to_string()),
                            is_auth_error: false,
                        });
                    }
                }
                _ => {}
            }
        }

        Ok(text)
    }

    /// Collect all events from the stream.
    ///
    /// Returns the final result message along with accumulated events.
    pub async fn collect_all(mut self) -> Result<CollectedResponse> {
        use futures::StreamExt;

        let mut response = CollectedResponse::default();

        while let Some(event) = self.next().await {
            let event = event?;
            match &event {
                StreamEvent::SessionInit(info) => {
                    response.session_id = Some(info.session_id.clone());
                }
                StreamEvent::TextDelta { text, .. } => {
                    response.text.push_str(text);
                }
                StreamEvent::AssistantMessage(msg) => {
                    response.messages.push(msg.clone());
                }
                StreamEvent::Complete(result) => {
                    response.result = Some(result.clone());
                    if let Some(ref usage) = result.usage {
                        response.usage = usage.clone();
                    }
                    response.cost_usd = result.total_cost_usd;
                }
                StreamEvent::UsageUpdate(usage) => {
                    response.usage = usage.clone();
                }
                _ => {}
            }
            response.events.push(event);
        }

        Ok(response)
    }
}

impl Stream for ResponseStream {
    type Item = Result<StreamEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        // Poll the receiver
        match this.rx.poll_recv(cx) {
            Poll::Ready(Some(Ok(event))) => {
                // Update session ID if we get it
                if let StreamEvent::SessionInit(ref info) = event {
                    this.session_id = Some(info.session_id.clone());
                }

                // Accumulate usage
                if let StreamEvent::UsageUpdate(ref usage) = event {
                    this.total_usage.accumulate(usage);
                }

                Poll::Ready(Some(Ok(event)))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => {
                // Channel closed, stream complete
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for ResponseStream {
    fn drop(&mut self) {
        // Cancel the background task
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }

        // Process will be killed when state is dropped (via SharedState)
    }
}

/// A collected response from a completed stream.
#[derive(Debug, Clone, Default)]
pub struct CollectedResponse {
    /// Session ID from the response.
    pub session_id: Option<SessionId>,
    /// All text content concatenated.
    pub text: String,
    /// All assistant messages in order.
    pub messages: Vec<AssistantMessage>,
    /// All events in order.
    pub events: Vec<StreamEvent>,
    /// Final result message.
    pub result: Option<ResultMessage>,
    /// Total usage.
    pub usage: Usage,
    /// Total cost in USD.
    pub cost_usd: Option<f64>,
}

impl CollectedResponse {
    /// Check if the response was successful.
    pub fn is_success(&self) -> bool {
        self.result.as_ref().is_some_and(|r| r.is_success())
    }

    /// Get the final text result from the result message.
    pub fn result_text(&self) -> Option<&str> {
        self.result.as_ref().and_then(|r| r.result.as_deref())
    }
}

/// Builder for content blocks from streaming deltas.
struct ContentBlockBuilder {
    block_type: BlockType,
    text: String,
    tool_id: Option<String>,
    tool_name: Option<String>,
    tool_input_json: String,
    thinking: String,
}

enum BlockType {
    Text,
    ToolUse,
    Thinking,
}

impl ContentBlockBuilder {
    fn new(info: ContentBlockInfo) -> Self {
        match info {
            ContentBlockInfo::Text { text } => Self {
                block_type: BlockType::Text,
                text,
                tool_id: None,
                tool_name: None,
                tool_input_json: String::new(),
                thinking: String::new(),
            },
            ContentBlockInfo::ToolUse { id, name, .. } => Self {
                block_type: BlockType::ToolUse,
                text: String::new(),
                tool_id: Some(id),
                tool_name: Some(name),
                tool_input_json: String::new(),
                thinking: String::new(),
            },
            ContentBlockInfo::Thinking { thinking } => Self {
                block_type: BlockType::Thinking,
                text: String::new(),
                tool_id: None,
                tool_name: None,
                tool_input_json: String::new(),
                thinking,
            },
        }
    }

    fn apply_delta(&mut self, delta: &ContentDelta) {
        match delta {
            ContentDelta::TextDelta { text } => {
                self.text.push_str(text);
            }
            ContentDelta::InputJsonDelta { partial_json } => {
                self.tool_input_json.push_str(partial_json);
            }
            ContentDelta::ThinkingDelta { thinking } => {
                self.thinking.push_str(thinking);
            }
        }
    }

    fn build(self) -> Option<ContentBlock> {
        match self.block_type {
            BlockType::Text => Some(ContentBlock::Text(TextBlock { text: self.text })),
            BlockType::ToolUse => {
                let id = self.tool_id?;
                let name = self.tool_name?;
                let input = serde_json::from_str(&self.tool_input_json).unwrap_or_default();
                Some(ContentBlock::ToolUse(ToolUseBlock { id, name, input }))
            }
            BlockType::Thinking => Some(ContentBlock::Thinking(ThinkingBlock {
                thinking: self.thinking,
            })),
        }
    }
}

/// Run a future with a timeout.
///
/// Returns an error if the future doesn't complete within the specified duration.
pub async fn with_timeout<F, T>(duration: Duration, future: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    match tokio_timeout(duration, future).await {
        Ok(result) => result,
        Err(_) => Err(Error::Timeout(duration)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_stream_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<ResponseStream>();
    }

    #[test]
    fn collected_response_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CollectedResponse>();
    }

    #[test]
    fn content_block_builder_text() {
        let mut builder = ContentBlockBuilder::new(ContentBlockInfo::Text {
            text: "Hello".to_string(),
        });
        builder.apply_delta(&ContentDelta::TextDelta {
            text: " World".to_string(),
        });
        let block = builder.build().unwrap();
        assert!(matches!(block, ContentBlock::Text(t) if t.text == "Hello World"));
    }

    #[test]
    fn content_block_builder_tool_use() {
        let mut builder = ContentBlockBuilder::new(ContentBlockInfo::ToolUse {
            id: "tool_123".to_string(),
            name: "Read".to_string(),
            input: serde_json::json!({}),
        });
        builder.apply_delta(&ContentDelta::InputJsonDelta {
            partial_json: r#"{"path": "#.to_string(),
        });
        builder.apply_delta(&ContentDelta::InputJsonDelta {
            partial_json: r#""/tmp/test"}"#.to_string(),
        });
        let block = builder.build().unwrap();
        if let ContentBlock::ToolUse(t) = block {
            assert_eq!(t.id, "tool_123");
            assert_eq!(t.name, "Read");
            assert_eq!(t.input["path"], "/tmp/test");
        } else {
            panic!("Expected ToolUse block");
        }
    }

    #[test]
    fn collected_response_default() {
        let response = CollectedResponse::default();
        assert!(response.session_id.is_none());
        assert!(response.text.is_empty());
        assert!(!response.is_success());
    }

    #[test]
    fn content_block_builder_thinking() {
        let mut builder = ContentBlockBuilder::new(ContentBlockInfo::Thinking {
            thinking: "Let me ".to_string(),
        });
        builder.apply_delta(&ContentDelta::ThinkingDelta {
            thinking: "think about this...".to_string(),
        });
        let block = builder.build().unwrap();
        if let ContentBlock::Thinking(t) = block {
            assert_eq!(t.thinking, "Let me think about this...");
        } else {
            panic!("Expected Thinking block");
        }
    }

    #[test]
    fn content_block_builder_empty_text() {
        let builder = ContentBlockBuilder::new(ContentBlockInfo::Text {
            text: String::new(),
        });
        let block = builder.build().unwrap();
        if let ContentBlock::Text(t) = block {
            assert!(t.text.is_empty());
        } else {
            panic!("Expected Text block");
        }
    }

    #[test]
    fn content_block_builder_tool_use_invalid_json() {
        let mut builder = ContentBlockBuilder::new(ContentBlockInfo::ToolUse {
            id: "tool_123".to_string(),
            name: "Read".to_string(),
            input: serde_json::json!({}),
        });
        builder.apply_delta(&ContentDelta::InputJsonDelta {
            partial_json: "not valid json".to_string(),
        });
        let block = builder.build().unwrap();
        // Invalid JSON defaults to null (from unwrap_or_default on serde_json::Value)
        if let ContentBlock::ToolUse(t) = block {
            assert_eq!(t.input, serde_json::Value::Null);
        } else {
            panic!("Expected ToolUse block");
        }
    }

    #[test]
    fn session_info_from_system_with_all_fields() {
        let system = SystemMessage {
            subtype: "init".to_string(),
            cwd: Some("/home/user".to_string()),
            session_id: Some("session-123".to_string()),
            tools: vec!["Read".to_string(), "Write".to_string()],
            model: Some("claude-opus".to_string()),
            permission_mode: Some("default".to_string()),
            claude_code_version: Some("2.0.76".to_string()),
        };
        let info = ResponseStream::session_info_from_system(&system);
        assert_eq!(info.session_id.as_str(), "session-123");
        assert_eq!(info.cwd, Some("/home/user".to_string()));
        assert_eq!(info.tools, vec!["Read", "Write"]);
        assert_eq!(info.model, Some("claude-opus".to_string()));
        assert_eq!(info.permission_mode, Some("default".to_string()));
        assert_eq!(info.claude_code_version, Some("2.0.76".to_string()));
    }

    #[test]
    fn session_info_from_system_with_missing_session_id() {
        let system = SystemMessage {
            subtype: "init".to_string(),
            cwd: None,
            session_id: None,
            tools: vec![],
            model: None,
            permission_mode: None,
            claude_code_version: None,
        };
        let info = ResponseStream::session_info_from_system(&system);
        assert_eq!(info.session_id.as_str(), "unknown");
    }

    #[test]
    fn transform_message_system_init() {
        let system = SystemMessage {
            subtype: "init".to_string(),
            cwd: Some("/home".to_string()),
            session_id: Some("test-id".to_string()),
            tools: vec!["Bash".to_string()],
            model: Some("claude".to_string()),
            permission_mode: None,
            claude_code_version: None,
        };
        let msg = CliMessage::System(system);
        let mut blocks = HashMap::new();
        let events = ResponseStream::transform_message(msg, &mut blocks).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], StreamEvent::SessionInit(_)));
    }

    #[test]
    fn transform_message_system_non_init() {
        let system = SystemMessage {
            subtype: "status".to_string(),
            cwd: None,
            session_id: None,
            tools: vec![],
            model: None,
            permission_mode: None,
            claude_code_version: None,
        };
        let msg = CliMessage::System(system);
        let mut blocks = HashMap::new();
        let events = ResponseStream::transform_message(msg, &mut blocks).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn transform_message_result() {
        let result = ResultMessage {
            subtype: "success".to_string(),
            is_error: false,
            duration_ms: Some(1000),
            num_turns: Some(1),
            result: Some("Done".to_string()),
            total_cost_usd: Some(0.01),
            usage: None,
            session_id: None,
        };
        let msg = CliMessage::Result(result);
        let mut blocks = HashMap::new();
        let events = ResponseStream::transform_message(msg, &mut blocks).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], StreamEvent::Complete(_)));
    }

    #[test]
    fn transform_message_stream_event_text_delta() {
        use crate::protocol::StreamEventMessage;
        let event = StreamEventMessage {
            event: StreamEventType::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::TextDelta {
                    text: "Hello".to_string(),
                },
            },
            session_id: None,
        };
        let msg = CliMessage::StreamEvent(event);
        let mut blocks = HashMap::new();
        let events = ResponseStream::transform_message(msg, &mut blocks).unwrap();
        assert_eq!(events.len(), 1);
        if let StreamEvent::TextDelta { index, text } = &events[0] {
            assert_eq!(*index, 0);
            assert_eq!(text, "Hello");
        } else {
            panic!("Expected TextDelta");
        }
    }

    #[test]
    fn transform_message_stream_event_content_block_lifecycle() {
        use crate::protocol::StreamEventMessage;
        let mut blocks = HashMap::new();

        // Start a text block
        let start_event = StreamEventMessage {
            event: StreamEventType::ContentBlockStart {
                index: 0,
                content_block: ContentBlockInfo::Text {
                    text: "Hi".to_string(),
                },
            },
            session_id: None,
        };
        let _ = ResponseStream::transform_message(CliMessage::StreamEvent(start_event), &mut blocks).unwrap();
        assert!(blocks.contains_key(&0));

        // Add delta
        let delta_event = StreamEventMessage {
            event: StreamEventType::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::TextDelta {
                    text: " there".to_string(),
                },
            },
            session_id: None,
        };
        let _ = ResponseStream::transform_message(CliMessage::StreamEvent(delta_event), &mut blocks).unwrap();

        // Stop the block
        let stop_event = StreamEventMessage {
            event: StreamEventType::ContentBlockStop { index: 0 },
            session_id: None,
        };
        let events = ResponseStream::transform_message(CliMessage::StreamEvent(stop_event), &mut blocks).unwrap();

        assert!(!blocks.contains_key(&0));
        assert_eq!(events.len(), 1);
        if let StreamEvent::ContentBlockComplete { index, block } = &events[0] {
            assert_eq!(*index, 0);
            if let ContentBlock::Text(t) = block {
                assert_eq!(t.text, "Hi there");
            } else {
                panic!("Expected Text block");
            }
        } else {
            panic!("Expected ContentBlockComplete");
        }
    }

    #[test]
    fn transform_message_stream_event_message_delta_with_usage() {
        use crate::protocol::{MessageDeltaInfo, StreamEventMessage};
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        let event = StreamEventMessage {
            event: StreamEventType::MessageDelta {
                delta: MessageDeltaInfo {
                    stop_reason: None,
                    stop_sequence: None,
                },
                usage: Some(usage.clone()),
            },
            session_id: None,
        };
        let msg = CliMessage::StreamEvent(event);
        let mut blocks = HashMap::new();
        let events = ResponseStream::transform_message(msg, &mut blocks).unwrap();
        assert_eq!(events.len(), 1);
        if let StreamEvent::UsageUpdate(u) = &events[0] {
            assert_eq!(u.input_tokens, 100);
            assert_eq!(u.output_tokens, 50);
        } else {
            panic!("Expected UsageUpdate");
        }
    }

    #[test]
    fn transform_message_stream_event_error() {
        use crate::protocol::{StreamError, StreamEventMessage};
        let event = StreamEventMessage {
            event: StreamEventType::Error {
                error: StreamError {
                    error_type: "auth_error".to_string(),
                    message: "Invalid API key".to_string(),
                },
            },
            session_id: None,
        };
        let msg = CliMessage::StreamEvent(event);
        let mut blocks = HashMap::new();
        let result = ResponseStream::transform_message(msg, &mut blocks);
        assert!(result.is_err());
        if let Err(Error::CliError { message, is_auth_error }) = result {
            assert_eq!(message, "Invalid API key");
            assert!(is_auth_error);
        }
    }

    #[test]
    fn transform_message_stream_event_ping() {
        use crate::protocol::StreamEventMessage;
        let event = StreamEventMessage {
            event: StreamEventType::Ping,
            session_id: None,
        };
        let msg = CliMessage::StreamEvent(event);
        let mut blocks = HashMap::new();
        let events = ResponseStream::transform_message(msg, &mut blocks).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn collected_response_is_success() {
        let mut response = CollectedResponse::default();
        assert!(!response.is_success());

        response.result = Some(ResultMessage {
            subtype: "success".to_string(),
            is_error: false,
            duration_ms: None,
            num_turns: None,
            result: Some("done".to_string()),
            total_cost_usd: None,
            usage: None,
            session_id: None,
        });
        assert!(response.is_success());
    }

    #[test]
    fn collected_response_result_text() {
        let mut response = CollectedResponse::default();
        assert!(response.result_text().is_none());

        response.result = Some(ResultMessage {
            subtype: "success".to_string(),
            is_error: false,
            duration_ms: None,
            num_turns: None,
            result: Some("the result".to_string()),
            total_cost_usd: None,
            usage: None,
            session_id: None,
        });
        assert_eq!(response.result_text(), Some("the result"));
    }

    #[tokio::test]
    async fn with_timeout_success() {
        let result = with_timeout(Duration::from_secs(1), async { Ok::<_, Error>(42) }).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn with_timeout_expires() {
        let result = with_timeout(Duration::from_millis(1), async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok::<_, Error>(42)
        })
        .await;
        assert!(matches!(result, Err(Error::Timeout(_))));
    }

    #[tokio::test]
    async fn with_timeout_inner_error() {
        let result = with_timeout(Duration::from_secs(1), async {
            Err::<i32, _>(Error::Cancelled)
        })
        .await;
        assert!(matches!(result, Err(Error::Cancelled)));
    }
}
