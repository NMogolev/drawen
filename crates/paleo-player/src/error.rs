//! Errors reported by the player.

use std::fmt;

/// A failure while opening, decoding, or presenting media.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The container, packet stream, or decoder failed.
    Media(paleo::Error),
    /// The audio device or PCM pipeline failed.
    Audio(paleo_audio::Error),
    /// The player was asked to do something it cannot do in this state.
    State(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Media(error) => write!(f, "{error}"),
            Self::Audio(error) => write!(f, "audio output failed: {error}"),
            Self::State(detail) => f.write_str(detail),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Media(error) => Some(error),
            Self::Audio(error) => Some(error),
            Self::State(_) => None,
        }
    }
}

impl From<paleo::Error> for Error {
    fn from(error: paleo::Error) -> Self {
        Self::Media(error)
    }
}

impl From<paleo_audio::Error> for Error {
    fn from(error: paleo_audio::Error) -> Self {
        Self::Audio(error)
    }
}

/// The result type used across the player.
pub type Result<T> = std::result::Result<T, Error>;
