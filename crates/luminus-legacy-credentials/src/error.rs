use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LegacyCredentialError {
    #[error("legacy ciphertext is invalid")]
    InvalidCiphertext,
    #[error("legacy encryption key is invalid")]
    InvalidKey,
    #[error("legacy credential material is invalid")]
    InvalidMaterial,
    #[error("legacy credential storage is unavailable")]
    Unavailable,
    #[error("legacy credential schema is corrupt")]
    CorruptSchema,
    #[error("legacy credential adapter failed internally")]
    Internal,
}
