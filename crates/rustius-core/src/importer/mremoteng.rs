use crate::connection::{
    AuthMethod, Connection, ConnectionType, Folder, RdpConfig, SshConfig, TreeNode,
};
use aes_gcm::aead::{Aead, KeyInit};
use anyhow::{anyhow, Context, Result};
use base64::prelude::*;
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use md5::{Digest as Md5Digest, Md5};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;
use uuid::Uuid;

/// Default master password for mRemoteNG when none is set.
pub const MREMOTENG_DEFAULT_PASSWORD: &str = "mR3m";

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

/// Decrypt an mRemoteNG password string.
///
/// Tries AES-256-GCM first (mRemoteNG >= 1.75), falling back to AES-128-CBC (mRemoteNG <= 1.74).
pub fn decrypt_mremoteng_password(
    encoded: &str,
    master_password: Option<&str>,
    preferred_cipher: Option<&str>,
) -> Result<String> {
    let trimmed = encoded.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }

    let password = master_password
        .filter(|p| !p.trim().is_empty())
        .unwrap_or(MREMOTENG_DEFAULT_PASSWORD);

    let raw_bytes = BASE64_STANDARD
        .decode(trimmed)
        .context("Invalid base64 in mRemoteNG password payload")?;

    let pref = preferred_cipher.unwrap_or("GCM").to_uppercase();
    if pref == "CBC" {
        if let Ok(res) = decrypt_legacy_cbc(&raw_bytes, password) {
            return Ok(res);
        }
        decrypt_modern_gcm(&raw_bytes, password)
    } else {
        if let Ok(res) = decrypt_modern_gcm(&raw_bytes, password) {
            return Ok(res);
        }
        decrypt_legacy_cbc(&raw_bytes, password)
    }
}

/// Decrypt modern mRemoteNG password (AES-256-GCM + PBKDF2-HMAC-SHA1 1000 rounds).
///
/// Payload format:
/// - Bytes 0..16: Salt (16 bytes)
/// - Bytes 16..32: Nonce / IV (16 bytes)
/// - Bytes 32..end: Ciphertext + Auth Tag (16 bytes)
pub fn decrypt_modern_gcm(raw_bytes: &[u8], password: &str) -> Result<String> {
    if raw_bytes.len() < 48 {
        return Err(anyhow!(
            "Payload too short for mRemoteNG AES-GCM (minimum 48 bytes, got {})",
            raw_bytes.len()
        ));
    }

    let salt = &raw_bytes[0..16];
    let nonce_bytes = &raw_bytes[16..32];
    let ciphertext_and_tag = &raw_bytes[32..];

    let mut key = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password.as_bytes(), salt, 1000, &mut key);

    type Aes256Gcm16 = aes_gcm::AesGcm<aes::Aes256, aes_gcm::aead::consts::U16>;
    let cipher = Aes256Gcm16::new_from_slice(&key)
        .map_err(|e| anyhow!("Failed to initialize Aes256Gcm16: {e}"))?;

    let payload = aes_gcm::aead::Payload {
        msg: ciphertext_and_tag,
        aad: salt,
    };
    let plaintext_bytes = cipher
        .decrypt(nonce_bytes.into(), payload)
        .map_err(|e| anyhow!("Failed to decrypt AES-GCM password: {e}"))?;

    String::from_utf8(plaintext_bytes)
        .map_err(|e| anyhow!("Decrypted payload is not valid UTF-8: {e}"))
}

/// Decrypt legacy mRemoteNG password (AES-128-CBC + MD5 key derivation).
///
/// Payload format:
/// - Bytes 0..16: IV (16 bytes)
/// - Bytes 16..end: Ciphertext
pub fn decrypt_legacy_cbc(raw_bytes: &[u8], password: &str) -> Result<String> {
    if raw_bytes.len() <= 16 {
        return Err(anyhow!(
            "Payload too short for mRemoteNG AES-CBC (minimum >16 bytes, got {})",
            raw_bytes.len()
        ));
    }

    let iv = &raw_bytes[0..16];
    let ciphertext = &raw_bytes[16..];

    let mut hasher = Md5::new();
    hasher.update(password.as_bytes());
    let key = hasher.finalize();

    let dec = Aes128CbcDec::new_from_slices(&key, iv)
        .map_err(|e| anyhow!("Failed to initialize Aes128Cbc: {e}"))?;

    let mut buf = ciphertext.to_vec();
    let plaintext_bytes = dec
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| anyhow!("Failed to decrypt AES-CBC password: {e}"))?;

    String::from_utf8(plaintext_bytes.to_vec())
        .map_err(|e| anyhow!("Decrypted payload is not valid UTF-8: {e}"))
}

