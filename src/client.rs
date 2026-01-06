//! High-level Claude client for sending prompts and managing sessions.
//!
//! This module provides [`ClaudeClient`], the main entry point for interacting
//! with Claude Code.
//!
//! # Example
//!
//! ```ignore
//! use libclaude::{ClaudeClient, Result};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Simple one-shot request
//!     let client = ClaudeClient::new()?;
//!     let response = client.send_and_collect("What is 2+2?").await?;
//!     println!("{}", response);
//!
//!     // Streaming response
//!     use futures::StreamExt;
//!     let mut stream = client.send("Write a haiku").await?;
//!     while let Some(event) = stream.next().await {
//!         // Process streaming events
//!     }
//!
//!     Ok(())
//! }
//! ```

use std::sync::Arc;
use std::time::Duration;

use crate::config::{ClientConfig, ClientConfigBuilder, SessionId};
use crate::process::ClaudeProcess;
use crate::session::Session;
use crate::stream::{with_timeout, ResponseStream};
use crate::Result;

/// A client for interacting with Claude Code.
///
/// `ClaudeClient` is the main entry point for sending prompts to Claude.
/// It holds the configuration and provides methods for:
/// - One-shot requests ([`send`](Self::send), [`send_and_collect`](Self::send_and_collect))
/// - Multi-turn sessions ([`start_session`](Self::start_session), [`continue_session`](Self::continue_session))
///
/// # Thread Safety
///
/// `ClaudeClient` is `Send + Sync` and can be safely shared across tasks.
/// Each request spawns a new CLI process, so concurrent requests are supported.
///
/// # Example
///
/// ```ignore
/// use libclaude::ClaudeClient;
///
/// let client = ClaudeClient::builder()
///     .api_key("sk-ant-...")
///     .model(libclaude::Model::Opus)
///     .build()?;
///
/// let response = client.send_and_collect("Hello!").await?;
/// ```
#[derive(Debug, Clone)]
pub struct ClaudeClient {
    config: Arc<ClientConfig>,
}

impl ClaudeClient {
    /// Create a new client with default configuration (auto-detect auth).
    ///
    /// This uses [`AuthMethod::Auto`](crate::AuthMethod::Auto) which tries:
    /// 1. OAuth credentials from `~/.claude/.credentials.json`
    /// 2. `CLAUDE_CODE_OAUTH_TOKEN` environment variable
    /// 3. `ANTHROPIC_API_KEY` environment variable
    ///
    /// # Errors
    ///
    /// Returns an error if no authentication method can be resolved.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let client = ClaudeClient::new()?;
    /// ```
    pub fn new() -> Result<Self> {
        let config = ClientConfig::builder().build()?;
        Ok(Self::with_config(config))
    }

