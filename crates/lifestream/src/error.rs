#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("decrypt or verify failed")]
    Crypto,
    #[error("object not found: {0}")]
    NotFound(String),
    #[error("corrupt object: {0}")]
    Corrupt(String),
    #[error("store: {0}")]
    Store(String),
}

pub type Result<T> = std::result::Result<T, Error>;
