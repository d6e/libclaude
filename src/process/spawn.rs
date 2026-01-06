//! Process spawning and lifecycle management.

use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::process::{Child, Command};

use super::io::{CliMessageStream, ProcessReader, ProcessWriter, StderrReader};
use super::MIN_CLI_VERSION;
use crate::config::{ClientConfig, SessionId};
use crate::{Error, Result};

/// Version check state. We only check once per process.
static VERSION_CHECKED: AtomicBool = AtomicBool::new(false);

/// A running Claude CLI process.
///
/// This struct manages the lifecycle of a single CLI invocation.
/// Each API call typically spawns a new process.
///
/// # Cancellation
///
/// Dropping a `ClaudeProcess` will kill the subprocess if it's still running.
pub struct ClaudeProcess {
    child: Child,
    reader: Option<ProcessReader>,
    stderr_reader: Option<StderrReader>,
    config: ClientConfig,
}

impl ClaudeProcess {
    /// Spawn a new Claude CLI process with the given prompt.
    ///
    /// The prompt is written to the subprocess stdin.
    pub async fn spawn(config: &ClientConfig, prompt: &str) -> Result<Self> {
        check_version_once(config).await;

        let mut cmd = build_command(config)?;
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::CliNotFound {
                    searched: config.cli_command().to_string(),
                }
            } else {
                Error::ProcessSpawn(e)
            }
        })?;

        // Write prompt via stdin
        let stdin = child.stdin.take().expect("stdin was configured");
        let writer = ProcessWriter::new(stdin);
        writer.write_prompt(prompt).await?;

        let stdout = child.stdout.take().expect("stdout was configured");
        let stderr = child.stderr.take().expect("stderr was configured");

        Ok(Self {
            child,
            reader: Some(ProcessReader::new(stdout)),
            stderr_reader: Some(StderrReader::new(stderr)),
            config: config.clone(),
        })
    }

    /// Spawn a new process continuing the most recent session.
    ///
    /// This uses the `--continue` flag to resume the last conversation.
    pub async fn spawn_continue(config: &ClientConfig, prompt: &str) -> Result<Self> {
        let mut continue_config = config.clone();
        continue_config.continue_session = true;
        Self::spawn(&continue_config, prompt).await
    }

    /// Spawn a new process resuming a specific session.
    ///
    /// This uses the `--resume <session_id>` flag to resume a specific conversation.
    pub async fn spawn_resume(
        config: &ClientConfig,
        session_id: &SessionId,
        prompt: &str,
    ) -> Result<Self> {
        let mut resume_config = config.clone();
        resume_config.session_id = Some(session_id.clone());
        Self::spawn(&resume_config, prompt).await
    }

    /// Take the message reader from this process.
    ///
    /// This transfers ownership of the reader, allowing it to be used
    /// independently of the process struct. The reader can only be taken once.
    pub fn take_reader(&mut self) -> Option<ProcessReader> {
        self.reader.take()
    }

    /// Take the stderr reader from this process.
    pub fn take_stderr_reader(&mut self) -> Option<StderrReader> {
        self.stderr_reader.take()
    }

    /// Convert the process reader into an async stream of messages.
    ///
    /// This consumes the reader and returns a stream that can be used
    /// with async stream combinators.
    pub fn into_stream(mut self) -> Option<CliMessageStream> {
        self.reader.take().map(CliMessageStream::new)
    }

    /// Get the process ID of the running CLI.
    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }

    /// Check if the process is still running.
    pub fn is_running(&self) -> bool {
        self.child.id().is_some()
    }

    /// Wait for the process to exit and return its exit status.
    pub async fn wait(&mut self) -> Result<std::process::ExitStatus> {
        self.child.wait().await.map_err(Error::io)
    }

    /// Kill the process immediately.
    pub async fn kill(&mut self) -> Result<()> {
        self.child.kill().await.map_err(Error::io)
    }

    /// Try to kill the process without waiting.
    pub fn start_kill(&mut self) -> Result<()> {
        self.child.start_kill().map_err(Error::io)
    }

    /// Get a reference to the underlying config.
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }
}

impl Drop for ClaudeProcess {
    fn drop(&mut self) {
        // Try to kill the process if it's still running
        let _ = self.start_kill();
    }
}

