//! Tool observation for monitoring tool execution.
//!
//! This module provides the [`ToolObserver`] trait for observing tool calls
//! during Claude's execution. The CLI executes tools automatically; this
//! is for observation and logging only.
//!
//! # Example
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
//!         println!("Tool called: {} with {:?}", name, input);
//!     }
//!
//!     fn on_tool_result(&self, tool_use_id: &str, content: &str, is_error: bool) {
//!         println!("Tool {} result (error={}): {}", tool_use_id, is_error, content);
//!     }
//! }
//!
//! let client = ClaudeClient::builder()
//!     .api_key("sk-ant-...")
//!     .tool_observer(Arc::new(MyObserver))
//!     .build()?;
//! ```

mod observer;

pub use observer::{LogLevel, LoggingObserver, ToolObserver};
