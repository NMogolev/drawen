//! Audio-mastered Paleo playback for [`crate::Video`].
//!
//! [`AudioPaleoSession`] is the audio-enabled counterpart of
//! [`crate::PaleoSession`]. It opens the same [`paleo_player::Player`], but with
//! audio output enabled, so the device position is the master clock and video
//! follows it. The player decodes audio into the device format and keeps a
//! bounded video queue, so this type is a thin adapter over the shared session.
//!
//! The session is behind the `audio` feature of `drawen-widgets`, so a normal UI
//! build does not link `cpal` or a native audio backend.

use std::path::Path;

use crate::media::{MediaClock, MediaError, MediaMetadata, MediaSession, MediaUpdate};
use crate::paleo_session::PaleoSession;

/// A [`MediaSession`] that plays audio through a device and follows it with
/// video.
///
/// Audio is the master clock whenever an audio track and an output device are
/// available. Without them the session falls back to the wall-clock behavior of
/// [`crate::PaleoSession`]. Every codec with a Paleo backend is supported, and a
/// video codec without one is skipped so the audio still plays.
pub struct AudioPaleoSession(PaleoSession);

impl AudioPaleoSession {
    /// Opens `path`, prepares audio output, and decodes the first preview frame.
    ///
    /// # Errors
    ///
    /// Returns [`MediaError`] when the file cannot be opened, the selected audio
    /// codec has no decoder, no output device can be opened and video is also
    /// unusable, or a stream is malformed.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MediaError> {
        PaleoSession::open_audio(path).map(Self)
    }

    /// Returns the text of a recoverable playback problem, if one occurred.
    #[must_use]
    pub fn warning(&self) -> Option<&str> {
        self.0.warning()
    }
}

impl MediaSession for AudioPaleoSession {
    fn metadata(&self) -> Option<&MediaMetadata> {
        self.0.metadata()
    }

    fn clock(&self) -> &MediaClock {
        self.0.clock()
    }

    fn clock_mut(&mut self) -> &mut MediaClock {
        self.0.clock_mut()
    }

    fn play(&mut self) -> Result<(), MediaError> {
        self.0.play()
    }

    fn pause(&mut self) {
        self.0.pause();
    }

    fn seek(&mut self, position: std::time::Duration) -> Result<(), MediaError> {
        self.0.seek(position)
    }

    fn set_rate(&mut self, rate: f64) {
        self.0.set_rate(rate);
    }

    fn volume(&self) -> f32 {
        self.0.volume()
    }

    fn set_volume(&mut self, volume: f32) -> bool {
        self.0.set_volume(volume)
    }

    fn is_muted(&self) -> bool {
        self.0.is_muted()
    }

    fn set_muted(&mut self, muted: bool) -> bool {
        self.0.set_muted(muted)
    }

    fn supports_video_pause(&self) -> bool {
        self.0.supports_video_pause()
    }

    fn set_video_enabled(&mut self, enabled: bool) -> Result<bool, MediaError> {
        self.0.set_video_enabled(enabled)
    }

    fn update(&mut self, delta: std::time::Duration) -> MediaUpdate {
        self.0.update(delta)
    }

    fn is_active(&self) -> bool {
        self.0.is_active()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_missing_file() {
        let error = AudioPaleoSession::open("drawen-paleo-audio-missing-file.mp4")
            .err()
            .expect("a missing file cannot be opened");
        assert!(error.message().contains("could not open media"), "{error}");
    }
}
