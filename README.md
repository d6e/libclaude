# libclaude

[![CI](https://github.com/anthropics/libclaude/actions/workflows/ci.yml/badge.svg)](https://github.com/anthropics/libclaude/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/anthropics/libclaude/graph/badge.svg)](https://codecov.io/gh/anthropics/libclaude)

Async Rust wrapper for the Claude Code CLI.

libclaude provides a typed, ergonomic interface for interacting with Claude from Rust applications. It handles subprocess management, JSON protocol communication, streaming responses, and multi-turn conversations.

## Features

- **Async-first**: Built on Tokio for non-blocking operations
- **Type-safe**: Strong typing for models, permissions, session IDs, and tool names
- **Streaming**: Async iterator over response events with text deltas, tool calls, and usage updates
- **Sessions**: Multi-turn conversations with automatic history tracking
- **Configurable**: Authentication, models, permissions, budgets, tools, and MCP support
- **Observable**: Hook into tool execution for logging and metrics
- **Thread-safe**: All types are `Send + Sync`

## Requirements

- Rust 1.75+
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) 2.0.0+

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
libclaude = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

## Quick Start

### One-shot Request

```rust
use libclaude::{ClaudeClient, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let client = ClaudeClient::new()?;
    let response = client.send_and_collect("What is 2+2?").await?;
    println!("{}", response);
    Ok(())
}
```

### Streaming

```rust
use libclaude::{ClaudeClient, StreamEvent, Result};
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    let client = ClaudeClient::new()?;
    let mut stream = client.send("Write a haiku about Rust").await?;

    while let Some(event) = stream.next().await {
        if let StreamEvent::TextDelta { text, .. } = event? {
            print!("{}", text);
        }
    }
    Ok(())
}
```

### Multi-turn Sessions

```rust
use libclaude::{ClaudeClient, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let client = ClaudeClient::new()?;

    let session = client.start_session("My name is Alice").await?;
    let response = session.send_and_collect("What's my name?").await?;
    println!("{}", response); // "Your name is Alice"

    // Resume later with the session ID
    let session_id = session.session_id().unwrap().clone();
    let resumed = client.resume_session(session_id, "Do you remember me?").await?;
    Ok(())
}
```

## Configuration

Use the builder pattern for full control:

```rust
use libclaude::{ClaudeClient, Model, PermissionMode, Result};
use std::time::Duration;

let client = ClaudeClient::builder()
    // Authentication
    .api_key("sk-ant-...")
    // Or: .oauth(oauth_token, refresh_token, expires_at)
    // Or: .auth_auto() (default - tries OAuth then env vars)

    // Model selection
    .model(Model::Opus)
    // Or: .model(Model::Sonnet), .model(Model::Haiku)
    // Or: .model(Model::Custom("claude-3-5-sonnet-20241022".into()))

    // Permissions
    .permission_mode(PermissionMode::AcceptEdits)

    // System prompt
    .system_prompt("You are a helpful coding assistant.")
    // Or append to default: .append_system_prompt("Additional context...")

    // Tool configuration
    .allowed_tools(vec!["Read".into(), "Glob".into(), "Grep".into()])
    // Or block specific tools: .disallowed_tools(vec!["Bash".into()])

    // Budget limit
    .max_budget_usd(5.00)

    // Process settings
    .working_directory("/path/to/project")
    .timeout(Duration::from_secs(300))
    .env("CUSTOM_VAR", "value")

    .build()?;
```

### Permission Modes

| Mode | Description |
|------|-------------|
| `Default` | Normal interactive mode |
| `Plan` | Planning only, no file modifications |
| `AcceptEdits` | Auto-accept file edits |
| `BypassPermissions` | Skip all permission prompts (use with caution) |

### Built-in Tool Constants

```rust
use libclaude::tool;

let tools = vec![
    tool::READ,
    tool::WRITE,
    tool::EDIT,
    tool::BASH,
    tool::GLOB,
    tool::GREP,
    tool::WEB_FETCH,
    tool::WEB_SEARCH,
    // ... and more
];
```

## Streaming Events

The `ResponseStream` yields these events:

| Event | Description |
|-------|-------------|
| `SessionInit` | Session metadata at start |
| `TextDelta` | Incremental text output |
| `ToolInputDelta` | Partial JSON for tool input |
| `ThinkingDelta` | Extended thinking content |
| `ContentBlockComplete` | A content block finished |
| `AssistantMessage` | Complete assistant message |
| `ToolResult` | Result from tool execution |
| `UsageUpdate` | Token usage update |
| `Complete` | Final result with total cost/usage |

## Tool Observation

Monitor tool execution without intercepting it:

```rust
use libclaude::{ClaudeClient, ToolObserver};
use serde_json::Value;
use std::sync::Arc;

struct MyObserver;

impl ToolObserver for MyObserver {
    fn on_tool_use(&self, id: &str, name: &str, input: &Value) {
        println!("Tool called: {} with input: {}", name, input);
    }

    fn on_tool_result(&self, id: &str, output: &str, is_error: bool) {
        println!("Tool result (error={}): {}", is_error, output);
    }
}

let client = ClaudeClient::builder()
    .tool_observer(Arc::new(MyObserver))
    .build()?;
```

A built-in `LoggingObserver` is available for tracing integration:

```rust
use libclaude::{ClaudeClient, LoggingObserver, LogLevel};

let client = ClaudeClient::builder()
    .tool_observer(Arc::new(LoggingObserver::new(LogLevel::Debug)))
    .build()?;
```

## Error Handling

```rust
use libclaude::{Error, Result};

match client.send_and_collect("prompt").await {
    Ok(response) => println!("{}", response),
    Err(e) => {
        if e.is_auth_error() {
            eprintln!("Authentication failed: {}", e);
        } else if e.is_retryable() {
            eprintln!("Transient error, retry: {}", e);
        } else {
            eprintln!("Error: {}", e);
        }
    }
}
```

## Usage and Cost Tracking

```rust
let session = client.start_session("Hello").await?;
session.send_and_collect("How are you?").await?;

// Get cumulative usage
let usage = session.total_usage();
println!("Input tokens: {}", usage.input_tokens);
println!("Output tokens: {}", usage.output_tokens);
println!("Cache read: {}", usage.cache_read_input_tokens);
println!("Cache write: {}", usage.cache_creation_input_tokens);

// Get total cost
let cost = session.total_cost_usd();
println!("Total cost: ${:.4}", cost);
```

## Module Overview

| Module | Purpose |
|--------|---------|
| `client` | `ClaudeClient` for sending prompts and managing sessions |
| `session` | `Session` for multi-turn conversations |
| `config` | Configuration builder, authentication, type-safe options |
| `process` | CLI subprocess spawning and I/O |
| `protocol` | JSON message types for CLI communication |
| `stream` | Streaming events and response handling |
| `tools` | Tool observation trait |
| `error` | Error types |

## License

MIT
