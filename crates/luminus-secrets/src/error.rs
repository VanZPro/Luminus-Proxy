use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SecretError {
    #[error("credential not found")]
    NotFound,
    #[error("credential material is invalid")]
    InvalidMaterial,
    #[error("credential decryption failed")]
    DecryptionFailed,
    #[error("credential source unavailable")]
    Unavailable,
    #[error("credential resolution failed internally")]
    Internal,
}
