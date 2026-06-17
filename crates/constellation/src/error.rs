#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("lifestream: {0}")]
    Lifestream(#[from] lifestream::Error),
    #[error("sync: {0}")]
    Sync(String),
    // Anything that went wrong on the network skin: QUIC, the Noise handshake,
    // framing, or a peer that reported an error of its own. The sync core never
    // produces this; only the `net` transport does.
    #[error("net: {0}")]
    Net(String),
}

pub type Result<T> = std::result::Result<T, Error>;
