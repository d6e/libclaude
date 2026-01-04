//! Client configuration and builder.
//!
//! This module provides the builder pattern for configuring the Claude CLI client.
//!
//! # Example
//!
//! ```ignore
//! use libclaude::config::{ClientConfig, Model, PermissionMode};
//!
//! let config = ClientConfig::builder()
//!     .model(Model::Opus)
//!     .permission_mode(PermissionMode::BypassPermissions)
//!     .system_prompt("You are a helpful assistant.")
//!     .max_budget_usd(5.00)
//!     .build()?;
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use super::auth::{resolve_auth, AuthMethod, ResolvedAuth};
use super::options::{Model, PermissionMode, SessionId};
use crate::{Error, Result};

/// Configuration for the Claude CLI client.
///
/// Use [`ClientConfig::builder()`] to create a new configuration.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    // Authentication
    pub(crate) resolved_auth: ResolvedAuth,

    // Model and permissions
    pub(crate) model: Option<Model>,
    pub(crate) permission_mode: PermissionMode,

    // System prompts
    pub(crate) system_prompt: Option<String>,
    pub(crate) append_system_prompt: Option<String>,

    // Tools configuration
    pub(crate) tools: Option<Vec<String>>,
    pub(crate) allowed_tools: Option<Vec<String>>,
    pub(crate) disallowed_tools: Option<Vec<String>>,

    // Budget
    pub(crate) max_budget_usd: Option<f64>,

    // MCP and structured output
    pub(crate) mcp_config: Option<PathBuf>,
    pub(crate) json_schema: Option<Value>,

    // Session options
    pub(crate) session_id: Option<SessionId>,
    pub(crate) continue_session: bool,
    pub(crate) include_partial_messages: bool,

    // Process options
    pub(crate) cli_path: Option<PathBuf>,
    pub(crate) working_directory: Option<PathBuf>,
    pub(crate) timeout: Option<Duration>,
    pub(crate) env_vars: HashMap<String, String>,
    pub(crate) inherit_env: bool,
}

impl ClientConfig {
    /// Create a new builder for ClientConfig.
    pub fn builder() -> ClientConfigBuilder {
        ClientConfigBuilder::default()
    }

    /// Get the model if set.
    pub fn model(&self) -> Option<&Model> {
        self.model.as_ref()
    }

    /// Get the permission mode.
    pub fn permission_mode(&self) -> PermissionMode {
        self.permission_mode
    }

    /// Get the timeout if set.
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Get the working directory if set.
    pub fn working_directory(&self) -> Option<&PathBuf> {
        self.working_directory.as_ref()
    }
}

/// Builder for [`ClientConfig`].
///
/// This builder validates the configuration when [`build()`](ClientConfigBuilder::build) is called,
/// ensuring that authentication is properly configured and the CLI can be found.
#[derive(Debug, Clone)]
pub struct ClientConfigBuilder {
    // Authentication
    auth_method: AuthMethod,
    fallbacks: Vec<AuthMethod>,

    // Model and permissions
    model: Option<Model>,
    permission_mode: PermissionMode,

    // System prompts
    system_prompt: Option<String>,
    append_system_prompt: Option<String>,

    // Tools configuration
    tools: Option<Vec<String>>,
    allowed_tools: Option<Vec<String>>,
    disallowed_tools: Option<Vec<String>>,

    // Budget
    max_budget_usd: Option<f64>,

    // MCP and structured output
    mcp_config: Option<PathBuf>,
    json_schema: Option<Value>,

    // Session options
    session_id: Option<SessionId>,
    continue_session: bool,
    include_partial_messages: bool,

    // Process options
    cli_path: Option<PathBuf>,
    working_directory: Option<PathBuf>,
    timeout: Option<Duration>,
    env_vars: HashMap<String, String>,
    inherit_env: bool,
}

impl Default for ClientConfigBuilder {
    fn default() -> Self {
        Self {
            auth_method: AuthMethod::default(),
            fallbacks: Vec::new(),
            model: None,
            permission_mode: PermissionMode::default(),
            system_prompt: None,
            append_system_prompt: None,
            tools: None,
            allowed_tools: None,
            disallowed_tools: None,
            max_budget_usd: None,
            mcp_config: None,
            json_schema: None,
            session_id: None,
            continue_session: false,
            include_partial_messages: false,
            cli_path: None,
            working_directory: None,
            timeout: None,
            env_vars: HashMap::new(),
            inherit_env: true, // Default: inherit parent environment
        }
    }
}

