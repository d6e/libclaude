//! Token usage tracking types.

use serde::{Deserialize, Serialize};

/// Token usage statistics from the API.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Usage {
    /// Number of input tokens consumed.
    #[serde(default)]
    pub input_tokens: u64,
    /// Number of output tokens generated.
    #[serde(default)]
    pub output_tokens: u64,
    /// Tokens read from cache (if caching enabled).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cache_read_input_tokens: u64,
    /// Tokens written to cache (if caching enabled).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cache_creation_input_tokens: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl Usage {
    /// Create a new empty Usage.
    pub fn new() -> Self {
        Self::default()
    }

    /// Total input tokens including cache operations.
    pub fn total_input_tokens(&self) -> u64 {
        self.input_tokens + self.cache_read_input_tokens + self.cache_creation_input_tokens
    }

    /// Total tokens (input + output).
    pub fn total_tokens(&self) -> u64 {
        self.total_input_tokens() + self.output_tokens
    }

    /// Accumulate usage from another Usage instance.
    pub fn accumulate(&mut self, other: &Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_input_tokens += other.cache_read_input_tokens;
        self.cache_creation_input_tokens += other.cache_creation_input_tokens;
    }
}

impl std::ops::Add for Usage {
    type Output = Usage;

    fn add(self, other: Usage) -> Usage {
        Usage {
            input_tokens: self.input_tokens + other.input_tokens,
            output_tokens: self.output_tokens + other.output_tokens,
            cache_read_input_tokens: self.cache_read_input_tokens + other.cache_read_input_tokens,
            cache_creation_input_tokens: self.cache_creation_input_tokens
                + other.cache_creation_input_tokens,
        }
    }
}

impl std::ops::AddAssign for Usage {
    fn add_assign(&mut self, other: Usage) {
        self.accumulate(&other);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_usage() {
        let json = r#"{"input_tokens": 100, "output_tokens": 50}"#;
        let usage: Usage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_input_tokens, 0);
        assert_eq!(usage.cache_creation_input_tokens, 0);
    }

    #[test]
    fn parse_usage_with_cache() {
        let json = r#"{
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_read_input_tokens": 1000,
            "cache_creation_input_tokens": 500
        }"#;
        let usage: Usage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_input_tokens, 1000);
        assert_eq!(usage.cache_creation_input_tokens, 500);
    }

    #[test]
    fn parse_empty_object() {
        let json = r#"{}"#;
        let usage: Usage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
    }

    #[test]
    fn total_calculations() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 1000,
            cache_creation_input_tokens: 500,
        };
        assert_eq!(usage.total_input_tokens(), 1600);
        assert_eq!(usage.total_tokens(), 1650);
    }

    #[test]
    fn accumulate_usage() {
        let mut usage1 = Usage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        let usage2 = Usage {
            input_tokens: 200,
            output_tokens: 100,
            cache_read_input_tokens: 50,
            ..Default::default()
        };
        usage1.accumulate(&usage2);
        assert_eq!(usage1.input_tokens, 300);
        assert_eq!(usage1.output_tokens, 150);
        assert_eq!(usage1.cache_read_input_tokens, 50);
    }

    #[test]
    fn add_operator() {
        let usage1 = Usage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        let usage2 = Usage {
            input_tokens: 200,
            output_tokens: 100,
            ..Default::default()
        };
        let sum = usage1 + usage2;
        assert_eq!(sum.input_tokens, 300);
        assert_eq!(sum.output_tokens, 150);
    }

    #[test]
    fn serialize_skips_zero_cache() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        };
        let json = serde_json::to_string(&usage).unwrap();
        assert!(!json.contains("cache_read"));
        assert!(!json.contains("cache_creation"));
    }

    #[test]
    fn usage_new() {
        let usage = Usage::new();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.cache_read_input_tokens, 0);
        assert_eq!(usage.cache_creation_input_tokens, 0);
    }

    #[test]
    fn usage_add_assign() {
        let mut usage1 = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 10,
            cache_creation_input_tokens: 5,
        };
        let usage2 = Usage {
            input_tokens: 200,
            output_tokens: 100,
            cache_read_input_tokens: 20,
            cache_creation_input_tokens: 10,
        };
        usage1 += usage2;
        assert_eq!(usage1.input_tokens, 300);
        assert_eq!(usage1.output_tokens, 150);
        assert_eq!(usage1.cache_read_input_tokens, 30);
        assert_eq!(usage1.cache_creation_input_tokens, 15);
    }

    #[test]
    fn usage_equality() {
        let usage1 = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        };
        let usage2 = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        };
        assert_eq!(usage1, usage2);
    }

    #[test]
    fn usage_clone() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 10,
            cache_creation_input_tokens: 5,
        };
        let cloned = usage.clone();
        assert_eq!(usage, cloned);
    }

    #[test]
    fn serialize_includes_non_zero_cache() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 10,
            cache_creation_input_tokens: 5,
        };
        let json = serde_json::to_string(&usage).unwrap();
        assert!(json.contains("cache_read_input_tokens"));
        assert!(json.contains("cache_creation_input_tokens"));
    }
}
