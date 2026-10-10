use thiserror::Error;

/// Every failure unifictl reports. `Display` is the user-facing message;
/// `app::main` prefixes it with `Error: `.
#[derive(Debug, Error)]
pub enum Error {
    #[error("config: {0}")]
    Config(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Unifi(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
