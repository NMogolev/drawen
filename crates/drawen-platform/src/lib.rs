//! Platform-independent window, event-loop, and framebuffer contracts for Drawen.
//!
//! A [`WindowApplication`] owns a `UiRuntime`, receives normalized
//! [`WindowEvent`]s, returns a [`DisplayList`] to present, and translates
//! widget intents into [`WindowAction`]s. Adapters such as
//! `drawen-platform-winit` implement the native event loop and input mapping.
//!
//! Coordinates remain logical: the adapter applies the platform scale factor
//! and reports logical sizes, so layout and hit testing are independent of the
//! display DPI.

use std::{sync::Arc, time::Duration};

use drawen_core::{InputEvent, Size};
use drawen_render::DisplayList;

/// A cloneable, thread-safe handle that wakes a native event loop.
///
/// Platform adapters create one per window and pass it to
/// [`WindowApplication::attach_waker`]. A worker thread keeps a clone and calls
/// [`Self::wake`] after handing a value to a UI dispatcher, so the UI thread
/// drains the value and repaints.
#[derive(Clone)]
pub struct WindowWaker {
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl WindowWaker {
    /// Creates a waker from a thread-safe wake-up function.
    ///
    /// The function should only nudge the event loop. It must not touch the
    /// widget tree, block, or run application logic.
    #[must_use]
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            wake: Arc::new(wake),
        }
    }

    /// Wakes the event loop.
    ///
    /// This is safe to call from any thread and is cheap enough for workers to
    /// call after each delivered value.
    pub fn wake(&self) {
        (self.wake)();
    }
}

impl core::fmt::Debug for WindowWaker {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("WindowWaker")
            .finish_non_exhaustive()
    }
}

/// Initial properties of a native application window.
// Four independent window states are clearer as named booleans than as an
// artificial state machine; every builder changes exactly one of them.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq)]
pub struct WindowOptions {
    /// Window title shown by the operating system.
    pub title: String,
    /// Initial content size in logical pixels.
    pub size: Size,
    /// Whether the operating system should draw its native window frame.
    ///
    /// Applications that provide a custom title bar set this to `false` and
    /// draw their own chrome. The winit adapter keeps window moving, resizing,
    /// minimizing, maximizing, and fullscreen available in both modes.
    pub decorations: bool,
    /// Whether the user may resize the window.
    pub resizable: bool,
    /// Whether the window starts maximized.
    pub maximized: bool,
    /// Whether the window starts in borderless fullscreen mode.
    pub fullscreen: bool,
    /// Optional minimum content size in logical pixels.
    pub min_size: Option<Size>,
}

impl WindowOptions {
    /// Creates window options with `title`, an initial logical `size`, and
    /// native decorations enabled.
    #[must_use]
    pub fn new(title: impl Into<String>, size: Size) -> Self {
        Self {
            title: title.into(),
            size,
            decorations: true,
            resizable: true,
            maximized: false,
            fullscreen: false,
            min_size: None,
        }
    }

    /// Returns these options with native decorations enabled or disabled.
    #[must_use]
    pub fn with_decorations(mut self, decorations: bool) -> Self {
        self.decorations = decorations;
        self
    }

    /// Returns these options with user resizing enabled or disabled.
    #[must_use]
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Returns these options with the initial maximized state set.
    #[must_use]
    pub fn with_maximized(mut self, maximized: bool) -> Self {
        self.maximized = maximized;
        self
    }

    /// Returns these options with the initial fullscreen state set.
    #[must_use]
    pub fn with_fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    /// Returns these options with a minimum logical content size.
    #[must_use]
    pub fn with_min_size(mut self, min_size: Size) -> Self {
        self.min_size = Some(min_size);
        self
    }
}

/// The default interval between ticks while a window is minimized or occluded.
///
/// The adapter does not present frames at this cadence. It only advances
/// application time so media clocks and background work keep running.
pub const BACKGROUND_FRAME_INTERVAL: Duration = Duration::from_millis(100);

/// The default frame interval cap while a window is visible but unfocused.
///
/// This keeps a sidelined window from competing at full cadence with the
/// foreground application without stopping playback or presentation completely.
pub const INACTIVE_FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// A normalized snapshot of the native window's presentation state.
///
/// The individual [`WindowEvent`] variants remain the primary notification
/// channel. This snapshot exists so an application can forward one value to
/// [`UiRuntime`](https://docs.rs/drawen-ui/latest/drawen_ui/struct.UiRuntime.html)
/// or decide whether a frame is worth drawing without reconstructing the state
/// from several events.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowPresentation {
    visible: bool,
    focused: bool,
    minimized: bool,
    occluded: bool,
}

impl WindowPresentation {
    /// A visible, focused, restored window.
    pub const VISIBLE: Self = Self {
        visible: true,
        focused: true,
        minimized: false,
        occluded: false,
    };

    /// A hidden, unfocused window.
    pub const HIDDEN: Self = Self {
        visible: false,
        focused: false,
        minimized: false,
        occluded: false,
    };