/// Build a tokio Command from the config.
fn build_command(config: &ClientConfig) -> Result<Command> {
    let mut cmd = Command::new(config.cli_command());

    // Set working directory if specified
    if let Some(ref dir) = config.working_directory {
        cmd.current_dir(dir);
    }

    // Set environment
    if !config.inherit_env {
        cmd.env_clear();
    }

    // Add auth and custom env vars
    for (key, value) in config.build_env() {
        cmd.env(key, value);
    }

    // Build arguments (prompt goes via stdin)
    let args = build_args(config);
    cmd.args(&args);

    Ok(cmd)
}

/// Build CLI arguments (prompt is sent via stdin, not as argument).
fn build_args(config: &ClientConfig) -> Vec<String> {
    let mut args = vec!["--output-format".to_string(), "json".to_string()];

    if let Some(ref model) = config.model {
        args.push("--model".to_string());
        args.push(model.to_string());
    }

    if config.permission_mode != crate::config::PermissionMode::Default {
        args.push("--permission-mode".to_string());
        args.push(config.permission_mode.to_string());
    }

    if let Some(ref prompt) = config.system_prompt {
        args.push("--system-prompt".to_string());
        args.push(prompt.clone());
    }

    if let Some(ref prompt) = config.append_system_prompt {
        args.push("--append-system-prompt".to_string());
        args.push(prompt.clone());
    }

    if let Some(ref tools) = config.tools {
        args.push("--tools".to_string());
        if tools.is_empty() {
            args.push(String::new());
        } else {
            args.push(tools.join(","));
        }
    }

    if let Some(ref tools) = config.allowed_tools {
        args.push("--allowedTools".to_string());
        args.push(tools.join(","));
    }

    if let Some(ref tools) = config.disallowed_tools {
        args.push("--disallowedTools".to_string());
        args.push(tools.join(","));
    }

    if let Some(budget) = config.max_budget_usd {
        args.push("--max-budget-usd".to_string());
        args.push(budget.to_string());
    }

    if let Some(ref path) = config.mcp_config {
        args.push("--mcp-config".to_string());
        args.push(path.display().to_string());
    }

    if let Some(ref schema) = config.json_schema {
        args.push("--json-schema".to_string());
        args.push(schema.to_string());
    }

    if let Some(ref id) = config.session_id {
        args.push("--resume".to_string());
        args.push(id.to_string());
    } else if config.continue_session {
        args.push("--continue".to_string());
    }

    if config.include_partial_messages {
        args.push("--include-partial-messages".to_string());
    }

    args
}

/// Check CLI version once per process.
async fn check_version_once(config: &ClientConfig) {
    if VERSION_CHECKED.swap(true, Ordering::SeqCst) {
        return;
    }

    if let Err(e) = check_cli_version(config).await {
        tracing::debug!("CLI version check failed: {}", e);
    }
}

/// Check the CLI version and warn if below minimum.
async fn check_cli_version(config: &ClientConfig) -> Result<()> {
    let output = Command::new(config.cli_command())
        .arg("--version")
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::CliNotFound {
                    searched: config.cli_command().to_string(),
                }
            } else {
                Error::io(e)
            }
        })?;

    if !output.status.success() {
        tracing::debug!("claude --version returned non-zero exit code");
        return Ok(());
    }

    let version_str = String::from_utf8_lossy(&output.stdout);
    let version = parse_version(&version_str);

    if let Some(version) = version {
        if version_below_min(&version) {
            tracing::warn!(
                "Claude CLI version {} is below minimum recommended version {}. \
                 Some features may not work correctly.",
                version_str.trim(),
                MIN_CLI_VERSION
            );
        } else {
            tracing::debug!("Claude CLI version: {}", version_str.trim());
        }
    } else {
        tracing::debug!("Could not parse CLI version from: {}", version_str.trim());
    }

    Ok(())
}

/// Parse a version string like "claude 2.0.76" into (major, minor, patch).
fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    // Find the first word that looks like a version
    for word in s.split_whitespace() {
        let word = word.strip_prefix('v').unwrap_or(word);
        let parts: Vec<&str> = word.split('.').collect();
        if parts.len() >= 3 {
            // Take just the numeric prefix of each part (handles "3-beta" -> "3")
            let major = parts[0]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>();
            let minor = parts[1]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>();
            let patch = parts[2]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>();

            if let (Ok(maj), Ok(min), Ok(pat)) = (
                major.parse::<u32>(),
                minor.parse::<u32>(),
                patch.parse::<u32>(),
            ) {
                return Some((maj, min, pat));
            }
        }
    }
    None
}

