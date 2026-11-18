//! Codec selection for Paleo video playback.
//!
//! Paleo demuxes video packets and defines the [`VideoDecoder`] contract, but
//! it does not link a decoder. This crate maps a track's codec to the backend
//! that can decode it, so an application does not have to know which backends
//! the build contains:
//!
//! ```
//! use paleo::{Event, Media, VideoDecoder};
//! use paleo_video::open_decoder;
//!
//! # fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let mut media = Media::open("clip.mp4")?;
//! let info = media.info().video().ok_or("the file has no video")?;
//! let mut decoder = open_decoder(info)?;
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
//! Every backend is optional. The default build contains H.264, HEVC, VP9, and
//! AV1; a build that only needs one codec enables that feature and drops the
//! rest:
//!
//! ```toml
//! paleo-video = { version = "0.2", default-features = false, features = ["h264"] }
//! ```
//!
//! A track whose codec has no backend in this build fails with
//! [`paleo::Error::UnsupportedCodec`], naming both the codec and the backends
//! that are present, so the failure is actionable instead of a silent black
//! frame.

#![forbid(unsafe_code)]

use paleo::{Result, VideoCodec, VideoDecoder, VideoTrackInfo};

/// Opens a decoder for the codec of `info`.
///
/// The returned decoder is configured and ready for the first packet. It starts
/// in the "awaiting keyframe" state: the first packet that is not a
/// random-access point is skipped, which matches how a freshly opened file or a
/// seek begins.
///
/// # Errors
///
/// Returns [`paleo::Error::UnsupportedCodec`] when this build has no backend
/// for the track's codec, and [`paleo::Error::Decode`] when the backend cannot
/// be configured from the track metadata.
pub fn open_decoder(info: &VideoTrackInfo) -> Result<Box<dyn VideoDecoder>> {
    match info.codec() {
        #[cfg(feature = "h264")]
        VideoCodec::H264 => Ok(Box::new(open_h264(info)?)),
        #[cfg(feature = "hevc")]
        VideoCodec::Hevc => Ok(Box::new(open_hevc(info).map_err(hevc_error)?)),
        #[cfg(feature = "vp9")]
        VideoCodec::Vp9 => Ok(Box::new(paleo_vp9::Vp9Decoder::new(info)?)),
        #[cfg(feature = "av1")]
        VideoCodec::Av1 => Ok(Box::new(open_av1(info).map_err(av1_error)?)),
        codec => Err(unsupported(codec)),
    }
}

/// Returns every codec this build can decode.
#[must_use]
pub fn codecs() -> &'static [VideoCodec] {
    const CODECS: &[VideoCodec] = &[
        #[cfg(feature = "h264")]
        VideoCodec::H264,
        #[cfg(feature = "hevc")]
        VideoCodec::Hevc,
        #[cfg(feature = "vp9")]
        VideoCodec::Vp9,
        #[cfg(feature = "av1")]
        VideoCodec::Av1,
    ];
    CODECS
}

/// Reports whether this build can decode `codec`.
#[must_use]
pub fn supports(codec: VideoCodec) -> bool {
    codecs().contains(&codec)
}

/// Builds the error reported for a codec without a backend.
fn unsupported(codec: VideoCodec) -> paleo::Error {
    let present = codecs()
        .iter()
        .map(|codec| codec.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let present = if present.is_empty() {
        "no video backends".to_owned()
    } else {
        format!("this build decodes: {present}")
    };
    paleo::Error::UnsupportedCodec(format!("{codec} video ({present})"))
}

#[cfg(feature = "h264")]
fn open_h264(info: &VideoTrackInfo) -> Result<paleo_h264::H264Decoder> {
    use paleo_h264::{Error as Source, H264Decoder};

    let mut decoder = H264Decoder::new();
    decoder
        .configure(info.extra_data())
        .map_err(|error| match error {
            Source::MissingConfig => {
                paleo::Error::UnsupportedCodec("H.264 video without an avcC record".to_owned())
            }
            other => paleo::Error::Decode(other.to_string()),
        })?;
    Ok(decoder)
}

#[cfg(feature = "hevc")]
fn open_hevc(
    info: &VideoTrackInfo,
) -> std::result::Result<paleo_hevc::HevcDecoder, paleo_hevc::Error> {
    paleo_hevc::HevcDecoder::new(info)
}

#[cfg(feature = "hevc")]
fn hevc_error(error: paleo_hevc::Error) -> paleo::Error {
    use paleo_hevc::Error as Source;

    match error {
        Source::MissingConfig => paleo::Error::UnsupportedCodec(
            "HEVC video without an hvcC record or Annex-B framing".to_owned(),
        ),
        Source::UnsupportedCodec(detail) => paleo::Error::UnsupportedCodec(detail),
        other => paleo::Error::Decode(other.to_string()),
    }
}

#[cfg(feature = "av1")]
fn open_av1(info: &VideoTrackInfo) -> std::result::Result<paleo_av1::Av1Decoder, paleo_av1::Error> {
    paleo_av1::Av1Decoder::new(info)
}

#[cfg(feature = "av1")]
fn av1_error(error: paleo_av1::Error) -> paleo::Error {
    use paleo_av1::Error as Source;

    match error {
        Source::UnsupportedCodec(detail) => paleo::Error::UnsupportedCodec(detail),
        other => paleo::Error::Decode(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_build_has_backends() {
        assert!(!codecs().is_empty());
        assert!(codecs().windows(2).all(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn supports_agrees_with_the_codec_list() {
        for codec in [
            VideoCodec::H264,
            VideoCodec::Hevc,
            VideoCodec::Av1,
            VideoCodec::Vp8,
            VideoCodec::Vp9,
            VideoCodec::Mpeg4,
            VideoCodec::Mjpeg,
        ] {
            assert_eq!(supports(codec), codecs().contains(&codec));
        }
    }

    #[test]
    fn unsupported_codecs_name_the_codec_and_the_build() {
        let message = unsupported(VideoCodec::Vp8).to_string();
        assert!(message.contains("vp8"), "{message}");
        assert!(
            message.contains("this build decodes") || message.contains("no video backends"),
            "{message}"
        );
    }
}
