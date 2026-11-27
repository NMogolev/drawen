use std::path::Path;
use std::time::Duration;

use drawen_core::{
    Color, Constraints, InputEvent, Key, NamedKey, Point, PointerButton, PointerId, Rect, Size,
    WheelDelta,
};
use drawen_render::{PaintContext, RasterImage, TextStyle};
use drawen_ui::{
    Accessibility, AccessibilityRole, DssStyle, EventContext, EventPhase, EventStatus,
    LayoutContext, LayoutResult, Signal, StyleTarget, UiEnvironment, Widget,
};

use crate::ImageFit;
use crate::image::{fitted_rect, image_size};
use crate::media::{MediaError, MediaMetadata, MediaSession, MediaState};
use crate::paleo_session::PaleoSession;
use crate::player::{
    PlayerBehavior, PlayerCore, PlayerTheme, PlayerTimeDisplay, VideoControls, cycle_rate,
    format_rate, paint_buffering, paint_fullscreen, paint_hover_background, paint_play_pause,
    paint_repeat, paint_skip, paint_vertical_gradient, paint_volume,
};
use crate::video_ffmpeg::{FfmpegConfig, FfmpegSession, VideoError};

const DEFAULT_WIDTH: f32 = 480.0;
const DEFAULT_HEIGHT: f32 = 270.0;
const CENTER_BADGE: f32 = 64.0;
const VOLUME_SLIDER_WIDTH: f32 = 76.0;
const TOP_SCRIM: f32 = 64.0;
const BUFFERING_RADIUS: f32 = 18.0;
/// Vertical reach of the seek and volume hit areas beyond the visible track.
const TRACK_HIT_PAD: f32 = 8.0;

/// The kind of pointer gesture currently owned by the player.
#[derive(Clone, Copy, Debug)]
enum Drag {
    Seek(PointerId),
    Volume(PointerId),
}

/// A transport button under the pointer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HoverTarget {
    Play,
    Back,
    Forward,
    Mute,
    Rate,
    Loop,
    Fullscreen,
    CenterPlay,
}

/// The video player widget: a picture surface with a YouTube-style control
/// overlay.
///
/// `Video` presents frames from a [`MediaSession`] or from application-supplied
/// [`RasterImage`] values. The overlay is fully configurable through
/// [`VideoControls`], [`PlayerTheme`], and [`PlayerBehavior`]:
///
/// ```no_run
/// use std::time::Duration;
/// use drawen_core::Color;
/// use drawen_widgets::{PlayerBehavior, PlayerTheme, Video, VideoControls};
///
/// let video = Video::open_paleo("clip.mp4")?
///     .with_title("A short film")
///     .with_subtitle("Paleo · 1080p")
///     .with_control_set(VideoControls::default())
///     .with_theme(PlayerTheme::new().with_accent(Color::rgba8(255, 45, 85, 255)))
///     .with_behavior(PlayerBehavior::new().with_auto_hide(Some(Duration::from_secs(3))));
/// # Ok::<(), drawen_widgets::MediaError>(())
/// ```
///
/// The widget never creates a window, starts a timer, or spawns a thread. It
/// paints, forwards transport intent, and emits typed signals. Applications tick
/// playback through [`Widget::advance_animation`] (normally via
/// `UiRuntime::next_frame_in`).
pub struct Video {
    core: PlayerCore,
    theme: PlayerTheme,
    behavior: PlayerBehavior,
    controls_config: VideoControls,
    time_display: PlayerTimeDisplay,
    fit: ImageFit,
    background: Option<Color>,
    label: Option<String>,
    title: Option<String>,
    subtitle: Option<String>,
    placeholder: Option<String>,
    controls: bool,
    viewport: Size,
    hover: Option<HoverTarget>,
    hover_position: Option<Point>,
    drag: Option<Drag>,
    pointer_inside: bool,
    controls_shown: bool,
    hide_in: Option<Duration>,
    play_toggled: Signal<bool>,
    seek_requested: Signal<Duration>,
    volume_changed: Signal<f32>,
    mute_toggled: Signal<bool>,
    rate_changed: Signal<f64>,
    fullscreen_requested: Signal<bool>,
    loop_toggled: Signal<bool>,
}

impl Video {
    /// Creates an empty video surface with the default controls.
    #[must_use]
    pub fn new() -> Self {
        let behavior = PlayerBehavior::default();
        let mut core = PlayerCore::new();
        core.apply_behavior(&behavior);
        Self {
            core,
            theme: PlayerTheme::default(),
            behavior,
            controls_config: VideoControls::default(),
            time_display: PlayerTimeDisplay::default(),
            fit: ImageFit::Contain,
            background: None,
            label: None,
            title: None,
            subtitle: None,
            placeholder: None,
            controls: true,
            viewport: Size::ZERO,
            hover: None,
            hover_position: None,
            drag: None,
            pointer_inside: false,
            controls_shown: true,
            hide_in: None,
            play_toggled: Signal::new(),
            seek_requested: Signal::new(),
            volume_changed: Signal::new(),
            mute_toggled: Signal::new(),
            rate_changed: Signal::new(),
            fullscreen_requested: Signal::new(),
            loop_toggled: Signal::new(),
        }
    }

    /// Creates a video surface showing `frame`.
    #[must_use]
    pub fn with_frame(frame: RasterImage) -> Self {
        let mut video = Self::new();
        video.set_frame(frame);
        video
    }

    /// Opens `path` with the built-in `FFmpeg` backend.
    ///
    /// This starts decoding immediately so the first frame can be shown while
    /// playback is paused. Playback, seeking, and looping then stay in sync
    /// with the session until the widget is dropped or [`Self::close_decoder`]
    /// is called.
    ///
    /// # Errors
    ///
    /// Returns [`VideoError`] when `ffprobe` or `ffmpeg` cannot be found, the
    /// container is rejected, or the decoded size cannot be represented.
    /// Both executables are discovered through [`FfmpegConfig`].
    pub fn open(path: impl AsRef<Path>) -> Result<Self, VideoError> {
        Self::open_with(path, FfmpegConfig::default())
    }

    /// Opens `path` with explicit `FFmpeg` settings.
    ///
    /// # Errors
    ///
    /// Returns [`VideoError`] when `ffprobe` or `ffmpeg` cannot be found, the
    /// container is rejected, or the decoded size cannot be represented.
    pub fn open_with(path: impl AsRef<Path>, config: FfmpegConfig) -> Result<Self, VideoError> {
        let session = FfmpegSession::open(path.as_ref(), config)?;
        Ok(Self::new().with_session(session))
    }

    /// Opens `path` with the pure-Rust Paleo backend.
    ///
    /// Unlike [`Self::open`], this needs no external executable. The default
    /// build decodes H.264, HEVC, VP9, and AV1; use [`Self::open`] for codecs
    /// and containers Paleo cannot handle yet. Decoding happens on the calling
    /// thread in bounded steps, and the first picture is ready when this
    /// returns.
    ///
    /// # Errors
    ///
    /// Returns [`MediaError`] when the file cannot be opened, has no video
    /// track, uses a codec this build has no backend for, or fails to decode.
    pub fn open_paleo(path: impl AsRef<Path>) -> Result<Self, MediaError> {
        let session = PaleoSession::open(path.as_ref())?;
        Ok(Self::new().with_session(session))
    }

    /// Opens `path` with Paleo audio output and follows video to the audio clock.
    ///
    /// Requires the `audio` feature. Audio is decoded by Paleo and played by
    /// `paleo-audio`; the device position is the master clock. If the file has
    /// no audio track or no output device is available, the session falls back
    /// to the wall-clock behavior of [`Self::open_paleo`]. Video is shown for
    /// every codec this build has a backend for; an unsupported video codec is
    /// skipped so the audio still plays.
    ///
    /// # Errors
    ///
    /// Returns [`MediaError`] when the file cannot be opened, no track can be
    /// played at all, or a stream is malformed.
    #[cfg(feature = "audio")]
    pub fn open_paleo_audio(path: impl AsRef<Path>) -> Result<Self, MediaError> {
        let session = crate::AudioPaleoSession::open(path.as_ref())?;
        Ok(Self::new().with_session(session))
    }

    /// Attaches an externally created [`MediaSession`].
    ///
    /// This is the escape hatch for custom decoders and for backends that are
    /// not built in, such as a future hardware session.
    #[must_use]
    pub fn with_session(mut self, session: impl MediaSession + 'static) -> Self {
        self.core.attach(session);
        self
    }

