//! One-object video and audio playback for [Paleo](https://docs.rs/paleo).
//!
//! Paleo demuxes containers and decodes audio; separate backend crates decode
//! video. Wiring them together by hand means opening a file, selecting a
//! decoder, adapting the audio format to the device, seeking to keyframes,
//! buffering both streams, and deciding which clock is the master. This crate
//! does all of that behind a [`Player`]:
//!
//! ```no_run
//! use std::time::Duration;
//! use paleo_player::{Player, PlayerState};
//!
//! # fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let mut player = Player::open("clip.mp4")?;
//! player.play()?;
//!
//! let mut now = Duration::ZERO;
//! loop {
//!     let update = player.update(now)?;
//!     if update.state() == PlayerState::Ended {
//!         break;
//!     }
//!     if update.frame_changed() {
//!         let pixels = player.frame().map(paleo::VideoFrame::pixels);
//!         let _rgba: Option<&[u8]> = pixels;
//!     }
//!     now += Duration::from_millis(16);
//! }
//! # Ok(()) }
//! ```
//!
//! The player never calls platform APIs from a thread of its own and never
//! spawns one: [`Player::update`] does a bounded amount of demuxing and
//! decoding on the caller's thread, so a UI can call it once per frame. That
//! keeps the object safe to move into a widget tree or a single-threaded event
//! loop, and it keeps the frame queue bounded.
//!
//! # Clocks
//!
//! With audio at the normal rate, the output device is the master clock:
//! [`paleo_audio::AudioOutput::position`] reports the frames the device has
//! actually played, and video follows it. Without audio, or at a rate other
//! than `1.0`, the playhead advances from the `now` values passed to
//! [`Player::update`], which must be monotonic and use any fixed epoch.
//!
//! # Codecs
//!
//! Video decode is provided by [`paleo_video`], so the default build plays
//! H.264, HEVC, VP9, and AV1 in pure Rust. A track whose codec has no backend is
//! not fatal: the player skips that track, records the reason in
//! [`PlayerInfo::video_error`], and still plays the other one. Audio fallback
//! works the same way through [`PlayerInfo::audio_error`].
//!
//! # Feature flags
//!
//! The `device` feature (enabled by default) links `paleo-audio`'s `cpal`
//! backend. A build with `default-features = false` still decodes video and
//! keeps a playhead, but reports that no audio device is available.

#![forbid(unsafe_code)]

mod error;
mod options;
mod output;
mod timeline;

use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;

use paleo::{Event, Media, MediaSelection, VideoDecoder, VideoFrame};
use paleo_audio::{PcmBuffer, PcmConverter, PcmFormat};

pub use error::{Error, Result};
pub use options::{PlayerInfo, PlayerOptions};

use output::Output;
use timeline::{Timeline, is_realtime};

/// How far before a seek target to look for a keyframe.
const KEYFRAME_LOOKBACK: Duration = Duration::from_secs(10);
/// How many decoded pictures to keep ahead of the playhead.
const VIDEO_QUEUE_TARGET: usize = 3;
/// How much audio to keep ahead of the device.
const AUDIO_QUEUE_TARGET: Duration = Duration::from_millis(300);
/// Headroom left below the device ring so a queued packet fits.
const AUDIO_QUEUE_SLACK: Duration = Duration::from_millis(40);
/// Demuxing and decoding budget for one [`Player::update`] call.
const MAX_PACKETS_PER_UPDATE: usize = 96;

/// Where playback is in its lifecycle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlayerState {
    /// The playhead is not advancing.
    #[default]
    Paused,
    /// The playhead advances and frames are produced.
    Playing,
    /// The end of the file was reached and playback stopped.
    Ended,
}

impl PlayerState {
    /// Returns whether the playhead is advancing.
    #[must_use]
    pub const fn is_playing(self) -> bool {
        matches!(self, Self::Playing)
    }
}

/// What changed during one [`Player::update`] call.
///
/// The picture itself is not included: [`Player::frame`] borrows it, so a view
/// can draw it without copying the RGBA buffer.
#[derive(Clone, Debug)]
pub struct PlayerUpdate {
    frame_changed: bool,
    position: Duration,
    state: PlayerState,
    state_changed: bool,
    error: Option<String>,
}

