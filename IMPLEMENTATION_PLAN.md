# libclaude Implementation Plan

## Goal
Create an async-first Rust library that wraps the Claude Code CLI for programmatic use, exposing the full JSON protocol with typed structs and providing high-level session management with streaming support.

## Success Criteria
- All JSON protocol message types have corresponding Rust types with serde
- Streaming works with async Stream trait (token-by-token)
- Multi-turn sessions can be started, resumed, and continued
- All major CLI flags are exposed via builder pattern
- Tests pass, library compiles

---

## Stage 1: Core Types and Protocol

**Goal**: Define all JSON protocol types with serde serialization

**Files**:
- `src/error.rs` - Error enum with thiserror
- `src/protocol/mod.rs` - Module exports
- `src/protocol/usage.rs` - Usage tracking struct
- `src/protocol/content.rs` - ContentBlock enum (text, tool_use, tool_result)
- `src/protocol/events.rs` - StreamEventType variants for streaming
- `src/protocol/messages.rs` - CliMessage enum (system, assistant, user, stream_event, result)

**Error enum**:
```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // Configuration errors (detected at build() time)
    #[error("no authentication method configured or resolvable")]
    AuthNotConfigured,
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    // Spawn errors
    #[error("claude CLI not found in PATH (searched: {searched})")]
    CliNotFound { searched: String },
    #[error("failed to spawn claude process: {0}")]
    ProcessSpawn(#[source] std::io::Error),

    // IO errors
    #[error("IO error communicating with claude process: {0}")]
    Io(#[from] std::io::Error),

    // Protocol errors
    #[error("failed to parse JSON from CLI output: {0}")]
    JsonParse(#[from] serde_json::Error),
    #[error("unexpected message type from CLI: {message_type}")]
    UnexpectedMessage { message_type: String },

    // Runtime errors
    #[error("request timed out after {duration:?}")]
    Timeout { duration: std::time::Duration },
    #[error("CLI reported error: {message}")]
    CliError { message: String, is_auth_error: bool },
}

pub type Result<T> = std::result::Result<T, Error>;
```

**Tests**: Unit tests for JSON parsing of each message type

**Status**: Complete

---

## Stage 2: Configuration and Authentication

**Goal**: Builder pattern for all CLI configuration options including authentication

**Files**:
- `src/config/mod.rs` - Module exports
- `src/config/options.rs` - Enums and newtypes (see Type Safety section)
- `src/config/builder.rs` - ClientConfig and ClientConfigBuilder
- `src/config/auth.rs` - AuthMethod enum and auth resolution

**Authentication**:
```rust
pub enum AuthMethod {
    /// Use API key directly (passed via ANTHROPIC_API_KEY env var to subprocess)
    ApiKey(String),
    /// Read API key from ANTHROPIC_API_KEY env var
    ApiKeyFromEnv,
    /// Use OAuth token directly (passed via CLAUDE_CODE_OAUTH_TOKEN env var to subprocess)
    OAuthToken(String),
    /// Read OAuth token from CLAUDE_CODE_OAUTH_TOKEN env var
    OAuthTokenFromEnv,
    /// Use OAuth credentials from ~/.claude/.credentials.json
    OAuth,
    /// Auto-detect: try OAuth -> OAuthTokenFromEnv -> ApiKeyFromEnv
    Auto,
}

impl Default for AuthMethod {
    fn default() -> Self {
        AuthMethod::Auto
    }
}

/// OAuth credentials read from ~/.claude/.credentials.json
pub struct OAuthCredentials {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,  // Unix timestamp in milliseconds
    pub scopes: Vec<String>,
}

impl OAuthCredentials {
    /// Load credentials from ~/.claude/.credentials.json
    pub fn load() -> Result<Option<Self>>;
    /// Check if access token is expired
    pub fn is_expired(&self) -> bool;
}

/// Shell out to `claude login` for interactive browser-based OAuth
pub async fn login_interactive() -> Result<()>;

/// Shell out to `claude setup-token` for long-lived token (also opens browser, requires subscription)
pub async fn setup_token() -> Result<()>;

/// Check if OAuth credentials exist in ~/.claude/
pub fn has_oauth_credentials() -> bool;
```

