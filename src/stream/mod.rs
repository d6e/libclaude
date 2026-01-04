//! Streaming response handling.
//!
//! This module provides types for consuming Claude responses as async streams.
//!
//! # Overview
//!
//! The streaming system transforms raw CLI messages into high-level [`StreamEvent`]s
//! that are convenient for consumers. The main types are:
//!
//! - [`StreamEvent`] - High-level events like text deltas, tool calls, and completion
//! - [`ResponseStream`] - An async stream of events from a Claude response
//! - [`CollectedResponse`] - A convenience type for collecting all response data
//!
//! # Example
//!
//! ```ignore
//! use futures::StreamExt;
//! use libclaude::stream::{StreamEvent, ResponseStream};
//!
//! let mut stream = client.send("Hello, Claude!").await?;
//!
//! while let Some(event) = stream.next().await {
//!     match event? {
//!         StreamEvent::SessionInit(info) => {
//!             println!("Session: {}", info.session_id);
//!         }
//!         StreamEvent::TextDelta { text, .. } => {
//!             print!("{}", text);
//!         }
//!         StreamEvent::Complete(result) => {
//!             println!("\nCost: ${:.4}", result.total_cost_usd.unwrap_or(0.0));
//!         }
//!         _ => {}
//!     }
//! }
//! ```
//!
//! # Cancellation
//!
//! Dropping a [`ResponseStream`] will:
//! 1. Cancel the background reader task
//! 2. Kill the CLI subprocess
//!
//! This ensures clean resource cleanup even when not consuming the full stream.

pub mod events;
pub mod response;

pub use events::{SessionInfo, StreamEvent};
pub use response::{with_timeout, CollectedResponse, ResponseStream};
