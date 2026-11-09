use serde::{Serialize, Serializer};

/// Error type returned by every Tauri command. It serialises to its display
/// string so the frontend can show it verbatim.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Discord(String),
    #[error("{0}")]
    Image(String),
    #[error("{0}")]
    Network(String),
    #[error("저장소 오류: {0}")]
    Storage(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
