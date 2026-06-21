use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    /// No single identity store could be found to boot under the search root.
    #[error("discover store: {0}")]
    Discover(String),
    /// The path given is not a Horizon store (no keysalt, no objects).
    #[error("not a Horizon store: {0}")]
    NotAStore(String),
    /// The on-disk keyslots file did not parse.
    #[error("keyslots: {0}")]
    Keyslots(String),
    /// The passphrase fallback could not produce a secret.
    #[error("passphrase: {0}")]
    Passphrase(String),
    /// The unlocked master did not decrypt the store: a wrong key, not a torn boot.
    #[error("the unlocked key did not open the store")]
    KeyMismatch,
    #[error("lifestream: {0}")]
    Lifestream(#[from] lifestream::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
