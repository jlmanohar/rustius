use crate::connection::{
    AuthMethod, Connection, ConnectionType, Folder, RdpConfig, SshConfig, TreeNode,
};
use anyhow::{anyhow, Result};
use base64::Engine;
use cfb_mode::cipher::{AsyncStreamCipher, KeyIvInit};
use std::collections::HashMap;
use uuid::Uuid;

type Aes128CfbDec = cfb_mode::Decryptor<aes::Aes128>;

/// Default session port for MobaXterm session type codes.
fn moba_type_to_connection_type(type_code: &str) -> (ConnectionType, u16) {
    match type_code {
        "0" => (ConnectionType::Ssh, 22),
        "1" => (ConnectionType::Rdp, 3389),
        "3" => (ConnectionType::Sftp, 22),
        _ => (ConnectionType::Ssh, 22),
    }
}

/// Derive MobaXterm default decryption key from username + hostname.
pub fn derive_mobaxterm_key(custom_key_or_password: Option<&str>) -> [u8; 16] {
    if let Some(custom) = custom_key_or_password.filter(|s| !s.trim().is_empty()) {
        use md5::{Digest, Md5};
        let mut hasher = Md5::new();
        hasher.update(custom.as_bytes());
        let res = hasher.finalize();
        let mut key = [0u8; 16];
        key.copy_from_slice(&res);
        return key;
    }

    // Default key derivation based on Windows environment variables
    let win_user = std::env::var("USERNAME").unwrap_or_else(|_| "User".into());
    let win_host = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into());
    let combined = format!("{win_host}{win_user}");

    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(combined.as_bytes());
    let res = hasher.finalize();
    let mut key = [0u8; 16];
    key.copy_from_slice(&res);
    key
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// Decrypt a MobaXterm stored password hex string using AES-128-CFB.
pub fn decrypt_mobaxterm_password(
    hex_str: &str,
    custom_master_password: Option<&str>,
) -> Result<String> {
    let trimmed = hex_str.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }

    let raw_bytes = decode_hex(trimmed)
        .or_else(|| base64::prelude::BASE64_STANDARD.decode(trimmed).ok())
        .ok_or_else(|| anyhow!("Failed to decode MobaXterm password string"))?;

    if raw_bytes.len() <= 16 {
        return Err(anyhow!(
            "Payload too short for MobaXterm encrypted password"
        ));
    }

    let iv = &raw_bytes[0..16];
    let ciphertext = &raw_bytes[16..];

    let key = derive_mobaxterm_key(custom_master_password);

    let dec = Aes128CfbDec::new_from_slices(&key, iv)
        .map_err(|e| anyhow!("Failed to initialize AES-CFB decryptor: {e}"))?;

    let mut plaintext = ciphertext.to_vec();
    dec.decrypt(&mut plaintext);

    String::from_utf8(plaintext).map_err(|e| anyhow!("Decrypted payload is not valid UTF-8: {e}"))
}

