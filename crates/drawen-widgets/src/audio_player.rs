#[cfg(feature = "audio")]
use std::path::Path;
use std::sync::Arc;
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

use crate::image::{fitted_rect, image_size};
#[cfg(feature = "audio")]
use crate::media::MediaError;
use crate::media::{MediaMetadata, MediaSession, MediaState};
use crate::player::{
    AudioControls, AudioPlayerLayout, PlayerBehavior, PlayerCore, PlayerTheme, PlayerTimeDisplay,
    RepeatMode, cycle_rate, format_rate, paint_hover_background, paint_play_pause, paint_repeat,
    paint_shuffle, paint_track_skip, paint_vertical_gradient, paint_volume,
};
use crate::{Alignment, ImageFit};

const DEFAULT_WIDTH: f32 = 360.0;
const INLINE_HEIGHT: f32 = 112.0;
const COMPACT_HEIGHT: f32 = 64.0;
const VOLUME_SLIDER_WIDTH: f32 = 58.0;

/// A control under the pointer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HoverTarget {
    Play,
    Previous,
    Next,
    Mute,
    Rate,
    Repeat,
    Shuffle,
}

/// A pointer gesture owned by the player.
#[derive(Clone, Copy, Debug)]
enum Drag {
    Seek(PointerId),
    Volume(PointerId),
}

/// An audio-only player with optional cover art and a configurable control row.
///
/// The widget plays through any [`MediaSession`] that produces an audio clock,
/// including [`crate::AudioPaleoSession`]. It paints metadata, artwork, a seek
/// bar, and transport controls; it does not own a timer or decode on a worker
/// thread.
///
/// ```no_run
/// use std::time::Duration;
/// use drawen_core::Color;
/// use drawen_widgets::{AudioControls, AudioPlayer, AudioPlayerLayout, PlayerTheme};
///
/// let player = AudioPlayer::new()
///     .with_title("Night drive")
///     .with_subtitle("Paleo · FLAC")
///     .with_duration(Duration::from_secs(214))
///     .with_layout(AudioPlayerLayout::Inline)
///     .with_control_set(AudioControls::default())
///     .with_theme(PlayerTheme::new().with_accent(Color::rgba8(46, 196, 182, 255)));
/// ```
pub struct AudioPlayer {
    core: PlayerCore,
    theme: PlayerTheme,
    behavior: PlayerBehavior,
    controls: AudioControls,
    layout: AudioPlayerLayout,
    time_display: PlayerTimeDisplay,
    title: Option<String>,
    subtitle: Option<String>,
    subtitle_fallback: Option<String>,
    cover: Option<Arc<RasterImage>>,
    cover_size: Option<f32>,
    repeat: RepeatMode,
    shuffle: bool,
    viewport: Size,
    hover: Option<HoverTarget>,
    drag: Option<Drag>,
    pointer_inside: bool,
    play_toggled: Signal<bool>,
    seek_requested: Signal<Duration>,
    volume_changed: Signal<f32>,
    mute_toggled: Signal<bool>,
    rate_changed: Signal<f64>,
    repeat_changed: Signal<RepeatMode>,
    shuffle_toggled: Signal<bool>,
    previous_requested: Signal<()>,
    next_requested: Signal<()>,
}

impl AudioPlayer {
    /// Creates an empty audio player.
    #[must_use]
    pub fn new() -> Self {
        // A music player's controls are normally permanent; video overlays hide.
        let behavior = PlayerBehavior {
            auto_hide: None,
            ..PlayerBehavior::default()
        };
        let mut core = PlayerCore::new();
        core.apply_behavior(&behavior);
        Self {
            core,
            theme: PlayerTheme::default(),
            behavior,
            controls: AudioControls::default(),
            layout: AudioPlayerLayout::Inline,
            time_display: PlayerTimeDisplay::default(),
            title: None,
            subtitle: None,
            subtitle_fallback: None,
            cover: None,
            cover_size: None,
            repeat: RepeatMode::Off,
            shuffle: false,
            viewport: Size::ZERO,
            hover: None,
            drag: None,
            pointer_inside: false,
            play_toggled: Signal::new(),
            seek_requested: Signal::new(),
            volume_changed: Signal::new(),
            mute_toggled: Signal::new(),
            rate_changed: Signal::new(),
            repeat_changed: Signal::new(),
            shuffle_toggled: Signal::new(),
            previous_requested: Signal::new(),
            next_requested: Signal::new(),
        }
    }

    /// Opens `path` with audio-mastered Paleo playback.
    ///
    /// Requires the `audio` feature. The file may be audio-only or contain
    /// video as well; the player shows only its audio controls either way.
    ///
    /// # Errors
    ///
    /// Returns [`MediaError`] when the file cannot be opened, no playable track
    /// exists, or an audio stream is malformed.
    #[cfg(feature = "audio")]
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MediaError> {
        let session = crate::AudioPaleoSession::open(path.as_ref())?;
        Ok(Self::new().with_session(session))
    }

    /// Attaches an externally created [`MediaSession`].
    #[must_use]
    pub fn with_session(mut self, session: impl MediaSession + 'static) -> Self {
        self.core.attach(session);
        self.refresh_metadata();
        self
    }

