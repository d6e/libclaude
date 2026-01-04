//! Process management for the Claude CLI.
//!
//! This module handles spawning and communicating with the Claude CLI subprocess.
//! Each API call spawns a new CLI process, with multi-turn sessions using
//! `--continue` or `--resume` flags to load conversation history.
//!
//! # Architecture
//!
//! ```text
//! libclaude                          claude CLI
//! ┌─────────────┐                   ┌─────────────┐
//! │ ClaudeProcess│───stdin (prompt)─▶│             │
//! │             │◀──stdout (JSON)───│             │
//! │             │◀──stderr (logs)───│             │
//! └─────────────┘                   └─────────────┘
//! ```
//!
//! # Input Protocol
//!
//! Prompts are written to the subprocess stdin, then stdin is closed to signal EOF.
//!
//! # Output Protocol
//!
//! The CLI outputs newline-delimited JSON to stdout. Each line is a complete
//! JSON message that can be parsed as [`CliMessage`](crate::protocol::CliMessage).

mod io;
mod spawn;

pub use io::{ProcessReader, ProcessWriter};
pub use spawn::ClaudeProcess;

/// Minimum CLI version required for full compatibility.
pub const MIN_CLI_VERSION: &str = "2.0.0";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ClaudeProcess>();
        assert_send_sync::<ProcessReader>();
    }
}