    /// Creates a snapshot from its independent native flags.
    #[must_use]
    pub const fn new(visible: bool, focused: bool, minimized: bool, occluded: bool) -> Self {
        Self {
            visible,
            focused,
            minimized,
            occluded,
        }
    }

    /// Returns whether the platform reports the window as visible.
    #[must_use]
    pub const fn visible(self) -> bool {
        self.visible
    }

    /// Returns whether the window currently owns keyboard focus.
    #[must_use]
    pub const fn focused(self) -> bool {
        self.focused
    }

    /// Returns whether the user minimized the window.
    #[must_use]
    pub const fn minimized(self) -> bool {
        self.minimized
    }

    /// Returns whether another window or the desktop fully covers the window.
    ///
    /// The flag is available on platforms that report occlusion to `winit`.
    /// Windows does not, so focus loss is the practical signal there.
    #[must_use]
    pub const fn occluded(self) -> bool {
        self.occluded
    }

    /// Returns whether the window can present pixels.
    ///
    /// A presented window is visible, restored, and not fully occluded. This is
    /// the condition for painting and GPU presentation, not for advancing time.
    #[must_use]
    pub const fn is_presented(self) -> bool {
        self.visible && !self.minimized && !self.occluded
    }

    /// Returns whether the window is presented and focused.
    ///
    /// Adapters use this to apply a softer frame cap to a visible window that
    /// lost focus without stopping it completely.
    #[must_use]
    pub const fn is_active(self) -> bool {
        self.is_presented() && self.focused
    }
}

/// A native-window event translated into Drawen types.
#[derive(Clone, Debug, PartialEq)]
pub enum WindowEvent {
    /// The content area changed to this logical size.
    Resized(Size),
    /// The window entered or left the maximized state.
    Maximized(bool),
    /// The window entered or left fullscreen mode.
    Fullscreen(bool),
    /// The window became visible or was hidden by the platform or application.
    ///
    /// This is a normalized presentation state, not a request to redraw. A
    /// media runtime should keep audio playing but may stop video presentation
    /// while the value is `false`.
    Visible(bool),
    /// The window was minimized or restored.
    ///
    /// Minimizing implies [`Self::Occluded`] on platforms that report both, but
    /// applications should treat this event as the explicit user intent to put
    /// the window in the background.
    Minimized(bool),
    /// The window is completely occluded by another window or the desktop, or
    /// it became visible again.
    Occluded(bool),
    /// An input event relative to the content area in logical pixels.
    Input(InputEvent),
    /// The operating system requested a new frame.
    RedrawRequested,
    /// The user requested that the window be closed.
    ///
    /// Closing the last window does not stop the event loop by itself;
    /// the application decides whether that means [`WindowAction::Exit`].
    /// [`drawen-director`](https://docs.rs/drawen-director) provides the default
    /// desktop policy: exit when the last window closes unless background
    /// playback was requested explicitly.
    CloseRequested,
}

/// The application's requested action after it receives a [`WindowEvent`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WindowAction {
    /// Keep processing native events without scheduling a frame.
    #[default]
    None,
    /// Ask the native window to issue a redraw event.
    RequestRedraw,
    /// Leave the native event loop.
    Exit,
    /// Minimize the native window.
    Minimize,
    /// Toggle between maximized and restored window geometry.
    ToggleMaximize,
    /// Toggle borderless fullscreen mode.
    ToggleFullscreen,
    /// Begin moving the window while the primary pointer button is held.
    StartDrag,
    /// Begin resizing the window from `direction` while the primary pointer
    /// button is held.
    StartResize(WindowResizeDirection),
}

/// An edge or corner from which a native window resize can start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowResizeDirection {
    /// Top edge.
    North,
    /// Bottom edge.
    South,
    /// Left edge.
    West,
    /// Right edge.
    East,
    /// Top-left corner.
    NorthWest,
    /// Top-right corner.
    NorthEast,
    /// Bottom-left corner.
    SouthWest,
    /// Bottom-right corner.
    SouthEast,
}

/// Application callbacks driven by a native window adapter.
pub trait WindowApplication {
    /// Returns the settings used to create the native window.
    fn window_options(&self) -> WindowOptions;

    /// Handles a translated native event.
    fn window_event(&mut self, event: WindowEvent) -> WindowAction;

    /// Handles a normalized change in the window's presentation state.
    ///
    /// The adapter calls this after it dispatches the individual `Visible`,
    /// `Minimized`, and `Occluded` events, and after focus changes. Returning
    /// [`WindowAction::RequestRedraw`] while the window is presented schedules
    /// a frame; while it is hidden the request is held until restore.
    ///
    /// Applications that use a `UiRuntime` normally forward
    /// [`WindowPresentation::is_presented`] to
    /// `UiRuntime::set_presented` here. The default ignores the update.
    fn presentation_changed(&mut self, _presentation: WindowPresentation) -> WindowAction {
        WindowAction::None
    }

