use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to spawn claude process: {0}")]
    SpawnError(#[source] std::io::Error),

    #[error("Process I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    SerializeError(#[source] serde_json::Error),

    #[error("JSON deserialization error: {message}")]
    DeserializeError {
        message: String,
        raw: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("Claude CLI not found in PATH")]
    CliNotFound,

    #[error("Claude CLI exited with code {code}: {stderr}")]
    ProcessFailed { code: i32, stderr: String },

    #[error("Session not found: {session_id}")]
    SessionNotFound { session_id: String },

    #[error("Stream closed unexpectedly")]
    StreamClosed,

    #[error("Tool execution error: {0}")]
    ToolError(String),

    #[error("Permission denied for tool {tool_name}: {reason}")]
    PermissionDenied { tool_name: String, reason: String },

    #[error("Budget exceeded: ${spent:.4} of ${budget:.4}")]
    BudgetExceeded { spent: f64, budget: f64 },

    #[error("Invalid configuration: {0}")]
    ConfigError(String),

    #[error("Timeout after {0:?}")]
    Timeout(std::time::Duration),

    #[error("No authentication configured: no OAuth credentials, CLAUDE_CODE_OAUTH_TOKEN, or ANTHROPIC_API_KEY found")]
    NoAuthConfigured,

    #[error("OAuth credentials not found in ~/.claude/ (run `claude login` first)")]
    OAuthNotConfigured,

    #[error("CLAUDE_CODE_OAUTH_TOKEN environment variable not set")]
    OAuthTokenNotFound,

    #[error("ANTHROPIC_API_KEY environment variable not set")]
    ApiKeyNotFound,

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("Request cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, Error>;