    /// Returns the metadata reported by the built-in session.
    #[must_use]
    pub fn metadata(&self) -> Option<MediaMetadata> {
        self.core.metadata()
    }

    /// Returns whether a media session is attached.
    #[must_use]
    pub const fn has_decoder(&self) -> bool {
        self.core.has_session()
    }

    /// Returns the most recent session error, if any.
    #[must_use]
    pub fn decoder_error(&self) -> Option<&str> {
        self.core.error()
    }

    /// Stops and releases the built-in session while keeping the last frame.
    pub fn close_decoder(&mut self) {
        self.core.close_session();
    }

    /// Returns the current frame, if one has been supplied or decoded.
    #[must_use]
    pub fn frame(&self) -> Option<&RasterImage> {
        self.core.frame()
    }

    /// Returns the declared media duration.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.core.duration()
    }

    /// Returns the current playback position.
    #[must_use]
    pub fn position(&self) -> Duration {
        self.core.position()
    }

    /// Returns whether playback is currently running.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.core.is_playing()
    }

    /// Returns whether playback reached the end of the media.
    #[must_use]
    pub fn is_ended(&self) -> bool {
        self.core.is_ended()
    }

    /// Returns whether playback wraps to the start at the end.
    #[must_use]
    pub fn is_looping(&self) -> bool {
        self.core.is_looping()
    }

    /// Returns whether the overlay controls are enabled.
    #[must_use]
    pub const fn controls_visible(&self) -> bool {
        self.controls
    }

    /// Returns whether the overlay controls are currently painted.
    ///
    /// This is `false` while auto-hide has hidden them; [`Self::controls_visible`]
    /// reports the configuration switch instead.
    #[must_use]
    pub const fn controls_shown(&self) -> bool {
        self.controls && self.controls_shown
    }

    /// Returns how frames are scaled into the widget bounds.
    #[must_use]
    pub const fn fit(&self) -> ImageFit {
        self.fit
    }

    /// Returns the optional accessible name.
    #[must_use]
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// Returns the configured visual theme.
    #[must_use]
    pub const fn theme(&self) -> &PlayerTheme {
        &self.theme
    }

    /// Returns the configured behavior.
    #[must_use]
    pub const fn behavior(&self) -> &PlayerBehavior {
        &self.behavior
    }

    /// Returns the configured control set.
    #[must_use]
    pub const fn control_set(&self) -> VideoControls {
        self.controls_config
    }

    /// Returns the configured time-label format.
    #[must_use]
    pub const fn time_display(&self) -> PlayerTimeDisplay {
        self.time_display
    }

    /// Returns the current output volume in the `0.0..=1.0` range.
    #[must_use]
    pub fn volume(&self) -> f32 {
        self.core.volume()
    }

    /// Returns whether output is muted.
    #[must_use]
    pub fn is_muted(&self) -> bool {
        self.core.muted()
    }

    /// Returns a signal emitted when the user starts or pauses playback.
    #[must_use]
    pub fn play_toggled(&self) -> Signal<bool> {
        self.play_toggled.clone()
    }

    /// Returns a signal emitted when the user seeks to a new position.
    #[must_use]
    pub fn seek_requested(&self) -> Signal<Duration> {
        self.seek_requested.clone()
    }

    /// Returns a signal emitted when the user changes the volume.
    #[must_use]
    pub fn volume_changed(&self) -> Signal<f32> {
        self.volume_changed.clone()
    }

    /// Returns a signal emitted when the user mutes or unmutes output.
    #[must_use]
    pub fn mute_toggled(&self) -> Signal<bool> {
        self.mute_toggled.clone()
    }

    /// Returns a signal emitted when the user selects a playback rate.
    #[must_use]
    pub fn rate_changed(&self) -> Signal<f64> {
        self.rate_changed.clone()
    }

    /// Returns a signal emitted when the user asks for fullscreen.
    ///
    /// The widget reports intent only; the application owns the native window
    /// transition.
    #[must_use]
    pub fn fullscreen_requested(&self) -> Signal<bool> {
        self.fullscreen_requested.clone()
    }

    /// Returns a signal emitted when the user toggles looping.
    #[must_use]
    pub fn loop_toggled(&self) -> Signal<bool> {
        self.loop_toggled.clone()
    }

    /// Replaces the displayed frame without changing playback state.
    pub fn set_frame(&mut self, frame: RasterImage) {
        self.core.set_frame(frame);
    }

    /// Clears the displayed frame.
    pub fn clear_frame(&mut self) {
        self.core.clear_frame();
    }

    /// Sets the media duration and clamps the current position to it.
    pub fn set_duration(&mut self, duration: Duration) {
        self.core.set_duration(duration);
    }

    /// Sets the playback position, clamping it to `0..=duration`.
    pub fn set_position(&mut self, position: Duration) {
        self.core.set_position(position);
    }

    /// Updates playback state without emitting [`Self::play_toggled`].
    pub fn set_playing(&mut self, playing: bool) {
        self.core.set_playing(playing);
        self.refresh_auto_hide();
    }

    /// Starts or pauses playback without emitting [`Self::play_toggled`].
    pub fn toggle_playback(&mut self) {
        self.set_playing(!self.is_playing());
    }

    /// Changes the playback rate, clamped by the media clock.
    pub fn set_rate(&mut self, rate: f64) {
        self.core.set_rate(rate);
    }

    /// Returns the current playback rate.
    #[must_use]
    pub fn rate(&self) -> f64 {
        self.core.rate()
    }

    /// Sets the output volume without emitting [`Self::volume_changed`].
    pub fn set_volume(&mut self, volume: f32) {
        self.core.set_volume(volume);
    }

    /// Mutes or unmutes output without emitting [`Self::mute_toggled`].
    pub fn set_muted(&mut self, muted: bool) {
        self.core.set_muted(muted);
    }

    /// Replaces the visual theme.
    pub fn set_theme(&mut self, theme: PlayerTheme) {
        self.theme = theme;
    }

    /// Replaces the behavior, including volume and looping.
    pub fn set_behavior(&mut self, behavior: PlayerBehavior) {
        self.behavior = behavior;
        self.core.apply_behavior(&self.behavior);
        self.refresh_auto_hide();
    }

    /// Replaces the control set.
    pub fn set_control_set(&mut self, controls: VideoControls) {
        self.controls_config = controls;
    }

    /// Sets the time-label format.
    pub fn set_time_display(&mut self, display: PlayerTimeDisplay) {
        self.time_display = display;
    }

    /// Shows or hides the overlay controls.
    pub fn set_controls_enabled(&mut self, enabled: bool) {
        self.controls = enabled;
        self.controls_shown = enabled;
        self.refresh_auto_hide();
    }

    /// Returns whether the attached session can stop video while audio plays.
    #[must_use]
    pub fn supports_video_pause(&self) -> bool {
        self.core.supports_video_pause()
    }

    /// Suspends or resumes video decoding and presentation.
    pub fn set_video_enabled(&mut self, enabled: bool) {
        self.core.set_video_enabled(enabled);
    }

    /// Detaches the media session and stops playback.
    pub fn release_session(&mut self) {
        self.core.release_session();
    }

    fn refresh_auto_hide(&mut self) {
        if !self.controls {
            self.hide_in = None;
            return;
        }
        self.controls_shown = true;
        self.hide_in = if self.is_playing() {
            self.behavior.auto_hide
        } else {
            None
        };
    }

    fn note_activity(&mut self) {
        if !self.controls {
            return;
        }
        self.controls_shown = true;
        self.hide_in = if self.is_playing() {
            self.behavior.auto_hide
        } else {
            None
        };
    }

    fn toggle(&mut self, context: &mut EventContext<'_>) {
        self.core.toggle_playback();
        self.note_activity();
        context.request_redraw();
        let playing = self.is_playing();
        self.play_toggled.emit(&playing);
    }

    fn seek_to(&mut self, position: Duration, context: &mut EventContext<'_>) {
        let duration = self.duration();
        let position = if duration.is_zero() {
            position
        } else {
            position.min(duration)
        };
        if position == self.position() {
            return;
        }
        self.core.set_position(position);
        self.note_activity();
        context.request_redraw();
        self.seek_requested.emit(&position);
    }

    fn seek_by(&mut self, seconds: f32, context: &mut EventContext<'_>) {
        let position = self.position().as_secs_f32() + seconds;
        let duration = self.duration();
        let position = if duration.is_zero() {
            position.max(0.0)
        } else {
            position.clamp(0.0, duration.as_secs_f32())
        };
        self.seek_to(Duration::from_secs_f32(position), context);
    }

    fn seek_from_pointer(&mut self, x: f32, context: &mut EventContext<'_>) {
        let track = video_geometry(Rect::new(Point::ZERO, self.viewport), &self.theme, self).track;
        let ratio = if track.size.width() > 0.0 {
            ((x - track.origin.x) / track.size.width()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let position = self.duration().mul_f32(ratio);
        self.seek_to(position, context);
    }

    fn volume_from_pointer(&mut self, x: f32, context: &mut EventContext<'_>) {
        let slider =
            video_geometry(Rect::new(Point::ZERO, self.viewport), &self.theme, self).volume_slider;
        let ratio = if slider.size.width() > 0.0 {
            ((x - slider.origin.x) / slider.size.width()).clamp(0.0, 1.0)
        } else {
            self.volume()
        };
        if self.core.set_volume(ratio) {
            self.note_activity();
            context.request_redraw();
            self.volume_changed.emit(&ratio);
        }
    }

    fn change_volume(&mut self, delta: f32, context: &mut EventContext<'_>) {
        let volume = (self.volume() + delta).clamp(0.0, 1.0);
        if self.core.set_volume(volume) {
            self.note_activity();
            context.request_redraw();
            self.volume_changed.emit(&volume);
        }
    }

    fn toggle_mute(&mut self, context: &mut EventContext<'_>) {
        let muted = !self.is_muted();
        self.core.set_muted(muted);
        self.note_activity();
        context.request_redraw();
        self.mute_toggled.emit(&muted);
    }

    fn cycle_rate(&mut self, context: &mut EventContext<'_>) {
        let rate = cycle_rate(self.rate(), &self.behavior.playback_rates);
        self.core.set_rate(rate);
        self.note_activity();
        context.request_redraw();
        self.rate_changed.emit(&rate);
    }

    fn request_fullscreen(&mut self, context: &mut EventContext<'_>) {
        context.request_redraw();
        self.fullscreen_requested.emit(&true);
    }

    fn set_loop(&mut self, looping: bool, context: &mut EventContext<'_>) {
        self.core.set_looping(looping);
        context.request_redraw();
        self.loop_toggled.emit(&looping);
    }

    fn toggle_loop(&mut self, context: &mut EventContext<'_>) {
        let looping = !self.is_looping();
        self.set_loop(looping, context);
    }

    fn hit_target(&self, point: Point) -> Option<HoverTarget> {
        let geometry = video_geometry(Rect::new(Point::ZERO, self.viewport), &self.theme, self);
        if geometry.play.contains(point) {
            Some(HoverTarget::Play)
        } else if geometry.back.contains(point) {
            Some(HoverTarget::Back)
        } else if geometry.forward.contains(point) {
            Some(HoverTarget::Forward)
        } else if geometry.mute.contains(point) {
            Some(HoverTarget::Mute)
        } else if geometry.rate.contains(point) {
            Some(HoverTarget::Rate)
        } else if geometry.loop_button.contains(point) {
            Some(HoverTarget::Loop)
        } else if geometry.fullscreen.contains(point) {
            Some(HoverTarget::Fullscreen)
        } else if geometry.center_play.contains(point) {
            Some(HoverTarget::CenterPlay)
        } else {
            None
        }
    }

    fn activate_target(&mut self, target: HoverTarget, context: &mut EventContext<'_>) {
        match target {
            HoverTarget::Play | HoverTarget::CenterPlay => self.toggle(context),
            HoverTarget::Back => self.seek_by(-self.behavior.seek_step.as_secs_f32(), context),
            HoverTarget::Forward => self.seek_by(self.behavior.seek_step.as_secs_f32(), context),
            HoverTarget::Mute => self.toggle_mute(context),
            HoverTarget::Rate => self.cycle_rate(context),
            HoverTarget::Loop => self.toggle_loop(context),
            HoverTarget::Fullscreen => self.request_fullscreen(context),
        }
    }

    /// Advances the session or manual clock and presents the matching frame.
    fn poll(&mut self, delta: Duration) -> bool {
        self.core.tick(delta)
    }

    fn paint_icon_button(
        &self,
        context: &mut PaintContext<'_>,
        rect: Rect,
        target: HoverTarget,
        foreground: Color,
    ) {
        paint_hover_background(
            context,
            rect,
            self.hover == Some(target),
            foreground,
            rect.size.height() / 2.0,
        );
    }

    /// Returns this video with an explicit duration.
    #[must_use]
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.set_duration(duration);
        self
    }

    /// Returns this video with an initial position.
    #[must_use]
    pub fn with_position(mut self, position: Duration) -> Self {
        self.set_position(position);
        self
    }

    /// Returns this video playing or paused.
    #[must_use]
    pub fn with_playing(mut self, playing: bool) -> Self {
        self.set_playing(playing);
        self
    }

    /// Returns this video with looping enabled or disabled.
    #[must_use]
    pub fn with_looping(mut self, looping: bool) -> Self {
        self.core.set_looping(looping);
        self.behavior.looping = looping;
        self
    }

    /// Returns this video with overlay controls enabled or disabled.
    #[must_use]
    pub fn with_controls(mut self, controls: bool) -> Self {
        self.controls = controls;
        self.controls_shown = controls;
        self.controls_config = if controls {
            VideoControls::default()
        } else {
            VideoControls::none()
        };
        self
    }

    /// Returns this video with a specific control set.
    #[must_use]
    pub fn with_control_set(mut self, controls: VideoControls) -> Self {
        self.controls = true;
        self.controls_config = controls;
        self.controls_shown = true;
        self
    }

    /// Returns this video with a specific visual theme.
    #[must_use]
    pub fn with_theme(mut self, theme: PlayerTheme) -> Self {
        self.theme = theme;
        self
    }

    /// Returns this video with a specific behavior.
    #[must_use]
    pub fn with_behavior(mut self, behavior: PlayerBehavior) -> Self {
        self.set_behavior(behavior);
        self
    }

    /// Returns this video with a specific time-label format.
    #[must_use]
    pub const fn with_time_display(mut self, display: PlayerTimeDisplay) -> Self {
        self.time_display = display;
        self
    }

    /// Returns this video with an initial volume.
    #[must_use]
    pub fn with_volume(mut self, volume: f32) -> Self {
        self.behavior.volume = crate::player::clamp_unit(volume);
        self.core.set_volume(volume);
        self
    }

    /// Returns this video starting muted or unmuted.
    #[must_use]
    pub fn with_muted(mut self, muted: bool) -> Self {
        self.behavior.muted = muted;
        self.core.set_muted(muted);
        self
    }

    /// Returns this video with an explicit frame scaling mode.
    #[must_use]
    pub const fn with_fit(mut self, fit: ImageFit) -> Self {
        self.fit = fit;
        self
    }

    /// Returns this video with a letterbox background color.
    #[must_use]
    pub const fn with_background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// Returns this video with an accessible name.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Returns this video with an overlay title.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Returns this video with an overlay subtitle.
    #[must_use]
    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Returns this video with placeholder text for an empty surface.
    #[must_use]
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }
}