impl ClientConfigBuilder {
    // -------------------------------------------------------------------------
    // Authentication methods
    // -------------------------------------------------------------------------

    /// Use API key directly (passed as ANTHROPIC_API_KEY to subprocess).
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.auth_method = AuthMethod::ApiKey(key.into());
        self
    }

    /// Read API key from ANTHROPIC_API_KEY env var.
    pub fn api_key_from_env(mut self) -> Self {
        self.auth_method = AuthMethod::ApiKeyFromEnv;
        self
    }

    /// Use OAuth token directly (passed as CLAUDE_CODE_OAUTH_TOKEN to subprocess).
    ///
    /// Get this token from `claude setup-token` (valid for 1 year).
    pub fn oauth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_method = AuthMethod::OAuthToken(token.into());
        self
    }

    /// Read OAuth token from CLAUDE_CODE_OAUTH_TOKEN env var.
    pub fn oauth_token_from_env(mut self) -> Self {
        self.auth_method = AuthMethod::OAuthTokenFromEnv;
        self
    }

    /// Use OAuth credentials from ~/.claude/ (requires prior `claude login`).
    pub fn oauth(mut self) -> Self {
        self.auth_method = AuthMethod::OAuth;
        self
    }

    /// Auto-detect auth method (default behavior).
    ///
    /// Tries: OAuth credentials -> CLAUDE_CODE_OAUTH_TOKEN -> ANTHROPIC_API_KEY
    pub fn auth_auto(mut self) -> Self {
        self.auth_method = AuthMethod::Auto;
        self
    }

    /// Add fallback auth method if primary is unavailable.
    ///
    /// Fallbacks are checked at config resolution time (before spawning CLI).
    /// Multiple fallbacks can be chained.
    pub fn fallback(mut self, auth: AuthMethod) -> Self {
        self.fallbacks.push(auth);
        self
    }

    // -------------------------------------------------------------------------
    // Model and permissions
    // -------------------------------------------------------------------------

    /// Set the model to use.
    pub fn model(mut self, model: impl Into<Model>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the permission mode for tool execution.
    pub fn permission_mode(mut self, mode: PermissionMode) -> Self {
        self.permission_mode = mode;
        self
    }

    // -------------------------------------------------------------------------
    // System prompts
    // -------------------------------------------------------------------------

    /// Set the system prompt (replaces default).
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Append to the system prompt (added after default).
    pub fn append_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.append_system_prompt = Some(prompt.into());
        self
    }

    // -------------------------------------------------------------------------
    // Tools configuration
    // -------------------------------------------------------------------------

    /// Set the complete list of available tools, replacing the default set.
    ///
    /// Use an empty slice to disable all tools, or provide specific tool names.
    /// Use constants from [`crate::config::tools`].
    ///
    /// This differs from `allowed_tools` which acts as a whitelist on top of defaults.
    pub fn tools(mut self, tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.tools = Some(tools.into_iter().map(Into::into).collect());
        self
    }

    /// Set allowed tools (whitelist).
    ///
    /// Only these tools will be available. Use constants from [`crate::config::tools`].
    pub fn allowed_tools(mut self, tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.allowed_tools = Some(tools.into_iter().map(Into::into).collect());
        self
    }

    /// Set disallowed tools (blacklist).
    ///
    /// These tools will not be available. Use constants from [`crate::config::tools`].
    pub fn disallowed_tools(mut self, tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.disallowed_tools = Some(tools.into_iter().map(Into::into).collect());
        self
    }

    // -------------------------------------------------------------------------
    // Budget
    // -------------------------------------------------------------------------

    /// Set the maximum budget in USD.
    ///
    /// The CLI will stop when this budget is reached.
    pub fn max_budget_usd(mut self, budget: f64) -> Self {
        self.max_budget_usd = Some(budget);
        self
    }

    // -------------------------------------------------------------------------
    // MCP and structured output
    // -------------------------------------------------------------------------

    /// Path to MCP configuration file.
    pub fn mcp_config(mut self, path: impl Into<PathBuf>) -> Self {
        self.mcp_config = Some(path.into());
        self
    }

    /// Constrain output to match JSON schema (enables structured output).
    ///
    /// When set, Claude's final text output will conform to the schema.
    pub fn json_schema(mut self, schema: Value) -> Self {
        self.json_schema = Some(schema);
        self
    }

    // -------------------------------------------------------------------------
    // Session options
    // -------------------------------------------------------------------------

    /// Resume a specific session by ID.
    pub fn session_id(mut self, id: impl Into<SessionId>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    /// Continue the most recent session.
    pub fn continue_session(mut self, cont: bool) -> Self {
        self.continue_session = cont;
        self
    }

    /// Include intermediate assistant messages during tool execution.
    ///
    /// When true, streams include partial assistant messages between tool calls.
    pub fn include_partial_messages(mut self, include: bool) -> Self {
        self.include_partial_messages = include;
        self
    }

    // -------------------------------------------------------------------------
    // Process options
    // -------------------------------------------------------------------------

    /// Path to claude CLI binary (default: search PATH for "claude").
    pub fn cli_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.cli_path = Some(path.into());
        self
    }

    /// Working directory for claude process.
    pub fn working_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(path.into());
        self
    }

    /// Timeout for requests.
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = Some(duration);
        self
    }

    /// Add/override environment variable for subprocess.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_vars.insert(key.into(), value.into());
        self
    }

    /// Don't inherit parent environment (default: inherit).
    pub fn inherit_env(mut self, inherit: bool) -> Self {
        self.inherit_env = inherit;
        self
    }

    // -------------------------------------------------------------------------
    // Build
    // -------------------------------------------------------------------------

    /// Build the configuration.
    ///
    /// This validates:
    /// - Authentication can be resolved
    /// - Budget is positive if set
    ///
    /// Note: CLI existence is checked lazily at spawn time.
    pub fn build(self) -> Result<ClientConfig> {
        // Resolve authentication
        let resolved_auth = resolve_auth(&self.auth_method, &self.fallbacks)?;

        // Validate budget
        if let Some(budget) = self.max_budget_usd {
            if budget <= 0.0 {
                return Err(Error::InvalidConfig(
                    "max_budget_usd must be positive".into(),
                ));
            }
        }

        // Validate working directory exists if specified
        if let Some(ref dir) = self.working_directory {
            if !dir.exists() {
                return Err(Error::InvalidConfig(format!(
                    "working directory does not exist: {}",
                    dir.display()
                )));
            }
        }

        Ok(ClientConfig {
            resolved_auth,
            model: self.model,
            permission_mode: self.permission_mode,
            system_prompt: self.system_prompt,
            append_system_prompt: self.append_system_prompt,
            tools: self.tools,
            allowed_tools: self.allowed_tools,
            disallowed_tools: self.disallowed_tools,
            max_budget_usd: self.max_budget_usd,
            mcp_config: self.mcp_config,
            json_schema: self.json_schema,
            session_id: self.session_id,
            continue_session: self.continue_session,
            include_partial_messages: self.include_partial_messages,
            cli_path: self.cli_path,
            working_directory: self.working_directory,
            timeout: self.timeout,
            env_vars: self.env_vars,
            inherit_env: self.inherit_env,
        })
    }
}

