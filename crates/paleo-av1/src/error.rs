//! Errors reported by the AV1 backend.

use std::fmt;

/// A failure while configuring or decoding AV1 video.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The track uses a codec this backend does not decode.
    UnsupportedCodec(String),
    /// The bitstream is malformed or uses a feature the decoder does not
    /// implement.
    Decode(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCodec(detail) => write!(f, "no decoder for {detail}"),
            Self::Decode(detail) => write!(f, "AV1 decode failed: {detail}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusty_av1d::Rav1dError> for Error {
    fn from(error: rusty_av1d::Rav1dError) -> Self {
        Self::Decode(error.to_string())
    }
}

/// The result type used across the backend.
pub type Result<T> = std::result::Result<T, Error>;
