//! Decoded video pictures shared by every backend.

use std::time::Duration;

use crate::error::{Error, Result};

/// One decoded picture in tightly packed RGBA.
///
/// Backends convert their native YUV output into this type so a player does not
/// need to know which codec produced a frame. Rows are `width * 4` bytes with
/// no padding.
#[derive(Clone, Debug)]
pub struct VideoFrame {
    pts: Duration,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl VideoFrame {
    /// Creates a frame from tightly packed RGBA pixels.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Decode`] when the pixel buffer does not match
    /// `width * height * 4` bytes.
    pub fn new(pts: Duration, width: u32, height: u32, pixels: Vec<u8>) -> Result<Self> {
        let expected = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4));
        if expected != Some(pixels.len()) {
            return Err(Error::Decode(format!(
                "decoded frame is {width}x{height} but carries {} RGBA bytes",
                pixels.len()
            )));
        }
        Ok(Self {
            pts,
            width,
            height,
            pixels,
        })
    }

    /// Returns the presentation timestamp of the picture.
    #[must_use]
    pub const fn pts(&self) -> Duration {
        self.pts
    }

    /// Returns the width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns the row stride in bytes. Pixels are tightly packed RGBA.
    #[must_use]
    pub fn stride(&self) -> usize {
        usize::try_from(self.width).unwrap_or_default() * 4
    }

    /// Returns the tightly packed RGBA pixels.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Consumes the frame and returns its RGBA pixels.
    #[must_use]
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }
}
