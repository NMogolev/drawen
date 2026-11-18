//! HEVC (H.265) video decoding backend for [Paleo](https://docs.rs/paleo).
//!
//! Paleo demuxes HEVC packets out of MP4 and Matroska containers. This crate
//! bridges them to the pure-Rust [`rusty_h265`] decoder:
//!
//! - the `hvcC` record supplies VPS/SPS/PPS and the NAL length size, and the
//!   parameter sets are replayed at every random-access point because
//!   length-prefixed packets leave them out of band;
//! - Annex-B packets are accepted as-is, which covers Matroska files that keep
//!   the stream in that form and files without an `hvcC` record at all;
//! - a keyframe starts a decodable sequence, so the backend waits for one
//!   after a reset;
//! - Main and Main 10 pictures at 4:2:0, 4:2:2, 4:4:4, and monochrome are
//!   converted to tightly packed RGBA through `paleo::rgba_from_yuv16`, and the
//!   picture's cropping window is applied on the way out.
//!
//! ```no_run
//! use paleo::{Event, Media, VideoDecoder};
//! use paleo_hevc::HevcDecoder;
//!
//! # fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let mut media = Media::open("clip.mp4")?;
//! let info = media.info().video().ok_or("the file has no video")?;
//! let mut decoder = HevcDecoder::new(info)?;
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
//! [`HevcDecoder::is_keyframe_data`], then [`paleo::VideoDecoder::reset`].

#![forbid(unsafe_code)]

mod error;
mod hvcc;

use std::time::Duration;

pub use error::{Error, Result};
use hvcc::HevcConfig;
use paleo::{
    ColorMatrix, VideoCodec, VideoDecoder, VideoExtraData, VideoExtraDataKind, VideoFrame,
    VideoPacket, VideoTrackInfo, YuvPlane16, YuvRange, rgba_from_yuv16,
};
use rusty_h265::{Decoder as Inner, Frame};

/// A streaming HEVC decoder driven by [`paleo::VideoPacket`] values.
pub struct HevcDecoder {
    decoder: Inner,
    config: Option<HevcConfig>,
    matrix: ColorMatrix,
    range: YuvRange,
    scratch: Vec<u8>,
    awaiting_keyframe: bool,
}

impl Default for HevcDecoder {
    fn default() -> Self {
        Self::new_unconfigured()
    }
}

impl HevcDecoder {
    /// Creates a decoder for an HEVC video track.
    ///
    /// The track's `hvcC` record is read when the container provides one. A
    /// track without it is still usable when its packets are Annex-B, because
    /// such streams carry VPS/SPS/PPS in band.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidConfig`] when the `hvcC` record is malformed,
    /// and [`Error::UnsupportedCodec`] when the track is not HEVC.
    pub fn new(info: &VideoTrackInfo) -> Result<Self> {
        let mut decoder = Self::with_codec(info.codec())?;
        if let Some(record) = info.hevc_decoder_config() {
            decoder.configure_hvcc(record)?;
        }
        Ok(decoder)
    }

    /// Creates a decoder for a known HEVC track without requiring
    /// [`VideoTrackInfo`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedCodec`] when `codec` is not
    /// [`paleo::VideoCodec::Hevc`].
    pub fn with_codec(codec: VideoCodec) -> Result<Self> {
        if codec != VideoCodec::Hevc {
            return Err(Error::UnsupportedCodec(format!(
                "{codec} video (the paleo-hevc backend decodes HEVC only)"
            )));
        }
        Ok(Self::new_unconfigured())
    }

    fn new_unconfigured() -> Self {
        Self {
            decoder: Inner::new(),
            config: None,
            matrix: ColorMatrix::Auto,
            range: YuvRange::Limited,
            scratch: Vec::new(),
            awaiting_keyframe: true,
        }
    }

    /// Reads the `hvcC` record from track metadata.
    ///
    /// # Errors
    ///
    /// Returns [`Error::MissingConfig`] when the track carries no `hvcC`
    /// record, and [`Error::InvalidConfig`] when the record is malformed.
    pub fn configure(&mut self, extra_data: &[VideoExtraData]) -> Result<()> {
        let data = extra_data
            .iter()
            .find(|extra| extra.kind() == VideoExtraDataKind::HevcDecoderConfig)
            .map(VideoExtraData::data)
            .ok_or(Error::MissingConfig)?;
        self.configure_hvcc(data)
    }

    /// Reads an `HEVCDecoderConfigurationRecord` directly.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidConfig`] when the record is malformed.
    pub fn configure_hvcc(&mut self, record: &[u8]) -> Result<()> {
        self.config = Some(HevcConfig::parse(record)?);
        self.reset();
        Ok(())
    }

    /// Sets the YUV to RGB matrix. The default derives it from the frame height.
    pub const fn set_color_matrix(&mut self, matrix: ColorMatrix) {
        self.matrix = matrix;
    }

    /// Sets whether samples use the full or the studio range.
    pub const fn set_range(&mut self, range: YuvRange) {
        self.range = range;
    }

    /// Drops decoder state and waits for the next random-access point.
    ///
    /// Call this after seeking. Packets before the next intra random access
    /// point are skipped because inter pictures cannot be decoded from an
    /// arbitrary point.
    pub fn reset(&mut self) {
        self.decoder = Inner::new();
        self.awaiting_keyframe = true;
    }