impl PlayerUpdate {
    /// Returns whether the displayed picture changed since the previous update.
    #[must_use]
    pub const fn frame_changed(&self) -> bool {
        self.frame_changed
    }

    /// Returns the playhead after the update.
    #[must_use]
    pub const fn position(&self) -> Duration {
        self.position
    }

    /// Returns the playback state after the update.
    #[must_use]
    pub const fn state(&self) -> PlayerState {
        self.state
    }

    /// Returns whether the playback state changed since the previous update.
    ///
    /// Transitions made by [`Player::play`], [`Player::pause`], and
    /// [`Player::seek`] also count, because the flag compares against the
    /// previous update rather than the state at the start of this call.
    #[must_use]
    pub const fn state_changed(&self) -> bool {
        self.state_changed
    }

    /// Returns a recoverable problem seen during the update, if any.
    ///
    /// Playback continued; for example, an audio buffer was dropped because the
    /// device ring was still full. Fatal failures are returned as [`Error`].
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

/// A video and audio player for one file.
///
/// The player owns the container reader, the video decoder, and the audio
/// device. It does not own a window or a renderer: call [`Self::update`] from
/// your frame loop and draw [`Self::frame`] when it changes.
#[allow(clippy::struct_excessive_bools)]
pub struct Player {
    media: Media,
    info: PlayerInfo,
    decoder: Option<Box<dyn VideoDecoder>>,
    output: Option<Output>,
    converter: Option<PcmConverter>,
    timeline: Timeline,
    state: PlayerState,
    state_serial: u64,
    reported_serial: u64,
    looping: bool,
    volume: f32,
    muted: bool,
    video_queue: VecDeque<VideoFrame>,
    video_trim_from: Option<Duration>,
    frame: Option<VideoFrame>,
    frame_version: u64,
    reported_version: u64,
    last_now: Option<Duration>,
    eof: bool,
    flushed: bool,
    warning: Option<String>,
}

impl Player {
    /// Opens a file with [`PlayerOptions::default`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::Media`] when the file cannot be read or probed, and when
    /// neither track can be played at all.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(path, &PlayerOptions::default())
    }

    /// Opens a file with explicit options.
    ///
    /// A track the build cannot play is skipped instead of failing the open:
    /// the reason is kept in [`PlayerInfo::video_error`] or
    /// [`PlayerInfo::audio_error`], and opening fails only when both tracks are
    /// unusable.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Media`] when the file cannot be read or probed, and when
    /// neither track can be played at all.
    pub fn open_with(path: impl AsRef<Path>, options: &PlayerOptions) -> Result<Self> {
        let path = path.as_ref();
        let (media, audio_error) = Self::open_media(path, options)?;

        let info = media.info();
        let mut info = PlayerInfo {
            duration: info.duration(),
            video: info.video().cloned(),
            audio: info.audio().copied(),
            video_enabled: false,
            audio_enabled: false,
            video_error: None,
            audio_error,
        };

        let mut decoder = None;
        if let Some(track) = info.video.as_ref() {
            match paleo_video::open_decoder(track) {
                Ok(opened) => decoder = Some(opened),
                Err(error) => info.video_error = Some(error.to_string()),
            }
        }

        let mut output = None;
        let mut converter = None;
        if let Some(track) = info.audio.as_ref()
            && options.audio
            && info.audio_error.is_none()
        {
            match Self::open_audio(options, track.sample_rate(), track.channels()) {
                Ok((opened, adapted)) => {
                    output = Some(opened);
                    converter = Some(adapted);
                }
                Err(error) => info.audio_error = Some(error.to_string()),
            }
        }

        info.video_enabled = options.video && decoder.is_some();
        info.audio_enabled = output.is_some();

        if decoder.is_none() && output.is_none() {
            let detail = info
                .video_error
                .clone()
                .or_else(|| info.audio_error.clone())
                .unwrap_or_else(|| "the file has no playable tracks".to_owned());
            return Err(Error::Media(paleo::Error::UnsupportedCodec(detail)));
        }

        let mut player = Self {
            media,
            info,
            decoder,
            output,
            converter,
            timeline: Timeline::default(),
            state: PlayerState::Paused,
            state_serial: 0,
            reported_serial: 0,
            looping: options.looping,
            volume: clamp_volume(options.volume),
            muted: options.muted,
            video_queue: VecDeque::new(),
            video_trim_from: None,
            frame: None,
            frame_version: 0,
            reported_version: 0,
            last_now: None,
            eof: false,
            flushed: false,
            warning: None,
        };
        player.timeline.set_duration(player.info.duration);
        player.timeline.set_rate(options.rate);
        player.apply_volume();

        player.seek(Duration::ZERO)?;
        if options.autoplay {
            player.play()?;
        }
        Ok(player)
    }

