//! Shared configuration and presentation helpers for the player widgets.
//!
//! [`PlayerTheme`], [`PlayerBehavior`], [`VideoControls`], and [`AudioControls`]
//! are plain values. Application code can change them with builder methods on
//! [`crate::Video`] and [`crate::AudioPlayer`], replace them as a whole, or keep
//! one configuration and reuse it for several players.
//!
//! The module also owns [`PlayerCore`], the session, clock, frame scheduler, and
//! volume state shared by every player. Keeping that logic in one place means a
//! video surface and an audio-only player observe the same clock semantics and
//! the same looping, seeking, and error behavior.

use std::time::Duration;

use drawen_core::{Affine, Color, Point, Rect, Size};
use drawen_render::PaintContext;
use drawen_ui::{DssStyle, Theme};

use crate::media::{MediaClock, MediaFrame, MediaFrameScheduler, MediaMetadata, MediaSession};

/// How the elapsed and total time labels are formatted.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlayerTimeDisplay {
    /// Show both elapsed and total time, for example `1:02 / 4:10`.
    #[default]
    ElapsedAndDuration,
    /// Show only the elapsed time.
    Elapsed,
    /// Show only the total duration.
    Duration,
    /// Show the remaining time with a leading minus.
    Remaining,
}

impl PlayerTimeDisplay {
    /// Formats `position` and `duration` according to this mode.
    #[must_use]
    pub fn format(self, position: Duration, duration: Duration) -> String {
        match self {
            Self::ElapsedAndDuration => {
                format!("{} / {}", format_time(position), format_time(duration))
            }
            Self::Elapsed => format_time(position),
            Self::Duration => format_time(duration),
            Self::Remaining => {
                format!("-{}", format_time(duration.saturating_sub(position)))
            }
        }
    }
}

/// Visual properties shared by video and audio players.
///
/// Every color is optional. When a color is absent the player uses the DSS
/// value resolved for its node, and then the runtime theme. That keeps players
/// consistent with an application stylesheet while still allowing a fully
/// custom palette.
///
/// The video overlay is the one exception: it paints on a dark scrim, so its
/// text and icons default to white regardless of the application theme. An
/// explicit [`Self::foreground`] (or a DSS `foreground` on the `video` node)
/// still overrides that default.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerTheme {
    /// Surface behind the picture or artwork. Resolves from the theme.
    pub background: Option<Color>,
    /// Panel or artwork background. Resolves from the theme surface.
    pub surface: Option<Color>,
    /// Primary text and icon color. Resolves from the DSS or theme foreground.
    /// The video overlay defaults to white instead, because its scrim is dark.
    pub foreground: Option<Color>,
    /// Secondary text color. Resolves from the theme muted foreground.
    pub muted_foreground: Option<Color>,
    /// Played portion, active button, and focus color.
    pub accent: Option<Color>,
    /// Unplayed seek-track color.
    pub track: Option<Color>,
    /// Background of the translucent control scrim.
    pub overlay: Option<Color>,
    /// Explicit color of the center play badge and the seek thumb. The center
    /// badge defaults to a translucent dark disc; the thumb defaults to the
    /// accent.
    pub badge: Option<Color>,
    /// Color used for fatal playback errors. Resolves from the theme danger.
    pub error: Option<Color>,
    /// Default corner radius of the surface and control badges.
    pub corner_radius: f32,
    /// Height of the video control row in logical pixels.
    pub control_height: f32,
    /// Inset from the player edge to its controls.
    pub padding: f32,
    /// Gap between adjacent controls.
    pub spacing: f32,
    /// Square size of a transport button.
    pub button_size: f32,
    /// Height of the seek track.
    pub track_height: f32,
    /// Size of regular labels.
    pub font_size: f32,
    /// Size of the title.
    pub title_size: f32,
    /// Size of the subtitle.
    pub subtitle_size: f32,
    /// Size of time labels.
    pub time_size: f32,
}

impl Default for PlayerTheme {
    fn default() -> Self {
        Self {
            background: None,
            surface: None,
            foreground: None,
            muted_foreground: None,
            accent: None,
            track: None,
            overlay: None,
            badge: None,
            error: None,
            corner_radius: 8.0,
            control_height: 50.0,
            padding: 12.0,
            spacing: 4.0,
            button_size: 34.0,
            track_height: 3.0,
            font_size: 13.0,
            title_size: 16.0,
            subtitle_size: 12.0,
            time_size: 13.0,
        }
    }
}

impl PlayerTheme {
    /// Creates a theme whose values resolve from DSS and the runtime theme.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns this theme with an explicit picture background.
    #[must_use]
    pub const fn with_background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// Returns this theme with an explicit panel or artwork background.
    #[must_use]
    pub const fn with_surface(mut self, color: Color) -> Self {
        self.surface = Some(color);
        self
    }

    /// Returns this theme with an explicit primary color.
    #[must_use]
    pub const fn with_foreground(mut self, color: Color) -> Self {
        self.foreground = Some(color);
        self
    }