**Credentials file format** (`~/.claude/.credentials.json`):
```json
{
  "claudeAiOauth": {
    "accessToken": "sk-ant-oat01-...",
    "refreshToken": "sk-ant-ort01-...",
    "expiresAt": 1767582136497,
    "scopes": ["user:inference", "user:profile", "user:sessions:claude_code"]
  }
}
```

**Auth resolution logic**:
1. `Auto` (default): Try OAuth -> OAuthTokenFromEnv -> ApiKeyFromEnv
2. `OAuth`: Use credentials from `~/.claude/.credentials.json`, error if not found
3. `OAuthToken(token)`: Pass token via `CLAUDE_CODE_OAUTH_TOKEN` env var to subprocess
4. `OAuthTokenFromEnv`: Read `CLAUDE_CODE_OAUTH_TOKEN`, pass to subprocess
5. `ApiKey(key)`: Pass key via `ANTHROPIC_API_KEY` env var to subprocess
6. `ApiKeyFromEnv`: Read `ANTHROPIC_API_KEY`, pass to subprocess

**Key options to expose**:
- `--model` (sonnet, opus, custom)
- `--permission-mode` (default, plan, acceptEdits, bypassPermissions)
- `--system-prompt`, `--append-system-prompt`
- `--tools`, `--allowedTools`, `--disallowedTools`
- `--max-budget-usd`
- `--mcp-config`
- `--json-schema`
- `--include-partial-messages`
- `--session-id`, `--continue`, `--resume`

**Additional builder options**:
```rust
impl ClientConfigBuilder {
    /// Path to claude CLI binary (default: search PATH for "claude")
    fn cli_path(self, path: impl Into<PathBuf>) -> Self;
    /// Working directory for claude process
    fn working_directory(self, path: impl Into<PathBuf>) -> Self;
    /// Timeout for requests (default: none)
    fn timeout(self, duration: Duration) -> Self;
    /// Add/override environment variable for subprocess
    fn env(self, key: impl Into<String>, value: impl Into<String>) -> Self;
    /// Don't inherit parent environment (default: inherit)
    fn inherit_env(self, inherit: bool) -> Self;
    /// Path to MCP configuration file
    fn mcp_config(self, path: impl Into<PathBuf>) -> Self;
    /// Constrain output to match JSON schema (enables structured output)
    fn json_schema(self, schema: Value) -> Self;
    /// Include intermediate assistant messages during tool execution
    fn include_partial_messages(self, include: bool) -> Self;
}
```

**Structured output** (JSON schema):
When `json_schema` is set, Claude's final text output will conform to the schema. This enables type-safe response parsing:
```rust
// Basic usage with raw schema
let client = ClaudeClient::builder()
    .json_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "answer": { "type": "string" },
            "confidence": { "type": "number" }
        },
        "required": ["answer", "confidence"]
    }))
    .build()?;

let response = client.send_and_collect("What is 2+2?").await?;
let parsed: MyResponse = serde_json::from_str(&response)?;
```

**Builder auth methods**:
```rust
impl ClientConfigBuilder {
    /// Use API key directly (passed as ANTHROPIC_API_KEY to subprocess)
    fn api_key(self, key: impl Into<String>) -> Self;
    /// Read API key from ANTHROPIC_API_KEY env var
    fn api_key_from_env(self) -> Self;
    /// Use OAuth token directly (passed as CLAUDE_CODE_OAUTH_TOKEN to subprocess)
    /// Get this token from `claude setup-token` (valid for 1 year)
    fn oauth_token(self, token: impl Into<String>) -> Self;
    /// Read OAuth token from CLAUDE_CODE_OAUTH_TOKEN env var
    fn oauth_token_from_env(self) -> Self;
    /// Use OAuth credentials from ~/.claude/ (requires prior `claude login`)
    fn oauth(self) -> Self;
    /// Auto-detect auth method (default behavior)
    fn auth_auto(self) -> Self;
    /// Add fallback auth method if primary is unavailable (checked at build time, not on failure)
    fn fallback(self, auth: AuthMethod) -> Self;
}
```
Custom env var? Read it yourself: `.api_key(std::env::var("MY_VAR")?)`