impl Default for Video {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Video {
    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("video")
    }

    fn layout(&mut self, _: &mut LayoutContext<'_>, constraints: Constraints) -> LayoutResult {
        let max = constraints.max();
        let intrinsic = self.frame().map_or(Size::ZERO, image_size);
        let width = if max.width().is_finite() {
            max.width()
        } else if intrinsic.width() > 0.0 {
            intrinsic.width()
        } else {
            DEFAULT_WIDTH
        };
        let height = if max.height().is_finite() {
            max.height()
        } else if intrinsic.height() > 0.0 {
            intrinsic.height()
        } else {
            DEFAULT_HEIGHT
        };
        self.viewport =
            constraints.constrain(Size::new(width, height).expect("video sizes are non-negative"));
        LayoutResult::new(self.viewport)
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        self.paint_styled(context, environment, bounds, &DssStyle::default());
    }

    #[allow(clippy::too_many_lines)]
    fn paint_styled(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        bounds: Rect,
        style: &DssStyle,
    ) {
        let theme = environment.theme();
        let background = self.theme.background(theme, style);
        context.fill_rect(bounds, background);
        if bounds.is_empty() {
            return;
        }

        // Overlay content sits on a dark scrim, so it defaults to white even
        // under a light application theme. The placeholder stays on the plain
        // surface and keeps the theme colors.
        let foreground = self.theme.overlay_foreground(style);
        let muted = self
            .theme
            .muted_foreground
            .unwrap_or(foreground.with_alpha(0.72));
        let accent = self.theme.accent(theme, style);
        let overlay = self.theme.overlay();
        let show_controls = self.controls && self.controls_shown && bounds.size.height() >= 72.0;

        if let Some(image) = self.core.frame_shared() {
            if image.width() > 0 && image.height() > 0 {
                let destination = fitted_rect(
                    bounds,
                    image_size(image),
                    self.fit,
                    crate::Alignment::Center,
                    crate::Alignment::Center,
                );
                context.with_clip(bounds, |context| {
                    context.draw_image(std::sync::Arc::clone(image), destination);
                });
            }
        } else {
            self.paint_placeholder(
                context,
                environment,
                bounds,
                style,
                self.theme.muted_foreground(theme),
            );
        }

        if show_controls {
            let geometry = video_geometry(bounds, &self.theme, self);
            let scrim_top = (geometry.bar.origin.y - 48.0).max(bounds.origin.y);
            paint_vertical_gradient(
                context,
                Rect::new(
                    Point::new(bounds.origin.x, scrim_top),
                    Size::new(bounds.size.width(), (bounds.bottom() - scrim_top).max(0.0))
                        .expect("video bottom scrim is finite"),
                ),
                Color::TRANSPARENT,
                overlay,
            );

            if self.controls_config.title
                && (self.title.is_some() || self.subtitle.is_some())
                && bounds.size.height() >= 140.0
            {
                let top = Rect::new(
                    bounds.origin,
                    Size::new(
                        bounds.size.width(),
                        TOP_SCRIM.min(bounds.size.height() * 0.35),
                    )
                    .expect("video top scrim is finite"),
                );
                paint_vertical_gradient(context, top, overlay, Color::TRANSPARENT);
                self.paint_title(context, environment, style, top, foreground, muted);
            }

            self.paint_progress(context, environment, style, &geometry, foreground, accent);
            self.paint_controls(context, environment, style, &geometry, foreground, accent);

            if self.controls_config.center_play
                && !self.is_playing()
                && !geometry.center_play.is_empty()
            {
                let badge_rect = geometry.center_play;
                let radius = badge_rect.size.height() / 2.0;
                let mut badge = self.theme.center_badge();
                if self.hover == Some(HoverTarget::CenterPlay) {
                    badge = badge.with_alpha((badge.alpha + 0.12).min(1.0));
                }
                context.draw_shadow(badge_rect, radius, 10.0, Color::rgba8(0, 0, 0, 90));
                context.fill_rounded_rect(badge_rect, radius, badge);
                paint_play_pause(context, badge_rect, false, foreground);
            }
        }

        if self.core.state() == MediaState::Buffering {
            let center = Point::new(
                bounds.origin.x + bounds.size.width() / 2.0,
                bounds.origin.y + bounds.size.height() / 2.0,
            );
            let radius = BUFFERING_RADIUS * 1.5;
            context.fill_rounded_rect(
                Rect::new(
                    Point::new(center.x - radius, center.y - radius),
                    Size::new(radius * 2.0, radius * 2.0).expect("buffering badge is finite"),
                ),
                radius,
                overlay,
            );
            paint_buffering(context, center, BUFFERING_RADIUS * 0.72, foreground);
        }

        if let Some(error) = self.core.error() {
            self.paint_error(context, environment, style, bounds, error);
        }
    }