    /// Returns this theme with an explicit secondary text color.
    #[must_use]
    pub const fn with_muted_foreground(mut self, color: Color) -> Self {
        self.muted_foreground = Some(color);
        self
    }

    /// Returns this theme with an explicit accent color.
    #[must_use]
    pub const fn with_accent(mut self, color: Color) -> Self {
        self.accent = Some(color);
        self
    }

    /// Returns this theme with an explicit unplayed track color.
    #[must_use]
    pub const fn with_track(mut self, color: Color) -> Self {
        self.track = Some(color);
        self
    }

    /// Returns this theme with an explicit control-scrim color.
    #[must_use]
    pub const fn with_overlay(mut self, color: Color) -> Self {
        self.overlay = Some(color);
        self
    }

    /// Returns this theme with an explicit center-badge color.
    #[must_use]
    pub const fn with_badge(mut self, color: Color) -> Self {
        self.badge = Some(color);
        self
    }

    /// Returns this theme with an explicit error color.
    #[must_use]
    pub const fn with_error(mut self, color: Color) -> Self {
        self.error = Some(color);
        self
    }

    /// Returns this theme with a different surface corner radius.
    #[must_use]
    pub const fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }

    /// Returns this theme with a different video control-row height.
    #[must_use]
    pub const fn with_control_height(mut self, height: f32) -> Self {
        self.control_height = height;
        self
    }

    /// Returns this theme with a different edge padding.
    #[must_use]
    pub const fn with_padding(mut self, padding: f32) -> Self {
        self.padding = padding;
        self
    }

    /// Returns this theme with a different gap between controls.
    #[must_use]
    pub const fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Returns this theme with a different transport-button size.
    #[must_use]
    pub const fn with_button_size(mut self, size: f32) -> Self {
        self.button_size = size;
        self
    }

    /// Returns this theme with a different seek-track height.
    #[must_use]
    pub const fn with_track_height(mut self, height: f32) -> Self {
        self.track_height = height;
        self
    }

    /// Returns this theme with a different regular font size.
    #[must_use]
    pub const fn with_font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// Returns this theme with a different title font size.
    #[must_use]
    pub const fn with_title_size(mut self, size: f32) -> Self {
        self.title_size = size;
        self
    }

    /// Returns this theme with a different subtitle font size.
    #[must_use]
    pub const fn with_subtitle_size(mut self, size: f32) -> Self {
        self.subtitle_size = size;
        self
    }

    /// Returns this theme with a different time-label font size.
    #[must_use]
    pub const fn with_time_size(mut self, size: f32) -> Self {
        self.time_size = size;
        self
    }

    pub(crate) fn background(&self, theme: &dyn Theme, style: &DssStyle) -> Color {
        self.background
            .or(style.background)
            .unwrap_or_else(|| theme.background())
    }

    pub(crate) fn surface(&self, theme: &dyn Theme) -> Color {
        self.surface.unwrap_or_else(|| theme.surface())
    }

    pub(crate) fn foreground(&self, theme: &dyn Theme, style: &DssStyle) -> Color {
        self.foreground
            .or(style.foreground)
            .unwrap_or_else(|| theme.foreground())
    }

    /// Foreground for controls painted on the dark video scrim.
    ///
    /// Defaults to white so the overlay stays legible under both light and
    /// dark application themes. Explicit overrides still win.
    pub(crate) fn overlay_foreground(&self, style: &DssStyle) -> Color {
        self.foreground.or(style.foreground).unwrap_or(Color::WHITE)
    }

    /// Background of the center play badge: a translucent dark disc unless an
    /// explicit badge color is configured.
    pub(crate) fn center_badge(&self) -> Color {
        self.badge.unwrap_or_else(|| Color::rgba8(8, 8, 10, 150))
    }

    pub(crate) fn muted_foreground(&self, theme: &dyn Theme) -> Color {
        self.muted_foreground
            .unwrap_or_else(|| theme.muted_foreground())
    }

    pub(crate) fn accent(&self, theme: &dyn Theme, style: &DssStyle) -> Color {
        self.accent
            .or(style.accent)
            .unwrap_or_else(|| theme.accent())
    }

    pub(crate) fn track(&self, foreground: Color) -> Color {
        self.track.unwrap_or_else(|| foreground.with_alpha(0.28))
    }

    pub(crate) fn overlay(&self) -> Color {
        self.overlay.unwrap_or_else(|| Color::rgba8(0, 0, 0, 168))
    }

    pub(crate) fn badge(&self, accent: Color) -> Color {
        self.badge.unwrap_or(accent)
    }

    pub(crate) fn error(&self, theme: &dyn Theme) -> Color {
        self.error.unwrap_or_else(|| theme.danger())
    }
}

