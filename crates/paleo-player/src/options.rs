//! Opening options and the metadata a player exposes.

use std::time::Duration;

use paleo::{AudioTrackInfo, VideoTrackInfo};

/// Options for [`crate::Player::open_with`].
///
/// The defaults play both audio and video, start paused on the first frame, and
/// open the system's default audio device at the normal rate.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug)]
pub struct PlayerOptions {
    pub(crate) audio: bool,
    pub(crate) video: bool,
    pub(crate) autoplay: bool,
    pub(crate) looping: bool,
    pub(crate) rate: f64,
    pub(crate) volume: f32,
    pub(crate) muted: bool,
    pub(crate) audio_device: Option<String>,
    pub(crate) audio_latency: Option<Duration>,
}

impl Default for PlayerOptions {
    fn default() -> Self {
        Self {
            audio: true,
            video: true,
            autoplay: false,
            looping: false,
            rate: 1.0,
            volume: 1.0,
            muted: false,
            audio_device: None,
            audio_latency: None,
        }
    }
}

impl PlayerOptions {
    /// Creates the default options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Decodes audio and opens an output device when the file has an audio
    /// track.
    ///
    /// With `false`, audio packets are not demuxed and no device is opened,
    /// which is what a video-only view or a test build wants.
    #[must_use]
    pub const fn with_audio(mut self, audio: bool) -> Self {
        self.audio = audio;
        self
    }

    /// Decodes video when the file has a video track.
    ///
    /// With `false`, video packets are skipped and only audio is played, so a
    /// codec this build cannot decode does not prevent playback.
    #[must_use]
    pub const fn with_video(mut self, video: bool) -> Self {
        self.video = video;
        self
    }

    /// Starts playback immediately instead of staying paused on the first
    /// frame.
    #[must_use]
    pub const fn with_autoplay(mut self, autoplay: bool) -> Self {
        self.autoplay = autoplay;
        self
    }

    /// Restarts playback from the beginning when the end is reached.
    #[must_use]
    pub const fn with_looping(mut self, looping: bool) -> Self {
        self.looping = looping;
        self
    }

    /// Sets the initial playback rate, clamped to `0.05..=16.0`.
    ///
    /// A rate other than `1.0` suspends audio output and lets the wall clock
    /// drive video, because the device has no rate control. Returning to `1.0`
    /// seeks to the current position so audio starts in sync again.
    #[must_use]
    pub const fn with_rate(mut self, rate: f64) -> Self {
        self.rate = rate;
        self
    }

    /// Sets the initial volume, clamped to `0.0..=1.0`.
    #[must_use]
    pub const fn with_volume(mut self, volume: f32) -> Self {
        self.volume = volume;
        self
    }

    /// Starts with audio muted.
    #[must_use]
    pub const fn with_muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    /// Opens a specific output device instead of the system default.
    #[must_use]
    pub fn with_audio_device(mut self, id: impl Into<String>) -> Self {
        self.audio_device = Some(id.into());
        self
    }

    /// Requests a device buffer latency instead of the backend default.
    #[must_use]
    pub const fn with_audio_latency(mut self, latency: Duration) -> Self {
        self.audio_latency = Some(latency);
        self
    }
}

/// The tracks a player found when it opened a file, and what it can do with
/// them.
///
/// The track accessors describe the container: a file with an audio track
/// reports it even when no output device could be opened. [`Self::has_audio`]
/// and [`Self::has_video`] report what the player actually decodes, and
/// [`Self::audio_error`] and [`Self::video_error`] explain the difference.
#[derive(Clone, Debug)]
pub struct PlayerInfo {
    pub(crate) duration: Option<Duration>,
    pub(crate) video: Option<VideoTrackInfo>,
    pub(crate) audio: Option<AudioTrackInfo>,
    pub(crate) video_enabled: bool,
    pub(crate) audio_enabled: bool,
    pub(crate) video_error: Option<String>,
    pub(crate) audio_error: Option<String>,
}

impl PlayerInfo {
    /// Returns the container duration, or `None` when it is unknown.
    #[must_use]
    pub const fn duration(&self) -> Option<Duration> {
        self.duration
    }

    /// Returns the video track, or `None` for an audio-only file.
    #[must_use]
    pub const fn video(&self) -> Option<&VideoTrackInfo> {
        self.video.as_ref()
    }

    /// Returns the audio track, or `None` for a video-only file.
    #[must_use]
    pub const fn audio(&self) -> Option<&AudioTrackInfo> {
        self.audio.as_ref()
    }

    /// Returns the video width in pixels, or zero for an audio-only file.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.video.as_ref().map_or(0, VideoTrackInfo::width)
    }

    /// Returns the video height in pixels, or zero for an audio-only file.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.video.as_ref().map_or(0, VideoTrackInfo::height)
    }

    /// Returns the nominal frame rate, or `None` when the container does not
    /// report a usable one.
    #[must_use]
    pub fn frame_rate(&self) -> Option<f64> {
        self.video.as_ref().and_then(VideoTrackInfo::frame_rate)
    }

    /// Returns whether the player decodes the video track.
    ///
    /// This is `false` when the file has no video, when [`crate::PlayerOptions`]
    /// disabled video, and when the build has no backend for the codec.
    #[must_use]
    pub const fn has_video(&self) -> bool {
        self.video_enabled
    }

    /// Returns whether the player produces audio for this file.
    ///
    /// This is `false` when the file has no audio, when
    /// [`crate::PlayerOptions`] disabled audio, and when no output device could
    /// be opened.
    #[must_use]
    pub const fn has_audio(&self) -> bool {
        self.audio_enabled
    }

    /// Returns why the video track is not played, if it is not.
    #[must_use]
    pub fn video_error(&self) -> Option<&str> {
        self.video_error.as_deref()
    }

    /// Returns why the audio track is not played, if it is not.
    #[must_use]
    pub fn audio_error(&self) -> Option<&str> {
        self.audio_error.as_deref()
    }
}
