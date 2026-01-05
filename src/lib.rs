//! # libclaude
//!
//! Async Rust wrapper for the Claude Code CLI.
//!
//! This library provides a typed interface to Claude Code, supporting:
//! - Streaming responses with async iterators
//! - Multi-turn sessions
//! - Multiple authentication methods
//! - Tool observation callbacks
//!
//! ## Quick Start
//!
//! ```ignore
//! use libclaude::{ClaudeClient, Result};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let client = ClaudeClient::new()?;
//!     let response = client.send_and_collect("What is 2+2?").await?;
//!     println!("{}", response);
//!     Ok(())
//! }
//! ```
//!
//! ## Streaming
//!
//! ```ignore
//! use futures::StreamExt;
//! use libclaude::{ClaudeClient, StreamEvent};
//!
//! let client = ClaudeClient::new()?;
//! let mut stream = client.send("Write a poem").await?;
//! while let Some(event) = stream.next().await {
//!     if let StreamEvent::TextDelta { text, .. } = event? {
//!         print!("{}", text);
//!     }
//! }
//! ```
//!
//! ## Multi-turn Sessions
//!
//! ```ignore
//! let session = client.start_session("My name is Alice").await?;
//! let response = session.send_and_collect("What's my name?").await?;
//! // Claude remembers: "Your name is Alice"
//! ```
//!
//! ## Configuration
//!
//! ```ignore
//! use libclaude::{ClaudeClient, Model, PermissionMode};
//!
//! let client = ClaudeClient::builder()
//!     .api_key("sk-ant-...")
//!     .model(Model::Opus)
//!     .permission_mode(PermissionMode::BypassPermissions)
//!     .max_budget_usd(5.00)
//!     .build()?;
//! ```
//!
//! ## Tool Observation
//!
//! ```ignore
//! use std::sync::Arc;
//! use libclaude::{ClaudeClient, ToolObserver};
//! use serde_json::Value;
//!
//! struct MyObserver;
//!
//! impl ToolObserver for MyObserver {
//!     fn on_tool_use(&self, id: &str, name: &str, input: &Value) {
//!         println!("Tool called: {}", name);
//!     }
//! }
//!
//! let client = ClaudeClient::builder()
//!     .tool_observer(Arc::new(MyObserver))
//!     .build()?;
//! ```

mod client;
mod error;
mod session;

// Public modules for advanced usage
pub mod config;
pub mod process;
pub mod protocol;
pub mod stream;
pub mod tools;

// ============================================================================
// Core types
// ============================================================================

pub use error::{Error, Result};
pub use client::{ClaudeClient, ClientBuilder};
pub use session::Session;

// ============================================================================
// Configuration
// ============================================================================

pub use config::{
    // Authentication
    AuthMethod, OAuthCredentials,
    has_oauth_credentials, login_interactive, setup_token,
    // Builder
    ClientConfig, ClientConfigBuilder,
    // Options
    Model, PermissionMode, SessionId,
};

// Tool name constants are available at libclaude::config::tools::{READ, BASH, ...}

// ============================================================================
// Protocol types
// ============================================================================

pub use protocol::{
    // Top-level message enum
    CliMessage,
    // Message types
    AssistantMessage, ResultMessage, SystemMessage, UserMessage,
    // Content blocks
    ContentBlock, TextBlock, ToolUseBlock, ToolResultBlock, ThinkingBlock,
    // Streaming events
    StreamEventType,
    // Usage tracking
    Usage,
};

// ============================================================================
// Streaming
// ============================================================================

pub use stream::{
    CollectedResponse, ResponseStream, SessionInfo, StreamEvent,
    with_timeout,
};

// ============================================================================
// Process management
// ============================================================================

pub use process::{ClaudeProcess, ProcessReader, MIN_CLI_VERSION};

// ============================================================================
// Tool observation
// ============================================================================

pub use tools::{LogLevel, LoggingObserver, ToolObserver};

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}
    fn assert_send<T: Send>() {}

    /// All major public types must be Send + Sync for use across async tasks.
    #[test]
    fn public_types_are_send_sync() {
        // Main client types
        assert_send_sync::<ClaudeClient>();
        assert_send_sync::<ClientBuilder>();
        assert_send_sync::<Session>();

        // Configuration types
        assert_send_sync::<ClientConfig>();
        assert_send_sync::<ClientConfigBuilder>();
        assert_send_sync::<AuthMethod>();
        assert_send_sync::<Model>();
        assert_send_sync::<PermissionMode>();
        assert_send_sync::<SessionId>();

        // Protocol types
        assert_send_sync::<CliMessage>();
        assert_send_sync::<ContentBlock>();
        assert_send_sync::<Usage>();
        assert_send_sync::<StreamEventType>();

        // Stream types
        assert_send_sync::<CollectedResponse>();
        assert_send_sync::<SessionInfo>();
        assert_send_sync::<StreamEvent>();

        // Process types
        assert_send_sync::<ClaudeProcess>();
        assert_send_sync::<ProcessReader>();

        // Error type
        assert_send_sync::<Error>();

        // Tools types
        assert_send_sync::<LoggingObserver>();
    }

    /// ResponseStream is Send but not Sync (contains mutable state).
    #[test]
    fn response_stream_is_send() {
        assert_send::<ResponseStream>();
    }
}