/// Runtime behavior shared by video and audio players.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct PlayerBehavior {
    /// Whether playback restarts at the beginning when it reaches the end.
    pub looping: bool,
    /// Initial output volume in the inclusive `0.0..=1.0` range.
    pub volume: f32,
    /// Whether output starts muted.
    pub muted: bool,
    /// Rates cycled by the rate button, in order.
    ///
    /// An empty list removes the rate control even when it is otherwise
    /// enabled.
    pub playback_rates: Vec<f64>,
    /// Amount seek buttons and arrow keys jump.
    pub seek_step: Duration,
    /// Amount the volume changes per key press or wheel notch.
    pub volume_step: f32,
    /// Whether keyboard shortcuts are handled by the player.
    pub keyboard: bool,
    /// Whether the mouse wheel over the player changes the volume.
    pub wheel_volume: bool,
    /// Delay before controls hide while playing.
    ///
    /// `None` keeps controls visible. A zero delay hides them as soon as the
    /// pointer leaves.
    pub auto_hide: Option<Duration>,
}

impl Default for PlayerBehavior {
    fn default() -> Self {
        Self {
            looping: false,
            volume: 1.0,
            muted: false,
            playback_rates: vec![0.5, 0.75, 1.0, 1.25, 1.5, 2.0],
            seek_step: Duration::from_secs(10),
            volume_step: 0.05,
            keyboard: true,
            wheel_volume: true,
            auto_hide: Some(Duration::from_millis(2500)),
        }
    }
}

impl PlayerBehavior {
    /// Creates the default behavior.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns this behavior with looping enabled or disabled.
    #[must_use]
    pub const fn with_looping(mut self, looping: bool) -> Self {
        self.looping = looping;
        self
    }

    /// Returns this behavior with an initial volume.
    ///
    /// Non-finite values fall back to `1.0`; finite values clamp to `0..=1`.
    #[must_use]
    pub fn with_volume(mut self, volume: f32) -> Self {
        self.volume = clamp_unit(volume);
        self
    }

    /// Returns this behavior starting muted or unmuted.
    #[must_use]
    pub const fn with_muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    /// Returns this behavior with a different list of selectable rates.
    #[must_use]
    pub fn with_playback_rates(mut self, rates: impl IntoIterator<Item = f64>) -> Self {
        self.playback_rates = rates
            .into_iter()
            .filter(|rate| rate.is_finite() && *rate > 0.0)
            .collect();
        self
    }

    /// Returns this behavior with a different seek step.
    #[must_use]
    pub const fn with_seek_step(mut self, step: Duration) -> Self {
        self.seek_step = step;
        self
    }

    /// Returns this behavior with a different volume step.
    #[must_use]
    pub fn with_volume_step(mut self, step: f32) -> Self {
        self.volume_step = if step.is_finite() {
            step.abs().clamp(0.01, 1.0)
        } else {
            0.05
        };
        self
    }

    /// Returns this behavior with keyboard shortcuts enabled or disabled.
    #[must_use]
    pub const fn with_keyboard(mut self, keyboard: bool) -> Self {
        self.keyboard = keyboard;
        self
    }

    /// Returns this behavior with wheel-volume enabled or disabled.
    #[must_use]
    pub const fn with_wheel_volume(mut self, wheel_volume: bool) -> Self {
        self.wheel_volume = wheel_volume;
        self
    }

    /// Returns this behavior with a different controls auto-hide delay.
    #[must_use]
    pub const fn with_auto_hide(mut self, delay: Option<Duration>) -> Self {
        self.auto_hide = delay;
        self
    }
}

/// Which overlay controls a [`crate::Video`] paints.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoControls {
    /// Play and pause button.
    pub play_pause: bool,
    /// Seekable progress bar.
    pub seek_bar: bool,
    /// Back and forward seek buttons.
    pub seek_seconds: bool,
    /// Elapsed and total time labels.
    pub time: bool,
    /// Volume slider.
    pub volume: bool,
    /// Mute toggle. Also shows a volume icon when the slider is hidden.
    pub mute: bool,
    /// Playback-rate button that cycles through the configured rates.
    pub rate: bool,
    /// Fullscreen request button. The widget emits a signal; the application
    /// owns the actual window transition.
    pub fullscreen: bool,
    /// Loop toggle.
    pub loop_button: bool,
    /// Large play badge in the center while paused.
    pub center_play: bool,
    /// Title and subtitle in the top scrim.
    pub title: bool,
    /// Time tooltip above the seek bar while hovering.
    pub hover_preview: bool,
}

impl Default for VideoControls {
    fn default() -> Self {
        Self {
            play_pause: true,
            seek_bar: true,
            seek_seconds: true,
            time: true,
            volume: true,
            mute: true,
            rate: true,
            fullscreen: true,
            loop_button: false,
            center_play: true,
            title: true,
            hover_preview: true,
        }
    }
}

impl VideoControls {
    /// Creates the default video control set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns this set with every control hidden.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            play_pause: false,
            seek_bar: false,
            seek_seconds: false,
            time: false,
            volume: false,
            mute: false,
            rate: false,
            fullscreen: false,
            loop_button: false,
            center_play: false,
            title: false,
            hover_preview: false,
        }
    }

    /// Returns this set with only the play badge, seek bar, and time visible.
    #[must_use]
    pub const fn minimal() -> Self {
        Self {
            play_pause: true,
            seek_bar: true,
            seek_seconds: false,
            time: true,
            volume: false,
            mute: false,
            rate: false,
            fullscreen: false,
            loop_button: false,
            center_play: true,
            title: false,
            hover_preview: true,
        }
    }
}