/// Parse `.mxtsessions` or `MobaXterm.ini` file into Rustius TreeNode hierarchy.
pub fn parse_mobaxterm_ini(
    ini_content: &str,
    master_password: Option<&str>,
) -> Result<Vec<TreeNode>> {
    let mut current_section = String::new();
    let mut sections: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut passwords_map: HashMap<String, String> = HashMap::new();

    for line in ini_content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_section = trimmed[1..trimmed.len() - 1].to_string();
            continue;
        }

        if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim().to_string();
            let val = v.trim().to_string();

            if current_section.eq_ignore_ascii_case("Passwords")
                || current_section.eq_ignore_ascii_case("Credentials")
            {
                passwords_map.insert(key, val);
            } else {
                sections
                    .entry(current_section.clone())
                    .or_default()
                    .push((key, val));
            }
        }
    }

    // Now organize bookmarks into tree
    let mut root_nodes: Vec<TreeNode> = Vec::new();
    let mut folder_map: HashMap<String, Folder> = HashMap::new();

    // Identify all Bookmarks sections
    let mut bookmark_sections: Vec<String> = sections
        .keys()
        .filter(|k| k.starts_with("Bookmarks"))
        .cloned()
        .collect();
    bookmark_sections.sort();

    for sec_name in bookmark_sections {
        let entries = &sections[&sec_name];

        // Determine path components
        // e.g., "Bookmarks\Production\Database" -> ["Production", "Database"]
        let path_parts: Vec<&str> = if sec_name == "Bookmarks" {
            Vec::new()
        } else {
            sec_name
                .strip_prefix("Bookmarks\\")
                .or_else(|| sec_name.strip_prefix("Bookmarks/"))
                .unwrap_or("")
                .split(['\\', '/'])
                .filter(|s| !s.is_empty())
                .collect()
        };

        let mut connections: Vec<TreeNode> = Vec::new();

        for (name, val) in entries {
            if name.eq_ignore_ascii_case("SubRep") || name.eq_ignore_ascii_case("ImgNum") {
                continue;
            }

            if let Some(conn) = parse_session_entry(name, val, &passwords_map, master_password) {
                connections.push(TreeNode::Connection(conn));
            }
        }

        if path_parts.is_empty() {
            root_nodes.extend(connections);
        } else {
            // Nested folder
            let mut current_path = String::new();
            let mut parent_id: Option<Uuid> = None;

            for (idx, part) in path_parts.iter().enumerate() {
                if !current_path.is_empty() {
                    current_path.push('\\');
                }
                current_path.push_str(part);

                if !folder_map.contains_key(&current_path) {
                    let folder = Folder {
                        id: Uuid::new_v4(),
                        name: part.to_string(),
                        parent_id,
                        children: Vec::new(),
                        expanded: true,
                    };
                    folder_map.insert(current_path.clone(), folder);
                }

                let this_folder_id = folder_map[&current_path].id;
                parent_id = Some(this_folder_id);

                if idx == path_parts.len() - 1 {
                    if let Some(f) = folder_map.get_mut(&current_path) {
                        f.children.extend(connections.clone());
                    }
                }
            }
        }
    }

    // Reconstruct tree from folder_map
    // Folders that have no parent belong at root
    let mut nested_folders: HashMap<Option<Uuid>, Vec<Folder>> = HashMap::new();
    for (_path, folder) in folder_map {
        nested_folders
            .entry(folder.parent_id)
            .or_default()
            .push(folder);
    }

    // Helper to recursively assemble folder children
    fn assemble_folder(folder: &mut Folder, map: &mut HashMap<Option<Uuid>, Vec<Folder>>) {
        if let Some(sub_folders) = map.remove(&Some(folder.id)) {
            for mut sub in sub_folders {
                assemble_folder(&mut sub, map);
                folder.children.push(TreeNode::Folder(sub));
            }
        }
    }

    if let Some(top_folders) = nested_folders.remove(&None) {
        for mut tf in top_folders {
            assemble_folder(&mut tf, &mut nested_folders);
            root_nodes.push(TreeNode::Folder(tf));
        }
    }

    Ok(root_nodes)
}

