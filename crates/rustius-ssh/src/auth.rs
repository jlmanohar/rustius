// SSH authentication helpers.
//
// This module provides shared authentication logic used by both
// the SSH client and SFTP client.

use rustius_core::connection::AuthMethod;

/// Determine the authentication strategy based on the AuthMethod.
pub fn describe_auth_method(method: &AuthMethod) -> &'static str {
    match method {
        AuthMethod::Password { .. } => "password",
        AuthMethod::KeyFile { .. } => "public-key",
        AuthMethod::Agent { .. } => "ssh-agent",
    }
}