/// Which controls an [`crate::AudioPlayer`] paints.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AudioControls {
    /// Play and pause button.
    pub play_pause: bool,
    /// Seekable progress bar.
    pub seek_bar: bool,
    /// Elapsed and total time labels.
    pub time: bool,
    /// Previous-track request button.
    pub previous: bool,
    /// Next-track request button.
    pub next: bool,
    /// Volume slider.
    pub volume: bool,
    /// Mute toggle.
    pub mute: bool,
    /// Playback-rate button.
    pub rate: bool,
    /// Shuffle toggle.
    pub shuffle: bool,
    /// Repeat-mode button.
    pub repeat: bool,
    /// Cover artwork.
    pub cover: bool,
}

impl Default for AudioControls {
    fn default() -> Self {
        Self {
            play_pause: true,
            seek_bar: true,
            time: true,
            previous: true,
            next: true,
            volume: true,
            mute: true,
            rate: true,
            shuffle: false,
            repeat: true,
            cover: true,
        }
    }
}

impl AudioControls {
    /// Creates the default audio control set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns this set with every control hidden.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            play_pause: false,
            seek_bar: false,
            time: false,
            previous: false,
            next: false,
            volume: false,
            mute: false,
            rate: false,
            shuffle: false,
            repeat: false,
            cover: false,
        }
    }
}

/// Arrangement of an [`crate::AudioPlayer`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AudioPlayerLayout {
    /// Cover on the left, metadata and a full control row on the right.
    #[default]
    Inline,
    /// A single compact row with optional cover and metadata.
    Compact,
}

/// How the repeat button behaves.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RepeatMode {
    /// Stop at the end of the track.
    #[default]
    Off,
    /// Restart the current track when it ends.
    All,
    /// Restart the current track and keep the application's queue position.
    One,
}

