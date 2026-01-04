//! Content block types for messages.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A content block within a message.
///
/// Content blocks can be text, tool use requests, or tool results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text content.
    Text(TextBlock),
    /// A tool use request from the assistant.
    ToolUse(ToolUseBlock),
    /// A tool result returned to the assistant.
    ToolResult(ToolResultBlock),
    /// Thinking block (extended thinking feature).
    Thinking(ThinkingBlock),
}

/// Plain text content block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextBlock {
    /// The text content.
    pub text: String,
}

/// A tool use request from the assistant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolUseBlock {
    /// Unique identifier for this tool use.
    pub id: String,
    /// Name of the tool being invoked.
    pub name: String,
    /// Input parameters as JSON object.
    pub input: Value,
}

/// A tool result returned to the assistant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultBlock {
    /// ID of the tool_use this result corresponds to.
    pub tool_use_id: String,
    /// The result content (can be string or structured).
    #[serde(default)]
    pub content: ToolResultContent,
    /// Whether the tool execution resulted in an error.
    #[serde(default)]
    pub is_error: bool,
}

/// Content of a tool result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    /// Simple string result.
    Text(String),
    /// Structured result with multiple content blocks.
    Blocks(Vec<ToolResultContentBlock>),
}

impl Default for ToolResultContent {
    fn default() -> Self {
        ToolResultContent::Text(String::new())
    }
}

impl ToolResultContent {
    /// Get the content as a string (concatenates text blocks if structured).
    pub fn as_text(&self) -> String {
        match self {
            ToolResultContent::Text(s) => s.clone(),
            ToolResultContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ToolResultContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

/// A content block within a tool result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResultContentBlock {
    /// Text content.
    Text {
        /// The text.
        text: String,
    },
    /// Image content (base64 encoded).
    Image {
        /// Base64 encoded image data.
        source: ImageSource,
    },
}

/// Image source data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageSource {
    /// Source type (always "base64" for tool results).
    #[serde(rename = "type")]
    pub source_type: String,
    /// Media type (e.g., "image/png").
    pub media_type: String,
    /// Base64 encoded image data.
    pub data: String,
}

/// Thinking block for extended thinking feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThinkingBlock {
    /// The thinking text (may be redacted).
    pub thinking: String,
}

impl ContentBlock {
    /// Check if this is a text block.
    pub fn is_text(&self) -> bool {
        matches!(self, ContentBlock::Text(_))
    }

    /// Check if this is a tool use block.
    pub fn is_tool_use(&self) -> bool {
        matches!(self, ContentBlock::ToolUse(_))
    }

    /// Check if this is a tool result block.
    pub fn is_tool_result(&self) -> bool {
        matches!(self, ContentBlock::ToolResult(_))
    }

    /// Get as text block if applicable.
    pub fn as_text(&self) -> Option<&TextBlock> {
        match self {
            ContentBlock::Text(t) => Some(t),
            _ => None,
        }
    }

    /// Get as tool use block if applicable.
    pub fn as_tool_use(&self) -> Option<&ToolUseBlock> {
        match self {
            ContentBlock::ToolUse(t) => Some(t),
            _ => None,
        }
    }

    /// Get as tool result block if applicable.
    pub fn as_tool_result(&self) -> Option<&ToolResultBlock> {
        match self {
            ContentBlock::ToolResult(t) => Some(t),
            _ => None,
        }
    }

    /// Extract the text content if this is a text block.
    pub fn text(&self) -> Option<&str> {
        self.as_text().map(|t| t.text.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_text_block() {
        let json = r#"{"type": "text", "text": "Hello, world!"}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        assert!(block.is_text());
        assert_eq!(block.text(), Some("Hello, world!"));
    }

    #[test]
    fn parse_tool_use_block() {
        let json = r#"{
            "type": "tool_use",
            "id": "toolu_01234",
            "name": "Bash",
            "input": {"command": "ls -la"}
        }"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        assert!(block.is_tool_use());
        let tool_use = block.as_tool_use().unwrap();
        assert_eq!(tool_use.id, "toolu_01234");
        assert_eq!(tool_use.name, "Bash");
        assert_eq!(tool_use.input["command"], "ls -la");
    }

    #[test]
    fn parse_tool_result_string() {
        let json = r#"{
            "type": "tool_result",
            "tool_use_id": "toolu_01234",
            "content": "file1.txt\nfile2.txt",
            "is_error": false
        }"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        assert!(block.is_tool_result());
        let result = block.as_tool_result().unwrap();
        assert_eq!(result.tool_use_id, "toolu_01234");
        assert_eq!(result.content.as_text(), "file1.txt\nfile2.txt");
        assert!(!result.is_error);
    }

    #[test]
    fn parse_tool_result_error() {
        let json = r#"{
            "type": "tool_result",
            "tool_use_id": "toolu_01234",
            "content": "command not found",
            "is_error": true
        }"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        let result = block.as_tool_result().unwrap();
        assert!(result.is_error);
    }

    #[test]
    fn parse_tool_result_structured() {
        let json = r#"{
            "type": "tool_result",
            "tool_use_id": "toolu_01234",
            "content": [
                {"type": "text", "text": "Result:"},
                {"type": "text", "text": "Success"}
            ]
        }"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        let result = block.as_tool_result().unwrap();
        assert_eq!(result.content.as_text(), "Result:\nSuccess");
    }

    #[test]
    fn parse_thinking_block() {
        let json = r#"{"type": "thinking", "thinking": "Let me analyze this..."}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        match block {
            ContentBlock::Thinking(t) => {
                assert_eq!(t.thinking, "Let me analyze this...");
            }
            _ => panic!("Expected thinking block"),
        }
    }

    #[test]
    fn serialize_text_block() {
        let block = ContentBlock::Text(TextBlock {
            text: "Hello".into(),
        });
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""type":"text""#));
        assert!(json.contains(r#""text":"Hello""#));
    }

    #[test]
    fn serialize_tool_use_block() {
        let block = ContentBlock::ToolUse(ToolUseBlock {
            id: "toolu_123".into(),
            name: "Read".into(),
            input: serde_json::json!({"path": "/tmp/test"}),
        });
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""type":"tool_use""#));
        assert!(json.contains(r#""id":"toolu_123""#));
        assert!(json.contains(r#""name":"Read""#));
    }
}
