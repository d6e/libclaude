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
}
