// SSH authentication helpers.
//
// This module provides shared authentication logic used by both
// the SSH client and SFTP client.

use anyhow::{Context, Result};
use russh::client::{AuthResult, Handle, Handler};
use rustius_core::connection::AuthMethod;
use std::sync::Arc;

/// Determine the authentication strategy based on the AuthMethod.
pub fn describe_auth_method(method: &AuthMethod) -> &'static str {
    match method {
        AuthMethod::Password { .. } => "password",
        AuthMethod::KeyFile { .. } => "public-key",
        AuthMethod::Agent { .. } => "ssh-agent",
    }
}

/// Authenticate an active russh client handle against the specified `AuthMethod`.
pub async fn authenticate<H: Handler>(handle: &mut Handle<H>, auth: &AuthMethod) -> Result<()> {
    tracing::debug!(
        "Authenticating using {} strategy",
        describe_auth_method(auth)
    );
    match auth {
        AuthMethod::Password {
            username, password, ..
        } => {
            let pass = password.as_deref().unwrap_or("");
            let res = handle.authenticate_password(username, pass).await?;
            if !matches!(res, AuthResult::Success) {
                anyhow::bail!("Password authentication failed for user {}", username);
            }
        }
        AuthMethod::KeyFile {
            username,
            key_path,
            passphrase,
            ..
        } => {
            let resolved_path = rustius_core::connection::resolve_key_path(key_path, None);
            let key = russh::keys::load_secret_key(&resolved_path, passphrase.as_deref()).context(
                format!("Failed to load key from {}", resolved_path.display()),
            )?;
            let key_with_alg = russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), None);
            let res = handle
                .authenticate_publickey(username, key_with_alg)
                .await?;
            if !matches!(res, AuthResult::Success) {
                anyhow::bail!("Public key authentication failed for user {}", username);
            }
        }
        AuthMethod::Agent { .. } => {
            anyhow::bail!("SSH agent authentication not yet implemented");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_describe_auth_method() {
        let pw = AuthMethod::Password {
            username: "admin".into(),
            password: Some("secret".into()),
            secret_ref: None,
        };
        assert_eq!(describe_auth_method(&pw), "password");

        let kf = AuthMethod::KeyFile {
            username: "root".into(),
            key_path: "/home/user/.ssh/id_rsa".into(),
            passphrase: None,
            secret_ref: None,
        };
        assert_eq!(describe_auth_method(&kf), "public-key");

        let ag = AuthMethod::Agent {
            username: "deploy".into(),
        };
        assert_eq!(describe_auth_method(&ag), "ssh-agent");
    }
}
