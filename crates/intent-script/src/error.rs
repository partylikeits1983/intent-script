use thiserror::Error;

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("Unknown network: {0}")]
    UnknownNetwork(String),

    #[error("Unknown asset '{asset}' on network '{network}'")]
    UnknownAsset { asset: String, network: String },

    #[error("Unknown protocol '{protocol}' on network '{network}'")]
    UnknownProtocol { protocol: String, network: String },

    #[error("Invalid amount: {0}")]
    InvalidAmount(String),

    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Unsupported step: {0}")]
    UnsupportedStep(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Invalid intent chain: {0}")]
    InvalidChain(String),

    #[error("Adapter error: {0}")]
    Adapter(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, CompileError>;
