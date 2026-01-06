//! Tool observer trait and implementations.

use serde_json::Value;

/// Observer for tool execution events.
///
/// Implementations receive callbacks when Claude calls tools and when
/// tool results are received. This is for observation only; the CLI
/// executes tools automatically.
///
/// # Implementation Notes
///
/// - Implementations must be lightweight; blocking delays stream processing.
/// - Methods have default empty implementations for selective observation.
/// - Observers are called synchronously during stream processing.
///
/// # Example
///
/// ```ignore
/// use libclaude::ToolObserver;
/// use serde_json::Value;
///
/// struct MetricsObserver {
///     tool_calls: std::sync::atomic::AtomicUsize,
/// }
///
/// impl ToolObserver for MetricsObserver {
///     fn on_tool_use(&self, _id: &str, name: &str, _input: &Value) {
///         self.tool_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
///         println!("Tool call count: {}", self.tool_calls.load(std::sync::atomic::Ordering::Relaxed));
///     }
/// }
/// ```
pub trait ToolObserver: Send + Sync {
    /// Called when Claude requests a tool call (from assistant message).
    ///
    /// This is called when a complete tool_use content block is received,
    /// after all streaming deltas have been accumulated.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for this tool use (e.g., "toolu_01234")
    /// * `name` - Name of the tool being invoked (e.g., "Read", "Bash")
    /// * `input` - Input parameters as JSON object
    fn on_tool_use(&self, id: &str, name: &str, input: &Value) {
        let _ = (id, name, input);
    }

    /// Called when a tool result is received (from user message).
    ///
    /// This is called when the CLI returns a tool execution result.
    ///
    /// # Arguments
    ///
    /// * `tool_use_id` - ID of the tool_use this result corresponds to
    /// * `content` - The result content (may be truncated for large outputs)
    /// * `is_error` - Whether the tool execution resulted in an error
    fn on_tool_result(&self, tool_use_id: &str, content: &str, is_error: bool) {
        let _ = (tool_use_id, content, is_error);
    }
}

/// Simple logging observer that logs tool events using tracing.
///
/// # Example
///
/// ```ignore
/// use libclaude::{ClaudeClient, LoggingObserver};
/// use std::sync::Arc;
///
/// let client = ClaudeClient::builder()
///     .api_key("sk-ant-...")
///     .tool_observer(Arc::new(LoggingObserver::new()))
///     .build()?;
/// ```
#[derive(Debug, Clone, Default)]
pub struct LoggingObserver {
    level: LogLevel,
}

/// Log level for LoggingObserver.
#[derive(Debug, Clone, Copy, Default)]
pub enum LogLevel {
    /// Log at trace level.
    Trace,
    /// Log at debug level (default).
    #[default]
    Debug,
    /// Log at info level.
    Info,
}

impl LoggingObserver {
    /// Create a new logging observer with debug level.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a logging observer with a specific level.
    pub fn with_level(level: LogLevel) -> Self {
        Self { level }
    }
}

impl ToolObserver for LoggingObserver {
    fn on_tool_use(&self, id: &str, name: &str, input: &Value) {
        match self.level {
            LogLevel::Trace => {
                tracing::trace!(tool_id = %id, tool_name = %name, ?input, "tool_use");
            }
            LogLevel::Debug => {
                tracing::debug!(tool_id = %id, tool_name = %name, ?input, "tool_use");
            }
            LogLevel::Info => {
                tracing::info!(tool_id = %id, tool_name = %name, ?input, "tool_use");
            }
        }
    }

