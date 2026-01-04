//! Configuration and authentication for the Claude CLI client.
//!
//! This module provides:
//!
//! - [`ClientConfig`] and [`ClientConfigBuilder`] for configuring the client
//! - [`AuthMethod`] for specifying authentication
//! - Type-safe options like [`Model`], [`PermissionMode`], and [`SessionId`]
//! - Built-in tool constants in [`tools`]
//!
//! # Example
//!
//! ```ignore
//! use libclaude::config::{ClientConfig, Model, PermissionMode, AuthMethod};
//!
//! let config = ClientConfig::builder()
//!     .api_key("sk-ant-...")
//!     .model(Model::Opus)
//!     .permission_mode(PermissionMode::BypassPermissions)
//!     .max_budget_usd(10.0)
//!     .build()?;
//! ```
//!
//! # Authentication
//!
//! Multiple authentication methods are supported:
//!
//! ```ignore
//! use libclaude::config::ClientConfig;
//!
//! // Auto-detect (default): tries OAuth, then env vars
//! let config = ClientConfig::builder().build()?;
//!
//! // Explicit API key
//! let config = ClientConfig::builder()
//!     .api_key("sk-ant-...")
//!     .build()?;
//!
//! // With fallback
//! let config = ClientConfig::builder()
//!     .oauth()
//!     .fallback(AuthMethod::ApiKeyFromEnv)
//!     .build()?;
//! ```

pub mod auth;
pub mod builder;
pub mod options;

// Re-export commonly used types
pub use auth::{
    has_oauth_credentials, login_interactive, setup_token, AuthMethod, OAuthCredentials,
    ENV_API_KEY, ENV_OAUTH_TOKEN,
};
pub use builder::{ClientConfig, ClientConfigBuilder};
pub use options::{tools, Model, PermissionMode, SessionId};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_exports_accessible() {
        // Verify all public types are accessible
        let _: AuthMethod = AuthMethod::Auto;
        let _: Model = Model::Sonnet;
        let _: PermissionMode = PermissionMode::Default;
        let _: SessionId = SessionId::new("test");

        // Verify tool constants are accessible
        let _: &str = tools::READ;
        let _: &str = tools::BASH;
    }

    #[test]
    fn builder_accessible() {
        // Should be able to create a builder
        let _ = ClientConfig::builder();
    }
}
