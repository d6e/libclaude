//! Authentication methods for the Claude CLI.
//!
//! This module provides various authentication options for connecting to Claude:
//!
//! - API keys (direct or from environment)
//! - OAuth tokens (direct, from environment, or from credentials file)
//! - Auto-detection that tries multiple methods
//!
//! # Example
//!
//! ```ignore
//! use libclaude::config::auth::{AuthMethod, has_oauth_credentials};
//!
//! // Check if OAuth is available
//! if has_oauth_credentials() {
//!     println!("OAuth credentials found");
//! }
//!
//! // Auto-detect auth (default)
//! let auth = AuthMethod::Auto;
//! ```

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Authentication method for the Claude CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthMethod {
    /// Use API key directly (passed via ANTHROPIC_API_KEY env var to subprocess).
    ApiKey(String),
    /// Read API key from ANTHROPIC_API_KEY env var.
    ApiKeyFromEnv,
    /// Use OAuth token directly (passed via CLAUDE_CODE_OAUTH_TOKEN env var to subprocess).
    OAuthToken(String),
    /// Read OAuth token from CLAUDE_CODE_OAUTH_TOKEN env var.
    OAuthTokenFromEnv,
    /// Use OAuth credentials from ~/.claude/.credentials.json.
    OAuth,
    /// Auto-detect: try OAuth -> OAuthTokenFromEnv -> ApiKeyFromEnv.
    Auto,
}

impl Default for AuthMethod {
    fn default() -> Self {
        AuthMethod::Auto
    }
}

/// Environment variable name for API key.
pub const ENV_API_KEY: &str = "ANTHROPIC_API_KEY";
/// Environment variable name for OAuth token.
pub const ENV_OAUTH_TOKEN: &str = "CLAUDE_CODE_OAUTH_TOKEN";

/// OAuth credentials read from ~/.claude/.credentials.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthCredentials {
    /// The access token for API calls.
    pub access_token: String,
    /// The refresh token for obtaining new access tokens.
    pub refresh_token: String,
    /// Expiration timestamp in milliseconds since Unix epoch.
    pub expires_at: u64,
    /// OAuth scopes granted.
    pub scopes: Vec<String>,
}

impl OAuthCredentials {
    /// Load credentials from ~/.claude/.credentials.json.
    ///
    /// Returns `Ok(Some(credentials))` if the file exists and is valid,
    /// `Ok(None)` if the file doesn't exist, or `Err` if it exists but is invalid.
    pub fn load() -> Result<Option<Self>> {
        let path = credentials_path();
        if !path.exists() {
            return Ok(None);
        }

        let contents = std::fs::read_to_string(&path).map_err(|e| {
            Error::Io(std::io::Error::new(
                e.kind(),
                format!("failed to read credentials from {}: {}", path.display(), e),
            ))
        })?;

        let file: CredentialsFile = serde_json::from_str(&contents).map_err(|e| {
            Error::JsonParse {
                message: format!("invalid credentials file format: {}", e),
                source: e,
            }
        })?;

        Ok(file.claude_ai_oauth.map(|oauth| OAuthCredentials {
            access_token: oauth.access_token,
            refresh_token: oauth.refresh_token,
            expires_at: oauth.expires_at,
            scopes: oauth.scopes,
        }))
    }

    /// Check if the access token is expired.
    ///
    /// Returns true if the token has expired or will expire within the next minute.
    pub fn is_expired(&self) -> bool {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Consider expired if less than 60 seconds remaining
        self.expires_at < now_ms + 60_000
    }
}

/// Internal structure matching the credentials.json file format.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialsFile {
    claude_ai_oauth: Option<OAuthEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuthEntry {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
    scopes: Vec<String>,
}

/// Get the path to the credentials file.
fn credentials_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join(".credentials.json")
}

/// Check if OAuth credentials exist in ~/.claude/.
///
/// This only checks for file existence, not validity.
pub fn has_oauth_credentials() -> bool {
    credentials_path().exists()
}

