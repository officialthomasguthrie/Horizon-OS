#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("lifestream: {0}")]
    Lifestream(#[from] lifestream::Error),
    #[error("sync: {0}")]
    Sync(String),
}

pub type Result<T> = std::result::Result<T, Error>;