**Fallback behavior**:
- Fallbacks are checked at config resolution time (before spawning CLI)
- If primary auth source is missing (env var not set, credentials file missing), try fallback
- No retry on auth failure (invalid key/token fails immediately)
- Multiple fallbacks can be chained: `.oauth().fallback(AuthMethod::ApiKeyFromEnv)`

**Type Safety**:
```rust
/// Model selection with escape hatch for new models
pub enum Model {
    Sonnet,
    Opus,
    Haiku,
    Custom(String),
}

/// Permission modes (fixed set from CLI)
pub enum PermissionMode {
    Default,
    Plan,
    AcceptEdits,
    BypassPermissions,
}

/// Newtype for session IDs (prevents string mixups)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SessionId(pub String);

/// Tool name constants (not enum - tools are extensible via MCP)
pub mod tools {
    pub const READ: &str = "Read";
    pub const WRITE: &str = "Write";
    pub const EDIT: &str = "Edit";
    pub const BASH: &str = "Bash";
    pub const GLOB: &str = "Glob";
    pub const GREP: &str = "Grep";
    pub const LS: &str = "LS";
    pub const TASK: &str = "Task";
    // ... etc
}
```

**Builder returns Result**:
```rust
impl ClientConfigBuilder {
    /// Validates config: auth resolvable, CLI exists
    pub fn build(self) -> Result<ClientConfig>;
}
```

**Tests**: Builder pattern tests, auth resolution tests

**Status**: Complete

---

## Stage 3: Process Management

**Goal**: Spawn and communicate with claude CLI subprocess

**Files**:
- `src/process/mod.rs` - Module exports
- `src/process/spawn.rs` - ClaudeProcess struct with spawn(), spawn_continue(), spawn_resume()
- `src/process/io.rs` - ProcessReader (line-by-line JSON), ProcessWriter (stdin for long prompts)

**Process model**:
Each API call spawns a new CLI process. Multi-turn sessions use `--continue` or `--resume` flags to load conversation history from CLI's session storage.

```
Turn 1: claude -p "prompt1" --output-format json
Turn 2: claude -p "prompt2" --output-format json --continue
Turn 3: claude -p "prompt3" --output-format json --resume <session_id>
```

**Input protocol**:
- Prompts passed via `-p "prompt"` argument (primary method)
- For prompts exceeding OS argument length limits (~128KB), fall back to stdin pipe
- Stdin fallback: write prompt bytes, close stdin, read JSON from stdout

**Session continuation**:
- `--continue`: Resume most recent session (no ID needed)
- `--resume <session_id>`: Resume specific session by ID
- Session history managed by CLI, not the library

**Key functionality**:
- Build CLI args from ClientConfig
- Handle stdin/stdout/stderr pipes
- Parse JSON lines from stdout (newline-delimited)
- Detect long prompts and use stdin fallback

**CLI version check**:
```rust
const MIN_CLI_VERSION: &str = "2.0.0";
```
- Lazy check on first spawn via `claude --version`
- If below minimum: `tracing::warn!` but continue
- Let JSON parse errors surface real incompatibilities

**Environment handling**:
- Inherit parent environment by default
- Auth env vars (ANTHROPIC_API_KEY, CLAUDE_CODE_OAUTH_TOKEN) set based on AuthMethod, override inherited
- Custom env vars via `.env()` builder method