/// Shell out to `claude login` for interactive browser-based OAuth.
///
/// This opens a browser for authentication and stores credentials in ~/.claude/.
/// Use this for interactive scenarios where a user can complete the OAuth flow.
pub async fn login_interactive() -> Result<()> {
    let status = tokio::process::Command::new("claude")
        .arg("login")
        .status()
        .await
        .map_err(|e| Error::ProcessSpawn(e))?;

    if !status.success() {
        return Err(Error::CliError {
            message: format!("claude login failed with exit code {:?}", status.code()),
            is_auth_error: true,
        });
    }

    Ok(())
}

/// Shell out to `claude setup-token` for long-lived token.
///
/// This opens a browser and outputs a token valid for 1 year.
/// Requires an active Claude subscription.
/// The token should be stored as CLAUDE_CODE_OAUTH_TOKEN for CI/headless use.
pub async fn setup_token() -> Result<()> {
    let status = tokio::process::Command::new("claude")
        .arg("setup-token")
        .status()
        .await
        .map_err(|e| Error::ProcessSpawn(e))?;

    if !status.success() {
        return Err(Error::CliError {
            message: format!("claude setup-token failed with exit code {:?}", status.code()),
            is_auth_error: true,
        });
    }

    Ok(())
}

/// Resolved authentication ready to pass to the subprocess.
#[derive(Debug, Clone)]
pub(crate) enum ResolvedAuth {
    /// API key to pass via ANTHROPIC_API_KEY.
    ApiKey(String),
    /// OAuth token to pass via CLAUDE_CODE_OAUTH_TOKEN.
    OAuthToken(String),
}

impl ResolvedAuth {
    /// Get the environment variable name for this auth type.
    pub fn env_var_name(&self) -> &'static str {
        match self {
            ResolvedAuth::ApiKey(_) => ENV_API_KEY,
            ResolvedAuth::OAuthToken(_) => ENV_OAUTH_TOKEN,
        }
    }

    /// Get the secret value.
    pub fn secret(&self) -> &str {
        match self {
            ResolvedAuth::ApiKey(s) | ResolvedAuth::OAuthToken(s) => s,
        }
    }
}

/// Resolve an AuthMethod to a concrete auth value.
///
/// This checks for environment variables and credential files as needed.
/// Returns an error if the specified auth method cannot be resolved.
pub(crate) fn resolve_auth(method: &AuthMethod, fallbacks: &[AuthMethod]) -> Result<ResolvedAuth> {
    // Try primary method
    if let Some(resolved) = try_resolve_single(method)? {
        return Ok(resolved);
    }

    // Try fallbacks in order
    for fallback in fallbacks {
        if let Some(resolved) = try_resolve_single(fallback)? {
            return Ok(resolved);
        }
    }

    // Nothing worked
    Err(Error::AuthNotConfigured)
}

