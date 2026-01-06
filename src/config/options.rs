//! Type-safe configuration options for the Claude CLI.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Model selection with escape hatch for new models.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    /// Claude Sonnet (balanced performance and cost).
    #[default]
    Sonnet,
    /// Claude Opus (highest capability).
    Opus,
    /// Claude Haiku (fastest, lowest cost).
    Haiku,
    /// Custom model identifier for new or specialized models.
    #[serde(untagged)]
    Custom(String),
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Model::Sonnet => write!(f, "sonnet"),
            Model::Opus => write!(f, "opus"),
            Model::Haiku => write!(f, "haiku"),
            Model::Custom(s) => write!(f, "{}", s),
        }
    }
}

impl From<&str> for Model {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "sonnet" => Model::Sonnet,
            "opus" => Model::Opus,
            "haiku" => Model::Haiku,
            _ => Model::Custom(s.to_string()),
        }
    }
}

impl From<String> for Model {
    fn from(s: String) -> Self {
        Model::from(s.as_str())
    }
}

/// Permission modes for CLI tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    /// Default mode: ask for permission for potentially dangerous operations.
    #[default]
    Default,
    /// Plan mode: read-only, no tool execution allowed.
    Plan,
    /// Accept edits mode: auto-approve file edits, ask for other tools.
    AcceptEdits,
    /// Bypass permissions: auto-approve all tool calls (use with caution).
    BypassPermissions,
}

impl fmt::Display for PermissionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PermissionMode::Default => write!(f, "default"),
            PermissionMode::Plan => write!(f, "plan"),
            PermissionMode::AcceptEdits => write!(f, "acceptEdits"),
            PermissionMode::BypassPermissions => write!(f, "bypassPermissions"),
        }
    }
}

/// Newtype for session IDs to prevent string mixups.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub String);

impl SessionId {
    /// Create a new SessionId from a string.
    pub fn new(id: impl Into<String>) -> Self {
        SessionId(id.into())
    }

    /// Get the session ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SessionId {
    fn from(s: String) -> Self {
        SessionId(s)
    }
}

impl From<&str> for SessionId {
    fn from(s: &str) -> Self {
        SessionId(s.to_string())
    }
}

impl AsRef<str> for SessionId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Built-in tool name constants.
///
/// Tools are extensible via MCP servers, so this is not an enum.
/// These constants cover the built-in CLI tools.
pub mod tools {
    /// Read file contents.
    pub const READ: &str = "Read";
    /// Write file contents.
    pub const WRITE: &str = "Write";
    /// Edit file with search/replace.
    pub const EDIT: &str = "Edit";
    /// Execute bash commands.
    pub const BASH: &str = "Bash";
    /// Find files by glob pattern.
    pub const GLOB: &str = "Glob";
    /// Search file contents with regex.
    pub const GREP: &str = "Grep";
    /// List directory contents.
    pub const LS: &str = "LS";
    /// Spawn sub-agents for parallel work.
    pub const TASK: &str = "Task";
    /// Fetch web content.
    pub const WEB_FETCH: &str = "WebFetch";
    /// Search the web.
    pub const WEB_SEARCH: &str = "WebSearch";
    /// Manage todo items.
    pub const TODO_READ: &str = "TodoRead";
    /// Write todo items.
    pub const TODO_WRITE: &str = "TodoWrite";
    /// Edit Jupyter notebooks.
    pub const NOTEBOOK_EDIT: &str = "NotebookEdit";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_display() {
        assert_eq!(Model::Sonnet.to_string(), "sonnet");
        assert_eq!(Model::Opus.to_string(), "opus");
        assert_eq!(Model::Haiku.to_string(), "haiku");
        assert_eq!(
            Model::Custom("claude-3-5-sonnet".into()).to_string(),
            "claude-3-5-sonnet"
        );
    }

    #[test]
    fn model_from_str() {
        assert_eq!(Model::from("sonnet"), Model::Sonnet);
        assert_eq!(Model::from("OPUS"), Model::Opus);
        assert_eq!(Model::from("Haiku"), Model::Haiku);
        assert_eq!(
            Model::from("custom-model"),
            Model::Custom("custom-model".into())
        );
    }

    #[test]
    fn model_serde_roundtrip() {
        let models = [
            Model::Sonnet,
            Model::Opus,
            Model::Haiku,
            Model::Custom("test".into()),
        ];
        for model in models {
            let json = serde_json::to_string(&model).unwrap();
            let parsed: Model = serde_json::from_str(&json).unwrap();
            assert_eq!(model, parsed);
        }
    }

    #[test]
    fn permission_mode_display() {
        assert_eq!(PermissionMode::Default.to_string(), "default");
        assert_eq!(PermissionMode::Plan.to_string(), "plan");
        assert_eq!(PermissionMode::AcceptEdits.to_string(), "acceptEdits");
        assert_eq!(
            PermissionMode::BypassPermissions.to_string(),
            "bypassPermissions"
        );
    }

    #[test]
    fn permission_mode_default() {
        assert_eq!(PermissionMode::default(), PermissionMode::Default);
    }

    #[test]
    fn session_id_usage() {
        let id = SessionId::new("test-session-123");
        assert_eq!(id.as_str(), "test-session-123");
        assert_eq!(id.to_string(), "test-session-123");

        let id2: SessionId = "other-session".into();
        assert_eq!(id2.as_ref(), "other-session");
    }

    #[test]
    fn session_id_serde() {
        let id = SessionId::new("test-123");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"test-123\"");

        let parsed: SessionId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn types_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Model>();
        assert_send_sync::<PermissionMode>();
        assert_send_sync::<SessionId>();
    }
}
