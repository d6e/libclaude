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

mod error;

pub use error::{Error, Result};