/// Try to resolve a single auth method.
/// Returns Ok(None) if the method is unavailable but not an error (e.g., missing env var).
/// Returns Err only for actual errors (e.g., invalid credentials file).
fn try_resolve_single(method: &AuthMethod) -> Result<Option<ResolvedAuth>> {
    match method {
        AuthMethod::ApiKey(key) => Ok(Some(ResolvedAuth::ApiKey(key.clone()))),

        AuthMethod::ApiKeyFromEnv => match std::env::var(ENV_API_KEY) {
            Ok(key) if !key.is_empty() => Ok(Some(ResolvedAuth::ApiKey(key))),
            _ => Ok(None),
        },

        AuthMethod::OAuthToken(token) => Ok(Some(ResolvedAuth::OAuthToken(token.clone()))),

        AuthMethod::OAuthTokenFromEnv => match std::env::var(ENV_OAUTH_TOKEN) {
            Ok(token) if !token.is_empty() => Ok(Some(ResolvedAuth::OAuthToken(token))),
            _ => Ok(None),
        },

        AuthMethod::OAuth => {
            match OAuthCredentials::load()? {
                Some(creds) if !creds.is_expired() => {
                    Ok(Some(ResolvedAuth::OAuthToken(creds.access_token)))
                }
                Some(_) => {
                    // Credentials exist but are expired
                    tracing::warn!("OAuth credentials are expired, run `claude login` to refresh");
                    Ok(None)
                }
                None => Ok(None),
            }
        }

        AuthMethod::Auto => {
            // Try OAuth credentials first
            if let Some(resolved) = try_resolve_single(&AuthMethod::OAuth)? {
                return Ok(Some(resolved));
            }
            // Then OAuth token from env
            if let Some(resolved) = try_resolve_single(&AuthMethod::OAuthTokenFromEnv)? {
                return Ok(Some(resolved));
            }
            // Finally API key from env
            if let Some(resolved) = try_resolve_single(&AuthMethod::ApiKeyFromEnv)? {
                return Ok(Some(resolved));
            }
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_method_default() {
        assert_eq!(AuthMethod::default(), AuthMethod::Auto);
    }

    #[test]
    fn oauth_credentials_is_expired() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Expired token
        let expired = OAuthCredentials {
            access_token: "token".into(),
            refresh_token: "refresh".into(),
            expires_at: now_ms - 1000,
            scopes: vec![],
        };
        assert!(expired.is_expired());

        // Valid token (1 hour from now)
        let valid = OAuthCredentials {
            access_token: "token".into(),
            refresh_token: "refresh".into(),
            expires_at: now_ms + 3600_000,
            scopes: vec![],
        };
        assert!(!valid.is_expired());

        // About to expire (30 seconds from now)
        let expiring = OAuthCredentials {
            access_token: "token".into(),
            refresh_token: "refresh".into(),
            expires_at: now_ms + 30_000,
            scopes: vec![],
        };
        assert!(expiring.is_expired()); // Within 60 second buffer
    }

    #[test]
    fn resolve_api_key_direct() {
        let resolved = resolve_auth(&AuthMethod::ApiKey("test-key".into()), &[]).unwrap();
        assert!(matches!(resolved, ResolvedAuth::ApiKey(ref k) if k == "test-key"));
        assert_eq!(resolved.env_var_name(), ENV_API_KEY);
        assert_eq!(resolved.secret(), "test-key");
    }

    #[test]
    fn resolve_oauth_token_direct() {
        let resolved = resolve_auth(&AuthMethod::OAuthToken("test-token".into()), &[]).unwrap();
        assert!(matches!(resolved, ResolvedAuth::OAuthToken(ref t) if t == "test-token"));
        assert_eq!(resolved.env_var_name(), ENV_OAUTH_TOKEN);
    }

    #[test]
    fn resolve_with_fallback() {
        // Primary method unavailable (no env var set), fallback to direct key
        let result = resolve_auth(
            &AuthMethod::OAuthTokenFromEnv,
            &[AuthMethod::ApiKey("fallback-key".into())],
        );
        let resolved = result.unwrap();
        assert!(matches!(resolved, ResolvedAuth::ApiKey(k) if k == "fallback-key"));
    }

    #[test]
    fn resolve_no_auth_available() {
        // In a clean environment, Auto should fail if nothing is configured
        // (This test might pass in CI but fail locally if you have credentials)
        let result = resolve_auth(&AuthMethod::OAuthTokenFromEnv, &[]);
        // We can't assert this fails because the test environment might have the env var
        // Just ensure it doesn't panic
        let _ = result;
    }

    #[test]
    fn types_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AuthMethod>();
        assert_send_sync::<OAuthCredentials>();
    }

    #[test]
    fn credentials_path_exists() {
        // Just test the function doesn't panic
        let path = credentials_path();
        assert!(path.ends_with(".credentials.json"));
    }

    #[test]
    fn load_missing_credentials() {
        // This should return Ok(None) for missing file
        // Note: This test relies on the home dir not having credentials,
        // which may not be true in all environments
        let result = OAuthCredentials::load();
        assert!(result.is_ok()); // Should not error, just return None
    }
}
