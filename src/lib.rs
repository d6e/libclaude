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
//! ## Configuration
//!
//! ```ignore
//! use libclaude::config::{ClientConfig, Model, PermissionMode};
//!
//! let config = ClientConfig::builder()
//!     .api_key("sk-ant-...")
//!     .model(Model::Opus)
//!     .permission_mode(PermissionMode::BypassPermissions)
//!     .max_budget_usd(5.00)
//!     .build()?;
//! ```

pub mod config;
mod error;
pub mod process;
pub mod protocol;
pub mod stream;

pub use error::{Error, Result};

// Re-export commonly used config types at crate root
pub use config::{
    has_oauth_credentials, login_interactive, setup_token, AuthMethod, ClientConfig,
    ClientConfigBuilder, Model, OAuthCredentials, PermissionMode, SessionId,
};

// Re-export commonly used protocol types at crate root
pub use protocol::{CliMessage, ContentBlock, StreamEventType, Usage};

// Re-export commonly used process types at crate root
pub use process::{ClaudeProcess, ProcessReader};

// Re-export commonly used stream types at crate root
pub use stream::{CollectedResponse, ResponseStream, SessionInfo, StreamEvent};
