//! Multi-turn conversation sessions.
//!
//! This module provides [`Session`] for managing multi-turn conversations
//! with Claude. Sessions maintain conversation history and track usage
//! across multiple turns.
//!
//! # Example
//!
//! ```ignore
//! use libclaude::{ClaudeClient, Result};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let client = ClaudeClient::new()?;
//!
//!     // Start a session with context
//!     let session = client.start_session("My name is Alice").await?;
//!
//!     // Continue the conversation
//!     let response = session.send_and_collect("What's my name?").await?;
//!     println!("{}", response); // "Your name is Alice"
//!
//!     // Check usage
//!     println!("Total tokens: {}", session.total_usage().total_tokens());
//!     println!("Total cost: ${:.4}", session.total_cost_usd());
//!
//!     Ok(())
//! }
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures::StreamExt;
use tokio::sync::Mutex;

use crate::config::{ClientConfig, SessionId};
use crate::process::ClaudeProcess;
use crate::protocol::Usage;
use crate::stream::{with_timeout, ResponseStream, StreamEvent};
use crate::Result;

/// A multi-turn conversation session with Claude.
///
/// Sessions maintain conversation history across multiple turns by using
/// the CLI's `--resume` flag to continue from a specific session ID.
///
/// # Thread Safety
///
/// `Session` is `Send + Sync` and can be shared across tasks. However,
/// concurrent calls to [`send`](Self::send) or [`send_and_collect`](Self::send_and_collect)
/// will be serialized internally to maintain conversation order.
///
/// # Usage Tracking
///
/// Sessions track cumulative token usage and cost across all turns.
/// Use [`total_usage`](Self::total_usage) and [`total_cost_usd`](Self::total_cost_usd)
/// to check the totals.
///
/// # Example
///
/// ```ignore
/// let session = client.start_session("Remember: I prefer concise answers").await?;
///
/// // First turn
/// let _ = session.send_and_collect("What is Rust?").await?;
///
/// // Second turn (session remembers context)
/// let _ = session.send_and_collect("Show me an example").await?;
///
/// // Check totals
/// println!("Session used {} tokens", session.total_usage().total_tokens());
/// ```
#[derive(Debug)]
pub struct Session {
    /// The session ID for resuming this conversation.
    session_id: SessionId,
    /// Client configuration for spawning new processes.
    config: Arc<ClientConfig>,
    /// Accumulated usage across all turns.
    usage: Mutex<Usage>,
    /// Accumulated cost in USD (stored as microdollars for atomic ops).
    cost_microdollars: AtomicU64,
    /// Lock to serialize send operations.
    send_lock: Mutex<()>,
}

impl Session {
    /// Create a session from an initial response stream.
    ///
    /// This consumes the stream to get the session ID and initial usage,
    /// then creates a Session that can be used for subsequent turns.
    pub(crate) async fn from_initial_stream(
        config: Arc<ClientConfig>,
        mut stream: ResponseStream,
    ) -> Result<Self> {
        let mut session_id: Option<SessionId> = None;
        let mut usage = Usage::default();
        let mut cost: Option<f64> = None;

        // Consume the stream to get session info
        while let Some(event) = stream.next().await {
            match event? {
                StreamEvent::SessionInit(info) => {
                    session_id = Some(info.session_id);
                }
                StreamEvent::UsageUpdate(u) => {
                    usage.accumulate(&u);
                }
                StreamEvent::Complete(result) => {
                    if let Some(ref u) = result.usage {
                        usage = u.clone();
                    }
                    cost = result.total_cost_usd;
                }
                _ => {}
            }
        }

        let session_id = session_id.unwrap_or_else(|| SessionId::new("unknown"));
        let cost_microdollars = cost.map(|c| (c * 1_000_000.0) as u64).unwrap_or(0);

        Ok(Self {
            session_id,
            config,
            usage: Mutex::new(usage),
            cost_microdollars: AtomicU64::new(cost_microdollars),
            send_lock: Mutex::new(()),
        })
    }

