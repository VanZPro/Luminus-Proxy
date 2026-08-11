use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StorageError {
    #[error("storage unavailable")]
    Unavailable,
    #[error("stored data is corrupt")]
    CorruptData,
    #[error("stored account record is invalid")]
    InvalidRecord,
    #[error("storage internal error")]
    Internal,
}