impl RepeatMode {
    /// Returns the next mode in the button cycle.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    /// Returns whether playback restarts at the end.
    #[must_use]
    pub const fn is_looping(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// Session, clock, frame scheduling, volume, and transport state for a player.
///
/// The core owns no painting and no input state. Both player widgets forward
/// transport commands and paint whatever [`Self::frame`] and [`Self::clock`]
/// report, which keeps video and audio on the same timeline.
pub(crate) struct PlayerCore {
    clock: MediaClock,
    session: Option<Box<dyn MediaSession>>,
    scheduler: MediaFrameScheduler,
    looping: bool,
    volume: f32,
    muted: bool,
    error: Option<String>,
}

impl PlayerCore {
    pub(crate) fn new() -> Self {
        Self {
            clock: MediaClock::new(),
            session: None,
            scheduler: MediaFrameScheduler::new(),
            looping: false,
            volume: 1.0,
            muted: false,
            error: None,
        }
    }

    pub(crate) fn apply_behavior(&mut self, behavior: &PlayerBehavior) {
        self.looping = behavior.looping;
        self.volume = clamp_unit(behavior.volume);
        self.muted = behavior.muted;
        self.sync_volume();
    }

    fn sync_volume(&mut self) {
        if let Some(session) = self.session.as_mut() {
            session.set_volume(self.volume);
            session.set_muted(self.muted);
        }
    }

    pub(crate) fn timeline(&self) -> &MediaClock {
        self.session
            .as_deref()
            .map_or(&self.clock, |session| session.clock())
    }

    pub(crate) fn metadata(&self) -> Option<MediaMetadata> {
        self.session
            .as_deref()
            .and_then(|session| session.metadata().copied())
    }

    pub(crate) const fn has_session(&self) -> bool {
        self.session.is_some()
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn attach(&mut self, session: impl MediaSession + 'static) {
        self.session = Some(Box::new(session));
        self.sync_volume();
    }

    pub(crate) fn close_session(&mut self) {
        if let Some(session) = self.session.take() {
            self.clock = *session.clock();
        }
        self.clock.pause();
        self.error = None;
    }

    pub(crate) fn release_session(&mut self) {
        if let Some(session) = self.session.as_deref() {
            self.clock = *session.clock();
        }
        if let Some(mut session) = self.session.take() {
            session.pause();
        }
    }

    pub(crate) fn frame(&self) -> Option<&drawen_render::RasterImage> {
        self.scheduler.current().map(MediaFrame::image)
    }

    pub(crate) fn frame_shared(&self) -> Option<&std::sync::Arc<drawen_render::RasterImage>> {
        self.scheduler.current().map(MediaFrame::shared_image)
    }

    pub(crate) fn duration(&self) -> Duration {
        self.timeline().duration().unwrap_or(Duration::ZERO)
    }

    pub(crate) fn position(&self) -> Duration {
        self.timeline().position()
    }

    pub(crate) fn is_playing(&self) -> bool {
        self.timeline().is_playing()
    }

    pub(crate) fn is_ended(&self) -> bool {
        self.timeline().is_ended()
    }

    pub(crate) fn state(&self) -> crate::MediaState {
        self.timeline().state()
    }

    pub(crate) const fn is_looping(&self) -> bool {
        self.looping
    }

    pub(crate) fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    pub(crate) fn rate(&self) -> f64 {
        self.timeline().rate()
    }

    pub(crate) const fn volume(&self) -> f32 {
        self.volume
    }

    pub(crate) const fn muted(&self) -> bool {
        self.muted
    }

    pub(crate) fn set_volume(&mut self, volume: f32) -> bool {
        let volume = clamp_unit(volume);
        if (self.volume - volume).abs() <= f32::EPSILON {
            return false;
        }
        self.volume = volume;
        if let Some(session) = self.session.as_mut() {
            session.set_volume(volume);
        }
        true
    }

    pub(crate) fn set_muted(&mut self, muted: bool) -> bool {
        if self.muted == muted {
            return false;
        }
        self.muted = muted;
        if let Some(session) = self.session.as_mut() {
            session.set_muted(muted);
        }
        true
    }

    pub(crate) fn set_frame(&mut self, frame: drawen_render::RasterImage) {
        let position = self.position();
        self.scheduler
            .replace(MediaFrame::new(frame, position, Duration::ZERO));
    }

    pub(crate) fn clear_frame(&mut self) {
        self.scheduler.clear();
    }

    pub(crate) fn set_duration(&mut self, duration: Duration) {
        if let Some(session) = self.session.as_mut() {
            session.clock_mut().set_duration(Some(duration));
        } else {
            self.clock.set_duration(Some(duration));
        }
        self.scheduler.seek(self.position());
    }

    pub(crate) fn set_position(&mut self, position: Duration) {
        if let Some(session) = self.session.as_mut() {
            if let Err(error) = session.seek(position) {
                self.error = Some(error.message().to_owned());
            }
        } else {
            self.clock.seek(position);
        }
        self.scheduler.seek(self.position());
    }

    pub(crate) fn set_playing(&mut self, playing: bool) {
        if let Some(session) = self.session.as_mut() {
            if playing {
                if let Err(error) = session.play() {
                    self.error = Some(error.message().to_owned());
                }
            } else {
                session.pause();
            }
        } else {
            if playing
                && !self.looping
                && !self.duration().is_zero()
                && self.position() >= self.duration()
            {
                self.clock.seek(Duration::ZERO);
            }
            self.clock.set_playing(playing);
        }
    }

    pub(crate) fn toggle_playback(&mut self) {
        self.set_playing(!self.is_playing());
    }

    pub(crate) fn set_rate(&mut self, rate: f64) {
        if let Some(session) = self.session.as_mut() {
            session.set_rate(rate);
        } else {
            self.clock.set_rate(rate);
        }
    }

    pub(crate) fn supports_video_pause(&self) -> bool {
        self.session
            .as_deref()
            .is_some_and(MediaSession::supports_video_pause)
    }

    pub(crate) fn set_video_enabled(&mut self, enabled: bool) {
        if let Some(session) = self.session.as_mut()
            && let Err(error) = session.set_video_enabled(enabled)
        {
            self.error = Some(error.message().to_owned());
        }
    }

    pub(crate) fn tick(&mut self, delta: Duration) -> bool {
        let mut changed = false;
        if let Some(session) = self.session.as_mut() {
            let update = session.update(delta);
            if let Some(frame) = update.frame {
                changed |= self.scheduler.push(frame);
            }
            if let Some(error) = update.error {
                self.error = Some(error.message().to_owned());
                changed = true;
            }
            changed |= update.state_changed;
            let position = session.clock().position();
            changed |= self.scheduler.present(position);
            if session.clock().is_ended() {
                if self.looping {
                    session.clock_mut().seek(Duration::ZERO);
                    if let Err(error) = session.play() {
                        self.error = Some(error.message().to_owned());
                    }
                    self.scheduler.seek(Duration::ZERO);
                }
                changed = true;
            }
        } else {
            if self.clock.is_playing() {
                changed |= if self.looping {
                    self.clock.advance_looping(delta)
                } else {
                    self.clock.advance(delta)
                };
            }
            changed |= self.scheduler.present(self.clock.position());
        }
        changed
    }

    pub(crate) fn is_animating(&self) -> bool {
        if let Some(session) = self.session.as_deref() {
            return session.clock().is_playing() || session.is_active();
        }
        let duration = self.clock.duration().unwrap_or(Duration::ZERO);
        self.clock.is_playing() && !duration.is_zero() && self.clock.position() < duration
    }
}

/// Clamps a volume to `0..=1`, mapping non-finite input to `1.0`.
pub(crate) fn clamp_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Formats a duration as `m:ss` or `h:mm:ss`.
#[must_use]
pub fn format_time(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Formats a playback rate without trailing zero decimals.
#[must_use]
pub fn format_rate(rate: f64) -> String {
    if (rate.fract()).abs() < f64::EPSILON {
        format!("{rate:.0}x")
    } else if (rate * 100.0).round() % 10.0 == 0.0 {
        format!("{rate:.1}x")
    } else {
        format!("{rate:.2}x")
    }
}

/// Returns the next rate in `rates`, or the current rate when the list is empty.
#[must_use]
pub(crate) fn cycle_rate(current: f64, rates: &[f64]) -> f64 {
    if rates.is_empty() {
        return current;
    }
    let index = rates
        .iter()
        .position(|rate| (rate - current).abs() <= 0.001)
        .map_or(0, |index| (index + 1) % rates.len());
    rates[index]
}

/// Paints a vertical color gradient with a bounded number of strips.
///
/// The fade follows a smoothstep curve so the transparent end dissolves
/// gently. Strips stay exactly adjacent: overlapping translucent strips would
/// double-blend and leave dark seams at every boundary.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn paint_vertical_gradient(
    context: &mut PaintContext<'_>,
    rect: Rect,
    top: Color,
    bottom: Color,
) {
    const STRIPS: u32 = 48;
    if rect.size.height() <= 0.0 || rect.size.width() <= 0.0 {
        return;
    }
    let strip_height = rect.size.height() / STRIPS as f32;
    for index in 0..STRIPS {
        let fraction = index as f32 / (STRIPS - 1) as f32;
        let amount = fraction * fraction * (3.0 - 2.0 * fraction);
        let y = rect.origin.y + index as f32 * strip_height;
        let height = if index + 1 == STRIPS {
            rect.bottom() - y
        } else {
            strip_height
        };
        let strip = Rect::new(
            Point::new(rect.origin.x, y),
            Size::new(rect.size.width(), height).expect("gradient strip size is finite"),
        );
        context.fill_rect(strip, top.mix(bottom, amount));
    }
}

/// Paints a centered play triangle or a pause pair inside `button`.
pub(crate) fn paint_play_pause(
    context: &mut PaintContext<'_>,
    button: Rect,
    playing: bool,
    color: Color,
) {
    let center = Point::new(
        button.origin.x + button.size.width() / 2.0,
        button.origin.y + button.size.height() / 2.0,
    );
    let height = button.size.height() * 0.55;
    let width = height * 0.82;
    if playing {
        let bar = (width * 0.28).max(2.0);
        let gap = (width * 0.18).max(2.0);
        let y = center.y - height / 2.0;
        for offset in [-1.0_f32, 1.0] {
            let x = center.x + offset * (bar + gap) / 2.0 - bar / 2.0;
            context.fill_rounded_rect(
                Rect::new(
                    Point::new(x, y),
                    Size::new(bar, height).expect("pause bar is finite"),
                ),
                bar / 2.0,
                color,
            );
        }
    } else {
        // Nudge the triangle right so its visual mass, not its bounding box,
        // sits on the button center.
        paint_triangle(
            context,
            Rect::new(
                Point::new(
                    center.x - width / 2.0 + width * 0.08,
                    center.y - height / 2.0,
                ),
                Size::new(width, height).expect("play triangle is finite"),
            ),
            1.0,
            color,
        );
    }
}

/// Paints a horizontal triangle in `bounds` using shaded strips.
///
/// The strip count follows the glyph height (about one strip per pixel), so
/// edges stay smooth at every button size, and strips overlap slightly to
/// hide antialiasing seams.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn paint_triangle(context: &mut PaintContext<'_>, bounds: Rect, direction: f32, color: Color) {
    if bounds.is_empty() {
        return;
    }
    let rows = (bounds.size.height().ceil() as usize).clamp(12, 48);
    let row_height = bounds.size.height() / rows as f32;
    for row in 0..rows {
        let fraction = (row as f32 + 0.5) / rows as f32;
        let row_width = bounds.size.width() * (1.0 - (2.0 * fraction - 1.0).abs());
        if row_width <= 0.0 {
            continue;
        }
        let x = if direction >= 0.0 {
            bounds.origin.x
        } else {
            bounds.right() - row_width
        };
        let y = bounds.origin.y + row as f32 * row_height;
        let height = (row_height + 0.4).min(bounds.bottom() - y);
        context.fill_rect(
            Rect::new(
                Point::new(x, y),
                Size::new(row_width, height).expect("triangle row is finite"),
            ),
            color,
        );
    }
}

/// Paints a `|<` or `>|` skip icon in `button`.
pub(crate) fn paint_skip(
    context: &mut PaintContext<'_>,
    button: Rect,
    forward: bool,
    color: Color,
) {
    let center = Point::new(
        button.origin.x + button.size.width() / 2.0,
        button.origin.y + button.size.height() / 2.0,
    );
    let height = button.size.height() * 0.46;
    let width = height * 0.72;
    let direction = if forward { 1.0 } else { -1.0 };
    let triangle = Rect::new(
        Point::new(center.x - width / 2.0, center.y - height / 2.0),
        Size::new(width, height).expect("skip triangle is finite"),
    );
    paint_triangle(context, triangle, direction, color);
    let bar = (height * 0.14).max(1.8);
    let x = center.x + direction * (width / 2.0 + bar);
    context.fill_rounded_rect(
        Rect::new(
            Point::new(x - bar / 2.0, center.y - height / 2.0),
            Size::new(bar, height).expect("skip bar is finite"),
        ),
        bar / 2.0,
        color,
    );
}

/// Paints a previous or next track icon in `button`.
pub(crate) fn paint_track_skip(
    context: &mut PaintContext<'_>,
    button: Rect,
    forward: bool,
    color: Color,
) {
    paint_skip(context, button, forward, color);
}

/// Paints a speaker icon, with an X when muted.
///
/// The cone flares away from the driver body, loud levels add one or two
/// arc-shaped waves, and mute draws a small cross beside the speaker.
pub(crate) fn paint_volume(
    context: &mut PaintContext<'_>,
    button: Rect,
    muted: bool,
    level: f32,
    color: Color,
) {
    let center = Point::new(
        button.origin.x + button.size.width() / 2.0,
        button.origin.y + button.size.height() / 2.0,
    );
    let height = button.size.height() * 0.46;
    let half = height / 2.0;
    let body_width = height * 0.30;
    let cone_width = height * 0.46;
    let left = center.x - button.size.width() * 0.30;
    context.fill_rounded_rect(
        Rect::new(
            Point::new(left, center.y - half * 0.45),
            Size::new(body_width, half * 0.9).expect("speaker body is finite"),
        ),
        1.0,
        color,
    );
    let cone_left = left + body_width - 0.75;
    paint_triangle(
        context,
        Rect::new(
            Point::new(cone_left, center.y - half),
            Size::new(cone_width, height).expect("speaker cone is finite"),
        ),
        -1.0,
        color,
    );
    let base_x = cone_left + cone_width;
    if muted {
        let arm = height * 0.26;
        let thickness = (height * 0.09).max(1.6);
        let cross = Point::new(base_x + height * 0.30, center.y);
        for angle in [
            std::f32::consts::FRAC_PI_4,
            -std::f32::consts::FRAC_PI_4,
        ] {
            let transform = Affine::translate(cross.x, cross.y).then(Affine::rotate(angle));
            context.with_transform(transform, |context| {
                context.fill_rounded_rect(
                    Rect::new(
                        Point::new(-arm, -thickness / 2.0),
                        Size::new(arm * 2.0, thickness).expect("mute cross is finite"),
                    ),
                    thickness / 2.0,
                    color,
                );
            });
        }
        return;
    }
    let wave_center = Point::new(base_x - cone_width * 0.15, center.y);
    if level > 0.02 {
        paint_volume_wave(context, wave_center, height * 0.42, color);
    }
    if level > 0.5 {
        paint_volume_wave(context, wave_center, height * 0.68, color);
    }
}

/// Paints one `)`-shaped volume wave: a ring clipped to its right side.
fn paint_volume_wave(context: &mut PaintContext<'_>, center: Point, radius: f32, color: Color) {
    let thickness = 1.6;
    let ring = Rect::new(
        Point::new(center.x - radius, center.y - radius),
        Size::new(radius * 2.0, radius * 2.0).expect("volume wave is finite"),
    );
    let clip = Rect::new(
        Point::new(center.x + radius * 0.30, center.y - radius - thickness),
        Size::new(radius + thickness, radius * 2.0 + thickness * 2.0)
            .expect("volume wave clip is finite"),
    );
    context.with_clip(clip, |context| {
        context.stroke_rounded_rect(ring, radius, thickness, color);
    });
}

/// Paints four fullscreen corner brackets in `button`.
pub(crate) fn paint_fullscreen(context: &mut PaintContext<'_>, button: Rect, color: Color) {
    let center = Point::new(
        button.origin.x + button.size.width() / 2.0,
        button.origin.y + button.size.height() / 2.0,
    );
    let half = button.size.height() * 0.23;
    let arm = half * 0.85;
    let thickness = 2.0;
    for (sx, sy) in [(-1.0_f32, -1.0_f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let corner = Point::new(center.x + sx * half, center.y + sy * half);
        context.fill_rounded_rect(
            Rect::new(
                Point::new(
                    if sx < 0.0 { corner.x } else { corner.x - arm },
                    corner.y - thickness / 2.0,
                ),
                Size::new(arm, thickness).expect("fullscreen arm is finite"),
            ),
            thickness / 2.0,
            color,
        );
        context.fill_rounded_rect(
            Rect::new(
                Point::new(
                    corner.x - thickness / 2.0,
                    if sy < 0.0 { corner.y } else { corner.y - arm },
                ),
                Size::new(thickness, arm).expect("fullscreen arm is finite"),
            ),
            thickness / 2.0,
            color,
        );
    }
}

/// Paints a circular repeat arrow in `button`.
pub(crate) fn paint_repeat(
    context: &mut PaintContext<'_>,
    button: Rect,
    active: bool,
    color: Color,
) {
    let center = Point::new(
        button.origin.x + button.size.width() / 2.0,
        button.origin.y + button.size.height() / 2.0,
    );
    let radius = button.size.height() * 0.21;
    let alpha = if active { 1.0 } else { 0.7 };
    let color = color.with_alpha(alpha);
    context.stroke_rounded_rect(
        Rect::new(
            Point::new(center.x - radius, center.y - radius),
            Size::new(radius * 2.0, radius * 2.0).expect("repeat ring is finite"),
        ),
        radius,
        if active { 2.0 } else { 1.6 },
        color,
    );
    // Arrowhead riding the top-right of the ring, pointing clockwise.
    let angle = -std::f32::consts::FRAC_PI_4;
    let tip = Point::new(
        center.x + angle.cos() * radius,
        center.y + angle.sin() * radius,
    );
    let arrow = radius * 0.55;
    let transform =
        Affine::translate(tip.x, tip.y).then(Affine::rotate(std::f32::consts::FRAC_PI_4));
    context.with_transform(transform, |context| {
        paint_triangle(
            context,
            Rect::new(
                Point::new(-arrow * 0.65, -arrow * 0.8),
                Size::new(arrow * 1.3, arrow * 1.6).expect("repeat arrow is finite"),
            ),
            1.0,
            color,
        );
    });
}

/// Paints a shuffle icon: two crossing arrows.
pub(crate) fn paint_shuffle(
    context: &mut PaintContext<'_>,
    button: Rect,
    active: bool,
    color: Color,
) {
    let center = Point::new(
        button.origin.x + button.size.width() / 2.0,
        button.origin.y + button.size.height() / 2.0,
    );
    let width = button.size.height() * 0.38;
    let height = button.size.height() * 0.14;
    let alpha = if active { 1.0 } else { 0.65 };
    let color = color.with_alpha(alpha);
    for direction in [-1.0_f32, 1.0] {
        let y = center.y + direction * height;
        context.fill_rect(
            Rect::new(
                Point::new(center.x - width, y - 0.75),
                Size::new(width * 1.6, 1.5).expect("shuffle line is finite"),
            ),
            color,
        );
        let arrow = width * 0.34;
        paint_triangle(
            context,
            Rect::new(
                Point::new(center.x + width * 0.6, y - arrow * 0.8),
                Size::new(arrow, arrow * 1.6).expect("shuffle arrow is finite"),
            ),
            1.0,
            color,
        );
    }
}

/// Paints a small buffering spinner as a ring of dots.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn paint_buffering(
    context: &mut PaintContext<'_>,
    center: Point,
    radius: f32,
    color: Color,
) {
    const DOTS: usize = 8;
    for index in 0..DOTS {
        let angle = index as f32 / DOTS as f32 * std::f32::consts::TAU;
        let alpha = (index + 1) as f32 / DOTS as f32;
        let point = Point::new(
            center.x + angle.cos() * radius,
            center.y + angle.sin() * radius,
        );
        let size = Size::new(radius * 0.32, radius * 0.32).expect("spinner dot is finite");
        context.fill_rounded_rect(
            Rect::new(
                Point::new(point.x - size.width() / 2.0, point.y - size.height() / 2.0),
                size,
            ),
            size.width() / 2.0,
            color.with_alpha(alpha),
        );
    }
}

/// Paints a rounded control hit area behind an icon when hovered.
pub(crate) fn paint_hover_background(
    context: &mut PaintContext<'_>,
    rect: Rect,
    hovered: bool,
    foreground: Color,
    radius: f32,
) {
    if hovered {
        context.fill_rounded_rect(rect, radius, foreground.with_alpha(0.14));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_elapsed_duration_and_remaining_labels() {
        assert_eq!(format_time(Duration::from_secs(5)), "0:05");
        assert_eq!(format_time(Duration::from_secs(3665)), "1:01:05");
        assert_eq!(
            PlayerTimeDisplay::ElapsedAndDuration
                .format(Duration::from_secs(65), Duration::from_secs(120)),
            "1:05 / 2:00"
        );
        assert_eq!(
            PlayerTimeDisplay::Remaining.format(Duration::from_secs(65), Duration::from_secs(120)),
            "-0:55"
        );
    }

    #[test]
    fn cycles_rates_and_wraps() {
        let rates = [0.5, 1.0, 2.0];
        assert!((cycle_rate(0.5, &rates) - 1.0).abs() < f64::EPSILON);
        assert!((cycle_rate(2.0, &rates) - 0.5).abs() < f64::EPSILON);
        assert!((cycle_rate(3.0, &rates) - 0.5).abs() < f64::EPSILON);
        assert!((cycle_rate(1.0, &[]) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn formats_rates_without_noise() {
        assert_eq!(format_rate(1.0), "1x");
        assert_eq!(format_rate(1.5), "1.5x");
        assert_eq!(format_rate(0.75), "0.75x");
    }

    #[test]
    fn repeat_cycles_through_modes() {
        assert_eq!(RepeatMode::Off.next(), RepeatMode::All);
        assert_eq!(RepeatMode::All.next(), RepeatMode::One);
        assert_eq!(RepeatMode::One.next(), RepeatMode::Off);
        assert!(!RepeatMode::Off.is_looping());
        assert!(RepeatMode::All.is_looping());
    }
}
