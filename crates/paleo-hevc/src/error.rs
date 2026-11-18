//! Errors reported by the HEVC backend.

use std::fmt;

/// A failure while configuring or decoding HEVC video.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The track has no `hvcC` configuration record and its packets are not
    /// Annex-B either, so the NAL framing cannot be determined.
    MissingConfig,
    /// The track uses a codec this backend does not decode.
    UnsupportedCodec(String),
    /// The `hvcC` record is malformed.
    InvalidConfig(&'static str),
    /// A packet does not match the NAL framing declared by the configuration.
    InvalidPacket(&'static str),
    /// The bitstream uses a feature the decoder does not implement.
    Decode(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingConfig => {
                f.write_str("the HEVC track has no hvcC record and its packets are not Annex-B")
            }
            Self::UnsupportedCodec(detail) => write!(f, "no decoder for {detail}"),
            Self::InvalidConfig(detail) => write!(f, "invalid hvcC record: {detail}"),
            Self::InvalidPacket(detail) => write!(f, "invalid HEVC packet: {detail}"),
            Self::Decode(detail) => write!(f, "HEVC decode failed: {detail}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusty_h265::Error> for Error {
    fn from(error: rusty_h265::Error) -> Self {
        match error {
            rusty_h265::Error::Unsupported(detail) => {
                Self::Decode(format!("unsupported HEVC tool: {detail}"))
            }
            other => Self::Decode(other.to_string()),
        }
    }
}

/// The result type used across the backend.
pub type Result<T> = std::result::Result<T, Error>;