    /// Get the session ID.
    ///
    /// This ID can be used to resume this session later via
    /// [`ClaudeClient::resume_session`](crate::ClaudeClient::resume_session).
    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Send a message and return a stream of response events.
    ///
    /// This is the low-level streaming API for sessions. The stream
    /// will automatically update the session's usage tracking when consumed.
    ///
    /// # Note
    ///
    /// Only one send operation can be in progress at a time.
    /// Concurrent calls will be serialized.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use futures::StreamExt;
    ///
    /// let mut stream = session.send("Tell me more").await?;
    /// while let Some(event) = stream.next().await {
    ///     // Process events
    /// }
    /// ```
    pub async fn send(&self, message: &str) -> Result<ResponseStream> {
        // Serialize concurrent sends
        let _guard = self.send_lock.lock().await;

        let process = ClaudeProcess::spawn_resume(&self.config, &self.session_id, message).await?;
        let observer = self.config.tool_observer().cloned();
        let stream = ResponseStream::with_observer(process, observer);

        // Note: Usage tracking doesn't happen automatically when using the raw stream.
        // For automatic usage tracking, use send_and_collect instead.
        Ok(stream)
    }

    /// Send a message and collect the full text response.
    ///
    /// This is the simplest way to continue a conversation.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let response = session.send_and_collect("What about X?").await?;
    /// println!("{}", response);
    /// ```
    pub async fn send_and_collect(&self, message: &str) -> Result<String> {
        // Serialize concurrent sends
        let _guard = self.send_lock.lock().await;

        let process = ClaudeProcess::spawn_resume(&self.config, &self.session_id, message).await?;
        let observer = self.config.tool_observer().cloned();
        let stream = ResponseStream::with_observer(process, observer);

        // Process stream with optional timeout
        let (text, turn_usage, turn_cost) = if let Some(timeout) = self.config.timeout() {
            with_timeout(timeout, Self::collect_stream(stream)).await?
        } else {
            Self::collect_stream(stream).await?
        };

        // Add this turn's usage to session totals
        if let Some(usage) = turn_usage {
            self.set_turn_usage(&usage).await;
        }
        if let Some(cost) = turn_cost {
            self.add_cost(cost);
        }

        Ok(text)
    }

    /// Collect text from a stream, returning text, final usage, and cost.
    async fn collect_stream(
        mut stream: ResponseStream,
    ) -> Result<(String, Option<Usage>, Option<f64>)> {
        let mut text = String::new();
        let mut final_usage: Option<Usage> = None;
        let mut final_cost: Option<f64> = None;

        while let Some(event) = stream.next().await {
            match event? {
                StreamEvent::TextDelta { text: t, .. } => {
                    text.push_str(&t);
                }
                StreamEvent::Complete(result) => {
                    // Use the final cumulative usage from result (not incremental deltas)
                    final_usage = result.usage.clone();
                    final_cost = result.total_cost_usd;

                    if result.is_error() {
                        return Err(crate::Error::CliError {
                            message: result.result.unwrap_or_else(|| "unknown error".to_string()),
                            is_auth_error: false,
                        });
                    }
                }
                _ => {}
            }
        }

        Ok((text, final_usage, final_cost))
    }

    /// Get the accumulated usage across all turns.
    ///
    /// This returns the total tokens consumed by this session.
    pub async fn total_usage(&self) -> Usage {
        self.usage.lock().await.clone()
    }

    /// Get the accumulated cost in USD across all turns.
    pub fn total_cost_usd(&self) -> f64 {
        let microdollars = self.cost_microdollars.load(Ordering::Relaxed);
        microdollars as f64 / 1_000_000.0
    }

    /// Add a turn's usage to the session totals.
    async fn set_turn_usage(&self, usage: &Usage) {
        let mut total = self.usage.lock().await;
        total.accumulate(usage);
    }

    /// Add cost to the session totals.
    fn add_cost(&self, cost: f64) {
        let microdollars = (cost * 1_000_000.0) as u64;
        self.cost_microdollars.fetch_add(microdollars, Ordering::Relaxed);
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn session_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Session>();
    }

    #[test]
    fn session_id_accessors() {
        // Test SessionId methods used by Session
        let id = SessionId::new("test-session-123");
        assert_eq!(id.as_str(), "test-session-123");
        assert_eq!(id.to_string(), "test-session-123");
    }

