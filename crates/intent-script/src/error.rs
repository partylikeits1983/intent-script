use alloc::format;
use alloc::string::String;
use core::fmt;

#[derive(Debug)]
pub enum CompileError {
    UnknownNetwork(String),
    UnknownAsset { asset: String, network: String },
    UnknownProtocol { protocol: String, network: String },
    InvalidAmount(String),
    InvalidAddress(String),
    Config(String),
    UnsupportedStep(String),
    Validation(String),
    InvalidChain(String),
    Adapter(String),
    Json(String),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::UnknownNetwork(s) => write!(f, "Unknown network: {s}"),
            CompileError::UnknownAsset { asset, network } => {
                write!(f, "Unknown asset '{asset}' on network '{network}'")
            }
            CompileError::UnknownProtocol { protocol, network } => {
                write!(f, "Unknown protocol '{protocol}' on network '{network}'")
            }
            CompileError::InvalidAmount(s) => write!(f, "Invalid amount: {s}"),
            CompileError::InvalidAddress(s) => write!(f, "Invalid address: {s}"),
            CompileError::Config(s) => write!(f, "Config error: {s}"),
            CompileError::UnsupportedStep(s) => write!(f, "Unsupported step: {s}"),
            CompileError::Validation(s) => write!(f, "Validation error: {s}"),
            CompileError::InvalidChain(s) => write!(f, "Invalid intent chain: {s}"),
            CompileError::Adapter(s) => write!(f, "Adapter error: {s}"),
            CompileError::Json(s) => write!(f, "JSON parse error: {s}"),
        }
    }
}

impl From<serde_json::Error> for CompileError {
    fn from(e: serde_json::Error) -> Self {
        CompileError::Json(format!("{e}"))
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CompileError {}

pub type Result<T> = core::result::Result<T, CompileError>;
