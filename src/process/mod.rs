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
//! - Prompts passed via `-p "prompt"` argument (primary method)
//! - For prompts exceeding OS argument length limits (~128KB), fall back to stdin pipe
//!
//! # Output Protocol
//!
//! The CLI outputs newline-delimited JSON to stdout. Each line is a complete
//! JSON message that can be parsed as [`CliMessage`](crate::protocol::CliMessage).

mod io;
mod spawn;

pub use io::{ProcessReader, ProcessWriter};
pub use spawn::ClaudeProcess;

/// Maximum prompt length to pass via command line argument.
/// Prompts longer than this will be piped via stdin.
/// Most systems have ~128KB argument limit; we use 64KB for safety.
pub const MAX_ARG_PROMPT_LEN: usize = 64 * 1024;

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

    #[test]
    fn constants_are_reasonable() {
        assert!(MAX_ARG_PROMPT_LEN > 1024, "max arg prompt len should be at least 1KB");
        assert!(MAX_ARG_PROMPT_LEN <= 128 * 1024, "max arg prompt len should be at most 128KB");
    }
}