    /// Returns the tracks the player opened.
    #[must_use]
    pub const fn info(&self) -> &PlayerInfo {
        &self.info
    }

    /// Returns the current playback state.
    #[must_use]
    pub const fn state(&self) -> PlayerState {
        self.state
    }

    /// Returns the playhead.
    #[must_use]
    pub const fn position(&self) -> Duration {
        self.timeline.position()
    }

    /// Returns the container duration, or `None` when it is unknown.
    #[must_use]
    pub const fn duration(&self) -> Option<Duration> {
        self.timeline.duration()
    }

    /// Returns the playback rate.
    #[must_use]
    pub const fn rate(&self) -> f64 {
        self.timeline.rate()
    }

    /// Returns the volume applied to the device.
    #[must_use]
    pub const fn volume(&self) -> f32 {
        self.volume
    }

    /// Returns whether audio is muted.
    #[must_use]
    pub const fn is_muted(&self) -> bool {
        self.muted
    }

    /// Returns whether playback restarts at the end.
    #[must_use]
    pub const fn is_looping(&self) -> bool {
        self.looping
    }

    /// Returns the picture that should be visible, if one has been produced.
    ///
    /// The frame is tightly packed RGBA with [`VideoFrame::stride`] bytes per
    /// row. Read [`Self::update`]'s
    /// [`frame_changed`](PlayerUpdate::frame_changed) before drawing it again.
    #[must_use]
    pub const fn frame(&self) -> Option<&VideoFrame> {
        self.frame.as_ref()
    }

    /// Starts or resumes playback.
    ///
    /// Playback that reached [`PlayerState::Ended`] starts again from the
    /// beginning.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Media`] when the file cannot be read or a packet cannot
    /// be decoded, and [`Error::Audio`] when the device cannot start.
    pub fn play(&mut self) -> Result<()> {
        let restart = self.state == PlayerState::Ended;
        let position = if restart {
            Duration::ZERO
        } else {
            self.timeline.position()
        };
        self.set_state(PlayerState::Playing);
        self.last_now = None;
        // Audio packets skipped while paused leave the stream out of step with
        // the playhead, so restart both queues at the current position.
        let prepared = if restart || self.output.is_some() {
            self.seek(position)
        } else {
            self.fill()
        };
        if let Err(error) = prepared {
            self.set_state(PlayerState::Paused);
            return Err(error);
        }
        if let Some(output) = self.output.as_mut()
            && let Err(error) = output.play()
        {
            self.set_state(PlayerState::Paused);
            return Err(error);
        }
        Ok(())
    }

