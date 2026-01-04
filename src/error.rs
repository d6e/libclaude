use std::time::Duration;

/// Errors that can occur when using libclaude.
///
/// Errors are organized by category:
/// - Configuration errors: detected at `build()` time
/// - Spawn errors: failed to start CLI process
/// - IO errors: communication failures with subprocess
/// - Protocol errors: unexpected or malformed CLI output
/// - Runtime errors: failures during execution
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    // -------------------------------------------------------------------------
    // Configuration errors (detected at build() time)
    // -------------------------------------------------------------------------
    /// No authentication method configured or resolvable.
    ///
    /// This occurs when using `AuthMethod::Auto` and none of the following
    /// are available:
    /// - OAuth credentials in `~/.claude/.credentials.json`
    /// - `CLAUDE_CODE_OAUTH_TOKEN` environment variable
    /// - `ANTHROPIC_API_KEY` environment variable
    #[error("no authentication configured: no OAuth credentials, CLAUDE_CODE_OAUTH_TOKEN, or ANTHROPIC_API_KEY found")]
    AuthNotConfigured,

    /// OAuth credentials file not found.
    ///
    /// Run `claude login` to authenticate via browser.
    #[error("OAuth credentials not found at {path} (run `claude login` first)")]
    OAuthCredentialsNotFound { path: String },

    /// Environment variable required for authentication is not set.
    #[error("environment variable {var} not set")]
    EnvVarNotFound { var: &'static str },

    /// Invalid configuration provided to builder.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    // -------------------------------------------------------------------------
    // Spawn errors
    // -------------------------------------------------------------------------
    /// Claude CLI binary not found in PATH.
    #[error("claude CLI not found (searched: {searched})")]
    CliNotFound { searched: String },

    /// Failed to spawn the claude subprocess.
    #[error("failed to spawn claude process: {0}")]
    ProcessSpawn(#[source] std::io::Error),

    // -------------------------------------------------------------------------
    // IO errors
    // -------------------------------------------------------------------------
    /// IO error communicating with the claude subprocess.
    #[error("IO error: {0}")]
    Io(#[source] std::io::Error),

    // -------------------------------------------------------------------------
    // Protocol errors
    // -------------------------------------------------------------------------
    /// Failed to parse JSON from CLI output.
    #[error("failed to parse JSON: {message}")]
    JsonParse {
        message: String,
        #[source]
        source: serde_json::Error,
    },

    /// Received an unexpected message type from the CLI.
    #[error("unexpected message type: {message_type}")]
    UnexpectedMessage { message_type: String },

    /// Stream closed before receiving expected data.
    #[error("stream closed unexpectedly")]
    StreamClosed,

    // -------------------------------------------------------------------------
    // Runtime errors
    // -------------------------------------------------------------------------
    /// Request exceeded the configured timeout.
    #[error("request timed out after {0:?}")]
    Timeout(Duration),

    /// CLI process exited with an error.
    #[error("CLI error: {message}")]
    CliError {
        message: String,
        /// Whether this error is authentication-related.
        is_auth_error: bool,
    },

    /// Budget limit exceeded.
    #[error("budget exceeded: ${spent:.4} of ${budget:.4} USD")]
    BudgetExceeded { spent: f64, budget: f64 },

    /// Request was cancelled by dropping the stream.
    #[error("request cancelled")]
    Cancelled,
}

/// A specialized Result type for libclaude operations.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Create a JSON parse error with context.
    pub fn json_parse(source: serde_json::Error, raw: &str) -> Self {
        Self::JsonParse {
            message: format!(
                "at position {}: {}",
                source.column(),
                raw.chars().take(100).collect::<String>()
            ),
            source,
        }
    }

    /// Check if this error is related to authentication.
    pub fn is_auth_error(&self) -> bool {
        matches!(
            self,
            Error::AuthNotConfigured
                | Error::OAuthCredentialsNotFound { .. }
                | Error::EnvVarNotFound { .. }
                | Error::CliError {
                    is_auth_error: true,
                    ..
                }
        )
    }

    /// Check if this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Error::Timeout(_) | Error::Io(_) | Error::StreamClosed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    }

    #[test]
    fn is_auth_error_detection() {
        assert!(Error::AuthNotConfigured.is_auth_error());
        assert!(Error::OAuthCredentialsNotFound {
            path: "~/.claude".into()
        }
        .is_auth_error());
        assert!(Error::EnvVarNotFound {
            var: "ANTHROPIC_API_KEY"
        }
        .is_auth_error());
        assert!(Error::CliError {
            message: "unauthorized".into(),
            is_auth_error: true
        }
        .is_auth_error());
        assert!(!Error::CliError {
            message: "other".into(),
            is_auth_error: false
        }
        .is_auth_error());
        assert!(!Error::Timeout(Duration::from_secs(30)).is_auth_error());
    }

    #[test]
    fn is_retryable_detection() {
        assert!(Error::Timeout(Duration::from_secs(30)).is_retryable());
        assert!(Error::StreamClosed.is_retryable());
        assert!(!Error::AuthNotConfigured.is_retryable());
        assert!(!Error::Cancelled.is_retryable());
    }
}
