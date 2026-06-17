#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("bad parameters: {0}")]
    Params(String),
    #[error("malformed share: {0}")]
    Malformed(String),
    #[error("shares are from different splits")]
    MixedSet,
    #[error("share index {0} appears twice")]
    Duplicate(u8),
    #[error("not enough shares: have {have}, need {need}")]
    Insufficient { have: usize, need: u8 },
    #[error("recovered secret failed its integrity check")]
    Integrity,
}

pub type Result<T> = std::result::Result<T, Error>;