/// Parse a single MobaXterm session bookmark value string:
/// `#<icon>#<type>%<host>%<port>%<user>%...#<settings>`
fn parse_session_entry(
    name: &str,
    val: &str,
    passwords: &HashMap<String, String>,
    master_password: Option<&str>,
) -> Option<Connection> {
    if !val.contains('%') {
        return None;
    }

    let parts: Vec<&str> = val.split('#').collect();
    if parts.len() < 3 {
        return None;
    }

    let params: Vec<&str> = parts[2].split('%').collect();
    if params.len() < 4 {
        return None;
    }

    let type_code = params[0];
    let (conn_type, default_port) = moba_type_to_connection_type(type_code);

    let host = params[1].trim();
    if host.is_empty() {
        return None;
    }

    let port: u16 = params[2].parse().unwrap_or(default_port);
    let username = params[3].trim().to_string();

    // Look for matching password in passwords_map
    let pass_key = format!("{host}:{username}");
    let decrypted_password = passwords
        .get(&pass_key)
        .or_else(|| passwords.get(host))
        .and_then(|raw| decrypt_mobaxterm_password(raw, master_password).ok());

    let now = chrono::Utc::now().to_rfc3339();

    let ssh_config = if conn_type == ConnectionType::Ssh || conn_type == ConnectionType::Sftp {
        Some(SshConfig {
            allow_legacy_algorithms: false,
            terminal_type: Some("xterm-256color".into()),
            keepalive_interval: Some(30),
            connect_timeout_secs: Some(10),
            legacy_ciphers: false,
            legacy_kex: false,
            port_forwards: Vec::new(),
        })
    } else {
        None
    };

    let (domain, final_username) = if conn_type == ConnectionType::Rdp && username.contains('\\') {
        let parts: Vec<&str> = username.splitn(2, '\\').collect();
        if parts.len() == 2 && !parts[0].trim().is_empty() {
            (Some(parts[0].trim().to_string()), parts[1].trim().to_string())
        } else {
            (None, username)
        }
    } else {
        (None, username)
    };

    let rdp_config = if conn_type == ConnectionType::Rdp {
        Some(RdpConfig {
            width: 1280,
            height: 800,
            color_depth: 24,
            fullscreen: false,
            clipboard_sharing: true,
            domain,
        })
    } else {
        None
    };

    Some(Connection {
        id: Uuid::new_v4(),
        name: name.to_string(),
        connection_type: conn_type,
        host: host.to_string(),
        port,
        auth: AuthMethod::Password {
            username: if final_username.is_empty() {
                "root".into()
            } else {
                final_username
            },
            password: decrypted_password,
            secret_ref: None,
        },
        tags: vec!["MobaXterm".into()],
        environment: None,
        notes: None,
        created_at: now.clone(),
        updated_at: now,
        ssh_config,
        rdp_config,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mxtsessions_ini() {
        let ini = r#"
[Bookmarks]
SubRep=
ImgNum=42
StandaloneSSH= #109#0%192.168.1.50%22%dev%%-1%-1%%%22%%0%0%0%%%-1%0%0%0%%1080%%0%0%1#MobaFont%

[Bookmarks\Production Servers]
SubRep=
WebServer= #109#0%10.0.0.1%22%root%%-1%-1%%%22%%0%0%0%%%-1%0%0%0%%1080%%0%0%1#MobaFont%
WindowsRDP= #109#1%10.0.0.2%3389%Administrator%%-1%-1%%%3389%%0%0%0%%%-1%0%0%0%%1080%%0%0%1#MobaFont%
"#;

        let nodes = parse_mobaxterm_ini(ini, None).unwrap();
        assert_eq!(nodes.len(), 2);

        // One standalone, one folder
        let standalone = nodes
            .iter()
            .find(|n| match n {
                TreeNode::Connection(c) => c.name == "StandaloneSSH",
                _ => false,
            })
            .expect("StandaloneSSH not found");

        if let TreeNode::Connection(c) = standalone {
            assert_eq!(c.host, "192.168.1.50");
            assert_eq!(c.port, 22);
            assert_eq!(c.connection_type, ConnectionType::Ssh);
        }

        let folder = nodes
            .iter()
            .find(|n| match n {
                TreeNode::Folder(f) => f.name == "Production Servers",
                _ => false,
            })
            .expect("Production Servers folder not found");

        if let TreeNode::Folder(f) = folder {
            assert_eq!(f.children.len(), 2);
        }
    }

    #[test]
    fn test_parse_mobaxterm_rdp_domain() {
        let ini = r#"
[Bookmarks]
SubRep=
ImgNum=42
DomainRDP= #109#1%10.0.0.5%3389%CORP\john.doe%%-1%-1%%%3389%%0%0%0%%%-1%0%0%0%%1080%%0%0%1#MobaFont%
"#;

        let nodes = parse_mobaxterm_ini(ini, None).unwrap();
        assert_eq!(nodes.len(), 1);

        if let TreeNode::Connection(c) = &nodes[0] {
            assert_eq!(c.name, "DomainRDP");
            assert_eq!(c.connection_type, ConnectionType::Rdp);
            if let AuthMethod::Password { username, .. } = &c.auth {
                assert_eq!(username, "john.doe");
            } else {
                panic!("Expected password auth");
            }
            let rdp = c.rdp_config.as_ref().expect("RDP config should be present");
            assert_eq!(rdp.domain.as_deref(), Some("CORP"));
        } else {
            panic!("Expected connection node");
        }
    }
}