    fn advance_animation(&mut self, delta: Duration, settle: bool) -> bool {
        let mut changed = self.poll(if settle { Duration::ZERO } else { delta });
        if settle && self.is_playing() {
            self.set_playing(false);
            changed = true;
        }
        if let Some(remaining) = self.hide_in {
            if !self.is_playing() {
                self.hide_in = None;
                self.controls_shown = true;
                changed = true;
            } else if settle {
                self.hide_in = None;
                self.controls_shown = false;
                changed = true;
            } else {
                let remaining = remaining.saturating_sub(delta);
                if remaining.is_zero() {
                    self.hide_in = None;
                    self.controls_shown = false;
                } else {
                    self.hide_in = Some(remaining);
                }
                changed = true;
            }
        }
        if !self.is_playing() && !self.controls_shown && self.controls {
            self.controls_shown = true;
            changed = true;
        }
        changed
    }

    fn is_animating(&self) -> bool {
        self.core.is_animating() || self.hide_in.is_some()
    }

    fn local_animation_damage(&self) -> bool {
        // Frames, controls, and the buffering badge stay inside the bounds;
        // the badge shadow (elevation 10) fits the standard effect margin.
        true
    }

    fn advances_while_hidden(&self) -> bool {
        self.core.is_animating()
    }

    #[allow(clippy::too_many_lines)]
    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        if context.phase() != EventPhase::Target {
            return EventStatus::Ignored;
        }
        match event {
            InputEvent::PointerEnter(pointer) => {
                self.pointer_inside = true;
                self.note_activity();
                self.hover = self.hit_target(pointer.position);
                EventStatus::Handled
            }
            InputEvent::PointerLeave(_) => {
                self.pointer_inside = false;
                self.hover = None;
                self.hover_position = None;
                if self.drag.is_none() && self.is_playing() {
                    self.hide_in = self.behavior.auto_hide;
                }
                EventStatus::Handled
            }
            InputEvent::PointerMove(pointer) => {
                self.pointer_inside = true;
                self.note_activity();
                self.hover_position = Some(pointer.position);
                if let Some(drag) = self.drag {
                    match drag {
                        Drag::Seek(_) => self.seek_from_pointer(pointer.position.x, context),
                        Drag::Volume(_) => self.volume_from_pointer(pointer.position.x, context),
                    }
                    return EventStatus::Handled;
                }
                let hover = self.hit_target(pointer.position);
                if hover != self.hover {
                    self.hover = hover;
                    context.request_redraw();
                }
                if self.controls_shown && self.controls {
                    EventStatus::Handled
                } else {
                    EventStatus::Ignored
                }
            }
            InputEvent::PointerDown(pointer) if pointer.button == Some(PointerButton::Primary) => {
                context.request_focus();
                self.note_activity();
                if self.controls && self.controls_shown {
                    let geometry =
                        video_geometry(Rect::new(Point::ZERO, self.viewport), &self.theme, self);
                    // Buttons win over the expanded track hit bands, which
                    // reach into the top of the button row.
                    if let Some(target) = self.hit_target(pointer.position) {
                        self.activate_target(target, context);
                        return EventStatus::Handled;
                    }
                    if geometry.track_hit.contains(pointer.position) {
                        self.drag = Some(Drag::Seek(pointer.pointer_id));
                        context.capture_pointer(pointer.pointer_id);
                        self.seek_from_pointer(pointer.position.x, context);
                        return EventStatus::Handled;
                    }
                    if geometry.volume_hit.contains(pointer.position) {
                        self.drag = Some(Drag::Volume(pointer.pointer_id));
                        context.capture_pointer(pointer.pointer_id);
                        self.volume_from_pointer(pointer.position.x, context);
                        return EventStatus::Handled;
                    }
                    if geometry.bar.contains(pointer.position) {
                        return EventStatus::Handled;
                    }
                }
                if self.controls_shown {
                    self.controls_shown = false;
                } else if self.controls {
                    self.controls_shown = true;
                }
                self.toggle(context);
                EventStatus::Handled
            }
            InputEvent::PointerUp(pointer) => {
                if let Some(drag) = self.drag
                    && drag_pointer(drag) == pointer.pointer_id
                {
                    self.drag = None;
                    context.release_pointer(pointer.pointer_id);
                    return EventStatus::Handled;
                }
                EventStatus::Ignored
            }
            InputEvent::PointerCancel(pointer) => {
                if let Some(drag) = self.drag
                    && drag_pointer(drag) == pointer.pointer_id
                {
                    self.drag = None;
                    return EventStatus::Handled;
                }
                EventStatus::Ignored
            }
            InputEvent::Wheel(wheel) if self.behavior.wheel_volume => {
                let rect = Rect::new(Point::ZERO, self.viewport);
                if rect.contains(wheel.position) {
                    let direction =
                        match wheel.delta {
                            WheelDelta::Lines(delta) | WheelDelta::Pixels(delta) => {
                                if delta.y < 0.0 { 1.0 } else { -1.0 }
                            }
                        };
                    self.change_volume(direction * self.behavior.volume_step, context);
                    EventStatus::Handled
                } else {
                    EventStatus::Ignored
                }
            }
            InputEvent::KeyDown(key) if self.behavior.keyboard => match &key.key {
                Key::Named(NamedKey::Space) => {
                    self.toggle(context);
                    EventStatus::Handled
                }
                Key::Named(NamedKey::ArrowLeft) => {
                    self.seek_by(-self.behavior.seek_step.as_secs_f32(), context);
                    EventStatus::Handled
                }
                Key::Named(NamedKey::ArrowRight) => {
                    self.seek_by(self.behavior.seek_step.as_secs_f32(), context);
                    EventStatus::Handled
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.change_volume(self.behavior.volume_step, context);
                    EventStatus::Handled
                }
                Key::Named(NamedKey::ArrowDown) => {
                    self.change_volume(-self.behavior.volume_step, context);
                    EventStatus::Handled
                }
                Key::Named(NamedKey::Home) => {
                    self.seek_to(Duration::ZERO, context);
                    EventStatus::Handled
                }
                Key::Named(NamedKey::End) => {
                    self.seek_to(self.duration(), context);
                    EventStatus::Handled
                }
                Key::Character(character) if character.eq_ignore_ascii_case("m") => {
                    self.toggle_mute(context);
                    EventStatus::Handled
                }
                Key::Character(character) if character.eq_ignore_ascii_case("f") => {
                    self.request_fullscreen(context);
                    EventStatus::Handled
                }
                Key::Character(character) if character.eq_ignore_ascii_case("l") => {
                    self.toggle_loop(context);
                    EventStatus::Handled
                }
                _ => EventStatus::Ignored,
            },
            _ => EventStatus::Ignored,
        }
    }

    fn accepts_focus(&self) -> bool {
        true
    }

    fn accessibility(&self) -> Accessibility {
        let accessibility = Accessibility::new(AccessibilityRole::Image);
        match &self.label {
            Some(label) => accessibility.with_label(label.clone()),
            None => accessibility,
        }
    }
}