**Tests**: Integration tests with real CLI (marked #[ignore])

**Status**: Not started

---

## Stage 4: Streaming

**Goal**: Async Stream implementation for real-time responses

**Files**:
- `src/stream/mod.rs` - Module exports
- `src/stream/events.rs` - High-level StreamEvent enum for consumers
- `src/stream/response.rs` - ResponseStream implementing futures::Stream

**StreamEvent variants**:
```rust
pub enum StreamEvent {
    /// Session metadata (from system/init message). Always emitted first.
    SessionInit(SessionInfo),
    /// Token-by-token text. Always emitted during streaming.
    TextDelta { index: usize, text: String },
    /// Partial tool input JSON. Always emitted during streaming.
    ToolInputDelta { index: usize, partial_json: String },
    /// Completed content block. Always emitted.
    ContentBlockComplete { index: usize, block: ContentBlock },
    /// Full assistant message. Final message always emitted.
    /// Intermediate messages only with `--include-partial-messages`.
    AssistantMessage(AssistantMessage),
    /// Tool execution result (from CLI's built-in tool execution).
    ToolResult(UserMessage),
    /// Final result with cost/usage. Always emitted last on success.
    Complete(ResultMessage),
    /// Incremental usage update. Emitted periodically during streaming.
    UsageUpdate(Usage),
}
```

**CLI flag requirements**:
| Event | Required flag |
|-------|---------------|
| `SessionInit` | Always (with `--output-format json`) |
| `TextDelta`, `ToolInputDelta` | Always (streaming is default) |
| `ContentBlockComplete` | Always |
| `AssistantMessage` (intermediate) | `--include-partial-messages` |
| `AssistantMessage` (final) | Always |
| `ToolResult` | Always (when tools execute) |
| `Complete` | Always |
| `UsageUpdate` | Always |

**Implementation**:
- Background tokio task reads ProcessReader
- Sends events via mpsc channel
- ResponseStream wraps receiver as Stream

**Cancellation & Shutdown**:
- Dropping `ResponseStream` kills the subprocess (RAII pattern)
- Background task watches for receiver drop, terminates process
- Graceful completion: consume stream fully before drop
- On timeout: kill process, return `Error::Timeout`

**Tests**: Mock process tests

**Status**: Not started

---

## Stage 5: Client and Session

**Goal**: High-level API for users

**Files**:
- `src/client.rs` - ClaudeClient, ClientBuilder
- `src/session.rs` - Session for multi-turn conversations

**ClaudeClient API**:
```rust
impl ClaudeClient {
    fn new() -> Result<Self>;                    // uses Auto auth
    fn with_config(config: ClientConfig) -> Self;
    fn builder() -> ClientBuilder;
    async fn send(&self, prompt: &str) -> Result<ResponseStream>;
    async fn send_and_collect(&self, prompt: &str) -> Result<String>;
    async fn start_session(&self, prompt: &str) -> Result<Session>;
    async fn continue_session(&self) -> Result<Session>;
    async fn resume_session(&self, session_id: &SessionId) -> Result<Session>;
}
```

**Session API**:
```rust
impl Session {
    fn session_id(&self) -> &SessionId;
    async fn send(&self, message: &str) -> Result<ResponseStream>;
    async fn send_and_collect(&self, message: &str) -> Result<String>;
    fn total_usage(&self) -> &Usage;
    fn total_cost_usd(&self) -> f64;
}
```

**Thread safety**:
All public types must be `Send + Sync` for use across async tasks. Add compile-time assertions:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn public_types_are_send_sync() {
        assert_send_sync::<ClaudeClient>();
        assert_send_sync::<Session>();
        assert_send_sync::<ClientConfig>();
        assert_send_sync::<ResponseStream>();
    }
}
```

**Tests**: Integration tests, thread safety assertions

**Status**: Not started

---

## Stage 6: Tool Observability

**Goal**: Observe tool calls for logging, monitoring, and integration

**Scope clarification**:
The CLI executes tools automatically (Read, Bash, etc.). There is no mechanism to intercept tool calls for custom execution mid-turn. For custom tools, users should implement MCP servers (separate from this library).

This stage provides **observability only**: callbacks when tools are called and when results are returned.

**Files**:
- `src/tools/mod.rs` - Module exports
- `src/tools/observer.rs` - ToolObserver trait

**ToolObserver trait**:
```rust
/// Observer for tool execution events. Called during stream processing.
/// Implementations must be lightweight; blocking delays stream processing.
pub trait ToolObserver: Send + Sync {
    /// Called when Claude requests a tool call (from assistant message).
    fn on_tool_use(&self, id: &str, name: &str, input: &Value) {}

