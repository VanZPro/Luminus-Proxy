use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("invalid role: {0}")]
    InvalidRole(String),
    #[error("invalid content: {0}")]
    InvalidContent(String),
    #[error("missing required field: {0}")]
    MissingRequiredField(String),
    #[error("unsupported feature: {0}")]
    UnsupportedFeature(String),
    #[error("invalid tool arguments for {0}")]
    InvalidToolArguments(String),
    #[error("invalid response shape: {0}")]
    InvalidResponseShape(String),
}