    /// Create a new client with the given configuration.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let config = ClientConfig::builder()
    ///     .api_key("sk-ant-...")
    ///     .model(Model::Opus)
    ///     .build()?;
    ///
    /// let client = ClaudeClient::with_config(config);
    /// ```
    pub fn with_config(config: ClientConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    /// Create a builder for configuring a new client.
    ///
    /// This returns a [`ClientBuilder`] that provides a fluent API for
    /// configuring the client before building it.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let client = ClaudeClient::builder()
    ///     .api_key("sk-ant-...")
    ///     .model(Model::Opus)
    ///     .permission_mode(PermissionMode::BypassPermissions)
    ///     .max_budget_usd(5.00)
    ///     .build()?;
    /// ```
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Send a prompt and return a stream of response events.
    ///
    /// This is the low-level streaming API. For simple use cases, prefer
    /// [`send_and_collect`](Self::send_and_collect).
    ///
    /// # Cancellation
    ///
    /// Dropping the returned [`ResponseStream`] will kill the subprocess
    /// and cancel the request.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use futures::StreamExt;
    /// use libclaude::StreamEvent;
    ///
    /// let mut stream = client.send("Write a poem").await?;
    /// while let Some(event) = stream.next().await {
    ///     match event? {
    ///         StreamEvent::TextDelta { text, .. } => print!("{}", text),
    ///         StreamEvent::Complete(_) => break,
    ///         _ => {}
    ///     }
    /// }
    /// ```
    pub async fn send(&self, prompt: &str) -> Result<ResponseStream> {
        let process = ClaudeProcess::spawn(&self.config, prompt).await?;
        let observer = self.config.tool_observer().cloned();
        let stream = ResponseStream::with_observer(process, observer);

        Ok(stream)
    }

    /// Send a prompt and collect the full text response.
    ///
    /// This is the simplest way to get a response from Claude.
    /// It waits for the complete response and returns all text content.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let response = client.send_and_collect("What is 2+2?").await?;
    /// println!("{}", response);
    /// ```
    pub async fn send_and_collect(&self, prompt: &str) -> Result<String> {
        let stream = self.send(prompt).await?;

        if let Some(timeout) = self.config.timeout() {
            with_timeout(timeout, stream.collect_text()).await
        } else {
            stream.collect_text().await
        }
    }

    /// Start a new multi-turn session with an initial prompt.
    ///
    /// Sessions maintain conversation history across multiple turns.
    /// The returned [`Session`] can be used to continue the conversation.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let session = client.start_session("My name is Alice").await?;
    /// let response = session.send_and_collect("What's my name?").await?;
    /// // Claude will remember: "Your name is Alice"
    /// ```
    pub async fn start_session(&self, prompt: &str) -> Result<Session> {
        let process = ClaudeProcess::spawn(&self.config, prompt).await?;
        let observer = self.config.tool_observer().cloned();
        let stream = ResponseStream::with_observer(process, observer);

        Session::from_initial_stream(Arc::clone(&self.config), stream).await
    }

    /// Continue the most recent session.
    ///
    /// This resumes the last conversation that was started, using
    /// the `--continue` CLI flag.
    ///
    /// # Note
    ///
    /// This method sends an empty prompt to establish the session.
    /// Use the returned [`Session`] to send actual messages.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Previous conversation from another process
    /// let session = client.continue_session().await?;
    /// let response = session.send_and_collect("What were we talking about?").await?;
    /// ```
    pub async fn continue_session(&self, prompt: &str) -> Result<Session> {
        let process = ClaudeProcess::spawn_continue(&self.config, prompt).await?;
        let observer = self.config.tool_observer().cloned();
        let stream = ResponseStream::with_observer(process, observer);

        Session::from_initial_stream(Arc::clone(&self.config), stream).await
    }

    /// Resume a specific session by ID.
    ///
    /// This resumes a conversation with the given session ID, using
    /// the `--resume <session_id>` CLI flag.
    ///
    /// Session IDs are returned from previous conversations via
    /// [`Session::session_id`] or from streaming events.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Save session ID from a previous conversation
    /// let session_id = previous_session.session_id().clone();
    ///
    /// // Later, resume the session
    /// let session = client.resume_session(&session_id, "Let's continue").await?;
    /// ```
    pub async fn resume_session(&self, session_id: &SessionId, prompt: &str) -> Result<Session> {
        let process = ClaudeProcess::spawn_resume(&self.config, session_id, prompt).await?;
        let observer = self.config.tool_observer().cloned();
        let stream = ResponseStream::with_observer(process, observer);

        Session::from_initial_stream(Arc::clone(&self.config), stream).await
    }

    /// Get a reference to the client's configuration.
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }
}

/// Builder for [`ClaudeClient`].
///
/// This wraps [`ClientConfigBuilder`] and builds directly into a [`ClaudeClient`].
///
/// # Example
///
/// ```ignore
/// let client = ClaudeClient::builder()
///     .api_key("sk-ant-...")
///     .model(Model::Opus)
///     .build()?;
/// ```
#[derive(Debug, Clone, Default)]
pub struct ClientBuilder {
    inner: ClientConfigBuilder,
}

impl ClientBuilder {
    /// Create a new client builder with default settings.
    pub fn new() -> Self {
        Self {
            inner: ClientConfigBuilder::default(),
        }
    }

    /// Build the client.
    ///
    /// This validates the configuration and creates the client.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No authentication method can be resolved
    /// - The configuration is invalid (e.g., negative budget)
    pub fn build(self) -> Result<ClaudeClient> {
        let config = self.inner.build()?;
        Ok(ClaudeClient::with_config(config))
    }

    // -------------------------------------------------------------------------
    // Authentication methods (delegated to ClientConfigBuilder)
    // -------------------------------------------------------------------------