    /// Stops the playhead and the audio device.
    pub fn pause(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.pause();
        }
        if self.state == PlayerState::Playing {
            self.set_state(PlayerState::Paused);
        }
        self.last_now = None;
    }

    /// Moves the playhead to `position`, clamped to the known duration.
    ///
    /// Video starts decoding at the keyframe at or before the target, then drops
    /// pictures before the target. This avoids filling the bounded picture queue
    /// with stale frames while audio starts at the requested position. When the
    /// player is paused, the picture at the new position is still decoded so a
    /// scrubber can show it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Media`] when the container refuses the seek or a packet
    /// cannot be decoded.
    pub fn seek(&mut self, position: Duration) -> Result<()> {
        let playing = self.state == PlayerState::Playing;
        self.timeline.seek(position);
        let target = self.timeline.position();

        self.video_queue.clear();
        self.video_trim_from = self.info.video_enabled.then_some(target);
        self.eof = false;
        self.flushed = false;

        let seek_video = self.info.video_enabled && self.decoder.is_some();
        if seek_video && let Some(decoder) = self.decoder.as_mut() {
            let media = &mut self.media;
            media.seek_keyframe(target, KEYFRAME_LOOKBACK, |data| {
                decoder.is_keyframe_data(data)
            })?;
            decoder.reset();
        } else {
            self.media.seek(target)?;
        }

        if let (Some(output), Some(converter)) = (self.output.as_mut(), self.converter.as_mut()) {
            output.reset(target);
            converter.reset_at(target);
            if playing {
                output.play()?;
            }
        }

        if self.state == PlayerState::Ended {
            self.set_state(PlayerState::Paused);
        }
        self.fill()?;
        self.present();
        Ok(())
    }

    /// Sets the playback rate, clamped to `0.05..=16.0`.
    ///
    /// A rate other than `1.0` pauses the audio device, because it has no rate
    /// control, and lets the wall clock drive video. Returning to `1.0` seeks to
    /// the current position so audio resumes in sync.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Media`] when the resynchronising seek fails.
    pub fn set_rate(&mut self, rate: f64) -> Result<()> {
        if !self.timeline.set_rate(rate) {
            return Ok(());
        }
        let position = self.timeline.position();
        self.last_now = None;
        if is_realtime(self.timeline.rate()) {
            // Audio was suspended on the way out of 1.0, so both queues have to
            // restart at the playhead before the device runs again.
            if self.output.is_some() {
                self.seek(position)?;
            }
            return Ok(());
        }
        if let Some(output) = self.output.as_mut() {
            output.pause();
        }
        Ok(())
    }

    /// Sets the volume, clamped to `0.0..=1.0`.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = clamp_volume(volume);
        self.apply_volume();
    }

    /// Mutes or unmutes audio without changing the volume.
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if let Some(output) = self.output.as_mut() {
            output.set_muted(muted);
        }
    }

    /// Restarts playback from the beginning when the end is reached.
    pub const fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    /// Enables or disables video decoding.
    ///
    /// Disabling video keeps audio playing and drops the buffered pictures;
    /// enabling it again resynchronises the decoder at the current position.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Media`] when video was never decodable, and when the
    /// resynchronising seek fails.
    pub fn set_video_enabled(&mut self, enabled: bool) -> Result<()> {
        if self.info.video_enabled == enabled {
            return Ok(());
        }
        if enabled {
            if self.decoder.is_none() {
                let detail =
                    self.info.video_error.clone().unwrap_or_else(|| {
                        "the video track has no decoder in this build".to_owned()
                    });
                return Err(Error::Media(paleo::Error::UnsupportedCodec(detail)));
            }
            self.info.video_enabled = true;
        } else {
            self.info.video_enabled = false;
        }
        self.seek(self.timeline.position())
    }

    /// Advances the player to `now` and returns what changed.
    ///
    /// `now` is a monotonic timestamp from any fixed epoch. It is only used as a
    /// difference, and it is ignored while the audio device is the master
    /// clock. Call this once per rendered frame.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Media`] when the container or a decoder fails, and
    /// [`Error::Audio`] when a device operation fails. Playback is paused on
    /// error so the caller can show the failure and retry.
    pub fn update(&mut self, now: Duration) -> Result<PlayerUpdate> {
        let result = self.update_inner(now);
        if result.is_err() {
            self.pause();
        }
        result
    }

    /// The body of [`Self::update`], split so every error path pauses once.
    fn update_inner(&mut self, now: Duration) -> Result<PlayerUpdate> {
        if self.state == PlayerState::Playing {
            self.advance(now);
            if self.reached_end() {
                if self.looping {
                    self.seek(Duration::ZERO)?;
                } else {
                    self.finish();
                }
            }
        }

        if self.state != PlayerState::Ended {
            self.fill()?;
            self.present();
        }

        let update = PlayerUpdate {
            frame_changed: self.frame_version != self.reported_version,
            position: self.timeline.position(),
            state: self.state,
            state_changed: self.state_serial != self.reported_serial,
            error: self.warning.take(),
        };
        self.reported_version = self.frame_version;
        self.reported_serial = self.state_serial;
        Ok(update)
    }

    /// Advances the playhead from the master clock.
    fn advance(&mut self, now: Duration) {
        if let Some(position) = self.audio_position() {
            self.timeline.synchronize(position);
        } else if let Some(last) = self.last_now {
            self.timeline.advance(now.saturating_sub(last));
        }
        self.last_now = Some(now);
    }

    /// Returns the device position when the device is the master clock.
    fn audio_position(&self) -> Option<Duration> {
        if is_realtime(self.timeline.rate()) {
            self.output.as_ref().map(Output::position)
        } else {
            None
        }
    }

    /// Returns whether every queued picture and sample has been presented.
    fn reached_end(&self) -> bool {
        if self.timeline.is_at_duration() {
            return true;
        }
        self.eof
            && self.video_queue.is_empty()
            && self
                .output
                .as_ref()
                .is_none_or(|output| output.buffered().is_zero())
    }

    /// Stops at the end of a non-looping file.
    fn finish(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.pause();
        }
        self.set_state(PlayerState::Ended);
        self.last_now = None;
    }

    /// Records a state transition so [`PlayerUpdate::state_changed`] can see it.
    fn set_state(&mut self, state: PlayerState) {
        if self.state != state {
            self.state = state;
            self.state_serial = self.state_serial.wrapping_add(1);
        }
    }

    /// Pushes the stored volume and mute flag into the device.
    fn apply_volume(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.set_volume(self.volume);
            output.set_muted(self.muted);
        }
    }

    /// Demuxes and decodes until both queues reach their targets.
    fn fill(&mut self) -> Result<()> {
        let audio_target = self.audio_target();
        let audio_active = audio_target.is_some();
        let mut packets = 0;
        while !self.eof && packets < MAX_PACKETS_PER_UPDATE {
            let need_video = self.info.video_enabled && self.video_queue.len() < VIDEO_QUEUE_TARGET;
            let need_audio = audio_target.is_some_and(|target| self.audio_buffered() < target);
            if !need_video && !need_audio {
                break;
            }
            packets += 1;

            match self.media.next_event()? {
                Event::Video(packet) => {
                    if self.info.video_enabled {
                        let frames = self.decoder.as_mut().map(|decoder| decoder.decode(&packet));
                        if let Some(frames) = frames {
                            for frame in frames? {
                                self.queue_frame(frame);
                            }
                        }
                    }
                }
                // Audio is dropped at a rate other than 1.0 and while paused:
                // queueing it would make the device disagree with the playhead.
                Event::Audio(buffer) => {
                    if audio_active {
                        self.queue_audio(buffer)?;
                    }
                }
                Event::EndOfStream => self.eof = true,
            }
        }

        if self.eof && !self.flushed {
            self.flushed = true;
            if self.info.video_enabled {
                let frames = self.decoder.as_mut().map(|decoder| decoder.flush());
                if let Some(frames) = frames {
                    for frame in frames {
                        self.queue_frame(frame);
                    }
                }
            }
        }
        Ok(())
    }

    /// Returns how much audio to keep queued, or `None` when audio is idle.
    fn audio_target(&self) -> Option<Duration> {
        if self.state != PlayerState::Playing || !is_realtime(self.timeline.rate()) {
            return None;
        }
        let output = self.output.as_ref()?;
        let capacity = output.capacity().saturating_sub(AUDIO_QUEUE_SLACK);
        Some(AUDIO_QUEUE_TARGET.min(capacity))
    }

    /// Returns how much audio is waiting in the device ring.
    fn audio_buffered(&self) -> Duration {
        self.output
            .as_ref()
            .map_or(Duration::ZERO, Output::buffered)
    }

    /// Converts one decoded block and queues it on the device.
    fn queue_audio(&mut self, buffer: paleo::AudioBuffer) -> Result<()> {
        let (Some(output), Some(converter)) = (self.output.as_mut(), self.converter.as_mut())
        else {
            return Ok(());
        };
        let pcm = converter.push(&PcmBuffer::from_paleo(buffer)?)?;
        if pcm.is_empty() {
            return Ok(());
        }
        let skipped = output.queue(&pcm)?;
        if skipped > 0 {
            self.warning = Some(format!("audio output dropped {skipped} frames"));
        }
        Ok(())
    }

    /// Inserts a picture in presentation order.
    fn queue_frame(&mut self, frame: VideoFrame) {
        let gate = gate_video_frame(frame.pts(), &mut self.video_trim_from);
        if gate == VideoFrameGate::Drop {
            return;
        }
        if gate == VideoFrameGate::Normal
            && self
                .frame
                .as_ref()
                .is_some_and(|current| frame.pts() <= current.pts())
        {
            return;
        }
        let index = self
            .video_queue
            .partition_point(|queued| queued.pts() <= frame.pts());
        self.video_queue.insert(index, frame);
    }

    /// Makes the newest picture at or before the playhead visible.
    fn present(&mut self) {
        let position = self.timeline.position();
        let mut presented = None;
        while let Some(frame) = self.video_queue.front() {
            if frame.pts() > position {
                break;
            }
            presented = self.video_queue.pop_front();
        }
        if let Some(frame) = presented {
            self.frame = Some(frame);
            self.frame_version = self.frame_version.wrapping_add(1);
        }
    }

    /// Opens the container, falling back to video only when its audio codec has
    /// no decoder.
    fn open_media(path: &Path, options: &PlayerOptions) -> Result<(Media, Option<String>)> {
        let audio_supported = cfg!(feature = "device") && options.audio;
        if !audio_supported {
            let reason = options.audio.then(|| {
                "this build has no audio device support (enable the `device` feature)".to_owned()
            });
            return Ok((Media::open_with(path, MediaSelection::VideoOnly)?, reason));
        }

        match Media::open(path) {
            Ok(media) => Ok((media, None)),
            Err(paleo::Error::UnsupportedCodec(detail)) => Ok((
                Media::open_with(path, MediaSelection::VideoOnly)?,
                Some(detail),
            )),
            Err(error) => Err(error.into()),
        }
    }

    /// Opens the device and builds the converter that targets its format.
    fn open_audio(
        options: &PlayerOptions,
        sample_rate: u32,
        channels: usize,
    ) -> Result<(Output, PcmConverter)> {
        let output = Output::open(options)?;
        let source = PcmFormat::new(sample_rate, channels)?;
        let converter = PcmConverter::new(source, output.format())?;
        Ok((output, converter))
    }
}