impl Video {
    #[allow(clippy::too_many_lines)]
    fn paint_placeholder(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        bounds: Rect,
        style: &DssStyle,
        muted: Color,
    ) {
        let center = Point::new(
            bounds.origin.x + bounds.size.width() / 2.0,
            bounds.origin.y + bounds.size.height() / 2.0,
        );
        let badge = CENTER_BADGE.min(bounds.size.height() / 3.0).max(20.0);
        let rect = Rect::new(
            Point::new(center.x - badge / 2.0, center.y - badge / 2.0),
            Size::new(badge, badge).expect("placeholder badge is finite"),
        );
        context.fill_rounded_rect(rect, badge / 2.0, muted.with_alpha(0.18));
        paint_play_pause(context, rect, false, muted);
        if let Some(text) = &self.placeholder {
            let font = Self::font(environment, style, self.theme.title_size);
            let layout = environment.text_shaper().layout(
                text,
                &TextStyle::new_with_font(font, muted),
                Some(bounds.size.width() * 0.8),
            );
            context.draw_text(
                layout.clone(),
                Point::new(
                    center.x - layout.size().width() / 2.0,
                    rect.bottom() + self.theme.spacing,
                ),
            );
        }
    }

    fn paint_title(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        style: &DssStyle,
        top: Rect,
        foreground: Color,
        muted: Color,
    ) {
        let padding = self.theme.padding;
        let mut y = top.origin.y + padding;
        if let Some(title) = &self.title {
            let font = Self::font(environment, style, self.theme.title_size);
            let layout = environment.text_shaper().layout(
                title,
                &TextStyle::new_with_font(font, foreground),
                Some((top.size.width() - padding * 2.0).max(0.0)),
            );
            let height = layout.size().height();
            context.draw_text(layout, Point::new(top.origin.x + padding, y));
            y += height + 2.0;
        }
        if let Some(subtitle) = &self.subtitle {
            let font = Self::font(environment, style, self.theme.subtitle_size);
            let layout = environment.text_shaper().layout(
                subtitle,
                &TextStyle::new_with_font(font, muted),
                Some((top.size.width() - padding * 2.0).max(0.0)),
            );
            context.draw_text(layout, Point::new(top.origin.x + padding, y));
        }
    }