    /// Returns the frame interval cap while the window is visible but unfocused.
    ///
    /// Adapters apply this as a lower bound on [`Self::next_frame_in`] so a
    /// sidelined window keeps repainting without competing at full cadence with
    /// the foreground application. Return `None` to keep the application
    /// cadence. The default is [`INACTIVE_FRAME_INTERVAL`].
    fn inactive_frame_interval(&self) -> Option<Duration> {
        Some(INACTIVE_FRAME_INTERVAL)
    }

    /// Advances application time while the window cannot be presented.
    ///
    /// Adapters call this at [`BACKGROUND_FRAME_INTERVAL`] while the window is
    /// minimized, hidden, or fully occluded. The return value asks for one
    /// repaint after the window becomes visible again.
    ///
    /// The default calls [`Self::redraw`] and discards the result, so existing
    /// applications keep advancing media clocks without changes. Override it
    /// to avoid building a display list; `UiRuntime::tick` is the usual
    /// replacement.
    fn tick(&mut self, _delta: Duration) -> bool {
        let _ = self.redraw();
        false
    }

    /// Records the display list for the next rendered frame.
    ///
    /// Platform adapters may execute this list asynchronously and continue to
    /// present the last completed frame while it is rendering.
    fn redraw(&mut self) -> DisplayList;

    /// Returns the current accessibility tree for this window, if any.
    ///
    /// Platform adapters publish the returned tree through the native
    /// accessibility bridge: UI Automation on Windows, AT-SPI on Linux, and the
    /// macOS Accessibility API. Returning `None` leaves accessibility disabled
    /// for applications that do not expose semantics.
    fn accessibility_tree(&self) -> Option<accesskit::TreeUpdate> {
        None
    }

    /// Handles an accessibility action requested by the native bridge.
    ///
    /// Return `true` when the action changed application state and a new frame
    /// should be produced. The default ignores every request.
    fn accessibility_action(&mut self, _request: &accesskit::ActionRequest) -> bool {
        false
    }

    /// Returns how long to wait before producing the next animation frame.
    ///
    /// Adapters schedule a redraw after this delay and keep requesting frames
    /// until it returns `None`. Applications with continuous animation, such as
    /// an FPS counter, should return a frame interval whenever they need to
    /// paint. The default disables timed redraws.
    fn next_frame_in(&self) -> Option<Duration> {
        None
    }

    /// Stores a thread-safe handle that wakes this application's event loop.
    ///
    /// Adapters call this once before the first event. Applications give a
    /// clone to worker threads, usually as the wake callback of a
    /// [`UiDispatcher`](https://docs.rs/drawen-tasks/latest/drawen_tasks/struct.UiDispatcher.html),
    /// so queued values are applied from [`Self::window_event`] on the UI
    /// thread. The default ignores the handle.
    fn attach_waker(&mut self, _waker: WindowWaker) {}
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use drawen_core::Size;

    use super::{WindowOptions, WindowPresentation, WindowWaker};

    #[test]
    fn waker_clones_share_the_wake_up() {
        let wake_count = Arc::new(AtomicUsize::new(0));
        let waker = WindowWaker::new({
            let wake_count = Arc::clone(&wake_count);
            move || {
                wake_count.fetch_add(1, Ordering::SeqCst);
            }
        });

        std::thread::spawn({
            let waker = waker.clone();
            move || waker.wake()
        })
        .join()
        .expect("the wake-up thread does not panic");

        assert_eq!(wake_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn new_options_enable_native_decorations() {
        let options =
            WindowOptions::new("Drawen", Size::new(640.0, 480.0).expect("valid test size"));

        assert!(options.decorations);
        assert!(options.resizable);
        assert!(!options.maximized);
        assert!(!options.fullscreen);
        assert_eq!(options.min_size, None);
    }

    #[test]
    fn presentation_distinguishes_presented_active_and_hidden_windows() {
        let visible = WindowPresentation::VISIBLE;
        assert!(visible.is_presented());
        assert!(visible.is_active());

        let unfocused = WindowPresentation::new(true, false, false, false);
        assert!(unfocused.is_presented());
        assert!(!unfocused.is_active());

        for hidden in [
            WindowPresentation::new(true, true, true, false),
            WindowPresentation::new(true, true, false, true),
            WindowPresentation::HIDDEN,
        ] {
            assert!(!hidden.is_presented());
            assert!(!hidden.is_active());
        }
    }

    #[test]
    fn builders_configure_custom_chrome() {
        let minimum = Size::new(320.0, 240.0).expect("valid test size");
        let options =
            WindowOptions::new("Drawen", Size::new(640.0, 480.0).expect("valid test size"))
                .with_decorations(false)
                .with_resizable(false)
                .with_maximized(true)
                .with_fullscreen(true)
                .with_min_size(minimum);

        assert!(!options.decorations);
        assert!(!options.resizable);
        assert!(options.maximized);
        assert!(options.fullscreen);
        assert_eq!(options.min_size, Some(minimum));
    }
}