/// Clamps a requested volume to the range the device applies.
fn clamp_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

fn frame_is_before_target(pts: Duration, target: Duration) -> bool {
    pts < target
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VideoFrameGate {
    Drop,
    FirstAfterSeek,
    Normal,
}

fn gate_video_frame(pts: Duration, trim_from: &mut Option<Duration>) -> VideoFrameGate {
    let Some(target) = *trim_from else {
        return VideoFrameGate::Normal;
    };
    if frame_is_before_target(pts, target) {
        return VideoFrameGate::Drop;
    }
    *trim_from = None;
    VideoFrameGate::FirstAfterSeek
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seek_gate_discards_old_video_and_accepts_the_first_new_frame() {
        let target = Duration::from_secs(60);
        let mut trim_from = Some(target);

        assert_eq!(
            gate_video_frame(Duration::from_millis(59_999), &mut trim_from),
            VideoFrameGate::Drop
        );
        assert_eq!(trim_from, Some(target));
        assert_eq!(
            gate_video_frame(target, &mut trim_from),
            VideoFrameGate::FirstAfterSeek
        );
        assert_eq!(trim_from, None);
        assert_eq!(
            gate_video_frame(Duration::from_millis(60_001), &mut trim_from),
            VideoFrameGate::Normal
        );
        assert!(frame_is_before_target(
            Duration::from_millis(59_999),
            target
        ));
    }
}