    /// Returns the metadata reported by the attached session.
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

    /// Stops and releases the attached session.
    pub fn close_decoder(&mut self) {
        self.core.close_session();
    }

    /// Detaches the media session and stops playback.
    pub fn release_session(&mut self) {
        self.core.release_session();
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

    /// Returns whether playback reached the end.
    #[must_use]
    pub fn is_ended(&self) -> bool {
        self.core.is_ended()
    }

    /// Returns the current playback rate.
    #[must_use]
    pub fn rate(&self) -> f64 {
        self.core.rate()
    }

    /// Returns the output volume in the `0.0..=1.0` range.
    #[must_use]
    pub fn volume(&self) -> f32 {
        self.core.volume()
    }

    /// Returns whether output is muted.
    #[must_use]
    pub fn is_muted(&self) -> bool {
        self.core.muted()
    }

    /// Returns the configured repeat mode.
    #[must_use]
    pub const fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    /// Returns whether shuffle is enabled.
    #[must_use]
    pub const fn shuffle(&self) -> bool {
        self.shuffle
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
    pub const fn control_set(&self) -> AudioControls {
        self.controls
    }

    /// Returns the configured layout.
    #[must_use]
    pub const fn layout(&self) -> AudioPlayerLayout {
        self.layout
    }

    /// Returns the configured time-label format.
    #[must_use]
    pub const fn time_display(&self) -> PlayerTimeDisplay {
        self.time_display
    }

    /// Returns the current title.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Returns the current subtitle.
    #[must_use]
    pub fn subtitle(&self) -> Option<&str> {
        self.subtitle.as_deref()
    }

    /// Returns the current cover artwork.
    #[must_use]
    pub fn cover(&self) -> Option<&RasterImage> {
        self.cover.as_deref()
    }

    /// Returns a signal emitted when the user starts or pauses playback.
    #[must_use]
    pub fn play_toggled(&self) -> Signal<bool> {
        self.play_toggled.clone()
    }

    /// Returns a signal emitted when the user seeks.
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

    /// Returns a signal emitted when the user changes the repeat mode.
    #[must_use]
    pub fn repeat_changed(&self) -> Signal<RepeatMode> {
        self.repeat_changed.clone()
    }

    /// Returns a signal emitted when the user toggles shuffle.
    #[must_use]
    pub fn shuffle_toggled(&self) -> Signal<bool> {
        self.shuffle_toggled.clone()
    }

    /// Returns a signal emitted when the user asks for the previous track.
    #[must_use]
    pub fn previous_requested(&self) -> Signal<()> {
        self.previous_requested.clone()
    }

    /// Returns a signal emitted when the user asks for the next track.
    #[must_use]
    pub fn next_requested(&self) -> Signal<()> {
        self.next_requested.clone()
    }

    /// Sets the title metadata.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = Some(title.into());
    }

    /// Sets the subtitle metadata.
    pub fn set_subtitle(&mut self, subtitle: impl Into<String>) {
        self.subtitle = Some(subtitle.into());
    }

    /// Sets or clears the cover artwork.
    pub fn set_cover(&mut self, cover: Option<RasterImage>) {
        self.cover = cover.map(Arc::new);
    }

    /// Sets the media duration.
    pub fn set_duration(&mut self, duration: Duration) {
        self.core.set_duration(duration);
    }

    /// Seeks to `position`.
    pub fn set_position(&mut self, position: Duration) {
        self.core.set_position(position);
    }

    /// Updates playback state without emitting [`Self::play_toggled`].
    pub fn set_playing(&mut self, playing: bool) {
        self.core.set_playing(playing);
    }

    /// Starts or pauses playback without emitting [`Self::play_toggled`].
    pub fn toggle_playback(&mut self) {
        self.set_playing(!self.is_playing());
    }

    /// Changes the playback rate.
    pub fn set_rate(&mut self, rate: f64) {
        self.core.set_rate(rate);
    }

    /// Sets the output volume.
    pub fn set_volume(&mut self, volume: f32) {
        self.core.set_volume(volume);
    }

    /// Mutes or unmutes output.
    pub fn set_muted(&mut self, muted: bool) {
        self.core.set_muted(muted);
    }

    /// Sets the repeat mode and updates looping.
    pub fn set_repeat(&mut self, repeat: RepeatMode) {
        self.repeat = repeat;
        self.core.set_looping(repeat.is_looping());
    }

    /// Enables or disables shuffle.
    pub fn set_shuffle(&mut self, shuffle: bool) {
        self.shuffle = shuffle;
    }

    /// Replaces the visual theme.
    pub fn set_theme(&mut self, theme: PlayerTheme) {
        self.theme = theme;
    }

    /// Replaces the behavior, including volume and looping.
    pub fn set_behavior(&mut self, behavior: PlayerBehavior) {
        self.behavior = behavior;
        self.core.apply_behavior(&self.behavior);
    }

    /// Replaces the control set.
    pub fn set_control_set(&mut self, controls: AudioControls) {
        self.controls = controls;
    }

    /// Replaces the layout.
    pub fn set_layout(&mut self, layout: AudioPlayerLayout) {
        self.layout = layout;
    }

    /// Sets the time-label format.
    pub fn set_time_display(&mut self, display: PlayerTimeDisplay) {
        self.time_display = display;
    }

    fn refresh_metadata(&mut self) {
        if self.subtitle.is_none() {
            self.subtitle_fallback = self.metadata().and_then(|metadata| {
                (metadata.width() > 0 && metadata.height() > 0)
                    .then(|| format!("{}x{}", metadata.width(), metadata.height()))
            });
        }
    }

    fn toggle(&mut self, context: &mut EventContext<'_>) {
        self.core.toggle_playback();
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
        let track = audio_geometry(Rect::new(Point::ZERO, self.viewport), &self.theme, self).track;
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
            audio_geometry(Rect::new(Point::ZERO, self.viewport), &self.theme, self).volume_slider;
        let ratio = if slider.size.width() > 0.0 {
            ((x - slider.origin.x) / slider.size.width()).clamp(0.0, 1.0)
        } else {
            self.volume()
        };
        if self.core.set_volume(ratio) {
            context.request_redraw();
            self.volume_changed.emit(&ratio);
        }
    }

    fn change_volume(&mut self, delta: f32, context: &mut EventContext<'_>) {
        let volume = (self.volume() + delta).clamp(0.0, 1.0);
        if self.core.set_volume(volume) {
            context.request_redraw();
            self.volume_changed.emit(&volume);
        }
    }

    fn toggle_mute(&mut self, context: &mut EventContext<'_>) {
        let muted = !self.is_muted();
        self.core.set_muted(muted);
        context.request_redraw();
        self.mute_toggled.emit(&muted);
    }

    fn cycle_rate(&mut self, context: &mut EventContext<'_>) {
        let rate = cycle_rate(self.rate(), &self.behavior.playback_rates);
        self.core.set_rate(rate);
        context.request_redraw();
        self.rate_changed.emit(&rate);
    }

    fn cycle_repeat(&mut self, context: &mut EventContext<'_>) {
        let repeat = self.repeat.next();
        self.set_repeat(repeat);
        context.request_redraw();
        self.repeat_changed.emit(&repeat);
    }

    fn toggle_shuffle(&mut self, context: &mut EventContext<'_>) {
        self.shuffle = !self.shuffle;
        context.request_redraw();
        self.shuffle_toggled.emit(&self.shuffle);
    }

    fn hit_target(&self, point: Point) -> Option<HoverTarget> {
        let geometry = audio_geometry(Rect::new(Point::ZERO, self.viewport), &self.theme, self);
        if geometry.play.contains(point) {
            Some(HoverTarget::Play)
        } else if geometry.previous.contains(point) {
            Some(HoverTarget::Previous)
        } else if geometry.next.contains(point) {
            Some(HoverTarget::Next)
        } else if geometry.mute.contains(point) {
            Some(HoverTarget::Mute)
        } else if geometry.rate.contains(point) {
            Some(HoverTarget::Rate)
        } else if geometry.repeat.contains(point) {
            Some(HoverTarget::Repeat)
        } else if geometry.shuffle.contains(point) {
            Some(HoverTarget::Shuffle)
        } else {
            None
        }
    }

    fn activate_target(&mut self, target: HoverTarget, context: &mut EventContext<'_>) {
        match target {
            HoverTarget::Play => self.toggle(context),
            HoverTarget::Previous => self.previous_requested.emit(&()),
            HoverTarget::Next => self.next_requested.emit(&()),
            HoverTarget::Mute => self.toggle_mute(context),
            HoverTarget::Rate => self.cycle_rate(context),
            HoverTarget::Repeat => self.cycle_repeat(context),
            HoverTarget::Shuffle => self.toggle_shuffle(context),
        }
    }

    fn poll(&mut self, delta: Duration) -> bool {
        self.core.tick(delta)
    }

    /// Returns this player with a title.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Returns this player with a subtitle.
    #[must_use]
    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Returns this player with cover artwork.
    #[must_use]
    pub fn with_cover(mut self, cover: RasterImage) -> Self {
        self.cover = Some(Arc::new(cover));
        self
    }

    /// Returns this player with a fixed artwork size.
    ///
    /// The default is derived from the layout and the available height.
    #[must_use]
    pub fn with_cover_size(mut self, size: f32) -> Self {
        self.cover_size = Some(size.max(0.0));
        self
    }

    /// Returns this player with a specific layout.
    #[must_use]
    pub const fn with_layout(mut self, layout: AudioPlayerLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Returns this player with a specific visual theme.
    #[must_use]
    pub fn with_theme(mut self, theme: PlayerTheme) -> Self {
        self.theme = theme;
        self
    }

    /// Returns this player with a specific behavior.
    #[must_use]
    pub fn with_behavior(mut self, behavior: PlayerBehavior) -> Self {
        self.set_behavior(behavior);
        self
    }

    /// Returns this player with a specific control set.
    #[must_use]
    pub fn with_control_set(mut self, controls: AudioControls) -> Self {
        self.controls = controls;
        self
    }

    /// Returns this player with a specific time-label format.
    #[must_use]
    pub const fn with_time_display(mut self, display: PlayerTimeDisplay) -> Self {
        self.time_display = display;
        self
    }

    /// Returns this player with an initial volume.
    #[must_use]
    pub fn with_volume(mut self, volume: f32) -> Self {
        self.behavior.volume = crate::player::clamp_unit(volume);
        self.core.set_volume(volume);
        self
    }

    /// Returns this player starting muted or unmuted.
    #[must_use]
    pub fn with_muted(mut self, muted: bool) -> Self {
        self.behavior.muted = muted;
        self.core.set_muted(muted);
        self
    }

    /// Returns this player with an explicit duration.
    #[must_use]
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.set_duration(duration);
        self
    }

    /// Returns this player with an initial position.
    #[must_use]
    pub fn with_position(mut self, position: Duration) -> Self {
        self.set_position(position);
        self
    }

    /// Returns this player playing or paused.
    #[must_use]
    pub fn with_playing(mut self, playing: bool) -> Self {
        self.set_playing(playing);
        self
    }

    /// Returns this player with a repeat mode.
    #[must_use]
    pub fn with_repeat(mut self, repeat: RepeatMode) -> Self {
        self.set_repeat(repeat);
        self
    }

    /// Returns this player with shuffle enabled or disabled.
    #[must_use]
    pub fn with_shuffle(mut self, shuffle: bool) -> Self {
        self.shuffle = shuffle;
        self
    }

    fn font(environment: &UiEnvironment, style: &DssStyle, size: f32) -> drawen_render::Font {
        let mut font = environment.font_for(style);
        if style.font_size.is_none() {
            font.set_pixel_size(size);
        }
        font
    }

    #[allow(clippy::too_many_arguments, clippy::unused_self)]
    fn paint_text(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        style: &DssStyle,
        text: &str,
        color: Color,
        size: f32,
        rect: Rect,
    ) {
        if rect.is_empty() || text.is_empty() {
            return;
        }
        let font = Self::font(environment, style, size);
        let layout = environment.text_shaper().layout(
            text,
            &TextStyle::new_with_font(font, color),
            Some(rect.size.width()),
        );
        context.with_clip(rect, |context| {
            context.draw_text(layout, rect.origin);
        });
    }

    #[allow(clippy::too_many_lines)]
    fn paint_player(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        style: &DssStyle,
        bounds: Rect,
    ) {
        let theme = environment.theme();
        let background = self.theme.surface(theme);
        context.fill_rounded_rect(bounds, self.theme.corner_radius, background);
        if let Some(border) = style.border {
            context.stroke_rounded_rect(
                bounds,
                self.theme.corner_radius,
                border.width,
                border.color,
            );
        }

        let foreground = self.theme.foreground(theme, style);
        let muted = self.theme.muted_foreground(theme);
        let accent = self.theme.accent(theme, style);
        let geometry = audio_geometry(bounds, &self.theme, self);

        if !geometry.cover.is_empty() {
            context.fill_rounded_rect(
                geometry.cover,
                self.theme.corner_radius,
                background.mix(accent, 0.14),
            );
            if let Some(cover) = self.cover.as_deref() {
                let destination = fitted_rect(
                    geometry.cover,
                    image_size(cover),
                    ImageFit::Cover,
                    Alignment::Center,
                    Alignment::Center,
                );
                context.with_clip(geometry.cover, |context| {
                    context.draw_image(
                        Arc::clone(self.cover.as_ref().expect("cover exists")),
                        destination,
                    );
                });
            } else {
                paint_play_pause(context, geometry.cover, true, muted.with_alpha(0.6));
            }
        }

        if let Some(subtitle) = self
            .subtitle
            .as_deref()
            .or(self.subtitle_fallback.as_deref())
        {
            self.paint_text(
                context,
                environment,
                style,
                subtitle,
                muted,
                self.theme.subtitle_size,
                geometry.subtitle,
            );
        }
        if let Some(title) = &self.title {
            self.paint_text(
                context,
                environment,
                style,
                title,
                foreground,
                self.theme.title_size,
                geometry.title,
            );
        }

        self.paint_progress(context, &geometry, foreground, accent);
        self.paint_controls(context, environment, style, &geometry, foreground, accent);

        if let Some(error) = self.core.error() {
            let font = Self::font(environment, style, self.theme.font_size);
            let color = self.theme.error(theme);
            let layout = environment.text_shaper().layout(
                error,
                &TextStyle::new_with_font(font, color),
                Some((bounds.size.width() - self.theme.padding * 2.0).max(0.0)),
            );
            context.draw_text(
                layout,
                Point::new(
                    bounds.origin.x + self.theme.padding,
                    bounds.origin.y + bounds.size.height()
                        - self.theme.font_size
                        - self.theme.padding,
                ),
            );
        }

        if self.core.state() == MediaState::Buffering {
            paint_vertical_gradient(
                context,
                geometry.track,
                Color::TRANSPARENT,
                accent.with_alpha(0.2),
            );
        }
    }

    fn paint_progress(
        &self,
        context: &mut PaintContext<'_>,
        geometry: &AudioGeometry,
        foreground: Color,
        accent: Color,
    ) {
        if !self.controls.seek_bar || geometry.track.is_empty() {
            return;
        }
        context.fill_rounded_rect(
            geometry.track,
            geometry.track.size.height() / 2.0,
            self.theme.track(foreground),
        );
        let ratio = if self.duration() > Duration::ZERO {
            (self.position().as_secs_f32() / self.duration().as_secs_f32()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let width = geometry.track.size.width() * ratio;
        if width > 0.0 {
            context.fill_rounded_rect(
                Rect::new(
                    geometry.track.origin,
                    Size::new(width, geometry.track.size.height())
                        .expect("audio progress fill is finite"),
                ),
                geometry.track.size.height() / 2.0,
                accent,
            );
        }
        let thumb = geometry.track.size.height() * 2.4;
        context.fill_rounded_rect(
            Rect::new(
                Point::new(
                    geometry.track.origin.x + width - thumb / 2.0,
                    geometry.track.origin.y + (geometry.track.size.height() - thumb) / 2.0,
                ),
                Size::new(thumb, thumb).expect("audio progress thumb is finite"),
            ),
            thumb / 2.0,
            self.theme.badge(accent),
        );
    }

    #[allow(clippy::too_many_lines)]
    fn paint_controls(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        style: &DssStyle,
        geometry: &AudioGeometry,
        foreground: Color,
        accent: Color,
    ) {
        let muted = self.theme.muted_foreground(environment.theme());
        if self.controls.play_pause {
            paint_hover_background(
                context,
                geometry.play,
                self.hover == Some(HoverTarget::Play),
                foreground,
                self.theme.corner_radius,
            );
            paint_play_pause(context, geometry.play, self.is_playing(), foreground);
        }
        if self.controls.previous {
            paint_hover_background(
                context,
                geometry.previous,
                self.hover == Some(HoverTarget::Previous),
                foreground,
                self.theme.corner_radius,
            );
            paint_track_skip(context, geometry.previous, false, foreground);
        }
        if self.controls.next {
            paint_hover_background(
                context,
                geometry.next,
                self.hover == Some(HoverTarget::Next),
                foreground,
                self.theme.corner_radius,
            );
            paint_track_skip(context, geometry.next, true, foreground);
        }
        if self.controls.time && !geometry.time.is_empty() {
            self.paint_text(
                context,
                environment,
                style,
                &self.time_display.format(self.position(), self.duration()),
                muted,
                self.theme.time_size,
                geometry.time,
            );
        }

        if self.controls.mute || self.controls.volume {
            paint_hover_background(
                context,
                geometry.mute,
                self.hover == Some(HoverTarget::Mute),
                foreground,
                self.theme.corner_radius,
            );
            let level = if self.is_muted() { 0.0 } else { self.volume() };
            paint_volume(context, geometry.mute, self.is_muted(), level, foreground);
        }
        if self.controls.volume && !geometry.volume_slider.is_empty() {
            context.fill_rounded_rect(
                geometry.volume_slider,
                geometry.volume_slider.size.height() / 2.0,
                self.theme.track(foreground),
            );
            let width = geometry.volume_slider.size.width() * self.volume();
            if width > 0.0 {
                context.fill_rounded_rect(
                    Rect::new(
                        geometry.volume_slider.origin,
                        Size::new(width, geometry.volume_slider.size.height())
                            .expect("audio volume fill is finite"),
                    ),
                    geometry.volume_slider.size.height() / 2.0,
                    accent,
                );
            }
        }
        if self.controls.rate && !geometry.rate.is_empty() {
            paint_hover_background(
                context,
                geometry.rate,
                self.hover == Some(HoverTarget::Rate),
                foreground,
                self.theme.corner_radius,
            );
            let font = Self::font(environment, style, self.theme.time_size);
            let layout = environment.text_shaper().layout(
                &format_rate(self.rate()),
                &TextStyle::new_with_font(font, foreground),
                None,
            );
            let size = layout.size();
            context.draw_text(
                layout,
                Point::new(
                    geometry.rate.origin.x + (geometry.rate.size.width() - size.width()) / 2.0,
                    geometry.rate.origin.y + (geometry.rate.size.height() - size.height()) / 2.0,
                ),
            );
        }
        if self.controls.repeat && !geometry.repeat.is_empty() {
            paint_hover_background(
                context,
                geometry.repeat,
                self.hover == Some(HoverTarget::Repeat),
                foreground,
                self.theme.corner_radius,
            );
            paint_repeat(
                context,
                geometry.repeat,
                self.repeat.is_looping(),
                if self.repeat.is_looping() {
                    accent
                } else {
                    foreground
                },
            );
        }
        if self.controls.shuffle && !geometry.shuffle.is_empty() {
            paint_hover_background(
                context,
                geometry.shuffle,
                self.hover == Some(HoverTarget::Shuffle),
                foreground,
                self.theme.corner_radius,
            );
            paint_shuffle(context, geometry.shuffle, self.shuffle, accent);
        }
    }
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for AudioPlayer {
    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("audio-player")
    }

    fn layout(&mut self, _: &mut LayoutContext<'_>, constraints: Constraints) -> LayoutResult {
        let max = constraints.max();
        let width = if max.width().is_finite() {
            max.width()
        } else {
            DEFAULT_WIDTH
        };
        let height = if max.height().is_finite() {
            max.height()
        } else {
            match self.layout {
                AudioPlayerLayout::Inline => INLINE_HEIGHT,
                AudioPlayerLayout::Compact => COMPACT_HEIGHT,
            }
        };
        self.viewport =
            constraints.constrain(Size::new(width, height).expect("audio player sizes are finite"));
        LayoutResult::new(self.viewport)
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        self.paint_styled(context, environment, bounds, &DssStyle::default());
    }

    fn paint_styled(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        bounds: Rect,
        style: &DssStyle,
    ) {
        self.paint_player(context, environment, style, bounds);
    }

    fn advance_animation(&mut self, delta: Duration, settle: bool) -> bool {
        self.poll(if settle { Duration::ZERO } else { delta })
    }

    fn is_animating(&self) -> bool {
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
                self.hover = self.hit_target(pointer.position);
                EventStatus::Handled
            }
            InputEvent::PointerLeave(_) => {
                self.pointer_inside = false;
                self.hover = None;
                EventStatus::Handled
            }
            InputEvent::PointerMove(pointer) => {
                self.pointer_inside = true;
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
                EventStatus::Handled
            }
            InputEvent::PointerDown(pointer) if pointer.button == Some(PointerButton::Primary) => {
                context.request_focus();
                let geometry =
                    audio_geometry(Rect::new(Point::ZERO, self.viewport), &self.theme, self);
                if self.controls.seek_bar && geometry.track.contains(pointer.position) {
                    self.drag = Some(Drag::Seek(pointer.pointer_id));
                    context.capture_pointer(pointer.pointer_id);
                    self.seek_from_pointer(pointer.position.x, context);
                    return EventStatus::Handled;
                }
                if self.controls.volume && geometry.volume_slider.contains(pointer.position) {
                    self.drag = Some(Drag::Volume(pointer.pointer_id));
                    context.capture_pointer(pointer.pointer_id);
                    self.volume_from_pointer(pointer.position.x, context);
                    return EventStatus::Handled;
                }
                if let Some(target) = self.hit_target(pointer.position) {
                    self.activate_target(target, context);
                    return EventStatus::Handled;
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
                Key::Character(character) if character.eq_ignore_ascii_case("n") => {
                    self.next_requested.emit(&());
                    EventStatus::Handled
                }
                Key::Character(character) if character.eq_ignore_ascii_case("p") => {
                    self.previous_requested.emit(&());
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
        let accessibility = Accessibility::new(AccessibilityRole::Group)
            .with_numeric_value(
                self.position().as_secs_f64(),
                0.0,
                self.duration().as_secs_f64().max(1.0),
            )
            .with_value(self.time_display.format(self.position(), self.duration()));
        match self.title.as_deref() {
            Some(title) => accessibility.with_label(title.to_owned()),
            None => accessibility,
        }
    }
}

fn drag_pointer(drag: Drag) -> PointerId {
    match drag {
        Drag::Seek(pointer) | Drag::Volume(pointer) => pointer,
    }
}

/// Rectangles used by painting and hit testing for an audio player.
#[derive(Clone, Copy, Debug)]
struct AudioGeometry {
    cover: Rect,
    title: Rect,
    subtitle: Rect,
    track: Rect,
    play: Rect,
    previous: Rect,
    next: Rect,
    time: Rect,
    mute: Rect,
    volume_slider: Rect,
    rate: Rect,
    repeat: Rect,
    shuffle: Rect,
}

#[allow(clippy::too_many_lines)]
fn audio_geometry(bounds: Rect, theme: &PlayerTheme, player: &AudioPlayer) -> AudioGeometry {
    let controls = player.controls;
    let padding = theme.padding;
    let spacing = theme.spacing;
    let compact = player.layout == AudioPlayerLayout::Compact;
    let button = if compact {
        theme.button_size.min(26.0)
    } else {
        theme.button_size
    };
    let empty = Rect::new(Point::ZERO, Size::ZERO);
    let inner = Rect::new(
        Point::new(bounds.origin.x + padding, bounds.origin.y + padding),
        Size::new(
            (bounds.size.width() - padding * 2.0).max(0.0),
            (bounds.size.height() - padding * 2.0).max(0.0),
        )
        .expect("audio player inner rect is finite"),
    );

    let cover_size = player
        .cover_size
        .unwrap_or(if compact {
            inner.size.height().min(40.0)
        } else {
            inner.size.height().min(78.0)
        })
        .min(inner.size.height())
        .min(inner.size.width());
    let cover = if controls.cover && player.cover.is_some() && cover_size > 0.0 {
        Rect::new(
            inner.origin,
            Size::new(cover_size, cover_size).expect("cover rect is finite"),
        )
    } else {
        empty
    };
    let content_x = if cover.is_empty() {
        inner.origin.x
    } else {
        cover.right() + spacing
    };
    let content_right = inner.right();
    let content_width = (content_right - content_x).max(0.0);

    if compact {
        let row_y = inner.origin.y + (inner.size.height() - button) / 2.0;
        let play = Rect::new(
            Point::new(content_x, row_y),
            Size::new(button, button).expect("compact play is finite"),
        );
        let mute = {
            let x = content_right - button;
            Rect::new(
                Point::new(x, row_y),
                Size::new(button, button).expect("compact mute is finite"),
            )
        };
        let title = Rect::new(
            Point::new(play.right() + spacing, inner.origin.y),
            Size::new(
                (content_right - play.right() - spacing - button - spacing).max(0.0),
                (inner.size.height() * 0.55).max(0.0),
            )
            .expect("compact title is finite"),
        );
        let track = Rect::new(
            Point::new(title.origin.x, inner.bottom() - theme.track_height - 4.0),
            Size::new(title.size.width(), theme.track_height).expect("compact track is finite"),
        );
        return AudioGeometry {
            cover,
            title,
            subtitle: empty,
            track,
            play,
            previous: empty,
            next: empty,
            time: empty,
            mute,
            volume_slider: empty,
            rate: empty,
            repeat: empty,
            shuffle: empty,
        };
    }

    let row_y = inner.bottom() - button;
    let track = Rect::new(
        Point::new(content_x, row_y - spacing - theme.track_height),
        Size::new(content_width, theme.track_height).expect("audio track is finite"),
    );
    let metadata_height = (track.origin.y - inner.origin.y - 4.0).max(0.0);
    let title = Rect::new(
        inner.origin,
        Size::new(content_width, (metadata_height * 0.55).max(0.0)).expect("audio title is finite"),
    );
    let title = Rect::new(Point::new(content_x, inner.origin.y), title.size);
    let subtitle = Rect::new(
        Point::new(content_x, title.bottom() + 2.0),
        Size::new(
            content_width,
            (metadata_height - title.size.height() - 2.0).max(0.0),
        )
        .expect("audio subtitle is finite"),
    );

    let mut x = content_x;
    let play = if controls.play_pause {
        let rect = Rect::new(
            Point::new(x, row_y),
            Size::new(button, button).expect("audio play is finite"),
        );
        x += button + spacing;
        rect
    } else {
        empty
    };
    let previous = if controls.previous {
        let rect = Rect::new(
            Point::new(x, row_y),
            Size::new(button, button).expect("audio previous is finite"),
        );
        x += button + spacing;
        rect
    } else {
        empty
    };
    let next = if controls.next {
        let rect = Rect::new(
            Point::new(x, row_y),
            Size::new(button, button).expect("audio next is finite"),
        );
        x += button + spacing;
        rect
    } else {
        empty
    };
    let time = Rect::new(
        Point::new(x, row_y),
        Size::new((content_right - x - spacing).clamp(0.0, 112.0), button)
            .expect("audio time is finite"),
    );

    let mut right = content_right;
    let shuffle = if controls.shuffle {
        right -= button;
        let rect = Rect::new(
            Point::new(right, row_y),
            Size::new(button, button).expect("audio shuffle is finite"),
        );
        right -= spacing;
        rect
    } else {
        empty
    };
    let repeat = if controls.repeat {
        right -= button;
        let rect = Rect::new(
            Point::new(right, row_y),
            Size::new(button, button).expect("audio repeat is finite"),
        );
        right -= spacing;
        rect
    } else {
        empty
    };
    let rate = if controls.rate && !player.behavior.playback_rates.is_empty() {
        right -= button;
        let rect = Rect::new(
            Point::new(right, row_y),
            Size::new(button, button).expect("audio rate is finite"),
        );
        right -= spacing;
        rect
    } else {
        empty
    };
    let volume_slider = if controls.volume {
        let width = VOLUME_SLIDER_WIDTH.min((right - padding).max(0.0));
        right -= width + spacing;
        Rect::new(
            Point::new(right, row_y + (button - theme.track_height) / 2.0),
            Size::new(width, theme.track_height).expect("audio volume slider is finite"),
        )
    } else {
        empty
    };
    let mute = if controls.mute || controls.volume {
        right -= button;
        Rect::new(
            Point::new(right, row_y),
            Size::new(button, button).expect("audio mute is finite"),
        )
    } else {
        empty
    };

    AudioGeometry {
        cover,
        title,
        subtitle,
        track,
        play,
        previous,
        next,
        time,
        mute,
        volume_slider,
        rate,
        repeat,
        shuffle,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use drawen_core::{Modifiers, PointerButtons, PointerEvent};
    use drawen_render::{DisplayCommand, DisplayList};
    use drawen_ui::UiRuntime;

    use super::*;
    use crate::media::{MediaClock, MediaError};

    struct MockSession {
        clock: MediaClock,
        metadata: MediaMetadata,
    }

    impl MockSession {
        fn new() -> Self {
            let mut clock = MediaClock::new();
            clock.set_duration(Some(Duration::from_secs(120)));
            clock.seek(Duration::ZERO);
            Self {
                clock,
                metadata: MediaMetadata::new(0, 0, Duration::from_secs(120)),
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

        fn update(&mut self, delta: Duration) -> crate::media::MediaUpdate {
            crate::media::MediaUpdate {
                frame: None,
                error: None,
                state_changed: self.clock.advance(delta),
            }
        }

        fn is_active(&self) -> bool {
            false
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

    fn constraints() -> Constraints {
        Constraints::tight(Size::new(360.0, 112.0).expect("valid size")).expect("valid constraints")
    }

    #[test]
    fn clicking_play_toggles_and_emits_a_signal() {
        let player = AudioPlayer::new().with_duration(Duration::from_secs(60));
        let observed = Rc::new(Cell::new(false));
        let _subscription = player.play_toggled().subscribe({
            let observed = Rc::clone(&observed);
            move |playing| observed.set(*playing)
        });
        let mut runtime = UiRuntime::new(player);
        runtime
            .layout(constraints())
            .expect("audio layout succeeds");
        let geometry = audio_geometry(
            Rect::new(Point::ZERO, Size::new(360.0, 112.0).expect("valid size")),
            &PlayerTheme::default(),
            &AudioPlayer::new(),
        );
        let down = InputEvent::PointerDown(pointer(
            Point::new(geometry.play.origin.x + 2.0, geometry.play.origin.y + 2.0),
            Some(PointerButton::Primary),
        ));
        assert_eq!(runtime.dispatch_event(&down), EventStatus::Handled);
        assert!(observed.get());
    }

    #[test]
    fn clicking_the_track_seeks() {
        let player = AudioPlayer::new().with_duration(Duration::from_secs(100));
        let observed = Rc::new(RefCell::new(Duration::ZERO));
        let _subscription = player.seek_requested().subscribe({
            let observed = Rc::clone(&observed);
            move |position| *observed.borrow_mut() = *position
        });
        let mut runtime = UiRuntime::new(player);
        runtime
            .layout(constraints())
            .expect("audio layout succeeds");
        let geometry = audio_geometry(
            Rect::new(Point::ZERO, Size::new(360.0, 112.0).expect("valid size")),
            &PlayerTheme::default(),
            &AudioPlayer::new(),
        );
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
    fn repeat_cycles_and_controls_looping() {
        let player = AudioPlayer::new().with_repeat(RepeatMode::Off);
        let observed = Rc::new(Cell::new(RepeatMode::Off));
        let _subscription = player.repeat_changed().subscribe({
            let observed = Rc::clone(&observed);
            move |repeat| observed.set(*repeat)
        });
        let mut runtime = UiRuntime::new(player);
        runtime
            .layout(constraints())
            .expect("audio layout succeeds");
        let geometry = audio_geometry(
            Rect::new(Point::ZERO, Size::new(360.0, 112.0).expect("valid size")),
            &PlayerTheme::default(),
            &AudioPlayer::new(),
        );
        let down = InputEvent::PointerDown(pointer(
            Point::new(
                geometry.repeat.origin.x + 2.0,
                geometry.repeat.origin.y + 2.0,
            ),
            Some(PointerButton::Primary),
        ));
        assert_eq!(runtime.dispatch_event(&down), EventStatus::Handled);
        assert_eq!(observed.get(), RepeatMode::All);
    }

    #[test]
    fn volume_and_mute_control_the_core() {
        let mut player = AudioPlayer::new().with_volume(0.4);
        assert!((player.volume() - 0.4).abs() < f32::EPSILON);
        assert!(!player.is_muted());
        player.set_muted(true);
        assert!(player.is_muted());
        player.set_volume(2.0);
        assert!((player.volume() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn plays_and_pauses_through_a_session() {
        let mut player = AudioPlayer::new().with_session(MockSession::new());
        assert!(player.has_decoder());
        player.set_playing(true);
        assert!(player.is_playing());
        assert!(player.advance_animation(Duration::from_millis(100), false));
        assert_eq!(player.position(), Duration::from_millis(100));
        player.set_playing(false);
        assert!(!player.is_playing());
        player.release_session();
        assert!(!player.has_decoder());
    }

    #[test]
    fn compact_layout_paints_without_a_cover() {
        let player = AudioPlayer::new()
            .with_title("Track")
            .with_layout(AudioPlayerLayout::Compact)
            .with_duration(Duration::from_secs(30));
        let mut runtime = UiRuntime::new(player);
        let mut list = DisplayList::new();
        runtime
            .layout(
                Constraints::tight(Size::new(320.0, 64.0).expect("valid size"))
                    .expect("valid constraints"),
            )
            .expect("audio layout succeeds");
        runtime
            .paint(&mut PaintContext::new(&mut list))
            .expect("audio paint succeeds");
        assert!(
            list.commands()
                .iter()
                .any(|command| matches!(command, DisplayCommand::DrawText { .. }))
        );
    }

    #[cfg(feature = "audio")]
    #[test]
    fn open_reports_a_missing_file() {
        let error = AudioPlayer::open("drawen-audio-player-missing-file.flac")
            .err()
            .expect("a missing file cannot be opened");
        assert!(error.message().contains("could not open media"), "{error}");
    }
}