/// Check if a version is below the minimum required.
fn version_below_min(version: &(u32, u32, u32)) -> bool {
    let min = parse_version(MIN_CLI_VERSION).unwrap_or((2, 0, 0));

    if version.0 < min.0 {
        return true;
    }
    if version.0 > min.0 {
        return false;
    }
    if version.1 < min.1 {
        return true;
    }
    if version.1 > min.1 {
        return false;
    }
    version.2 < min.2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_formats() {
        assert_eq!(parse_version("claude 2.0.76"), Some((2, 0, 76)));
        assert_eq!(parse_version("2.0.76"), Some((2, 0, 76)));
        assert_eq!(parse_version("claude version 2.1.0"), Some((2, 1, 0)));
        assert_eq!(parse_version("v1.2.3-beta"), Some((1, 2, 3)));
        assert_eq!(parse_version("no version"), None);
    }

    #[test]
    fn version_comparison() {
        // MIN_CLI_VERSION is "2.0.0"
        assert!(!version_below_min(&(2, 0, 0)));
        assert!(!version_below_min(&(2, 0, 1)));
        assert!(!version_below_min(&(2, 1, 0)));
        assert!(!version_below_min(&(3, 0, 0)));
        assert!(version_below_min(&(1, 9, 9)));
        assert!(version_below_min(&(1, 0, 0)));
    }

    #[test]
    fn build_args_basic() {
        let config = crate::config::ClientConfig::builder()
            .api_key("test")
            .build()
            .unwrap();

        let args = build_args(&config);
        assert!(args.contains(&"--output-format".to_string()));
        assert!(args.contains(&"json".to_string()));
        // Should NOT contain -p (prompt goes via stdin)
        assert!(!args.contains(&"-p".to_string()));
    }

    #[test]
    fn build_args_with_options() {
        let config = crate::config::ClientConfig::builder()
            .api_key("test")
            .model(crate::config::Model::Opus)
            .continue_session(true)
            .build()
            .unwrap();

        let args = build_args(&config);
        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"opus".to_string()));
        assert!(args.contains(&"--continue".to_string()));
    }

    #[test]
    fn process_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ClaudeProcess>();
    }

    #[test]
    fn build_args_with_system_prompt() {
        let config = crate::config::ClientConfig::builder()
            .api_key("test")
            .system_prompt("You are a helpful assistant")
            .build()
            .unwrap();

        let args = build_args(&config);
        assert!(args.contains(&"--system-prompt".to_string()));
        assert!(args.contains(&"You are a helpful assistant".to_string()));
    }

    #[test]
    fn build_args_with_append_system_prompt() {
        let config = crate::config::ClientConfig::builder()
            .api_key("test")
            .append_system_prompt("Additional context")
            .build()
            .unwrap();

        let args = build_args(&config);
        assert!(args.contains(&"--append-system-prompt".to_string()));
        assert!(args.contains(&"Additional context".to_string()));
    }

    #[test]
    fn build_args_with_tools() {
        let config = crate::config::ClientConfig::builder()
            .api_key("test")
            .tools(vec!["Read".to_string(), "Write".to_string()])
            .build()
            .unwrap();

        let args = build_args(&config);
        assert!(args.contains(&"--tools".to_string()));
        assert!(args.contains(&"Read,Write".to_string()));
    }

    #[test]
    fn build_args_with_empty_tools() {
        let config = crate::config::ClientConfig::builder()
            .api_key("test")
            .tools(Vec::<String>::new())
            .build()
            .unwrap();

        let args = build_args(&config);
        assert!(args.contains(&"--tools".to_string()));
        // Empty tools list results in empty string arg
        let tools_idx = args.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(args[tools_idx + 1], "");
    }

    #[test]
    fn build_args_with_allowed_tools() {
        let config = crate::config::ClientConfig::builder()
            .api_key("test")
            .allowed_tools(vec!["Bash".to_string()])
            .build()
            .unwrap();

        let args = build_args(&config);
        assert!(args.contains(&"--allowedTools".to_string()));
        assert!(args.contains(&"Bash".to_string()));
    }

    #[test]
    fn build_args_with_disallowed_tools() {
        let config = crate::config::ClientConfig::builder()
            .api_key("test")
            .disallowed_tools(vec!["Bash".to_string(), "Write".to_string()])
            .build()
            .unwrap();

        let args = build_args(&config);
        assert!(args.contains(&"--disallowedTools".to_string()));
        assert!(args.contains(&"Bash,Write".to_string()));
    }

    #[test]
    fn build_args_with_max_budget() {
        let config = crate::config::ClientConfig::builder()
            .api_key("test")
            .max_budget_usd(5.0)
            .build()
            .unwrap();

        let args = build_args(&config);
        assert!(args.contains(&"--max-budget-usd".to_string()));
        assert!(args.contains(&"5".to_string()));
    }

    #[test]
    fn build_args_with_mcp_config() {
        use std::path::PathBuf;
        let config = crate::config::ClientConfig::builder()
            .api_key("test")
            .mcp_config(PathBuf::from("/tmp/mcp.json"))
            .build()
            .unwrap();

        let args = build_args(&config);
        assert!(args.contains(&"--mcp-config".to_string()));
        assert!(args.contains(&"/tmp/mcp.json".to_string()));
    }

    #[test]
    fn build_args_with_json_schema() {
        let config = crate::config::ClientConfig::builder()
            .api_key("test")
            .json_schema(serde_json::json!({"type": "object"}))
            .build()
            .unwrap();

        let args = build_args(&config);
        assert!(args.contains(&"--json-schema".to_string()));
    }

    #[test]
    fn build_args_with_session_id() {
        let config = crate::config::ClientConfig::builder()
            .api_key("test")
            .session_id(crate::config::SessionId::new("test-session"))
            .build()
            .unwrap();

        let args = build_args(&config);
        assert!(args.contains(&"--resume".to_string()));
        assert!(args.contains(&"test-session".to_string()));
        // Should not have --continue when session_id is set
        assert!(!args.contains(&"--continue".to_string()));
    }

    #[test]
    fn build_args_with_include_partial_messages() {
        let mut config = crate::config::ClientConfig::builder()
            .api_key("test")
            .build()
            .unwrap();
        config.include_partial_messages = true;

        let args = build_args(&config);
        assert!(args.contains(&"--include-partial-messages".to_string()));
    }

    #[test]
    fn build_args_with_permission_mode_default() {
        let config = crate::config::ClientConfig::builder()
            .api_key("test")
            .permission_mode(crate::config::PermissionMode::Default)
            .build()
            .unwrap();

        let args = build_args(&config);
        // Default mode should not add --permission-mode flag
        assert!(!args.contains(&"--permission-mode".to_string()));
    }

    #[test]
    fn build_args_with_permission_mode_plan() {
        let config = crate::config::ClientConfig::builder()
            .api_key("test")
            .permission_mode(crate::config::PermissionMode::Plan)
            .build()
            .unwrap();

        let args = build_args(&config);
        assert!(args.contains(&"--permission-mode".to_string()));
        assert!(args.contains(&"plan".to_string()));
    }

    #[test]
    fn build_args_with_permission_mode_bypass_permissions() {
        let config = crate::config::ClientConfig::builder()
            .api_key("test")
            .permission_mode(crate::config::PermissionMode::BypassPermissions)
            .build()
            .unwrap();

        let args = build_args(&config);
        assert!(args.contains(&"--permission-mode".to_string()));
        assert!(args.contains(&"bypassPermissions".to_string()));
    }

    #[test]
    fn build_args_with_all_models() {
        for (model, expected) in [
            (crate::config::Model::Sonnet, "sonnet"),
            (crate::config::Model::Opus, "opus"),
            (crate::config::Model::Haiku, "haiku"),
        ] {
            let config = crate::config::ClientConfig::builder()
                .api_key("test")
                .model(model)
                .build()
                .unwrap();

            let args = build_args(&config);
            assert!(args.contains(&"--model".to_string()));
            assert!(args.contains(&expected.to_string()));
        }
    }

    #[test]
    fn version_below_min_edge_cases() {
        // Test minor version comparison
        assert!(version_below_min(&(2, 0, 0)) == false); // Equal to min
        assert!(version_below_min(&(1, 99, 99))); // Major < min
        assert!(!version_below_min(&(3, 0, 0))); // Major > min

        // Test when major is equal
        assert!(!version_below_min(&(2, 1, 0))); // Minor > min
        assert!(!version_below_min(&(2, 0, 1))); // Patch > min
    }

    #[test]
    fn parse_version_edge_cases() {
        // Version with extra text
        assert_eq!(
            parse_version("claude cli v2.1.3-beta+build"),
            Some((2, 1, 3))
        );

        // Only numbers
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));

        // With prefix 'v'
        assert_eq!(parse_version("v3.4.5"), Some((3, 4, 5)));

        // Multiple version-like strings (takes first valid)
        assert_eq!(parse_version("1.2.3 and 4.5.6"), Some((1, 2, 3)));

        // Two parts only - not valid
        assert_eq!(parse_version("1.2"), None);

        // Non-numeric
        assert_eq!(parse_version("abc"), None);

        // Empty
        assert_eq!(parse_version(""), None);
    }
}