impl ClientConfig {
    /// Get the environment variables to set for the subprocess.
    pub(crate) fn build_env(&self) -> HashMap<String, String> {
        let mut env = self.env_vars.clone();

        // Set auth env var
        env.insert(
            self.resolved_auth.env_var_name().to_string(),
            self.resolved_auth.secret().to_string(),
        );

        env
    }

    /// Get the CLI path, or default to "claude".
    pub(crate) fn cli_command(&self) -> &str {
        self.cli_path
            .as_ref()
            .and_then(|p| p.to_str())
            .unwrap_or("claude")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_with_api_key() {
        let config = ClientConfigBuilder::default()
            .api_key("test-key")
            .build()
            .unwrap();

        assert!(matches!(config.resolved_auth, ResolvedAuth::ApiKey(k) if k == "test-key"));
    }

    #[test]
    fn builder_with_oauth_token() {
        let config = ClientConfigBuilder::default()
            .oauth_token("test-token")
            .build()
            .unwrap();

        assert!(matches!(config.resolved_auth, ResolvedAuth::OAuthToken(t) if t == "test-token"));
    }

    #[test]
    fn builder_with_model_and_permissions() {
        let config = ClientConfigBuilder::default()
            .api_key("key")
            .model(Model::Opus)
            .permission_mode(PermissionMode::BypassPermissions)
            .build()
            .unwrap();

        assert_eq!(config.model(), Some(&Model::Opus));
        assert_eq!(config.permission_mode(), PermissionMode::BypassPermissions);
    }

    #[test]
    fn builder_invalid_budget() {
        let result = ClientConfigBuilder::default()
            .api_key("key")
            .max_budget_usd(-1.0)
            .build();

        assert!(matches!(result, Err(Error::InvalidConfig(_))));
    }

    #[test]
    fn builder_invalid_working_directory() {
        let result = ClientConfigBuilder::default()
            .api_key("key")
            .working_directory("/nonexistent/path/that/does/not/exist")
            .build();

        assert!(matches!(result, Err(Error::InvalidConfig(_))));
    }

    #[test]
    fn build_env() {
        let config = ClientConfigBuilder::default()
            .api_key("secret-key")
            .env("CUSTOM_VAR", "custom_value")
            .build()
            .unwrap();

        let env = config.build_env();
        assert_eq!(env.get("ANTHROPIC_API_KEY"), Some(&"secret-key".to_string()));
        assert_eq!(env.get("CUSTOM_VAR"), Some(&"custom_value".to_string()));
    }

    #[test]
    fn cli_command_default() {
        let config = ClientConfigBuilder::default()
            .api_key("key")
            .build()
            .unwrap();

        assert_eq!(config.cli_command(), "claude");
    }

    #[test]
    fn cli_command_custom() {
        let config = ClientConfigBuilder::default()
            .api_key("key")
            .cli_path("/usr/local/bin/claude")
            .build()
            .unwrap();

        assert_eq!(config.cli_command(), "/usr/local/bin/claude");
    }

    #[test]
    fn allowed_tools() {
        let config = ClientConfigBuilder::default()
            .api_key("key")
            .allowed_tools(vec!["Read", "Glob"])
            .build()
            .unwrap();

        assert_eq!(
            config.allowed_tools,
            Some(vec!["Read".to_string(), "Glob".to_string()])
        );
    }

    #[test]
    fn tools_replaces_default_set() {
        let config = ClientConfigBuilder::default()
            .api_key("key")
            .tools(vec!["Bash", "Read"])
            .build()
            .unwrap();

        assert_eq!(
            config.tools,
            Some(vec!["Bash".to_string(), "Read".to_string()])
        );
    }

    #[test]
    fn tools_empty_disables_all() {
        let config = ClientConfigBuilder::default()
            .api_key("key")
            .tools(Vec::<String>::new())
            .build()
            .unwrap();

        assert_eq!(config.tools, Some(vec![]));
    }

    #[test]
    fn types_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ClientConfig>();
        assert_send_sync::<ClientConfigBuilder>();
    }

    #[test]
    fn model_from_string() {
        let config = ClientConfigBuilder::default()
            .api_key("key")
            .model("claude-3-5-sonnet-20241022")
            .build()
            .unwrap();

        assert_eq!(
            config.model(),
            Some(&Model::Custom("claude-3-5-sonnet-20241022".into()))
        );
    }

    #[test]
    fn inherit_env_defaults_to_true() {
        // Per IMPLEMENTATION_PLAN.md: "Don't inherit parent environment (default: inherit)"
        // This means inherit_env should default to true
        let config = ClientConfigBuilder::default()
            .api_key("key")
            .build()
            .unwrap();

        assert!(config.inherit_env, "inherit_env should default to true per plan");
    }

    #[test]
    fn inherit_env_can_be_disabled() {
        let config = ClientConfigBuilder::default()
            .api_key("key")
            .inherit_env(false)
            .build()
            .unwrap();

        assert!(!config.inherit_env);
    }
}
