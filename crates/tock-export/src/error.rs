//! Export error types.

/// Errors that can occur during export operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Storage / database error.
    #[error("storage: {0}")]
    Storage(#[from] tock_storage::Error),

    /// Template rendering error.
    #[error("template: {0}")]
    Template(#[from] tera::Error),

    /// JSON serialization error.
    #[error("serialization: {0}")]
    Serialization(#[from] serde_json::Error),

    /// I/O error (e.g. reading a custom template file).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Unknown built-in template name.
    #[error("unknown built-in template: {0}")]
    UnknownTemplate(String),
}
