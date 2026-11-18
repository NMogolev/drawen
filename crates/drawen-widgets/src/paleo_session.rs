//! Pure-Rust Paleo playback for [`crate::Video`].
//!
//! [`PaleoSession`] adapts a [`paleo_player::Player`] to the backend-neutral
//! [`MediaSession`] contract. The player demuxes, decodes, and keeps its own
//! picture queue on the calling thread; this session mirrors the player's
//! timeline into a [`MediaClock`] so widgets can draw progress and controls,
//! and converts the current picture into a [`MediaFrame`].
//!
//! Every video codec with a backend in the Paleo stack is available here: the
//! default build decodes H.264, HEVC, VP9, and AV1 in pure Rust.

use std::path::Path;
use std::time::Duration;

use drawen_render::RasterImage;
use paleo_player::{Player, PlayerOptions, PlayerState};

use crate::media::{MediaClock, MediaError, MediaFrame, MediaMetadata, MediaSession, MediaUpdate};

/// Frame rate assumed when the container does not report one.
const DEFAULT_FRAME_RATE: f64 = 30.0;

/// A [`MediaSession`] backed by Paleo and pure-Rust decoders.
///
/// The session decodes on the calling thread in bounded steps: each
/// [`MediaSession::update`] advances the player and returns at most one new
/// picture, so the view never builds an unbounded frame queue.
pub struct PaleoSession {
    player: Player,
    clock: MediaClock,
    metadata: MediaMetadata,
    frame_duration: Duration,
    now: Duration,
    pending_error: Option<MediaError>,
}

impl PaleoSession {
    /// Opens `path` and decodes the first picture.
    ///
    /// # Errors
    ///
    /// Returns [`MediaError`] when the file cannot be opened, has no video
    /// track, uses a codec this build cannot decode, or fails to decode.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MediaError> {
        Self::open_player(path.as_ref(), &PlayerOptions::new().with_audio(false))
    }

    /// Opens `path` with audio output and follows video to the audio clock.
    ///
    /// If the file has no audio track, or no output device can be opened, the
    /// session falls back to wall-clock playback of the video alone.
    ///
    /// # Errors
    ///
    /// Returns [`MediaError`] when the file cannot be opened or fails to decode.
    #[cfg(feature = "audio")]
    pub fn open_audio(path: impl AsRef<Path>) -> Result<Self, MediaError> {
        Self::open_player(path.as_ref(), &PlayerOptions::new())
    }

    /// Opens `path` with explicit player options.
    fn open_player(path: &Path, options: &PlayerOptions) -> Result<Self, MediaError> {
        let player = Player::open_with(path, options)
            .map_err(|error| MediaError::new(format!("could not open media: {error}")))?;

        let info = player.info();
        let duration = player.duration().unwrap_or_default();
        let frame_duration = info
            .frame_rate()
            .filter(|rate| rate.is_finite() && *rate > 0.0)
            .map_or_else(
                || Duration::from_secs_f64(1.0 / DEFAULT_FRAME_RATE),
                |rate| Duration::from_secs_f64(1.0 / rate),
            );
        let metadata = MediaMetadata::new(info.width(), info.height(), duration);

        let mut clock = MediaClock::new();
        clock.set_duration(Some(duration));
        clock.seek(Duration::ZERO);

        Ok(Self {
            player,
            clock,
            metadata,
            frame_duration,
            now: Duration::ZERO,
            pending_error: None,
        })
    }

    /// Returns the text of a recoverable playback problem, if one occurred.
    #[must_use]
    pub fn warning(&self) -> Option<&str> {
        self.player
            .info()
            .audio_error()
            .or_else(|| self.player.info().video_error())
    }

    /// Copies the current picture into a presentable frame.
    fn current_frame(&self) -> Option<MediaFrame> {
        let frame = self.player.frame()?;
        let image =
            RasterImage::new(frame.width(), frame.height(), frame.pixels().to_vec()).ok()?;
        Some(MediaFrame::new(image, frame.pts(), self.frame_duration))
    }

    /// Mirrors the player's state into the widget-facing clock.
    fn synchronize_clock(&mut self, position: Duration, state: PlayerState) -> bool {
        let mut changed = self.clock.synchronize(position);
        changed |= match state {
            PlayerState::Playing => self.clock.play(),
            PlayerState::Paused => self.clock.pause(),
            PlayerState::Ended => self.clock.finish(),
        };
        changed
    }
}

impl MediaSession for PaleoSession {
    fn metadata(&self) -> Option<&MediaMetadata> {
        Some(&self.metadata)
    }

    fn clock(&self) -> &MediaClock {
        &self.clock
    }

    fn clock_mut(&mut self) -> &mut MediaClock {
        &mut self.clock
    }

    fn play(&mut self) -> Result<(), MediaError> {
        self.player
            .play()
            .map_err(|error| MediaError::new(format!("could not play: {error}")))?;
        self.clock.synchronize(self.player.position());
        self.clock.play();
        Ok(())
    }

    fn pause(&mut self) {
        self.player.pause();
        self.clock.pause();
    }

    fn seek(&mut self, position: Duration) -> Result<(), MediaError> {
        self.player
            .seek(position)
            .map_err(|error| MediaError::new(format!("seek failed: {error}")))?;
        self.clock.seek(self.player.position());
        Ok(())
    }

    fn set_rate(&mut self, rate: f64) {
        if let Err(error) = self.player.set_rate(rate) {
            self.pending_error = Some(MediaError::new(format!("could not change rate: {error}")));
        }
        self.clock.set_rate(self.player.rate());
    }

    fn supports_video_pause(&self) -> bool {
        // Only a session with an independent audio path can keep playing while
        // video stops; without audio, "pause video" would stop the media.
        self.player.info().has_audio()
    }

    fn set_video_enabled(&mut self, enabled: bool) -> Result<bool, MediaError> {
        let before = self.player.info().has_video();
        self.player.set_video_enabled(enabled).map_err(|error| {
            MediaError::new(format!("could not change video decoding: {error}"))
        })?;
        Ok(before != self.player.info().has_video())
    }

    fn update(&mut self, delta: Duration) -> MediaUpdate {
        if let Some(error) = self.pending_error.take() {
            self.clock.fail();
            return MediaUpdate {
                frame: None,
                error: Some(error),
                state_changed: true,
            };
        }

        self.now = self.now.saturating_add(delta);
        match self.player.update(self.now) {
            Ok(update) => {
                let state_changed = self.synchronize_clock(update.position(), update.state());
                let frame = if update.frame_changed() {
                    self.current_frame()
                } else {
                    None
                };
                MediaUpdate {
                    frame,
                    error: update
                        .error()
                        .map(|detail| MediaError::new(detail.to_owned())),
                    state_changed,
                }
            }
            Err(error) => {
                self.clock.fail();
                MediaUpdate {
                    frame: None,
                    error: Some(MediaError::new(error.to_string())),
                    state_changed: true,
                }
            }
        }
    }

    fn is_active(&self) -> bool {
        self.player.state() == PlayerState::Playing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_missing_file() {
        let error = PaleoSession::open("drawen-paleo-missing-file.mp4")
            .err()
            .expect("a missing file cannot be opened");
        assert!(error.message().contains("could not open media"), "{error}");
    }
}
