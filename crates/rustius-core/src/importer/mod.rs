pub mod mobaxterm;
pub mod mremoteng;

use crate::connection::TreeNode;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Summary of import operations.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub connections_imported: usize,
    pub folders_imported: usize,
}

/// Target import format / source application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ImportFormat {
    #[default]
    Auto,
    MRemoteNG,
    MobaXterm,
}

/// Parse and import session trees from raw content.
pub fn import_content(
    content: &str,
    format: ImportFormat,
    master_password: Option<&str>,
) -> Result<Vec<TreeNode>> {
    match format {
        ImportFormat::MRemoteNG => mremoteng::parse_mremoteng_xml(content, master_password),
        ImportFormat::MobaXterm => mobaxterm::parse_mobaxterm_ini(content, master_password),
        ImportFormat::Auto => {
            let trimmed = content.trim();
            if trimmed.starts_with('<')
                || trimmed.contains("<Connections")
                || trimmed.contains("<Node")
            {
                mremoteng::parse_mremoteng_xml(content, master_password)
            } else if trimmed.contains("[Bookmarks") || trimmed.contains("[Passwords]") {
                mobaxterm::parse_mobaxterm_ini(content, master_password)
            } else {
                // Try mRemoteNG first, then MobaXterm
                mremoteng::parse_mremoteng_xml(content, master_password)
                    .or_else(|_| mobaxterm::parse_mobaxterm_ini(content, master_password))
            }
        }
    }
}

/// Parse and import session trees from a file path.
pub fn import_from_file<P: AsRef<Path>>(
    path: P,
    format: ImportFormat,
    master_password: Option<&str>,
) -> Result<Vec<TreeNode>> {
    let p = path.as_ref();
    let content = std::fs::read_to_string(p)
        .map_err(|e| anyhow!("Failed to read import file at {}: {e}", p.display()))?;

    let detected_format = match format {
        ImportFormat::Auto => {
            if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                if ext.eq_ignore_ascii_case("xml") || ext.eq_ignore_ascii_case("mremoteng") {
                    ImportFormat::MRemoteNG
                } else if ext.eq_ignore_ascii_case("mxtsessions") || ext.eq_ignore_ascii_case("ini")
                {
                    ImportFormat::MobaXterm
                } else {
                    ImportFormat::Auto
                }
            } else {
                ImportFormat::Auto
            }
        }
        f => f,
    };

    import_content(&content, detected_format, master_password)
}

/// Helper function to count imported connections and folders recursively.
pub fn count_imported(nodes: &[TreeNode]) -> ImportSummary {
    let mut summary = ImportSummary::default();
    fn walk(nodes: &[TreeNode], summary: &mut ImportSummary) {
        for node in nodes {
            match node {
                TreeNode::Connection(_) => summary.connections_imported += 1,
                TreeNode::Folder(f) => {
                    summary.folders_imported += 1;
                    walk(&f.children, summary);
                }
            }
        }
    }
    walk(nodes, &mut summary);
    summary
}
