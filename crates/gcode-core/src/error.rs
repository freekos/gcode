use thiserror::Error;

/// All core errors. CLI/UI render `Display`; variants let callers branch.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("state db: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("already exists: {0}")]
    AlreadyExists(String),

    #[error("invalid: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