    /// Called when a tool result is received (from user message).
    fn on_tool_result(&self, tool_use_id: &str, content: &str, is_error: bool) {}
}

/// Simple logging observer implementation.
pub struct LoggingObserver {
    level: tracing::Level,
}
```

**Usage**:
```rust
struct MyObserver;
impl ToolObserver for MyObserver {
    fn on_tool_use(&self, id: &str, name: &str, input: &Value) {
        println!("Tool called: {} with {:?}", name, input);
    }
}

let client = ClaudeClient::builder()
    .tool_observer(Arc::new(MyObserver))
    .build()?;
```

**Future work** (not in v1):
- MCP server helpers for implementing custom tool servers
- Tool call interception (if CLI adds support)

**Tests**: Unit tests for observer dispatch

**Status**: Not started

---

## Stage 7: Public API and Polish

**Goal**: Clean public exports and documentation

**Files**:
- `src/lib.rs` - Re-exports and module declarations

**Re-exports**:
```rust
pub use client::{ClaudeClient, ClientBuilder};
pub use config::auth::{
    AuthMethod, OAuthCredentials,
    login_interactive, setup_token, has_oauth_credentials,
};
pub use config::options::{Model, PermissionMode, SessionId, tools};
pub use error::{Error, Result};
pub use session::Session;
pub use stream::events::StreamEvent;
pub use stream::response::ResponseStream;
pub use tools::observer::{ToolObserver, LoggingObserver};
pub use protocol::{CliMessage, ContentBlock, Usage, ...};
```

**Status**: Not started

---

## Dependencies

**MSRV**: 1.75 (for native async fn in traits, no `async-trait` crate needed)

```toml
[package]
rust-version = "1.75"

[dependencies]
tokio = { version = "1", features = ["process", "io-util", "sync", "macros", "rt-multi-thread"] }
tokio-stream = "0.1"
futures = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tracing = "0.1"
uuid = { version = "1", features = ["v4", "serde"] }
pin-project-lite = "0.2"
dirs = "5"  # for ~/.claude path resolution

[dev-dependencies]
tokio-test = "0.4"
tempfile = "3"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

---

## Module Structure

```
src/
├── lib.rs
├── client.rs
├── session.rs
├── error.rs
├── config/
│   ├── mod.rs
│   ├── auth.rs
│   ├── builder.rs
│   └── options.rs
├── protocol/
│   ├── mod.rs
│   ├── messages.rs
│   ├── content.rs
│   ├── events.rs
│   └── usage.rs
├── stream/
│   ├── mod.rs
│   ├── response.rs
│   └── events.rs
├── process/
│   ├── mod.rs
│   ├── spawn.rs
│   └── io.rs
└── tools/
    ├── mod.rs
    └── observer.rs
```

---

## JSON Protocol Reference

### Message Types

**system** (init):
```json
{
  "type": "system",
  "subtype": "init",
  "cwd": "/path",
  "session_id": "uuid",
  "tools": ["Bash", "Read", "Edit"],
  "model": "claude-opus-4-5-20251101",
  "permissionMode": "default",
  "claude_code_version": "2.0.76"
}
```