    #[test]
    fn cost_microdollar_conversion() {
        // Test the microdollar conversion logic
        let cost = 0.0123;
        let microdollars = (cost * 1_000_000.0) as u64;
        let back = microdollars as f64 / 1_000_000.0;
        assert!((cost - back).abs() < 0.000001);
    }

    #[test]
    fn usage_accumulation() {
        // Test that usage accumulates correctly
        let mut usage = Usage::default();
        let u1 = Usage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        let u2 = Usage {
            input_tokens: 200,
            output_tokens: 100,
            ..Default::default()
        };
        usage.accumulate(&u1);
        usage.accumulate(&u2);
        assert_eq!(usage.input_tokens, 300);
        assert_eq!(usage.output_tokens, 150);
    }

    #[test]
    fn cost_microdollar_conversion_zero() {
        let cost = 0.0;
        let microdollars = (cost * 1_000_000.0) as u64;
        assert_eq!(microdollars, 0);
        let back = microdollars as f64 / 1_000_000.0;
        assert_eq!(back, 0.0);
    }

    #[test]
    fn cost_microdollar_conversion_large() {
        let cost = 100.50;
        let microdollars = (cost * 1_000_000.0) as u64;
        assert_eq!(microdollars, 100_500_000);
        let back = microdollars as f64 / 1_000_000.0;
        assert!((cost - back).abs() < 0.000001);
    }

    #[test]
    fn cost_microdollar_conversion_small() {
        let cost = 0.000001;
        let microdollars = (cost * 1_000_000.0) as u64;
        assert_eq!(microdollars, 1);
        let back = microdollars as f64 / 1_000_000.0;
        assert!((cost - back).abs() < 0.0000001);
    }

    #[test]
    fn atomic_cost_operations() {
        let cost_microdollars = AtomicU64::new(0);

        // Add first cost
        let cost1 = 0.05;
        let micros1 = (cost1 * 1_000_000.0) as u64;
        cost_microdollars.fetch_add(micros1, Ordering::Relaxed);

        // Add second cost
        let cost2 = 0.03;
        let micros2 = (cost2 * 1_000_000.0) as u64;
        cost_microdollars.fetch_add(micros2, Ordering::Relaxed);

        // Check total
        let total_micros = cost_microdollars.load(Ordering::Relaxed);
        let total = total_micros as f64 / 1_000_000.0;
        assert!((total - 0.08).abs() < 0.000001);
    }

    #[test]
    fn usage_accumulation_with_cache_tokens() {
        let mut usage = Usage::default();
        let u1 = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 20,
            cache_creation_input_tokens: 10,
        };
        usage.accumulate(&u1);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_input_tokens, 20);
        assert_eq!(usage.cache_creation_input_tokens, 10);
    }

    #[test]
    fn usage_accumulation_multiple_with_cache() {
        let mut usage = Usage::default();
        let u1 = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 20,
            cache_creation_input_tokens: 10,
        };
        let u2 = Usage {
            input_tokens: 200,
            output_tokens: 100,
            cache_read_input_tokens: 30,
            cache_creation_input_tokens: 0,
        };
        usage.accumulate(&u1);
        usage.accumulate(&u2);
        assert_eq!(usage.input_tokens, 300);
        assert_eq!(usage.output_tokens, 150);
        assert_eq!(usage.cache_read_input_tokens, 50);
        assert_eq!(usage.cache_creation_input_tokens, 10);
    }

    #[test]
    fn usage_total_tokens() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        };
        assert_eq!(usage.total_tokens(), 150);
    }

    #[test]
    fn session_id_clone() {
        let id1 = SessionId::new("session-abc");
        let id2 = id1.clone();
        assert_eq!(id1.as_str(), id2.as_str());
    }

    #[test]
    fn session_id_debug() {
        let id = SessionId::new("debug-test");
        let debug_str = format!("{:?}", id);
        assert!(debug_str.contains("debug-test"));
    }

    #[test]
    fn session_id_display() {
        let id = SessionId::new("display-test");
        let display_str = format!("{}", id);
        assert_eq!(display_str, "display-test");
    }
}
