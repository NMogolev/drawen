//! AV1 video decoding backend for [Paleo](https://docs.rs/paleo).
//!
//! Paleo demuxes AV1 packets out of MP4 and Matroska containers. This crate
//! bridges them to the pure-Rust [`rusty_av1d`] decoder (the rav1d port of
//! dav1d, built without its assembly kernels):
//!
//! - one packet is one temporal unit, and the decoder consumes the OBU stream
//!   directly, so the packet is forwarded unchanged;
//! - a keyframe is the only picture that starts decoding from an arbitrary
//!   point, so the backend waits for one after a reset by reading the frame
//!   header, without decoding the packet twice;
//! - 8-, 10-, and 12-bit pictures at 4:2:0, 4:2:2, 4:4:4, and monochrome are
//!   converted to tightly packed RGBA through `paleo::rgba_from_yuv`;
//! - the picture's own colour metadata selects the YUV matrix and range.
//!
//! ```no_run
//! use paleo::{Event, Media, VideoDecoder};
//! use paleo_av1::Av1Decoder;
//!
//! # fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let mut media = Media::open("clip.mkv")?;
//! let info = media.info().video().ok_or("the file has no video")?;
//! let mut decoder = Av1Decoder::new(info)?;
//!
//! loop {
//!     match media.next_event()? {
//!         Event::Video(packet) => {
//!             for frame in decoder.decode(&packet)? {
//!                 let _rgba: &[u8] = frame.pixels();
//!             }
//!         }
//!         Event::Audio(_) => {}
//!         Event::EndOfStream => break,
//!     }
//! }
//! # Ok(()) }
//! ```
//!
//! The decoder is synchronous and owns no packet queue. A caller that needs to
//! seek should call [`paleo::Media::seek_keyframe`] with
//! [`Av1Decoder::is_keyframe_data`], then [`paleo::VideoDecoder::reset`].

#![forbid(unsafe_code)]

mod error;
mod obu;

use std::time::Duration;

pub use error::{Error, Result};
use paleo::{
    ColorMatrix, VideoCodec, VideoDecoder, VideoFrame, VideoPacket, VideoTrackInfo, YuvPlane,
    YuvRange, rgba_from_yuv,
};
use rusty_av1d::{
    BitsPerComponent, Decoder as Inner, Picture, PixelLayout, PlanarImageComponent, Rav1dError,
};

/// A streaming AV1 decoder driven by [`paleo::VideoPacket`] values.
pub struct Av1Decoder {
    /// `None` between [`Av1Decoder::reset`] and the next keyframe: a decoder
    /// instance is cheap to rebuild, and rebuilding is how its reference
    /// pictures are dropped.
    inner: Option<Inner>,
    awaiting_keyframe: bool,
}

impl Av1Decoder {
    /// Creates a decoder for an AV1 video track.
    ///
    /// The `av1C` record is not needed: AV1 carries its sequence header in
    /// band, so the decoder does not have to be configured up front.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedCodec`] when the track is not AV1, and
    /// [`Error::Decode`] when the decoder cannot be created.
    pub fn new(info: &VideoTrackInfo) -> Result<Self> {
        Self::with_codec(info.codec())
    }

    /// Creates a decoder for a known AV1 track without requiring
    /// [`VideoTrackInfo`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedCodec`] when `codec` is not
    /// [`paleo::VideoCodec::Av1`].
    pub fn with_codec(codec: VideoCodec) -> Result<Self> {
        if codec != VideoCodec::Av1 {
            return Err(Error::UnsupportedCodec(format!(
                "{codec} video (the paleo-av1 backend decodes AV1 only)"
            )));
        }
        Ok(Self {
            inner: Some(Inner::new()?),
            awaiting_keyframe: true,
        })
    }

    /// Drops decoder state and waits for the next keyframe.
    ///
    /// Call this after seeking. Packets before the next keyframe are skipped
    /// because inter pictures cannot be decoded from an arbitrary point.
    pub fn reset(&mut self) {
        self.inner = None;
        self.awaiting_keyframe = true;
    }

    /// Returns the decoder, opening it again after a [`Self::reset`].
    fn decoder(&mut self) -> Result<&mut Inner> {
        if self.inner.is_none() {
            self.inner = Some(Inner::new()?);
        }
        self.inner
            .as_mut()
            .ok_or_else(|| Error::Decode("AV1 decoder is unavailable".to_owned()))
    }

    /// Reports whether the packet is a random-access point.
    #[must_use]
    pub fn is_keyframe(&self, packet: &VideoPacket) -> bool {
        self.is_keyframe_data(packet.data())
    }

    /// Reports whether raw packet data contains a keyframe.
    #[must_use]
    pub fn is_keyframe_data(&self, data: &[u8]) -> bool {
        obu::is_keyframe(data)
    }

    /// Reports whether decoding is waiting for the next keyframe.
    #[must_use]
    pub const fn is_awaiting_keyframe(&self) -> bool {
        self.awaiting_keyframe
    }

    /// Decodes one packet and returns the pictures that are ready to present.
    ///
    /// A packet can produce no picture while the decoder's reference window
    /// fills, and a packet that follows several slow ones can produce more than
    /// one.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Decode`] when the packet is malformed or uses a feature
    /// this build cannot decode.
    pub fn decode(&mut self, packet: &VideoPacket) -> Result<Vec<VideoFrame>> {
        let keyframe = self.is_keyframe_data(packet.data());
        if self.awaiting_keyframe {
            if !keyframe {
                return Ok(Vec::new());
            }
            self.awaiting_keyframe = false;
        }

        let pts = i64::try_from(packet.pts().as_nanos()).unwrap_or(i64::MAX);
        let mut frames = Vec::new();
        self.send(packet.data().into(), Some(pts), &mut frames)?;
        Ok(frames)
    }