**assistant**:
```json
{
  "type": "assistant",
  "message": {
    "id": "msg_xxx",
    "model": "...",
    "role": "assistant",
    "content": [
      {"type": "text", "text": "..."},
      {"type": "tool_use", "id": "toolu_xxx", "name": "Bash", "input": {...}}
    ],
    "stop_reason": "end_turn",
    "usage": {"input_tokens": 100, "output_tokens": 50}
  },
  "session_id": "uuid"
}
```

**user** (tool results):
```json
{
  "type": "user",
  "message": {
    "role": "user",
    "content": [
      {"type": "tool_result", "tool_use_id": "toolu_xxx", "content": "...", "is_error": false}
    ]
  },
  "session_id": "uuid"
}
```

**stream_event**:
```json
{
  "type": "stream_event",
  "event": {
    "type": "content_block_delta",
    "index": 0,
    "delta": {"type": "text_delta", "text": "token"}
  },
  "session_id": "uuid"
}
```

Event subtypes: `message_start`, `content_block_start`, `content_block_delta`, `content_block_stop`, `message_delta`, `message_stop`

**result**:
```json
{
  "type": "result",
  "subtype": "success",
  "is_error": false,
  "duration_ms": 1234,
  "num_turns": 1,
  "result": "final text",
  "total_cost_usd": 0.01,
  "usage": {...}
}
```

---

## Usage Examples

### One-shot
```rust
let client = ClaudeClient::builder()
    .model(Model::Sonnet)
    .max_budget_usd(0.10)
    .build();
let response = client.send_and_collect("What is 2+2?").await?;
```

### Streaming
```rust
let mut stream = client.send("Write a poem").await?;
while let Some(event) = stream.next().await {
    if let StreamEvent::TextDelta { text, .. } = event? {
        print!("{}", text);
    }
}
```

### Multi-turn
```rust
let session = client.start_session("My name is Alice").await?;
let response = session.send_and_collect("What's my name?").await?;
```

### Full configuration
```rust
let client = ClaudeClient::builder()
    .model(Model::Opus)
    .permission_mode(PermissionMode::BypassPermissions)
    .system_prompt("You are a coding assistant.")
    .append_system_prompt("Always use Rust.")
    .allowed_tools(vec!["Read".into(), "Glob".into()])
    .max_budget_usd(5.00)
    .working_directory("/home/user/project")
    .build();
```

### Authentication examples
```rust
// Auto-detect (default): tries OAuth, falls back to ANTHROPIC_API_KEY
let client = ClaudeClient::new();

// Explicit API key
let client = ClaudeClient::builder()
    .api_key("sk-ant-...")
    .build();

// API key from ANTHROPIC_API_KEY env var
let client = ClaudeClient::builder()
    .api_key_from_env()
    .build();

// API key from custom env var (read it yourself)
let client = ClaudeClient::builder()
    .api_key(std::env::var("MY_CLAUDE_KEY")?)
    .build();

// Require OAuth credentials from ~/.claude/ (must have run `claude login`)
let client = ClaudeClient::builder()
    .oauth()
    .build();

// Use long-lived OAuth token (from `claude setup-token`, valid 1 year)
// Best for CI/headless: run setup-token once, store token as secret
let client = ClaudeClient::builder()
    .oauth_token("token-from-setup-token")
    .build();

// Read OAuth token from CLAUDE_CODE_OAUTH_TOKEN env var
let client = ClaudeClient::builder()
    .oauth_token_from_env()
    .build();

// Trigger interactive OAuth login (opens browser)
libclaude::login_interactive().await?;

// Long-lived token setup (opens browser, requires subscription, outputs token for CLAUDE_CODE_OAUTH_TOKEN)
libclaude::setup_token().await?;

// Check if OAuth is already configured
if libclaude::has_oauth_credentials() {
    println!("OAuth ready");
}

// Fallback chain: try OAuth token first, fall back to API key
let client = ClaudeClient::builder()
    .oauth_token_from_env()
    .fallback(AuthMethod::ApiKeyFromEnv)
    .build();
```
