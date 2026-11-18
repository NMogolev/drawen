//! VP9 video decoding backend for [Paleo](https://docs.rs/paleo).
//!
//! Paleo demuxes VP9 packets out of `WebM` and Matroska containers. This crate
//! bridges them to the pure-Rust [`rusty_vp9`] decoder:
//!
//! - a keyframe starts a decodable sequence, so the backend waits for one after
//!   a reset;
//! - superframes are split by the decoder itself, and hidden reference frames
//!   are not presented;
//! - 8-, 10-, and 12-bit 4:2:0, 4:2:2, and 4:4:4 pictures are converted to
//!   tightly packed RGBA through `paleo::rgba_from_yuv`.
//!
//! ```no_run
//! use paleo::{Event, Media, VideoDecoder};
//! use paleo_vp9::Vp9Decoder;
//!
//! # fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let mut media = Media::open("clip.webm")?;
//! let info = media.info().video().ok_or("the file has no video")?;
//! let mut decoder = Vp9Decoder::new(info)?;
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
//! [`Vp9Decoder::is_keyframe_data`], then [`paleo::VideoDecoder::reset`].

#![forbid(unsafe_code)]

use std::time::Duration;

use paleo::{
    ColorMatrix, Error, Result, VideoCodec, VideoDecoder, VideoFrame, VideoPacket, VideoTrackInfo,
    YuvPlane, YuvRange, rgba_from_yuv,
};
use rusty_vp9::{BitReader, Error as Vp9Error, Vp9Decoder as Inner, parse_uncompressed_header};

/// A streaming VP9 decoder driven by [`paleo::VideoPacket`] values.
pub struct Vp9Decoder {
    inner: Inner,
    awaiting_keyframe: bool,
}

impl Vp9Decoder {
    /// Creates a decoder for a VP9 video track.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedCodec`] when the track is not VP9.
    pub fn new(info: &VideoTrackInfo) -> Result<Self> {
        Self::with_codec(info.codec())
    }

    /// Creates a decoder for a known VP9 track without requiring [`VideoTrackInfo`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedCodec`] when `codec` is not [`VideoCodec::Vp9`].
    pub fn with_codec(codec: VideoCodec) -> Result<Self> {
        if codec != VideoCodec::Vp9 {
            return Err(Error::UnsupportedCodec(format!(
                "{codec} video (the paleo-vp9 backend decodes VP9 only)"
            )));
        }
        Ok(Self {
            inner: Inner::new(),
            awaiting_keyframe: true,
        })
    }
}

impl VideoDecoder for Vp9Decoder {
    fn decode(&mut self, packet: &VideoPacket) -> Result<Vec<VideoFrame>> {
        let keyframe = self.is_keyframe(packet);
        if self.awaiting_keyframe {
            if !keyframe {
                return Ok(Vec::new());
            }
            self.awaiting_keyframe = false;
        }

        let pts = timestamp(packet.pts());
        self.inner
            .push(packet.data(), Some(pts))
            .map_err(|error| decode_error("VP9 packet", &error))?;

        let mut frames = Vec::new();
        loop {
            match self.inner.next_frame() {
                Ok(frame) => frames.push(convert(&frame)?),
                Err(Vp9Error::Again | Vp9Error::Eof) => break,
                Err(error) => return Err(decode_error("VP9 frame", &error)),
            }
        }
        Ok(frames)
    }

    fn flush(&mut self) -> Vec<VideoFrame> {
        self.inner.flush();
        let mut frames = Vec::new();
        while let Ok(frame) = self.inner.next_frame() {
            if let Ok(frame) = convert(&frame) {
                frames.push(frame);
            }
        }
        frames
    }

    fn reset(&mut self) {
        self.inner = Inner::new();
        self.awaiting_keyframe = true;
    }

    fn is_keyframe_data(&self, data: &[u8]) -> bool {
        let mut reader = BitReader::new(data);
        parse_uncompressed_header(&mut reader, &[(0, 0); 8])
            .is_ok_and(|header| header.key_frame && !header.show_existing_frame)
    }

    fn is_awaiting_keyframe(&self) -> bool {
        self.awaiting_keyframe
    }
}

fn timestamp(pts: Duration) -> i64 {
    i64::try_from(pts.as_nanos()).unwrap_or(i64::MAX)
}

fn decode_error(context: &str, error: &Vp9Error) -> Error {
    Error::Decode(format!("{context}: {error}"))
}

fn convert(frame: &rusty_vp9::DecodedFrame) -> Result<VideoFrame> {
    let width = usize::try_from(frame.width)
        .map_err(|_| Error::Decode("VP9 frame width is out of range".to_owned()))?;
    let height = usize::try_from(frame.height)
        .map_err(|_| Error::Decode("VP9 frame height is out of range".to_owned()))?;
    let bit_depth = u8::try_from(frame.bit_depth)
        .map_err(|_| Error::Decode("VP9 bit depth is out of range".to_owned()))?;
    let bytes_per_sample = usize::from(bit_depth > 8) + 1;

    let plane = |index: usize| -> YuvPlane<'_> {
        let data = frame.planes.get(index).map_or(&[][..], Vec::as_slice);
        let stride = frame
            .strides
            .get(index)
            .map_or(0, |stride| stride / bytes_per_sample);
        YuvPlane::new(data, stride)
    };

    let mut pixels = Vec::new();
    if !rgba_from_yuv(
        width,
        height,
        bit_depth,
        frame.subsampling_x != 0,
        frame.subsampling_y != 0,
        [plane(0), plane(1), plane(2)],
        ColorMatrix::Auto,
        YuvRange::Limited,
        &mut pixels,
    ) {
        return Err(Error::Decode("VP9 frame planes are malformed".to_owned()));
    }

    let pts = frame
        .pts
        .and_then(|pts| u64::try_from(pts).ok())
        .map_or(Duration::ZERO, Duration::from_nanos);
    let width = u32::try_from(width).unwrap_or_default();
    let height = u32::try_from(height).unwrap_or_default();
    VideoFrame::new(pts, width, height, pixels)
}