    fn on_tool_result(&self, tool_use_id: &str, content: &str, is_error: bool) {
        // Truncate content for logging
        let display_content = if content.len() > 200 {
            format!("{}... ({} bytes total)", &content[..200], content.len())
        } else {
            content.to_string()
        };

        match self.level {
            LogLevel::Trace => {
                tracing::trace!(
                    tool_use_id = %tool_use_id,
                    is_error = %is_error,
                    content = %display_content,
                    "tool_result"
                );
            }
            LogLevel::Debug => {
                tracing::debug!(
                    tool_use_id = %tool_use_id,
                    is_error = %is_error,
                    content = %display_content,
                    "tool_result"
                );
            }
            LogLevel::Info => {
                tracing::info!(
                    tool_use_id = %tool_use_id,
                    is_error = %is_error,
                    content = %display_content,
                    "tool_result"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn tool_observer_is_send_sync() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn ToolObserver>();
        assert_send_sync::<LoggingObserver>();
    }

    #[test]
    fn logging_observer_is_clone() {
        fn assert_clone<T: Clone>() {}
        assert_clone::<LoggingObserver>();
    }

    struct CountingObserver {
        tool_uses: AtomicUsize,
        tool_results: AtomicUsize,
    }

    impl CountingObserver {
        fn new() -> Self {
            Self {
                tool_uses: AtomicUsize::new(0),
                tool_results: AtomicUsize::new(0),
            }
        }
    }

    impl ToolObserver for CountingObserver {
        fn on_tool_use(&self, _id: &str, _name: &str, _input: &Value) {
            self.tool_uses.fetch_add(1, Ordering::Relaxed);
        }

        fn on_tool_result(&self, _tool_use_id: &str, _content: &str, _is_error: bool) {
            self.tool_results.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn counting_observer_tracks_calls() {
        let observer = CountingObserver::new();

        observer.on_tool_use("tool_1", "Read", &serde_json::json!({"path": "/tmp/test"}));
        observer.on_tool_use("tool_2", "Bash", &serde_json::json!({"command": "ls"}));
        observer.on_tool_result("tool_1", "file contents", false);

        assert_eq!(observer.tool_uses.load(Ordering::Relaxed), 2);
        assert_eq!(observer.tool_results.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn default_trait_methods_are_no_ops() {
        struct EmptyObserver;
        impl ToolObserver for EmptyObserver {}

        let observer = EmptyObserver;
        // These should not panic
        observer.on_tool_use("id", "name", &serde_json::json!({}));
        observer.on_tool_result("id", "content", false);
    }

    #[test]
    fn arc_observer_works() {
        let observer: Arc<dyn ToolObserver> = Arc::new(CountingObserver::new());
        observer.on_tool_use("id", "Read", &serde_json::json!({}));
    }

    #[test]
    fn logging_observer_new_creates_default() {
        let observer = LoggingObserver::new();
        assert!(matches!(observer.level, LogLevel::Debug));
    }

    #[test]
    fn logging_observer_with_level() {
        let trace = LoggingObserver::with_level(LogLevel::Trace);
        assert!(matches!(trace.level, LogLevel::Trace));

        let info = LoggingObserver::with_level(LogLevel::Info);
        assert!(matches!(info.level, LogLevel::Info));
    }

    #[test]
    fn log_level_default_is_debug() {
        let level = LogLevel::default();
        assert!(matches!(level, LogLevel::Debug));
    }

    #[test]
    fn logging_observer_on_tool_use_all_levels() {
        let input = serde_json::json!({"path": "/test"});

        // Test all log levels (they just call tracing macros which are no-ops without subscriber)
        let trace_obs = LoggingObserver::with_level(LogLevel::Trace);
        trace_obs.on_tool_use("tool_1", "Read", &input);

        let debug_obs = LoggingObserver::with_level(LogLevel::Debug);
        debug_obs.on_tool_use("tool_2", "Write", &input);

        let info_obs = LoggingObserver::with_level(LogLevel::Info);
        info_obs.on_tool_use("tool_3", "Bash", &input);
    }

    #[test]
    fn logging_observer_on_tool_result_all_levels() {
        // Test all log levels for tool_result
        let trace_obs = LoggingObserver::with_level(LogLevel::Trace);
        trace_obs.on_tool_result("tool_1", "short result", false);

        let debug_obs = LoggingObserver::with_level(LogLevel::Debug);
        debug_obs.on_tool_result("tool_2", "another result", true);

        let info_obs = LoggingObserver::with_level(LogLevel::Info);
        info_obs.on_tool_result("tool_3", "info result", false);
    }

    #[test]
    fn logging_observer_on_tool_result_truncates_long_content() {
        let long_content = "x".repeat(300);

        // This should trigger the truncation branch (content > 200 chars)
        let observer = LoggingObserver::new();
        observer.on_tool_result("tool_long", &long_content, false);

        // Test with exactly 200 chars (no truncation)
        let exactly_200 = "y".repeat(200);
        observer.on_tool_result("tool_200", &exactly_200, false);

        // Test with 201 chars (triggers truncation)
        let over_200 = "z".repeat(201);
        observer.on_tool_result("tool_201", &over_200, false);
    }

    #[test]
    fn logging_observer_on_tool_result_with_error() {
        let observer = LoggingObserver::new();
        observer.on_tool_result("tool_err", "error message", true);
    }
}