    fn paint_progress(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        style: &DssStyle,
        geometry: &VideoGeometry,
        foreground: Color,
        accent: Color,
    ) {
        if !self.controls_config.seek_bar {
            return;
        }
        let track = geometry.track;
        let active = matches!(self.drag, Some(Drag::Seek(_))) || self.hover_over_track(track);
        // The bar rests thin and grows under the pointer, YouTube-style.
        let height = if active {
            (track.size.height() + 2.0).min(6.0)
        } else {
            track.size.height()
        };
        let paint_track = Rect::new(
            Point::new(
                track.origin.x,
                track.origin.y + (track.size.height() - height) / 2.0,
            ),
            Size::new(track.size.width(), height).expect("painted seek track is finite"),
        );
        context.fill_rounded_rect(paint_track, height / 2.0, self.theme.track(foreground));
        let ratio = if self.duration() > Duration::ZERO {
            (self.position().as_secs_f32() / self.duration().as_secs_f32()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let filled_width = track.size.width() * ratio;
        if filled_width > 0.0 {
            context.fill_rounded_rect(
                Rect::new(
                    paint_track.origin,
                    Size::new(filled_width, height).expect("filled progress width is finite"),
                ),
                height / 2.0,
                accent,
            );
        }
        if active {
            let thumb_size = height * 2.6;
            context.fill_rounded_rect(
                Rect::new(
                    Point::new(
                        track.origin.x + filled_width - thumb_size / 2.0,
                        paint_track.origin.y + (height - thumb_size) / 2.0,
                    ),
                    Size::new(thumb_size, thumb_size).expect("progress thumb is finite"),
                ),
                thumb_size / 2.0,
                self.theme.badge(accent),
            );
        }

        if self.controls_config.hover_preview
            && self.hover_over_track(track)
            && let Some(position) = self.hover_position
            && !self.duration().is_zero()
        {
            let ratio = ((position.x - track.origin.x) / track.size.width()).clamp(0.0, 1.0);
            let seconds = self.duration().as_secs_f32() * ratio;
            let label = crate::player::format_time(Duration::from_secs_f32(seconds));
            let font = Self::font(environment, style, self.theme.time_size);
            let layout = environment.text_shaper().layout(
                &label,
                &TextStyle::new_with_font(font, foreground),
                None,
            );
            let width = layout.size().width() + 16.0;
            let height = layout.size().height() + 8.0;
            let x = (position.x - width / 2.0)
                .clamp(track.origin.x, (track.right() - width).max(track.origin.x));
            let top = (track.origin.y - height - 10.0).max(0.0);
            context.fill_rounded_rect(
                Rect::new(
                    Point::new(x, top),
                    Size::new(width, height).expect("preview label is finite"),
                ),
                height / 2.0,
                self.theme.overlay(),
            );
            context.draw_text(layout, Point::new(x + 8.0, top + 4.0));
        }
    }

    /// Returns whether the pointer rests in the seek bar's hover band.
    fn hover_over_track(&self, track: Rect) -> bool {
        self.hover_position.is_some_and(|point| {
            point.x >= track.origin.x - 8.0
                && point.x <= track.right() + 8.0
                && point.y >= track.origin.y - 12.0
                && point.y <= track.bottom() + 12.0
        })
    }

    fn paint_controls(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        style: &DssStyle,
        geometry: &VideoGeometry,
        foreground: Color,
        accent: Color,
    ) {
        if self.controls_config.play_pause {
            self.paint_icon_button(context, geometry.play, HoverTarget::Play, foreground);
            paint_play_pause(context, geometry.play, self.is_playing(), foreground);
        }
        if self.controls_config.seek_seconds {
            self.paint_icon_button(context, geometry.back, HoverTarget::Back, foreground);
            paint_skip(context, geometry.back, false, foreground);
            self.paint_icon_button(context, geometry.forward, HoverTarget::Forward, foreground);
            paint_skip(context, geometry.forward, true, foreground);
        }
        if self.controls_config.time {
            let font = Self::font(environment, style, self.theme.time_size);
            let label = self.time_display.format(self.position(), self.duration());
            let layout = environment.text_shaper().layout(
                &label,
                &TextStyle::new_with_font(font, foreground),
                None,
            );
            context.draw_text(
                layout.clone(),
                Point::new(
                    geometry.time_origin,
                    geometry.row_top + (geometry.row_height - layout.size().height()) / 2.0,
                ),
            );
        }

        if self.controls_config.mute || self.controls_config.volume {
            self.paint_icon_button(context, geometry.mute, HoverTarget::Mute, foreground);
            let level = if self.is_muted() { 0.0 } else { self.volume() };
            paint_volume(context, geometry.mute, self.is_muted(), level, foreground);
            if self.controls_config.volume && !geometry.volume_slider.is_empty() {
                self.paint_volume_slider(context, geometry, foreground);
            }
        }
        if self.controls_config.rate {
            self.paint_icon_button(context, geometry.rate, HoverTarget::Rate, foreground);
            let font = Self::font(environment, style, self.theme.time_size);
            let layout = environment.text_shaper().layout(
                &format_rate(self.rate()),
                &TextStyle::new_with_font(font, foreground),
                None,
            );
            context.draw_text(
                layout.clone(),
                Point::new(
                    geometry.rate.origin.x
                        + (geometry.rate.size.width() - layout.size().width()) / 2.0,
                    geometry.rate.origin.y
                        + (geometry.rate.size.height() - layout.size().height()) / 2.0,
                ),
            );
        }
        if self.controls_config.loop_button {
            self.paint_icon_button(context, geometry.loop_button, HoverTarget::Loop, foreground);
            paint_repeat(
                context,
                geometry.loop_button,
                self.is_looping(),
                if self.is_looping() {
                    accent
                } else {
                    foreground
                },
            );
        }
        if self.controls_config.fullscreen {
            self.paint_icon_button(
                context,
                geometry.fullscreen,
                HoverTarget::Fullscreen,
                foreground,
            );
            paint_fullscreen(context, geometry.fullscreen, foreground);
        }
    }

    fn paint_volume_slider(
        &self,
        context: &mut PaintContext<'_>,
        geometry: &VideoGeometry,
        foreground: Color,
    ) {
        let slider = geometry.volume_slider;
        context.fill_rounded_rect(
            slider,
            slider.size.height() / 2.0,
            self.theme.track(foreground),
        );
        let width = slider.size.width() * self.volume();
        if width > 0.0 {
            context.fill_rounded_rect(
                Rect::new(
                    slider.origin,
                    Size::new(width, slider.size.height()).expect("volume fill is finite"),
                ),
                slider.size.height() / 2.0,
                foreground,
            );
        }
        let active = matches!(self.drag, Some(Drag::Volume(_)))
            || self.hover_position.is_some_and(|point| {
                point.x >= slider.origin.x - 6.0
                    && point.x <= slider.right() + 6.0
                    && point.y >= slider.origin.y - 10.0
                    && point.y <= slider.bottom() + 10.0
            });
        if active {
            let thumb_size = slider.size.height() * 3.2;
            context.fill_rounded_rect(
                Rect::new(
                    Point::new(
                        slider.origin.x + width - thumb_size / 2.0,
                        slider.origin.y + (slider.size.height() - thumb_size) / 2.0,
                    ),
                    Size::new(thumb_size, thumb_size).expect("volume thumb is finite"),
                ),
                thumb_size / 2.0,
                foreground,
            );
        }
    }

    fn paint_error(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        style: &DssStyle,
        bounds: Rect,
        error: &str,
    ) {
        context.fill_rect(bounds, Color::rgba8(0, 0, 0, 180));
        let font = Self::font(environment, style, self.theme.font_size);
        let color = self.theme.error(environment.theme());
        let layout = environment.text_shaper().layout(
            error,
            &TextStyle::new_with_font(font, color),
            Some((bounds.size.width() - self.theme.padding * 2.0).max(0.0)),
        );
        let origin = Point::new(
            bounds.origin.x + (bounds.size.width() - layout.size().width()) / 2.0,
            bounds.origin.y + (bounds.size.height() - layout.size().height()) / 2.0,
        );
        context.draw_text(layout, origin);
    }

    fn font(environment: &UiEnvironment, style: &DssStyle, size: f32) -> drawen_render::Font {
        let mut font = environment.font_for(style);
        if style.font_size.is_none() {
            font.set_pixel_size(size);
        }
        font
    }
}

fn drag_pointer(drag: Drag) -> PointerId {
    match drag {
        Drag::Seek(pointer) | Drag::Volume(pointer) => pointer,
    }
}

/// Rectangles used by painting and hit testing for a video control overlay.
#[derive(Clone, Copy, Debug)]
struct VideoGeometry {
    bar: Rect,
    row_top: f32,
    row_height: f32,
    track: Rect,
    /// Seek track expanded vertically so the thin bar is easy to grab.
    track_hit: Rect,
    play: Rect,
    back: Rect,
    forward: Rect,
    time_origin: f32,
    mute: Rect,
    volume_slider: Rect,
    /// Volume slider expanded vertically, matching the seek hit area.
    volume_hit: Rect,
    rate: Rect,
    loop_button: Rect,
    fullscreen: Rect,
    center_play: Rect,
}

#[allow(clippy::too_many_lines)]
fn video_geometry(bounds: Rect, theme: &PlayerTheme, video: &Video) -> VideoGeometry {
    let controls = video.controls_config;
    let padding = theme.padding;
    let button = theme.button_size;
    let spacing = theme.spacing;
    let control_height = theme.control_height.min(bounds.size.height());
    let bar = Rect::new(
        Point::new(
            bounds.origin.x,
            bounds.origin.y + bounds.size.height() - control_height,
        ),
        Size::new(bounds.size.width(), control_height).expect("video control bar is finite"),
    );
    let track = Rect::new(
        Point::new(bar.origin.x + padding, bar.origin.y + padding * 0.5),
        Size::new(
            (bar.size.width() - padding * 2.0).max(0.0),
            theme.track_height,
        )
        .expect("video seek track is finite"),
    );
    let track_hit = Rect::new(
        Point::new(track.origin.x, track.origin.y - TRACK_HIT_PAD),
        Size::new(
            track.size.width(),
            track.size.height() + TRACK_HIT_PAD * 2.0,
        )
        .expect("video seek hit area is finite"),
    );
    let row_top = track.bottom() + 2.0;
    let row_height = (bar.bottom() - row_top - 4.0).max(button);
    let button_y = row_top + (row_height - button) / 2.0;

    let empty = Rect::new(Point::ZERO, Size::ZERO);
    let mut x = bar.origin.x + padding;
    let play = if controls.play_pause {
        let rect = Rect::new(
            Point::new(x, button_y),
            Size::new(button, button).expect("play button is finite"),
        );
        x += button + spacing;
        rect
    } else {
        empty
    };
    let back = if controls.seek_seconds {
        let rect = Rect::new(
            Point::new(x, button_y),
            Size::new(button, button).expect("back button is finite"),
        );
        x += button + spacing;
        rect
    } else {
        empty
    };
    let forward = if controls.seek_seconds {
        let rect = Rect::new(
            Point::new(x, button_y),
            Size::new(button, button).expect("forward button is finite"),
        );
        x += button + spacing;
        rect
    } else {
        empty
    };
    let time_origin = x;

    let mut right = bar.right() - padding;
    let fullscreen = if controls.fullscreen {
        right -= button;
        let rect = Rect::new(
            Point::new(right, button_y),
            Size::new(button, button).expect("fullscreen button is finite"),
        );
        right -= spacing;
        rect
    } else {
        empty
    };
    let loop_button = if controls.loop_button {
        right -= button;
        let rect = Rect::new(
            Point::new(right, button_y),
            Size::new(button, button).expect("loop button is finite"),
        );
        right -= spacing;
        rect
    } else {
        empty
    };
    let rate = if controls.rate && !video.behavior.playback_rates.is_empty() {
        right -= button;
        let rect = Rect::new(
            Point::new(right, button_y),
            Size::new(button, button).expect("rate button is finite"),
        );
        right -= spacing;
        rect
    } else {
        empty
    };
    let mute = if controls.mute || controls.volume {
        right -= button;
        let rect = Rect::new(
            Point::new(right, button_y),
            Size::new(button, button).expect("mute button is finite"),
        );
        right -= spacing;
        rect
    } else {
        empty
    };
    // The slider fills whatever gap remains between the time label and the
    // right cluster. On narrow surfaces it collapses instead of overlapping
    // the transport buttons; the mute icon keeps reporting the volume state.
    let time_reserve = if controls.time {
        let label = video
            .time_display
            .format(video.position(), video.duration());
        #[allow(clippy::cast_precision_loss)]
        let width = label.chars().count() as f32 * theme.time_size * 0.55;
        width + spacing
    } else {
        0.0
    };
    let volume_slider = if controls.volume && !mute.is_empty() {
        let width = VOLUME_SLIDER_WIDTH.min((right - spacing - (x + time_reserve)).max(0.0));
        if width >= 24.0 {
            right -= width + spacing;
            Rect::new(
                Point::new(right, button_y + (button - theme.track_height) / 2.0),
                Size::new(width, theme.track_height).expect("volume slider is finite"),
            )
        } else {
            empty
        }
    } else {
        empty
    };
    let volume_hit = Rect::new(
        Point::new(
            volume_slider.origin.x,
            volume_slider.origin.y - TRACK_HIT_PAD,
        ),
        Size::new(
            volume_slider.size.width(),
            volume_slider.size.height() + TRACK_HIT_PAD * 2.0,
        )
        .expect("video volume hit area is finite"),
    );
    let _ = right;

    let badge = CENTER_BADGE
        .min(bounds.size.width())
        .min(bounds.size.height());
    let center_play = Rect::new(
        Point::new(
            bounds.origin.x + (bounds.size.width() - badge) / 2.0,
            bounds.origin.y + (bounds.size.height() - badge) / 2.0,
        ),
        Size::new(badge, badge).expect("center badge is finite"),
    );

    VideoGeometry {
        bar,
        row_top,
        row_height,
        track,
        track_hit,
        play,
        back,
        forward,
        time_origin,
        mute,
        volume_slider,
        volume_hit,
        rate,
        loop_button,
        fullscreen,
        center_play,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use drawen_core::{Modifiers, PointerButtons, PointerEvent, WheelEvent};
    use drawen_render::{DisplayCommand, DisplayList};
    use drawen_ui::UiRuntime;

    use super::*;
    use crate::media::{MediaClock, MediaFrame, MediaUpdate};

    fn frame() -> RasterImage {
        RasterImage::new(2, 2, vec![255; 16]).expect("valid RGBA image")
    }

    struct MockSession {
        clock: MediaClock,
        metadata: MediaMetadata,
        pending: Vec<MediaFrame>,
        video_pausable: bool,
        video_enabled: bool,
    }

    impl MockSession {
        fn new() -> Self {
            let mut clock = MediaClock::new();
            clock.set_duration(Some(Duration::from_secs(2)));
            clock.seek(Duration::ZERO);
            Self {
                clock,
                metadata: MediaMetadata::new(4, 2, Duration::from_secs(2)),
                pending: Vec::new(),
                video_pausable: false,
                video_enabled: true,
            }
        }
    }

    impl MediaSession for MockSession {
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
            self.clock.play();
            Ok(())
        }

        fn pause(&mut self) {
            self.clock.pause();
        }

        fn seek(&mut self, position: Duration) -> Result<(), MediaError> {
            self.clock.seek(position);
            Ok(())
        }

        fn set_rate(&mut self, rate: f64) {
            self.clock.set_rate(rate);
        }

        fn supports_video_pause(&self) -> bool {
            self.video_pausable
        }

        fn set_video_enabled(&mut self, enabled: bool) -> Result<bool, MediaError> {
            if !self.video_pausable || self.video_enabled == enabled {
                return Ok(false);
            }
            self.video_enabled = enabled;
            Ok(true)
        }

        fn update(&mut self, delta: Duration) -> MediaUpdate {
            let state_changed = self.clock.advance(delta);
            let position = self.clock.position();
            let index = self
                .pending
                .iter()
                .position(|frame| frame.pts() <= position);
            let frame = index.map(|index| self.pending.remove(index));
            MediaUpdate {
                frame,
                error: None,
                state_changed,
            }
        }

        fn is_active(&self) -> bool {
            !self.pending.is_empty()
        }
    }

    fn pointer(position: Point, button: Option<PointerButton>) -> PointerEvent {
        PointerEvent {
            pointer_id: PointerId(1),
            position,
            button,
            buttons: if button.is_some() {
                PointerButtons::PRIMARY
            } else {
                PointerButtons::NONE
            },
            modifiers: Modifiers::NONE,
        }
    }

    fn viewport() -> Constraints {
        Constraints::tight(Size::new(320.0, 180.0).expect("valid viewport"))
            .expect("valid constraints")
    }

    #[test]
    fn clicking_toggles_playback_and_emits_a_signal() {
        let video = Video::with_frame(frame());
        let observed = Rc::new(Cell::new(false));
        let _subscription = video.play_toggled().subscribe({
            let observed = Rc::clone(&observed);
            move |playing| observed.set(*playing)
        });
        let mut runtime = UiRuntime::new(video);
        runtime.layout(viewport()).expect("video layout succeeds");

        let down = InputEvent::PointerDown(pointer(
            Point::new(100.0, 100.0),
            Some(PointerButton::Primary),
        ));
        assert_eq!(runtime.dispatch_event(&down), EventStatus::Handled);
        assert!(observed.get());
    }

    #[test]
    fn clicking_the_progress_track_seeks() {
        let video = Video::new().with_duration(Duration::from_secs(100));
        let observed = Rc::new(RefCell::new(Duration::ZERO));
        let _subscription = video.seek_requested().subscribe({
            let observed = Rc::clone(&observed);
            move |position| *observed.borrow_mut() = *position
        });
        let mut runtime = UiRuntime::new(video);
        runtime.layout(viewport()).expect("video layout succeeds");

        let bounds = Rect::new(Point::ZERO, Size::new(320.0, 180.0).expect("valid size"));
        let geometry = video_geometry(bounds, &PlayerTheme::default(), &Video::new());
        let down = InputEvent::PointerDown(pointer(
            Point::new(
                geometry.track.origin.x + geometry.track.size.width() / 2.0,
                geometry.track.origin.y,
            ),
            Some(PointerButton::Primary),
        ));
        assert_eq!(runtime.dispatch_event(&down), EventStatus::Handled);
        let seconds = observed.borrow().as_secs_f32();
        assert!((seconds - 50.0).abs() < 1.0);
    }

    #[test]
    fn seek_hit_area_reaches_beyond_the_thin_track() {
        let video = Video::new().with_duration(Duration::from_secs(100));
        let observed = Rc::new(RefCell::new(Duration::ZERO));
        let _subscription = video.seek_requested().subscribe({
            let observed = Rc::clone(&observed);
            move |position| *observed.borrow_mut() = *position
        });
        let mut runtime = UiRuntime::new(video);
        runtime.layout(viewport()).expect("video layout succeeds");

        let bounds = Rect::new(Point::ZERO, Size::new(320.0, 180.0).expect("valid size"));
        let geometry = video_geometry(bounds, &PlayerTheme::default(), &Video::new());
        let down = InputEvent::PointerDown(pointer(
            Point::new(
                geometry.track.origin.x + geometry.track.size.width() / 2.0,
                geometry.track.origin.y - 6.0,
            ),
            Some(PointerButton::Primary),
        ));
        assert_eq!(runtime.dispatch_event(&down), EventStatus::Handled);
        let seconds = observed.borrow().as_secs_f32();
        assert!((seconds - 50.0).abs() < 1.0);
    }

    #[test]
    fn buttons_win_over_the_expanded_seek_hit_area() {
        let video = Video::new().with_duration(Duration::from_secs(100));
        let played = Rc::new(Cell::new(false));
        let _play_subscription = video.play_toggled().subscribe({
            let played = Rc::clone(&played);
            move |playing| played.set(*playing)
        });
        let seeked = Rc::new(Cell::new(false));
        let _seek_subscription = video.seek_requested().subscribe({
            let seeked = Rc::clone(&seeked);
            move |_| seeked.set(true)
        });
        let mut runtime = UiRuntime::new(video);
        runtime.layout(viewport()).expect("video layout succeeds");

        let bounds = Rect::new(Point::ZERO, Size::new(320.0, 180.0).expect("valid size"));
        let geometry = video_geometry(bounds, &PlayerTheme::default(), &Video::new());
        // The top of the play button overlaps the expanded seek hit band.
        let point = Point::new(geometry.play.origin.x + 5.0, geometry.play.origin.y + 1.0);
        assert!(geometry.track_hit.contains(point));
        assert!(geometry.play.contains(point));
        let down = InputEvent::PointerDown(pointer(point, Some(PointerButton::Primary)));
        assert_eq!(runtime.dispatch_event(&down), EventStatus::Handled);
        assert!(played.get());
        assert!(!seeked.get());
    }

    #[test]
    fn volume_slider_aligns_with_the_button_row() {
        let bounds = Rect::new(Point::ZERO, Size::new(480.0, 270.0).expect("valid size"));
        let geometry = video_geometry(bounds, &PlayerTheme::default(), &Video::new());
        assert!(!geometry.volume_slider.is_empty());
        let slider_center =
            geometry.volume_slider.origin.y + geometry.volume_slider.size.height() / 2.0;
        let mute_center = geometry.mute.origin.y + geometry.mute.size.height() / 2.0;
        assert!((slider_center - mute_center).abs() < 0.5);
    }

    #[test]
    fn narrow_surfaces_collapse_the_volume_slider_instead_of_overlapping() {
        let bounds = Rect::new(Point::ZERO, Size::new(320.0, 180.0).expect("valid size"));
        let geometry = video_geometry(bounds, &PlayerTheme::default(), &Video::new());
        assert!(geometry.volume_slider.is_empty());
        assert!(geometry.forward.right() <= geometry.mute.origin.x);
        assert!(geometry.mute.right() <= geometry.rate.origin.x);
    }

    #[test]
    fn playback_advances_and_stops_at_the_end() {
        let mut video = Video::new()
            .with_duration(Duration::from_secs(1))
            .with_playing(true);
        assert!(video.advance_animation(Duration::from_millis(400), false));
        assert_eq!(video.position(), Duration::from_millis(400));
        assert!(video.advance_animation(Duration::from_millis(900), false));
        assert_eq!(video.position(), Duration::from_secs(1));
        assert!(!video.is_playing());
        assert!(!video.is_animating());
    }

    #[test]
    fn looping_playback_wraps_the_position() {
        let mut video = Video::new()
            .with_duration(Duration::from_secs(1))
            .with_looping(true)
            .with_playing(true);
        video.advance_animation(Duration::from_millis(1250), false);
        assert_eq!(video.position(), Duration::from_millis(250));
        assert!(video.is_animating());
    }

    #[test]
    fn paints_the_frame_and_controls() {
        let mut runtime =
            UiRuntime::new(Video::with_frame(frame()).with_duration(Duration::from_secs(90)));
        let mut list = DisplayList::new();
        runtime.layout(viewport()).expect("video layout succeeds");
        runtime
            .paint(&mut PaintContext::new(&mut list))
            .expect("video paint succeeds");

        assert!(
            list.commands()
                .iter()
                .any(|command| matches!(command, DisplayCommand::DrawImage { .. }))
        );
        assert!(
            list.commands()
                .iter()
                .any(|command| matches!(command, DisplayCommand::DrawText { .. }))
        );
    }

    #[test]
    fn open_reports_a_missing_ffprobe() {
        let config = FfmpegConfig::new()
            .with_ffmpeg("drawen-missing-ffmpeg")
            .with_ffprobe("drawen-missing-ffprobe");
        let Err(error) = Video::open_with("clip.mp4", config) else {
            panic!("opening without ffprobe must fail");
        };
        assert!(matches!(error, VideoError::FfprobeNotFound { .. }));
    }

    #[test]
    fn playing_from_the_end_restarts_at_zero() {
        let mut video = Video::new()
            .with_duration(Duration::from_secs(1))
            .with_position(Duration::from_secs(1));
        video.set_playing(true);
        assert_eq!(video.position(), Duration::ZERO);
        assert!(video.is_playing());
    }

    #[test]
    fn transport_commands_are_forwarded_to_the_session() {
        let mut video = Video::new().with_session(MockSession::new());
        assert_eq!(video.metadata().map(|metadata| metadata.width()), Some(4));

        video.set_playing(true);
        assert!(video.is_playing());
        video.set_position(Duration::from_secs(1));
        assert_eq!(video.position(), Duration::from_secs(1));
        video.set_rate(2.0);
        assert!((video.rate() - 2.0).abs() < f64::EPSILON);
        video.set_playing(false);
        assert!(!video.is_playing());

        video.close_decoder();
        assert!(!video.has_decoder());
        assert_eq!(video.position(), Duration::from_secs(1));
        assert_eq!(video.duration(), Duration::from_secs(2));
    }

    #[test]
    fn session_frames_reach_the_view_at_their_presentation_time() {
        let mut session = MockSession::new();
        session.pending.push(MediaFrame::new(
            RasterImage::new(3, 3, vec![255; 36]).expect("valid RGBA image"),
            Duration::ZERO,
            Duration::from_millis(100),
        ));
        let mut video = Video::new().with_session(session);

        assert!(video.advance_animation(Duration::ZERO, false));
        assert_eq!(video.frame().map(RasterImage::width), Some(3));
        assert!(!video.is_animating());
    }

    #[test]
    fn video_pause_is_forwarded_and_release_keeps_the_position() {
        let mut session = MockSession::new();
        session.video_pausable = true;
        session.clock.seek(Duration::from_millis(500));
        let mut video = Video::new().with_session(session);

        assert!(video.supports_video_pause());
        video.set_video_enabled(false);
        assert_eq!(video.decoder_error(), None);
        video.set_video_enabled(true);
        assert_eq!(video.decoder_error(), None);

        video.release_session();
        assert!(!video.supports_video_pause());
        assert!(!video.has_decoder());
        assert_eq!(video.position(), Duration::from_millis(500));
    }

    #[test]
    fn video_pause_is_ignored_without_session_support() {
        let mut video = Video::new().with_session(MockSession::new());

        assert!(!video.supports_video_pause());
        video.set_video_enabled(false);
        assert_eq!(video.decoder_error(), None);
        assert!(video.has_decoder());
    }

    #[test]
    fn volume_and_mute_emit_signals_and_clamp() {
        let video = Video::new().with_volume(0.25);
        assert!((video.volume() - 0.25).abs() < f32::EPSILON);
        let observed = Rc::new(Cell::new(0.0_f32));
        let _subscription = video.volume_changed().subscribe({
            let observed = Rc::clone(&observed);
            move |volume| observed.set(*volume)
        });
        let muted = Rc::new(Cell::new(false));
        let _mute_subscription = video.mute_toggled().subscribe({
            let muted = Rc::clone(&muted);
            move |value| muted.set(*value)
        });
        let mut runtime = UiRuntime::new(video);
        runtime.layout(viewport()).expect("video layout succeeds");

        let bounds = Rect::new(Point::ZERO, Size::new(320.0, 180.0).expect("valid size"));
        let geometry = video_geometry(bounds, &PlayerTheme::default(), &Video::new());
        let down = InputEvent::PointerDown(pointer(
            Point::new(geometry.mute.origin.x + 2.0, geometry.mute.origin.y + 2.0),
            Some(PointerButton::Primary),
        ));
        assert_eq!(runtime.dispatch_event(&down), EventStatus::Handled);
        assert!(muted.get());

        let wheel = InputEvent::Wheel(WheelEvent {
            position: Point::new(100.0, 100.0),
            delta: WheelDelta::Lines(Point::new(0.0, -1.0)),
            modifiers: Modifiers::NONE,
        });
        assert_eq!(runtime.dispatch_event(&wheel), EventStatus::Handled);
        assert!(observed.get() > 0.25);
    }

    #[test]
    fn maximum_volume_is_one() {
        let video = Video::new().with_volume(4.0);
        assert!((video.volume() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn control_set_can_hide_the_overlay() {
        let video = Video::with_frame(frame()).with_control_set(VideoControls::none());
        let mut runtime = UiRuntime::new(video);
        let mut list = DisplayList::new();
        runtime.layout(viewport()).expect("video layout succeeds");
        runtime
            .paint(&mut PaintContext::new(&mut list))
            .expect("video paint succeeds");
        assert!(
            !list
                .commands()
                .iter()
                .any(|command| matches!(command, DisplayCommand::DrawText { .. }))
        );
    }

    #[test]
    fn formats_times_with_hours_when_needed() {
        assert_eq!(crate::player::format_time(Duration::from_secs(5)), "0:05");
        assert_eq!(crate::player::format_time(Duration::from_secs(65)), "1:05");
        assert_eq!(
            crate::player::format_time(Duration::from_secs(3665)),
            "1:01:05"
        );
    }
}
