use serde::Serialize;

/// Errors returned to the frontend. Serialized as a plain message string so
/// the UI can display it verbatim.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("{0}")]
    InvalidInput(String),
    #[error("internal state lock was poisoned")]
    LockPoisoned,
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