/// Parse mRemoteNG protocol string into Rustius ConnectionType and default port.
fn parse_protocol(proto: &str) -> (ConnectionType, u16) {
    match proto.to_uppercase().as_str() {
        "SSH2" | "SSH1" | "SSH" => (ConnectionType::Ssh, 22),
        "SFTP" => (ConnectionType::Sftp, 22),
        "RDP" => (ConnectionType::Rdp, 3389),
        _ => (ConnectionType::Ssh, 22),
    }
}

/// Parse mRemoteNG XML (confCons.xml) content into Rustius TreeNodes.
pub fn parse_mremoteng_xml(
    xml_content: &str,
    master_password: Option<&str>,
) -> Result<Vec<TreeNode>> {
    let mut content = xml_content.trim();

    // Check for entire file encryption
    let decrypted_buf;
    if !content.starts_with('<') {
        let raw = BASE64_STANDARD
            .decode(content)
            .context("Encrypted confCons file is not valid base64")?;
        let pw = master_password
            .filter(|p| !p.trim().is_empty())
            .unwrap_or(MREMOTENG_DEFAULT_PASSWORD);
        let dec = decrypt_modern_gcm(&raw, pw)
            .or_else(|_| decrypt_legacy_cbc(&raw, pw))
            .context("Failed to decrypt completely encrypted confCons file")?;
        decrypted_buf = dec;
        content = decrypted_buf.trim();
    }

    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut block_cipher_mode: Option<String> = None;
    let mut root_nodes: Vec<TreeNode> = Vec::new();
    let mut folder_stack: Vec<Folder> = Vec::new();

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let attrs = extract_attributes(&reader, e)?;

                if tag_name.eq_ignore_ascii_case("Connections") {
                    if let Some(mode) = attrs.get("BlockCipherMode") {
                        block_cipher_mode = Some(mode.clone());
                    }
                } else if tag_name.eq_ignore_ascii_case("Node") {
                    let is_container = attrs
                        .get("Type")
                        .map(|t| t.eq_ignore_ascii_case("Container"))
                        .unwrap_or(false);

                    if is_container {
                        let name = attrs
                            .get("Name")
                            .cloned()
                            .unwrap_or_else(|| "Unnamed Folder".into());
                        let parent_id = folder_stack.last().map(|f| f.id);
                        folder_stack.push(Folder {
                            id: Uuid::new_v4(),
                            name,
                            parent_id,
                            children: Vec::new(),
                            expanded: true,
                        });
                    } else {
                        // Leaf connection started as container (unlikely, but handle gracefully)
                        let conn =
                            build_connection(&attrs, master_password, block_cipher_mode.as_deref());
                        if let Some(top) = folder_stack.last_mut() {
                            top.children.push(TreeNode::Connection(conn));
                        } else {
                            root_nodes.push(TreeNode::Connection(conn));
                        }
                    }
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let attrs = extract_attributes(&reader, e)?;

                if tag_name.eq_ignore_ascii_case("Connections") {
                    if let Some(mode) = attrs.get("BlockCipherMode") {
                        block_cipher_mode = Some(mode.clone());
                    }
                } else if tag_name.eq_ignore_ascii_case("Node") {
                    let is_container = attrs
                        .get("Type")
                        .map(|t| t.eq_ignore_ascii_case("Container"))
                        .unwrap_or(false);

                    if is_container {
                        let name = attrs
                            .get("Name")
                            .cloned()
                            .unwrap_or_else(|| "Unnamed Folder".into());
                        let parent_id = folder_stack.last().map(|f| f.id);
                        let folder = Folder {
                            id: Uuid::new_v4(),
                            name,
                            parent_id,
                            children: Vec::new(),
                            expanded: true,
                        };
                        if let Some(top) = folder_stack.last_mut() {
                            top.children.push(TreeNode::Folder(folder));
                        } else {
                            root_nodes.push(TreeNode::Folder(folder));
                        }
                    } else {
                        let conn =
                            build_connection(&attrs, master_password, block_cipher_mode.as_deref());
                        if let Some(top) = folder_stack.last_mut() {
                            top.children.push(TreeNode::Connection(conn));
                        } else {
                            root_nodes.push(TreeNode::Connection(conn));
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag_name.eq_ignore_ascii_case("Node") {
                    if let Some(finished_folder) = folder_stack.pop() {
                        if let Some(parent) = folder_stack.last_mut() {
                            parent.children.push(TreeNode::Folder(finished_folder));
                        } else {
                            root_nodes.push(TreeNode::Folder(finished_folder));
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(anyhow!(
                    "Error parsing XML at position {}: {e}",
                    reader.buffer_position()
                ))
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(root_nodes)
}

fn extract_attributes<R: std::io::BufRead>(
    reader: &Reader<R>,
    e: &quick_xml::events::BytesStart<'_>,
) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    for attr in e.attributes() {
        let a = attr.map_err(|err| anyhow!("XML attribute error: {err}"))?;
        let key = String::from_utf8_lossy(a.key.as_ref()).to_string();
        let value = a
            .decode_and_unescape_value(reader.decoder())
            .map_err(|err| anyhow!("XML value error: {err}"))?
            .to_string();
        map.insert(key, value);
    }
    Ok(map)
}

fn build_connection(
    attrs: &HashMap<String, String>,
    master_password: Option<&str>,
    cipher_mode: Option<&str>,
) -> Connection {
    let name = attrs
        .get("Name")
        .cloned()
        .unwrap_or_else(|| "Unnamed Connection".into());

    let host = attrs
        .get("Hostname")
        .cloned()
        .unwrap_or_else(|| "127.0.0.1".into());

    let proto_str = attrs.get("Protocol").map(|s| s.as_str()).unwrap_or("SSH2");
    let (conn_type, default_port) = parse_protocol(proto_str);

    let port: u16 = attrs
        .get("Port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(default_port);

    let username = attrs
        .get("Username")
        .cloned()
        .unwrap_or_else(|| "root".into());

    let raw_password = attrs.get("Password").map(|p| p.as_str()).unwrap_or("");
    let decrypted_password = if !raw_password.is_empty() {
        decrypt_mremoteng_password(raw_password, master_password, cipher_mode).ok()
    } else {
        None
    };

    let domain = attrs
        .get("Domain")
        .cloned()
        .filter(|d| !d.trim().is_empty())
        .or_else(|| {
            if conn_type == ConnectionType::Rdp && username.contains('\\') {
                username.split_once('\\').map(|(d, _)| d.trim().to_string())
            } else {
                None
            }
        });
    let description = attrs
        .get("Descr")
        .or_else(|| attrs.get("Description"))
        .cloned();

    let mut tags = Vec::new();
    if let Some(panel) = attrs.get("Panel") {
        if !panel.trim().is_empty() && panel != "General" {
            tags.push(panel.clone());
        }
    }

    let auth = AuthMethod::Password {
        username: if let Some(dom) = &domain {
            if !dom.is_empty() && !username.contains('\\') {
                format!("{dom}\\{username}")
            } else {
                username
            }
        } else {
            username
        },
        password: decrypted_password,
        secret_ref: None,
    };

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

    let rdp_config = if conn_type == ConnectionType::Rdp {
        Some(RdpConfig {
            width: 1280,
            height: 800,
            color_depth: 24,
            fullscreen: false,
            clipboard_sharing: true,
            domain: domain.clone(),
        })
    } else {
        None
    };

    let now = chrono::Utc::now().to_rfc3339();

    Connection {
        id: Uuid::new_v4(),
        name,
        connection_type: conn_type,
        host,
        port,
        auth,
        tags,
        environment: None,
        notes: description,
        created_at: now.clone(),
        updated_at: now,
        ssh_config,
        rdp_config,
        credential_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::BlockEncryptMut;

    #[test]
    fn test_legacy_cbc_roundtrip() {
        // Known legacy test vector generated with default mR3m password
        let test_pass = "MySecret123!";
        let master = "mR3m";

        let mut hasher = Md5::new();
        hasher.update(master.as_bytes());
        let key = hasher.finalize();

        let iv = [7u8; 16];
        type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
        let enc = Aes128CbcEnc::new_from_slices(&key, &iv).unwrap();
        let mut buf = [0u8; 32];
        let ct = enc
            .encrypt_padded_b2b_mut::<Pkcs7>(test_pass.as_bytes(), &mut buf)
            .unwrap();

        let mut payload = iv.to_vec();
        payload.extend_from_slice(ct);
        let b64 = BASE64_STANDARD.encode(&payload);

        let decrypted = decrypt_mremoteng_password(&b64, None, Some("CBC")).unwrap();
        assert_eq!(decrypted, test_pass);
    }

    #[test]
    fn test_modern_gcm_roundtrip() {
        let test_pass = "ModernVaultPassword99#";
        let master = "CustomMasterPass";

        let salt = [42u8; 16];
        let nonce_bytes = [13u8; 16];

        let mut key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha1::Sha1>(master.as_bytes(), &salt, 1000, &mut key);

        type Aes256Gcm16 = aes_gcm::AesGcm<aes::Aes256, aes_gcm::aead::consts::U16>;
        let mut cipher = Aes256Gcm16::new_from_slice(&key).unwrap();
        // Encrypt properly:
        use aes_gcm::aead::AeadMutInPlace;
        let mut in_out = test_pass.as_bytes().to_vec();
        let tag = cipher
            .encrypt_in_place_detached(nonce_bytes.as_slice().into(), &salt, &mut in_out)
            .unwrap();

        let mut payload = salt.to_vec();
        payload.extend_from_slice(&nonce_bytes);
        payload.extend_from_slice(&in_out);
        payload.extend_from_slice(tag.as_slice());

        let b64 = BASE64_STANDARD.encode(&payload);

        let decrypted =
            decrypt_mremoteng_password(&b64, Some("CustomMasterPass"), Some("GCM")).unwrap();
        assert_eq!(decrypted, test_pass);
    }

    #[test]
    fn test_parse_mremoteng_xml_structure() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<Connections Name="Connections" Export="False" EncryptionEngine="AES" BlockCipherMode="GCM" KdfIterations="1000">
    <Node Name="Production Servers" Type="Container" Expanded="True">
        <Node Name="Web App 01" Type="Connection" Hostname="10.0.0.10" Port="22" Protocol="SSH2" Username="ubuntu" Password="" />
        <Node Name="Windows Host" Type="Connection" Hostname="10.0.0.20" Port="3389" Protocol="RDP" Username="admin" Domain="CORP" Password="" />
    </Node>
    <Node Name="Standalone SSH" Type="Connection" Hostname="jumpbox.internal" Port="2222" Protocol="SSH2" Username="dev" Password="" />
</Connections>"#;

        let nodes = parse_mremoteng_xml(xml, None).unwrap();
        assert_eq!(nodes.len(), 2);

        match &nodes[0] {
            TreeNode::Folder(f) => {
                assert_eq!(f.name, "Production Servers");
                assert_eq!(f.children.len(), 2);
                match &f.children[0] {
                    TreeNode::Connection(c) => {
                        assert_eq!(c.name, "Web App 01");
                        assert_eq!(c.host, "10.0.0.10");
                        assert_eq!(c.port, 22);
                        assert_eq!(c.connection_type, ConnectionType::Ssh);
                    }
                    _ => panic!("Expected connection"),
                }
                match &f.children[1] {
                    TreeNode::Connection(c) => {
                        assert_eq!(c.name, "Windows Host");
                        assert_eq!(c.host, "10.0.0.20");
                        assert_eq!(c.port, 3389);
                        assert_eq!(c.connection_type, ConnectionType::Rdp);
                        if let AuthMethod::Password { username, .. } = &c.auth {
                            assert_eq!(username, "CORP\\admin");
                        } else {
                            panic!("Expected password auth");
                        }
                        let rdp = c.rdp_config.as_ref().expect("RDP config should be present");
                        assert_eq!(rdp.domain.as_deref(), Some("CORP"));
                    }
                    _ => panic!("Expected connection"),
                }
            }
            _ => panic!("Expected folder"),
        }

        match &nodes[1] {
            TreeNode::Connection(c) => {
                assert_eq!(c.name, "Standalone SSH");
                assert_eq!(c.port, 2222);
            }
            _ => panic!("Expected standalone connection"),
        }
    }
}