    /// Use API key directly.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.inner = self.inner.api_key(key);
        self
    }

    /// Read API key from ANTHROPIC_API_KEY env var.
    pub fn api_key_from_env(mut self) -> Self {
        self.inner = self.inner.api_key_from_env();
        self
    }

    /// Use OAuth token directly.
    pub fn oauth_token(mut self, token: impl Into<String>) -> Self {
        self.inner = self.inner.oauth_token(token);
        self
    }

    /// Read OAuth token from CLAUDE_CODE_OAUTH_TOKEN env var.
    pub fn oauth_token_from_env(mut self) -> Self {
        self.inner = self.inner.oauth_token_from_env();
        self
    }

    /// Use OAuth credentials from ~/.claude/.
    pub fn oauth(mut self) -> Self {
        self.inner = self.inner.oauth();
        self
    }

    /// Auto-detect auth method (default).
    pub fn auth_auto(mut self) -> Self {
        self.inner = self.inner.auth_auto();
        self
    }

    /// Add fallback auth method.
    pub fn fallback(mut self, auth: crate::config::AuthMethod) -> Self {
        self.inner = self.inner.fallback(auth);
        self
    }

    // -------------------------------------------------------------------------
    // Model and permissions
    // -------------------------------------------------------------------------

    /// Set the model to use.
    pub fn model(mut self, model: impl Into<crate::config::Model>) -> Self {
        self.inner = self.inner.model(model);
        self
    }

    /// Set the permission mode.
    pub fn permission_mode(mut self, mode: crate::config::PermissionMode) -> Self {
        self.inner = self.inner.permission_mode(mode);
        self
    }

    // -------------------------------------------------------------------------
    // System prompts
    // -------------------------------------------------------------------------

    /// Set the system prompt.
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.inner = self.inner.system_prompt(prompt);
        self
    }

    /// Append to the system prompt.
    pub fn append_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.inner = self.inner.append_system_prompt(prompt);
        self
    }

    // -------------------------------------------------------------------------
    // Tools configuration
    // -------------------------------------------------------------------------

    /// Set the complete list of available tools.
    pub fn tools(mut self, tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.inner = self.inner.tools(tools);
        self
    }

    /// Set allowed tools (whitelist).
    pub fn allowed_tools(mut self, tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.inner = self.inner.allowed_tools(tools);
        self
    }

    /// Set disallowed tools (blacklist).
    pub fn disallowed_tools(mut self, tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.inner = self.inner.disallowed_tools(tools);
        self
    }

    // -------------------------------------------------------------------------
    // Budget
    // -------------------------------------------------------------------------

    /// Set the maximum budget in USD.
    pub fn max_budget_usd(mut self, budget: f64) -> Self {
        self.inner = self.inner.max_budget_usd(budget);
        self
    }

    // -------------------------------------------------------------------------
    // MCP and structured output
    // -------------------------------------------------------------------------

    /// Path to MCP configuration file.
    pub fn mcp_config(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.inner = self.inner.mcp_config(path);
        self
    }

    /// Constrain output to match JSON schema.
    pub fn json_schema(mut self, schema: serde_json::Value) -> Self {
        self.inner = self.inner.json_schema(schema);
        self
    }

    // -------------------------------------------------------------------------
    // Session options
    // -------------------------------------------------------------------------

    /// Resume a specific session by ID.
    ///
    /// When set, the client will use `--resume <session_id>` to continue
    /// an existing conversation.
    pub fn session_id(mut self, id: impl Into<crate::config::SessionId>) -> Self {
        self.inner = self.inner.session_id(id);
        self
    }

    /// Continue the most recent session.
    ///
    /// When set, the client will use `--continue` to resume the last conversation.
    pub fn continue_session(mut self, cont: bool) -> Self {
        self.inner = self.inner.continue_session(cont);
        self
    }

    /// Include intermediate assistant messages during tool execution.
    pub fn include_partial_messages(mut self, include: bool) -> Self {
        self.inner = self.inner.include_partial_messages(include);
        self
    }

    // -------------------------------------------------------------------------
    // Process options
    // -------------------------------------------------------------------------

    /// Path to claude CLI binary.
    pub fn cli_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.inner = self.inner.cli_path(path);
        self
    }

    /// Working directory for claude process.
    pub fn working_directory(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.inner = self.inner.working_directory(path);
        self
    }

    /// Timeout for requests.
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.inner = self.inner.timeout(duration);
        self
    }

    /// Add/override environment variable for subprocess.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner = self.inner.env(key, value);
        self
    }

    /// Don't inherit parent environment.
    pub fn inherit_env(mut self, inherit: bool) -> Self {
        self.inner = self.inner.inherit_env(inherit);
        self
    }

    // -------------------------------------------------------------------------
    // Tool observer
    // -------------------------------------------------------------------------

    /// Set a tool observer for monitoring tool execution.
    ///
    /// The observer will be called when Claude invokes tools and when
    /// tool results are received. This is for observation only.
    pub fn tool_observer(
        mut self,
        observer: std::sync::Arc<dyn crate::tools::ToolObserver>,
    ) -> Self {
        self.inner = self.inner.tool_observer(observer);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthMethod, Model, PermissionMode};

    #[test]
    fn client_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ClaudeClient>();
    }

    #[test]
    fn client_builder_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ClientBuilder>();
    }

    #[test]
    fn client_is_clone() {
        fn assert_clone<T: Clone>() {}
        assert_clone::<ClaudeClient>();
    }

    #[test]
    fn builder_builds_with_api_key() {
        let client = ClaudeClient::builder().api_key("test-key").build().unwrap();
        assert!(client.config().model().is_none());
    }

    #[test]
    fn builder_chains_options() {
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .model(crate::config::Model::Opus)
            .max_budget_usd(10.0)
            .permission_mode(crate::config::PermissionMode::BypassPermissions)
            .build()
            .unwrap();

        assert_eq!(client.config().model(), Some(&crate::config::Model::Opus));
        assert_eq!(
            client.config().permission_mode(),
            crate::config::PermissionMode::BypassPermissions
        );
    }

    #[test]
    fn with_config_works() {
        let config = ClientConfig::builder().api_key("test-key").build().unwrap();
        let client = ClaudeClient::with_config(config);
        assert!(client.config().model().is_none());
    }

    #[test]
    fn builder_with_oauth_token() {
        let client = ClaudeClient::builder()
            .oauth_token("oauth-token-123")
            .build()
            .unwrap();
        assert!(client.config().model().is_none());
    }

    #[test]
    fn builder_with_system_prompt() {
        // Just verify the builder accepts system_prompt
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .system_prompt("You are a helpful assistant")
            .build()
            .unwrap();
        // Verify build succeeded
        assert!(client.config().model().is_none());
    }

    #[test]
    fn builder_with_append_system_prompt() {
        // Just verify the builder accepts append_system_prompt
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .system_prompt("Base prompt")
            .append_system_prompt(" with extra")
            .build()
            .unwrap();
        // Verify build succeeded
        assert!(client.config().model().is_none());
    }

    #[test]
    fn builder_with_tools() {
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .tools(["Read", "Write", "Bash"])
            .build()
            .unwrap();
        // Verify build succeeded
        assert!(client.config().model().is_none());
    }

    #[test]
    fn builder_with_allowed_tools() {
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .allowed_tools(["Read", "Write"])
            .build()
            .unwrap();
        // Verify build succeeded
        assert!(client.config().model().is_none());
    }

    #[test]
    fn builder_with_disallowed_tools() {
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .disallowed_tools(["Bash"])
            .build()
            .unwrap();
        // Verify build succeeded
        assert!(client.config().model().is_none());
    }

    #[test]
    fn builder_with_timeout() {
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap();
        assert_eq!(client.config().timeout(), Some(Duration::from_secs(60)));
    }

    #[test]
    fn builder_with_working_directory() {
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .working_directory("/tmp")
            .build()
            .unwrap();
        assert!(client.config().working_directory().is_some());
    }

    #[test]
    fn builder_with_cli_path() {
        // Just verify the builder accepts cli_path
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .cli_path("/usr/local/bin/claude")
            .build()
            .unwrap();
        // Verify build succeeded
        assert!(client.config().model().is_none());
    }

    #[test]
    fn builder_with_env() {
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .env("MY_VAR", "my_value")
            .build()
            .unwrap();
        // Env vars are stored in config
        assert!(client.config().model().is_none()); // Just verify build works
    }

    #[test]
    fn builder_with_inherit_env_false() {
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .inherit_env(false)
            .build()
            .unwrap();
        assert!(client.config().model().is_none()); // Just verify build works
    }

    #[test]
    fn builder_with_mcp_config() {
        // Just verify the builder accepts mcp_config
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .mcp_config("/path/to/mcp.json")
            .build()
            .unwrap();
        // Verify build succeeded
        assert!(client.config().model().is_none());
    }

    #[test]
    fn builder_with_json_schema() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "answer": {"type": "string"}
            }
        });
        // Just verify the builder accepts json_schema
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .json_schema(schema)
            .build()
            .unwrap();
        // Verify build succeeded
        assert!(client.config().model().is_none());
    }

    #[test]
    fn builder_with_include_partial_messages() {
        // Just verify the builder accepts include_partial_messages
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .include_partial_messages(true)
            .build()
            .unwrap();
        // Verify build succeeded
        assert!(client.config().model().is_none());
    }

    #[test]
    fn builder_with_all_models() {
        // Test all model variants
        for model in [Model::Opus, Model::Sonnet, Model::Haiku] {
            let client = ClaudeClient::builder()
                .api_key("test-key")
                .model(model.clone())
                .build()
                .unwrap();
            assert_eq!(client.config().model(), Some(&model));
        }
    }

    #[test]
    fn builder_with_all_permission_modes() {
        // Test all permission mode variants
        for mode in [
            PermissionMode::Default,
            PermissionMode::BypassPermissions,
            PermissionMode::Plan,
        ] {
            let client = ClaudeClient::builder()
                .api_key("test-key")
                .permission_mode(mode.clone())
                .build()
                .unwrap();
            assert_eq!(client.config().permission_mode(), mode);
        }
    }

    #[test]
    fn builder_with_fallback_auth() {
        let client = ClaudeClient::builder()
            .api_key("primary-key")
            .fallback(AuthMethod::ApiKeyFromEnv)
            .build()
            .unwrap();
        // Just verify it builds
        assert!(client.config().model().is_none());
    }

    #[test]
    fn builder_with_max_budget() {
        // Just verify the builder accepts max_budget_usd
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .max_budget_usd(5.0)
            .build()
            .unwrap();
        // Verify build succeeded
        assert!(client.config().model().is_none());
    }

    #[test]
    fn client_clone_shares_config() {
        let client1 = ClaudeClient::builder()
            .api_key("test-key")
            .model(Model::Opus)
            .build()
            .unwrap();
        let client2 = client1.clone();
        assert_eq!(client1.config().model(), client2.config().model());
    }

    #[test]
    fn builder_default_creates_empty_builder() {
        let _builder = ClientBuilder::default();
        // Default builder exists - verified by compilation
    }

    #[test]
    fn builder_new_same_as_default() {
        let _builder1 = ClientBuilder::new();
        let _builder2 = ClientBuilder::default();
        // Both exist - verified by compilation
    }

    #[test]
    fn builder_with_session_id() {
        // Just verify the builder accepts session_id
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .session_id("test-session-123")
            .build()
            .unwrap();
        // Verify build succeeded
        assert!(client.config().model().is_none());
    }

    #[test]
    fn builder_with_continue_session() {
        // Just verify the builder accepts continue_session
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .continue_session(true)
            .build()
            .unwrap();
        // Verify build succeeded
        assert!(client.config().model().is_none());
    }

    #[test]
    fn builder_with_api_key_from_env() {
        // This will fail if ANTHROPIC_API_KEY is not set, but the builder method works
        let builder = ClaudeClient::builder().api_key_from_env();
        // Verify the builder chain works
        let _ = builder;
    }

    #[test]
    fn builder_with_oauth_token_from_env() {
        // This will fail if CLAUDE_CODE_OAUTH_TOKEN is not set, but the builder method works
        let builder = ClaudeClient::builder().oauth_token_from_env();
        // Verify the builder chain works
        let _ = builder;
    }

    #[test]
    fn builder_with_oauth() {
        // This will fail if OAuth credentials don't exist, but the builder method works
        let builder = ClaudeClient::builder().oauth();
        // Verify the builder chain works
        let _ = builder;
    }

    #[test]
    fn builder_with_auth_auto() {
        // auth_auto is the default
        let builder = ClaudeClient::builder().auth_auto();
        // Verify the builder chain works
        let _ = builder;
    }

    #[test]
    fn builder_with_tool_observer() {
        use std::sync::Arc;

        struct TestObserver;
        impl crate::tools::ToolObserver for TestObserver {}

        let observer: Arc<dyn crate::tools::ToolObserver> = Arc::new(TestObserver);
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .tool_observer(observer)
            .build()
            .unwrap();

        assert!(client.config().tool_observer().is_some());
    }

    #[test]
    fn client_config_accessor() {
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .model(Model::Sonnet)
            .build()
            .unwrap();

        // config() returns a reference to the config
        let config = client.config();
        assert_eq!(config.model(), Some(&Model::Sonnet));
    }

    #[test]
    fn builder_full_chain() {
        // Test chaining many options together
        let client = ClaudeClient::builder()
            .api_key("test-key")
            .model(Model::Opus)
            .permission_mode(PermissionMode::Plan)
            .system_prompt("Test prompt")
            .append_system_prompt(" additional")
            .tools(["Read"])
            .allowed_tools(["Read", "Write"])
            .disallowed_tools(["Bash"])
            .max_budget_usd(10.0)
            .timeout(Duration::from_secs(30))
            .working_directory("/tmp")
            .env("KEY", "VALUE")
            .inherit_env(true)
            .include_partial_messages(false)
            .build()
            .unwrap();

        assert_eq!(client.config().model(), Some(&Model::Opus));
        assert_eq!(client.config().permission_mode(), PermissionMode::Plan);
    }
}