    /// Emits every buffered picture at end of stream.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Decode`] when a buffered picture cannot be converted.
    pub fn flush(&mut self) -> Result<Vec<VideoFrame>> {
        let Some(inner) = self.inner.as_mut() else {
            return Ok(Vec::new());
        };
        inner.flush();
        let mut frames = Vec::new();
        loop {
            match inner.get_picture() {
                Ok(picture) => frames.push(convert(&picture)?),
                Err(Rav1dError::TryAgain) => return Ok(frames),
                Err(error) => return Err(error.into()),
            }
        }
    }

    /// Submits one temporal unit and drains every picture it releases.
    ///
    /// `send_data` refuses new data while pictures are pending, so the loop
    /// first collects what the decoder finished and then retries the remainder
    /// of the packet.
    fn send(
        &mut self,
        data: Box<[u8]>,
        pts: Option<i64>,
        frames: &mut Vec<VideoFrame>,
    ) -> Result<()> {
        let inner = self.decoder()?;
        let mut pending = match inner.send_data(data, None, pts, None) {
            Ok(()) => false,
            Err(Rav1dError::TryAgain) => true,
            Err(error) => return Err(error.into()),
        };

        loop {
            self.drain(frames)?;
            if !pending {
                return Ok(());
            }
            let inner = self.decoder()?;
            pending = match inner.send_pending_data() {
                Ok(()) => false,
                Err(Rav1dError::TryAgain) => true,
                Err(error) => return Err(error.into()),
            };
        }
    }

    /// Moves every finished picture into `frames`.
    fn drain(&mut self, frames: &mut Vec<VideoFrame>) -> Result<()> {
        let inner = self.decoder()?;
        loop {
            match inner.get_picture() {
                Ok(picture) => frames.push(convert(&picture)?),
                Err(Rav1dError::TryAgain) => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl VideoDecoder for Av1Decoder {
    fn decode(&mut self, packet: &VideoPacket) -> paleo::Result<Vec<VideoFrame>> {
        Av1Decoder::decode(self, packet).map_err(|error| paleo::Error::Decode(error.to_string()))
    }

    fn flush(&mut self) -> Vec<VideoFrame> {
        Av1Decoder::flush(self).unwrap_or_default()
    }

    fn reset(&mut self) {
        Av1Decoder::reset(self);
    }

    fn is_keyframe_data(&self, data: &[u8]) -> bool {
        Av1Decoder::is_keyframe_data(self, data)
    }

    fn is_awaiting_keyframe(&self) -> bool {
        Av1Decoder::is_awaiting_keyframe(self)
    }
}

/// Converts one decoded picture into RGBA.
fn convert(picture: &Picture) -> Result<VideoFrame> {
    let width = usize::try_from(picture.width())
        .map_err(|_| Error::Decode("picture width is out of range".to_owned()))?;
    let height = usize::try_from(picture.height())
        .map_err(|_| Error::Decode("picture height is out of range".to_owned()))?;
    let Some(BitsPerComponent(bit_depth)) = picture.bits_per_component() else {
        return Err(Error::Decode("picture has no component depth".to_owned()));
    };

    let (subsampling_x, subsampling_y) = match picture.pixel_layout() {
        PixelLayout::I400 | PixelLayout::I420 => (true, true),
        PixelLayout::I422 => (true, false),
        PixelLayout::I444 => (false, false),
    };
    // `plane` returns the component buffer; its stride is measured in pixels at
    // every bit depth, which is exactly what `rgba_from_yuv` expects.
    let plane = |component| {
        YuvPlane::new(picture.plane(component), {
            usize::try_from(picture.stride(component)).unwrap_or_default()
        })
    };
    let (u, v) = if picture.pixel_layout() == PixelLayout::I400 {
        (YuvPlane::new(&[], 0), YuvPlane::new(&[], 0))
    } else {
        (
            plane(PlanarImageComponent::U),
            plane(PlanarImageComponent::V),
        )
    };

    let matrix = match picture.matrix_coefficients() {
        rusty_av1d::pixel::MatrixCoefficients::BT709 => ColorMatrix::Bt709,
        rusty_av1d::pixel::MatrixCoefficients::BT470BG
        | rusty_av1d::pixel::MatrixCoefficients::ST170M => ColorMatrix::Bt601,
        _ => ColorMatrix::Auto,
    };
    let range = match picture.color_range() {
        rusty_av1d::pixel::YUVRange::Full => YuvRange::Full,
        rusty_av1d::pixel::YUVRange::Limited => YuvRange::Limited,
    };

    let mut pixels = Vec::new();
    let converted = rgba_from_yuv(
        width,
        height,
        bit_depth,
        subsampling_x,
        subsampling_y,
        [plane(PlanarImageComponent::Y), u, v],
        matrix,
        range,
        &mut pixels,
    );
    if !converted {
        return Err(Error::Decode("picture planes are malformed".to_owned()));
    }

    let pts = picture
        .timestamp()
        .and_then(|pts| u64::try_from(pts).ok())
        .map_or(Duration::ZERO, Duration::from_nanos);
    let width = u32::try_from(width).unwrap_or_default();
    let height = u32::try_from(height).unwrap_or_default();
    VideoFrame::new(pts, width, height, pixels).map_err(|error| Error::Decode(error.to_string()))
}