    /// Reports whether the packet is a random-access point.
    #[must_use]
    pub fn is_keyframe(&self, packet: &VideoPacket) -> bool {
        self.is_keyframe_data(packet.data())
    }

    /// Reports whether raw packet data contains an intra random access point.
    ///
    /// This is the byte-level form of [`Self::is_keyframe`], used while scanning
    /// the demuxer for the keyframe that precedes a seek target.
    #[must_use]
    pub fn is_keyframe_data(&self, data: &[u8]) -> bool {
        match self.config.as_ref() {
            Some(config) => config.is_irap(data),
            None => hvcc::annex_b_is_irap(data),
        }
    }

    /// Reports whether decoding is waiting for the next random-access packet.
    #[must_use]
    pub const fn is_awaiting_keyframe(&self) -> bool {
        self.awaiting_keyframe
    }

    /// Decodes one packet and returns the pictures that are ready to present.
    ///
    /// The result is empty while waiting for a random-access picture after a
    /// reset, and while the decoder's reorder window holds pictures whose
    /// display order is not known yet. Because HEVC permits reordering, one
    /// packet can also complete an earlier picture and return it here.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidPacket`] for malformed NAL framing and
    /// [`Error::Decode`] when the bitstream uses an unsupported tool.
    pub fn decode(&mut self, packet: &VideoPacket) -> Result<Vec<VideoFrame>> {
        let keyframe = self.is_keyframe_data(packet.data());
        if self.awaiting_keyframe {
            if !keyframe {
                return Ok(Vec::new());
            }
            self.awaiting_keyframe = false;
        }

        if let Some(config) = self.config.as_ref() {
            config.to_annex_b(packet.data(), &mut self.scratch, keyframe)?;
        } else {
            // Without an `hvcC` record the only framing we can honour is
            // Annex-B, where parameter sets travel with the pictures.
            if !hvcc::annex_b_nal_header_is_valid(packet.data()) {
                return Err(Error::MissingConfig);
            }
            self.scratch.clear();
            self.scratch.extend_from_slice(packet.data());
        }

        let pts = i64::try_from(packet.pts().as_nanos()).unwrap_or(i64::MAX);
        self.decoder
            .push_annexb(&self.scratch, Some(pts))
            .map_err(Error::from)?;

        let mut frames = Vec::new();
        while let Ok(frame) = self.decoder.next_frame() {
            frames.push(self.convert(&frame)?);
        }
        Ok(frames)
    }

    /// Emits every buffered picture at end of stream.
    #[must_use]
    pub fn flush(&mut self) -> Vec<VideoFrame> {
        self.decoder.flush();
        let mut frames = Vec::new();
        while let Ok(frame) = self.decoder.next_frame() {
            if let Ok(frame) = self.convert(&frame) {
                frames.push(frame);
            }
        }
        frames
    }

    /// Converts one decoded picture, applying its cropping window.
    fn convert(&self, frame: &Frame) -> Result<VideoFrame> {
        let picture = &frame.picture;
        let width = frame.width;
        let height = frame.height;
        if width == 0 || height == 0 {
            return Err(Error::Decode(
                "decoded picture has no visible area".to_owned(),
            ));
        }

        let (crop_x, crop_y, _, _) = picture.crop;
        let (subsampling_x, subsampling_y) = match picture.chroma_format_idc {
            1 => (true, true),
            2 => (true, false),
            _ => (false, false),
        };
        let (chroma_x, chroma_y) = match picture.chroma_format_idc {
            1 => (crop_x / 2, crop_y / 2),
            2 => (crop_x / 2, crop_y),
            _ => (crop_x, crop_y),
        };

        let plane = |index: usize, x: usize, y: usize| {
            let plane = &picture.planes[index];
            YuvPlane16::with_offset(&plane.data, plane.stride, x, y)
        };

        let bit_depth = picture.bit_depth_luma;
        let mut pixels = Vec::new();
        let converted = rgba_from_yuv16(
            width,
            height,
            bit_depth,
            subsampling_x,
            subsampling_y,
            [
                plane(0, crop_x, crop_y),
                plane(1, chroma_x, chroma_y),
                plane(2, chroma_x, chroma_y),
            ],
            self.matrix,
            self.range,
            &mut pixels,
        );
        if !converted {
            return Err(Error::Decode(
                "decoded picture planes do not cover the visible area".to_owned(),
            ));
        }

        let pts = frame
            .pts
            .and_then(|pts| u64::try_from(pts).ok())
            .map_or(Duration::ZERO, Duration::from_nanos);
        let width = u32::try_from(width).unwrap_or_default();
        let height = u32::try_from(height).unwrap_or_default();
        VideoFrame::new(pts, width, height, pixels)
            .map_err(|error| Error::Decode(error.to_string()))
    }
}

impl VideoDecoder for HevcDecoder {
    fn decode(&mut self, packet: &VideoPacket) -> paleo::Result<Vec<VideoFrame>> {
        HevcDecoder::decode(self, packet).map_err(|error| paleo::Error::Decode(error.to_string()))
    }

    fn flush(&mut self) -> Vec<VideoFrame> {
        HevcDecoder::flush(self)
    }

    fn reset(&mut self) {
        HevcDecoder::reset(self);
    }

    fn is_keyframe_data(&self, data: &[u8]) -> bool {
        HevcDecoder::is_keyframe_data(self, data)
    }

    fn is_awaiting_keyframe(&self) -> bool {
        HevcDecoder::is_awaiting_keyframe(self)
    }
}
