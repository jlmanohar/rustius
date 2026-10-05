use thiserror::Error;

/// Unified error type for the rustius-core crate.
#[derive(Debug, Error)]
pub enum RustiusError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Connection not found: {0}")]
    ConnectionNotFound(String),

    #[error("Folder not found: {0}")]
    FolderNotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Storage error: {0}")]
    Storage(String),
}

/// Convenience alias for Results using our error type.
pub type Result<T> = std::result::Result<T, RustiusError>;
