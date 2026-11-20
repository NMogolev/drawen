use std::{error::Error, fmt};

/// An error returned while parsing DSS source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DssError {
    pub(super) position: usize,
    pub(super) message: String,
}

impl DssError {
    pub(super) fn new(position: usize, message: impl Into<String>) -> Self {
        Self {
            position,
            message: message.into(),
        }
    }

    /// Returns the zero-based UTF-8 byte offset at which parsing failed.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.position
    }

    /// Returns a human-readable explanation of the parsing error.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for DssError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "DSS parse error at byte {}: {}",
            self.position, self.message
        )
    }
}

impl Error for DssError {}
