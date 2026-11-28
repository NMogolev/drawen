//! Interactive component laboratory for the whole Drawen library.
//!
//! Run with `cargo run -p drawen --example style_showcase`.
//!
//! The showcase follows the Qt/AUI idea of a directly configurable interface:
//! the "Appearance" page edits the same semantic theme that every standard
//! widget resolves during layout and paint, so palette, typography, density,
//! corner radius, and animation changes apply live. Fourteen pages cover the
//! full surface of the toolkit: controls, data entry, virtualized data views,
//! layout primitives, docking, media, software 3D, DSS styling, overlays,
//! background tasks and frame diagnostics, the agent bridge, and Servo
//! interop.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use drawen::{
    Alert, AlertTone, Align, Alignment, Autocomplete, Avatar, AvatarStatus, Badge, BadgeTone,
    Binding, Box as DrawenBox, Button, Card, CardStyle, Checkbox, ChildLayout, Clipboard, Color,
    CommandPalette, Constraints, Date, DatePicker, Divider, DockArea, DockAxis, Dropdown, Expander,
    Image, ImageViewer, Insets, LayoutContext, LayoutNode, LayoutResult, ListView, Menu, MenuBar,
    MenuEntry, Modal, NumberInput, Padding, PaletteCommand, PanelId, Point, Popover,
    PopoverTrigger, ProgressBar, Property, RadioButton, RadioGroup, RangeSlider, Rect, Row,
    ScrollView, SegmentedControl, SelectionMode, Size, SizedBox, Skeleton, Slider, Spinner,
    SplitAxis, SplitButton, Splitter, Stack, Styled, Subscription, SurfaceStyle, Switch, TabBar,
    Text, TextEdit, TextField, Theme, TitleBar, TitleBarStyle, Toast, ToastHandle, ToastHost,
    ToastTone, Toolbar, Tooltip, TreeNode, TreeView, UiEnvironment, UiRuntime, VirtualDataSource,
    VirtualList, VirtualTable, VirtualTableSource, Widget, WindowChromeState, WindowControl,
    WindowFrame, WorkspaceLayout,
    platform::{
        WindowAction, WindowApplication, WindowEvent, WindowOptions, WindowPresentation,
        WindowWaker, action_for,
    },
    render::{DisplayList, FrameDamage, PaintContext, RasterImage, TextLayout, TextStyle},
    tasks::{TaskHandle, TaskPool, UiDispatcher, UiSender},
};

#[cfg(feature = "cubex")]
use drawen::{
    CubeXView,
    cubex::{
        Camera, Color as SceneColor, DirectionalLight, Material, Quat, Scene, Transform, Vec3,
        primitives,
    },
};

#[cfg(feature = "media")]
use drawen::{
    AnimatedImage, AnimationFrame, AudioPlayer, AudioPlayerLayout, PlayerBehavior, PlayerTheme,
    Video, VideoControls,
};

const WINDOW_WIDTH: f32 = 1_440.0;
const WINDOW_HEIGHT: f32 = 860.0;
const WINDOW_WIDTH_PX: u32 = 1_440;
const WINDOW_HEIGHT_PX: u32 = 860;
const MENU_MAX_HEIGHT: f32 = 58.0;
const TOOLBAR_MAX_HEIGHT: f32 = 78.0;
const STATUS_HEIGHT: f32 = 28.0;
const TITLE_BAR_HEIGHT: f32 = 42.0;
const PAGE_PADDING: f32 = 24.0;
const BASE_FONT_SIZE: f32 = 14.0;
const BASE_SPACING: f32 = 8.0;
const PAGE_COUNT: usize = 14;

/// Scoped DSS rules used by the dedicated stylesheet page. They deliberately
/// target only `.dss-sample`, leaving the semantic-theme laboratory untouched.
const SHOWCASE_DSS: &str = r"
    :root {
        --sample-space: 10px;
        --sample-radius: 12px;
        --sample-accent: hsl(168 76% 52%);
    }

    box.dss-sample {
        background: rgb(12 24 44 / 0.92);
        border: calc(var(--sample-space) / 5) solid var(--sample-accent);
        border-radius: var(--sample-radius);
        padding: calc(var(--sample-space) * 1.5);
    }

    .dss-sample > box { padding: var(--sample-space); }
    .dss-sample button { background: steelblue; padding: 7px calc(var(--sample-space) * 1.5); }
    .dss-sample button:not(.emphasis) { border: 1px solid rgb(255 255 255 / 0.28); }
    .dss-sample button:nth-child(2) { background: hsl(38 92% 52%); }
    .dss-sample button:is(.emphasis, :last-child) { border-radius: calc(var(--sample-radius) * 1.5); }
    .dss-sample button:last-child { background: rebeccapurple !important; }
    .dss-sample checkbox:checked { accent-color: var(--sample-accent); }
    .dss-sample text { color: white; font-size: calc(12px + 2px); }
";

#[cfg(test)]
mod idle_probe {
    use super::*;

    #[test]
    fn idle_showcase_schedules_no_frames() {
        let mut app = StyleShowcase::new(0);
        app.runtime.settle_animations();
        assert_eq!(app.runtime.next_frame_in(), None);
    }

    #[test]
    fn probe_every_page() {
        for page in 0..PAGE_COUNT {
            let mut app = StyleShowcase::new(page);
            // Simulate the native loop: paint advances animations by wall time.
            for _ in 0..8 {
                let mut list = DisplayList::new();
                app.runtime
                    .paint(&mut PaintContext::new(&mut list))
                    .expect("paint");
                let _ = app.runtime.take_redraw_request();
                if app.runtime.next_frame_in().is_none() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            println!(
                "page {page}: next_frame_in={:?} damage={:?}",
                app.runtime.next_frame_in(),
                app.runtime.frame_damage()
            );
        }
    }
}

fn main() -> Result<(), drawen_platform_winit::RunError> {
    if std::env::args().any(|argument| argument == "--snapshot") {
        write_snapshots();
        return Ok(());
    }
    drawen_platform_winit::run(StyleShowcase::new(0))
}

/// Renders every page to `target/showcase/*.png` without opening a window.
fn write_snapshots() {
    std::fs::create_dir_all("target/showcase").expect("snapshot directory is writable");
    for (index, name) in [
        "overview",
        "components",
        "entry",
        "data",
        "layout",
        "workspace",
        "media",
        "cubex",
        "appearance",
        "dss",
        "overlays",
        "runtime",
        "agents",
        "servo",
    ]
    .iter()
    .enumerate()
    {
        let mut app = StyleShowcase::new(index);
        render_snapshot(&mut app, name);
    }

    let mut light = StyleShowcase::new(8);
    {
        let mut appearance = light.appearance.borrow_mut();
        appearance.preset = Preset::Daylight;
        appearance.accent_hue = 215.0;
        appearance.font_scale = 1.18;
        appearance.spacing_scale = 1.22;
        appearance.radius = 16.0;
        appearance.animations = false;
    }
    light
        .runtime
        .environment_mut()
        .set_animations_enabled(false);
    light.runtime.invalidate_layout();
    render_snapshot(&mut light, "appearance-light");

    let mut components = StyleShowcase::new(1);
    components.layout(valid_size(WINDOW_WIDTH, 3_600.0));
    render_snapshot_sized(&mut components, "components-tall", WINDOW_WIDTH_PX, 3_600);
}

fn render_snapshot(app: &mut StyleShowcase, name: &str) {
    render_snapshot_sized(app, name, WINDOW_WIDTH_PX, WINDOW_HEIGHT_PX);
}

fn render_snapshot_sized(app: &mut StyleShowcase, name: &str, width: u32, height: u32) {
    use drawen::software::{PixelBuffer, SoftwareRenderer};

    let mut display_list = DisplayList::new();
    app.runtime
        .paint(&mut PaintContext::new(&mut display_list))
        .expect("showcase paints");
    let mut pixels = PixelBuffer::new(width, height).expect("snapshot buffer is valid");
    SoftwareRenderer
        .render(&display_list, &mut pixels)
        .expect("showcase renders");
    let path = format!("target/showcase/{name}.png");
    pixels.write_png(&path).expect("snapshot is saved");
    println!("{path}");
}

// ---------------------------------------------------------------------------
// Appearance model
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Preset {
    Midnight,
    Daylight,
    Graphite,
}

impl Preset {
    const ALL: [Self; 3] = [Self::Midnight, Self::Daylight, Self::Graphite];
    const LABELS: [&'static str; 3] = ["Midnight", "Daylight", "Graphite"];

    const fn label(self) -> &'static str {
        match self {
            Self::Midnight => "Midnight",
            Self::Daylight => "Daylight",
            Self::Graphite => "Graphite",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Midnight => 0,
            Self::Daylight => 1,
            Self::Graphite => 2,
        }
    }

    const fn is_dark(self) -> bool {
        !matches!(self, Self::Daylight)
    }
}

#[derive(Clone, Copy, Debug)]
struct Appearance {
    preset: Preset,
    accent_hue: f32,
    font_scale: f32,
    spacing_scale: f32,
    radius: f32,
    animations: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            preset: Preset::Midnight,
            accent_hue: 168.0,
            font_scale: 1.0,
            spacing_scale: 1.0,
            radius: 10.0,
            animations: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Palette {
    background: Color,
    surface: Color,
    raised: Color,
    control: Color,
    control_hover: Color,
    control_pressed: Color,
    foreground: Color,
    muted: Color,
    border: Color,
    accent: Color,
    accent_foreground: Color,
    title_bar: Color,
    shadow: Color,
}

fn palette_for(appearance: &Appearance) -> Palette {
    let (
        background,
        surface,
        raised,
        control,
        control_hover,
        control_pressed,
        foreground,
        muted,
        border,
        shadow,
    ) = match appearance.preset {
        Preset::Midnight => (
            Color::rgba8(12, 15, 23, 255),
            Color::rgba8(19, 24, 35, 255),
            Color::rgba8(25, 31, 45, 255),
            Color::rgba8(32, 40, 56, 255),
            Color::rgba8(42, 52, 71, 255),
            Color::rgba8(54, 65, 88, 255),
            Color::rgba8(233, 238, 247, 255),
            Color::rgba8(151, 163, 186, 255),
            Color::rgba8(48, 58, 79, 255),
            Color::rgba8(0, 0, 0, 150),
        ),
        Preset::Daylight => (
            Color::rgba8(242, 245, 250, 255),
            Color::rgba8(255, 255, 255, 255),
            Color::rgba8(255, 255, 255, 255),
            Color::rgba8(237, 241, 247, 255),
            Color::rgba8(226, 232, 241, 255),
            Color::rgba8(214, 222, 234, 255),
            Color::rgba8(28, 32, 41, 255),
            Color::rgba8(101, 110, 126, 255),
            Color::rgba8(211, 219, 231, 255),
            Color::rgba8(23, 32, 53, 42),
        ),
        Preset::Graphite => (
            Color::rgba8(28, 29, 32, 255),
            Color::rgba8(36, 37, 41, 255),
            Color::rgba8(44, 45, 50, 255),
            Color::rgba8(50, 52, 58, 255),
            Color::rgba8(60, 62, 69, 255),
            Color::rgba8(72, 74, 83, 255),
            Color::rgba8(241, 242, 245, 255),
            Color::rgba8(166, 169, 178, 255),
            Color::rgba8(61, 63, 70, 255),
            Color::rgba8(0, 0, 0, 135),
        ),
    };

    let accent = accent_color(appearance.preset.is_dark(), appearance.accent_hue);
    Palette {
        background,
        surface,
        raised,
        control,
        control_hover,
        control_pressed,
        foreground,
        muted,
        border,
        accent,
        accent_foreground: contrast_on(accent),
        title_bar: background.mix(surface, 0.5),
        shadow,
    }
}

fn accent_color(dark: bool, hue: f32) -> Color {
    if dark {
        hsl(hue, 0.64, 0.62)
    } else {
        hsl(hue, 0.72, 0.43)
    }
}

fn accent_name(hue: f32) -> &'static str {
    let hue = hue.rem_euclid(360.0);
    if !(20.0..340.0).contains(&hue) {
        "Crimson"
    } else if hue < 45.0 {
        "Amber"
    } else if hue < 70.0 {
        "Lemon"
    } else if hue < 150.0 {
        "Emerald"
    } else if hue < 195.0 {
        "Mint"
    } else if hue < 250.0 {
        "Azure"
    } else if hue < 290.0 {
        "Lilac"
    } else if hue < 320.0 {
        "Purple"
    } else {
        "Rose"
    }
}

fn hsl(hue: f32, saturation: f32, lightness: f32) -> Color {
    let hue = hue.rem_euclid(360.0);
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let scaled = hue / 60.0;
    let x = chroma * (1.0 - (scaled % 2.0 - 1.0).abs());
    #[allow(clippy::cast_possible_truncation)]
    let sector = scaled.floor() as i32;
    let (red, green, blue) = match sector {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    Color::rgb(red + m, green + m, blue + m)
}

fn contrast_on(color: Color) -> Color {
    let luminance = 0.2126 * color.red + 0.7152 * color.green + 0.0722 * color.blue;
    if luminance > 0.58 {
        Color::rgba8(13, 16, 22, 255)
    } else {
        Color::WHITE
    }
}

/// Semantic theme backed by the shared, live [`Appearance`] state.
struct ShowcaseTheme {
    appearance: Rc<RefCell<Appearance>>,
}

impl ShowcaseTheme {
    fn palette(&self) -> Palette {
        palette_for(&self.appearance.borrow())
    }

    fn appearance(&self) -> Appearance {
        *self.appearance.borrow()
    }
}

impl Theme for ShowcaseTheme {
    fn background(&self) -> Color {
        self.palette().background
    }

    fn surface(&self) -> Color {
        self.palette().surface
    }

    fn raised_surface(&self) -> Color {
        self.palette().raised
    }

    fn foreground(&self) -> Color {
        self.palette().foreground
    }

    fn accent(&self) -> Color {
        self.palette().accent
    }

    fn accent_foreground(&self) -> Color {
        self.palette().accent_foreground
    }

    fn control_background(&self) -> Color {
        self.palette().control
    }

    fn control_hover_background(&self) -> Color {
        self.palette().control_hover
    }

    fn control_pressed_background(&self) -> Color {
        self.palette().control_pressed
    }

    fn border(&self) -> Color {
        self.palette().border
    }

    fn focus_ring(&self) -> Color {
        self.palette().accent
    }

    fn shadow(&self) -> Color {
        self.palette().shadow
    }

    fn muted_foreground(&self) -> Color {
        self.palette().muted
    }

    fn title_bar_background(&self) -> Color {
        self.palette().title_bar
    }

    fn title_bar_hover_background(&self) -> Color {
        self.palette().control_hover
    }

    fn control_padding(&self) -> Insets {
        let scale = self.appearance().spacing_scale;
        let vertical = 6.0 + scale * 1.5;
        let horizontal = 10.0 + scale * 3.0;
        Insets::new(vertical, horizontal, vertical, horizontal)
    }

    fn spacing(&self) -> f32 {
        BASE_SPACING * self.appearance().spacing_scale
    }

    fn font_size(&self) -> f32 {
        BASE_FONT_SIZE * self.appearance().font_scale
    }

    fn corner_radius(&self) -> f32 {
        self.appearance().radius
    }

    fn animation_duration(&self) -> Duration {
        if self.appearance().animations {
            Duration::from_millis(150)
        } else {
            Duration::ZERO
        }
    }
}

// ---------------------------------------------------------------------------
// Background work model
// ---------------------------------------------------------------------------

/// Values delivered from the worker pool to the UI thread.
enum TaskMessage {
    Progress(f32),
    Finished(u64),
    Cancelled,
}

/// Plain data owned by the UI thread; workers only capture the sender.
struct TaskDemo {
    pool: TaskPool,
    sender: RefCell<Option<UiSender<TaskMessage>>>,
    handle: RefCell<Option<TaskHandle<()>>>,
    progress: Property<f32>,
    status: Rc<RefCell<String>>,
    busy: Cell<bool>,
}

impl TaskDemo {
    fn new() -> Self {
        Self {
            pool: TaskPool::new(2).expect("worker threads start"),
            sender: RefCell::new(None),
            handle: RefCell::new(None),
            progress: Property::new(0.0),
            status: Rc::new(RefCell::new("Idle — run the checksum job.".to_owned())),
            busy: Cell::new(false),
        }
    }

    fn start(&self) {
        if self.busy.replace(true) {
            return;
        }
        self.progress.set(0.0);
        "Running on a worker thread…".clone_into(&mut self.status.borrow_mut());
        let Some(sender) = self.sender.borrow().clone() else {
            self.busy.set(false);
            "No dispatcher attached yet.".clone_into(&mut self.status.borrow_mut());
            return;
        };

        let progress_sender = sender.clone();
        let finish_sender = sender;
        let handle = self.pool.spawn_with_progress(
            move |context| {
                const STEPS: u64 = 160;
                let mut checksum = 0u64;
                for step in 0..STEPS {
                    if context.is_cancelled() {
                        let _ = finish_sender.send(TaskMessage::Cancelled);
                        return;
                    }
                    // Stand-in for decoding, parsing, or network work.
                    std::thread::sleep(Duration::from_millis(10));
                    checksum = checksum.wrapping_add(step * step);
                    context.report_units(step + 1, STEPS);
                }
                let _ = finish_sender.send(TaskMessage::Finished(checksum));
            },
            move |progress| {
                let _ =
                    progress_sender.send(TaskMessage::Progress(progress.fraction().unwrap_or(0.0)));
            },
        );
        *self.handle.borrow_mut() = Some(handle);
    }

    fn cancel(&self) {
        if let Some(handle) = self.handle.borrow().as_ref() {
            handle.cancel();
            "Cancelling…".clone_into(&mut self.status.borrow_mut());
        }
    }
}

// ---------------------------------------------------------------------------
// Application shell
// ---------------------------------------------------------------------------

/// Subscriptions and bindings owned by one mounted page. Dropping the value
/// disconnects the page's callbacks when its content is torn down.
struct MountedPage {
    _subscriptions: Vec<Subscription>,
    _bindings: Vec<Binding>,
}

struct StyleShowcase {
    runtime: UiRuntime,
    size: Size,
    chrome: WindowChromeState,
    appearance: Rc<RefCell<Appearance>>,
    dirty: Rc<Cell<bool>>,
    #[cfg(feature = "agent")]
    agent_observation_requested: Rc<Cell<bool>>,
    #[cfg(feature = "agent")]
    agent_report: Rc<RefCell<String>>,
    toast_queue: Rc<RefCell<Vec<ToastTone>>>,
    toast_handle: Option<ToastHandle>,
    task_demo: Rc<TaskDemo>,
    task_dispatcher: Option<UiDispatcher<TaskMessage>>,
    task_status_id: Option<drawen::WidgetId>,
    diagnostics: Rc<RefCell<String>>,
    diagnostics_id: Option<drawen::WidgetId>,
    diagnostics_cache: String,
    // One lightweight empty host per tab; heavy content mounts on first
    // activation, so hidden pages cost no widgets, shaping, or image memory.
    page_hosts: Vec<drawen::WidgetId>,
    pages: Vec<Option<MountedPage>>,
    page_request: Rc<Cell<Option<usize>>>,
    #[cfg(feature = "media")]
    media_import_requested: Rc<Cell<Option<MediaImport>>>,
    #[cfg(feature = "audio")]
    media_preview_frame: Option<drawen::WidgetId>,
    #[cfg(feature = "audio")]
    media_preview: Option<drawen::WidgetId>,
    #[cfg(feature = "audio")]
    audio_preview_parent: Option<drawen::WidgetId>,
    #[cfg(feature = "audio")]
    audio_preview: Option<drawen::WidgetId>,
    #[cfg(feature = "cubex")]
    cubex_mount: Option<drawen::WidgetId>,
    #[cfg(feature = "cubex")]
    cubex_view: Option<drawen::WidgetId>,
    #[cfg(feature = "cubex")]
    cubex_start_requested: Rc<Cell<bool>>,
    _bindings: Vec<Binding>,
    _subscriptions: Vec<Subscription>,
}

#[cfg(feature = "media")]
#[derive(Clone, Copy)]
enum MediaImport {
    Video,
    Audio,
}

impl StyleShowcase {
    #[allow(clippy::too_many_lines)]
    fn new(selected_tab: usize) -> Self {
        let size = valid_size(WINDOW_WIDTH, WINDOW_HEIGHT);
        let appearance = Rc::new(RefCell::new(Appearance::default()));
        let dirty = Rc::new(Cell::new(false));
        #[cfg(feature = "agent")]
        let agent_observation_requested = Rc::new(Cell::new(false));
        #[cfg(feature = "agent")]
        let agent_report = Rc::new(RefCell::new(
            "Press \"Capture semantic tree\" to inspect this retained UI through drawen-agent."
                .to_owned(),
        ));
        let toast_queue = Rc::new(RefCell::new(Vec::new()));
        let task_demo = Rc::new(TaskDemo::new());
        let diagnostics = Rc::new(RefCell::new(
            "Interact with the lab — counters sample on every input event.".to_owned(),
        ));
        #[cfg(feature = "media")]
        let media_import_requested = Rc::new(Cell::new(None));
        #[cfg(feature = "cubex")]
        let cubex_start_requested = Rc::new(Cell::new(false));
        let chrome = WindowChromeState::new();
        let mut environment = UiEnvironment::from_dss(
            ShowcaseTheme {
                appearance: Rc::clone(&appearance),
            },
            SHOWCASE_DSS,
        )
        .expect("showcase DSS is valid");
        environment.use_system_fallback_fonts();
        environment.set_animations_enabled(appearance.borrow().animations);
        let frame = WindowFrame::new()
            .with_title_bar_height(TITLE_BAR_HEIGHT)
            .with_resize_border(7.0);
        let mut runtime = UiRuntime::with_environment(frame, environment);
        let root = runtime.root();
        let title_bar = TitleBar::new("Drawen Style Lab")
            .with_icon("◆")
            .with_subtitle("  the complete component and platform laboratory")
            .with_controls([
                WindowControl::Minimize,
                WindowControl::Maximize,
                WindowControl::ToggleFullscreen,
                WindowControl::Close,
            ])
            .with_state(chrome.clone())
            .with_style(TitleBarStyle {
                height: TITLE_BAR_HEIGHT,
                ..TitleBarStyle::default()
            });
        runtime
            .append_child(root, title_bar)
            .expect("window frame exists");
        let root = runtime
            .append_child(root, ShowcaseShell)
            .expect("window frame exists");
        let mut subscriptions = Vec::new();
        let bindings = Vec::new();

        let menu = MenuBar::new(["File", "Edit", "View", "Help"]);
        subscriptions.push(menu.activated().subscribe(|index| {
            let names = ["File", "Edit", "View", "Help"];
            println!("MenuBar: opened section \"{}\".", names[*index]);
        }));
        runtime
            .append_child(root, menu)
            .expect("showcase root exists");

        let toolbar = runtime
            .append_child(
                root,
                Toolbar::new().with_padding(Insets::new(6.0, 10.0, 6.0, 10.0)),
            )
            .expect("showcase root exists");
        for (label, message) in [
            ("+ New", "Created a new document."),
            ("Save", "State saved."),
            ("Undo", "Reverted the last action."),
            ("Settings", "Open the \"Appearance\" tab."),
        ] {
            append_tool_button(&mut runtime, &mut subscriptions, toolbar, label, message);
        }

        let mut tabs = TabBar::new([
            "Overview",
            "Components",
            "Entry",
            "Data",
            "Layout",
            "Workspace",
            "Media",
            "3D",
            "Appearance",
            "DSS",
            "Overlays",
            "Runtime",
            "Agents",
            "Servo",
        ]);
        tabs.set_selected(Some(selected_tab.min(PAGE_COUNT - 1)));
        let page_request = Rc::new(Cell::new(None));
        subscriptions.push(tabs.changed().subscribe({
            let page_request = Rc::clone(&page_request);
            move |index| {
                println!("TabBar: opened page {}.", index + 1);
                page_request.set(Some(*index));
            }
        }));
        let tabs = runtime
            .append_child(root, tabs)
            .expect("showcase root exists");

        // Pages mount lazily: every tab keeps an empty host so tab indices stay
        // stable, and the page's content is built only when it is first opened.
        // Snapshot mode therefore builds just the requested page.
        let mut page_hosts = Vec::with_capacity(PAGE_COUNT);
        for _ in 0..PAGE_COUNT {
            page_hosts.push(runtime.append_child(tabs, Page).expect("tab bar exists"));
        }

        runtime
            .append_child(root, StatusBar::new(Rc::clone(&appearance)))
            .expect("showcase root exists");

        let mut app = Self {
            runtime,
            size,
            chrome,
            appearance,
            dirty,
            #[cfg(feature = "agent")]
            agent_observation_requested,
            #[cfg(feature = "agent")]
            agent_report,
            toast_queue,
            toast_handle: None,
            task_demo,
            task_dispatcher: None,
            task_status_id: None,
            diagnostics,
            diagnostics_id: None,
            diagnostics_cache: String::new(),
            page_hosts,
            pages: (0..PAGE_COUNT).map(|_| None).collect(),
            page_request,
            #[cfg(feature = "media")]
            media_import_requested,
            #[cfg(feature = "audio")]
            media_preview_frame: None,
            #[cfg(feature = "audio")]
            media_preview: None,
            #[cfg(feature = "audio")]
            audio_preview_parent: None,
            #[cfg(feature = "audio")]
            audio_preview: None,
            #[cfg(feature = "cubex")]
            cubex_mount: None,
            #[cfg(feature = "cubex")]
            cubex_view: None,
            #[cfg(feature = "cubex")]
            cubex_start_requested,
            _bindings: bindings,
            _subscriptions: subscriptions,
        };
        app.mount_page(selected_tab.min(PAGE_COUNT - 1));
        app.layout(size);
        app
    }

    fn layout(&mut self, size: Size) {
        self.size = size;
        if let Ok(constraints) = Constraints::tight(size) {
            let _ = self.runtime.layout(constraints);
        }
    }

    /// Builds a page's content into its empty host on first activation.
    ///
    /// Hidden tabs keep only the lightweight host, so unopened pages cost no
    /// widgets, text shaping, or procedural images. Per-page subscriptions and
    /// bindings stay alive exactly as long as the page content does.
    #[allow(clippy::too_many_lines)]
    fn mount_page(&mut self, index: usize) {
        if self.pages[index].is_some() {
            return;
        }
        let host = self.page_hosts[index];
        let mut subscriptions = Vec::new();
        let mut bindings = Vec::new();
        match index {
            0 => append_overview_page(&mut self.runtime, host),
            1 => append_components_page(&mut self.runtime, &mut subscriptions, host),
            2 => append_entry_page(&mut self.runtime, &mut subscriptions, &mut bindings, host),
            3 => append_data_page(&mut self.runtime, &mut subscriptions, host),
            4 => append_layout_page(&mut self.runtime, host),
            5 => append_workspace_page(&mut self.runtime, host),
            6 => {
                let previews = append_media_page(
                    &mut self.runtime,
                    &mut subscriptions,
                    #[cfg(feature = "media")]
                    &self.media_import_requested,
                    host,
                );
                #[cfg(feature = "audio")]
                {
                    let (frame, preview, audio_parent, audio) = previews;
                    self.media_preview_frame = Some(frame);
                    self.media_preview = Some(preview);
                    self.audio_preview_parent = Some(audio_parent);
                    self.audio_preview = Some(audio);
                }
                #[cfg(not(feature = "audio"))]
                let _ = previews;
            }
            7 => {
                #[cfg(feature = "cubex")]
                {
                    self.cubex_mount = Some(append_cubex_page(
                        &mut self.runtime,
                        &mut subscriptions,
                        &self.cubex_start_requested,
                        host,
                    ));
                }
                #[cfg(not(feature = "cubex"))]
                let _ = append_cubex_page(&mut self.runtime, &mut subscriptions, host);
            }
            8 => append_appearance_page(
                &mut self.runtime,
                &mut subscriptions,
                &self.appearance,
                &self.dirty,
                host,
            ),
            9 => append_dss_page(&mut self.runtime, host),
            10 => {
                self.toast_handle = Some(append_overlay_page(
                    &mut self.runtime,
                    &mut subscriptions,
                    &self.dirty,
                    &self.toast_queue,
                    host,
                ));
            }
            11 => {
                let (task_status, diagnostics) = append_runtime_page(
                    &mut self.runtime,
                    &mut subscriptions,
                    &self.task_demo,
                    &self.diagnostics,
                    &mut bindings,
                    host,
                );
                self.task_status_id = Some(task_status);
                self.diagnostics_id = Some(diagnostics);
            }
            12 => append_agent_page(
                &mut self.runtime,
                &mut subscriptions,
                #[cfg(feature = "agent")]
                &self.agent_observation_requested,
                #[cfg(feature = "agent")]
                &self.agent_report,
                host,
            ),
            13 => append_servo_page(&mut self.runtime, host),
            _ => unreachable!("page index is bounded by PAGE_COUNT"),
        }
        self.pages[index] = Some(MountedPage {
            _subscriptions: subscriptions,
            _bindings: bindings,
        });
        self.runtime.invalidate_layout();
    }

    #[cfg(feature = "cubex")]
    fn start_cubex_demo(&mut self) -> bool {
        if !self.cubex_start_requested.replace(false) || self.cubex_view.is_some() {
            return false;
        }
        let Some(mount) = self.cubex_mount else {
            return false;
        };
        let frame = self
            .runtime
            .append_child(mount, FixedHeight::new(470.0))
            .expect("CubeX mount remains in the showcase tree");
        let view = self
            .runtime
            .append_child(frame, cubex_demo_view())
            .expect("CubeX frame remains in the showcase tree");
        self.cubex_view = Some(view);
        self.runtime.invalidate_layout();
        true
    }

    /// Delivers worker results on the UI thread; returns true when anything
    /// arrived. The progress property binding damages the bar by itself.
    fn drain_tasks(&mut self) -> bool {
        let Some(dispatcher) = &self.task_dispatcher else {
            return false;
        };
        let messages = dispatcher.drain();
        if messages.is_empty() {
            return false;
        }
        for message in messages {
            match message {
                TaskMessage::Progress(fraction) => {
                    self.task_demo.progress.set(fraction);
                }
                TaskMessage::Finished(checksum) => {
                    self.task_demo.busy.set(false);
                    *self.task_demo.status.borrow_mut() = format!("Done · checksum {checksum}");
                }
                TaskMessage::Cancelled => {
                    self.task_demo.busy.set(false);
                    "Cancelled.".clone_into(&mut self.task_demo.status.borrow_mut());
                }
            }
        }
        true
    }

    /// Samples the runtime's frame diagnostics into the live readout. Only a
    /// changed report invalidates, and only the readout's own region.
    fn capture_diagnostics(&mut self) {
        let stats = self.runtime.stats();
        let damage = match self.runtime.frame_damage() {
            FrameDamage::Full => "full frame".to_owned(),
            FrameDamage::Region(rect) => format!(
                "region {:.0}×{:.0} at ({:.0}, {:.0})",
                rect.size.width(),
                rect.size.height(),
                rect.origin.x,
                rect.origin.y
            ),
            FrameDamage::Unchanged => "unchanged (retained frame)".to_owned(),
        };
        let text = format!(
            "layout passes: {} · paint passes: {}\nlast layout: {} µs{}\nlast paint: {} µs · {} commands{}\nframe damage: {}",
            stats.layout_passes(),
            stats.paint_passes(),
            stats.last_layout_time().as_micros(),
            if stats.last_layout_reused() {
                " (reused)"
            } else {
                ""
            },
            stats.last_paint_time().as_micros(),
            stats.last_paint_command_count(),
            if stats.last_paint_reused() {
                " (reused)"
            } else {
                ""
            },
            damage,
        );
        if text != self.diagnostics_cache {
            self.diagnostics_cache.clone_from(&text);
            *self.diagnostics.borrow_mut() = text;
            if let Some(id) = self.diagnostics_id {
                self.runtime.invalidate_paint_for(id);
            }
        }
    }

    #[cfg(feature = "audio")]
    fn import_media(&mut self, import: MediaImport) {
        let Some(path) = rfd::FileDialog::new()
            .set_title(match import {
                MediaImport::Video => "Import video with audio into Drawen Style Lab",
                MediaImport::Audio => "Import audio into Drawen Style Lab",
            })
            .add_filter(
                match import {
                    MediaImport::Video => "Video",
                    MediaImport::Audio => "Audio",
                },
                match import {
                    MediaImport::Video => &["mp4", "mkv", "webm", "mov", "avi"][..],
                    MediaImport::Audio => &["mp3", "flac", "wav", "ogg", "m4a", "aac"][..],
                },
            )
            .pick_file()
        else {
            return;
        };

        match import {
            MediaImport::Video => match Video::open_paleo_audio(&path) {
                Ok(video) => {
                    let frame = self
                        .media_preview_frame
                        .expect("import is requested from the mounted media page");
                    let preview = self
                        .media_preview
                        .expect("import is requested from the mounted media page");
                    self.runtime
                        .remove_subtree(preview)
                        .expect("media preview remains in the showcase tree");
                    self.media_preview = Some(
                        self.runtime
                            .append_child(frame, video)
                            .expect("media preview frame remains in the showcase tree"),
                    );
                    self.runtime.invalidate_layout();
                }
                Err(error) => eprintln!("Could not open {}: {error}", path.display()),
            },
            MediaImport::Audio => match AudioPlayer::open(&path) {
                Ok(player) => {
                    let parent = self
                        .audio_preview_parent
                        .expect("import is requested from the mounted media page");
                    let preview = self
                        .audio_preview
                        .expect("import is requested from the mounted media page");
                    self.runtime
                        .remove_subtree(preview)
                        .expect("audio preview remains in the showcase tree");
                    self.audio_preview = Some(
                        self.runtime
                            .append_child(parent, player)
                            .expect("media card remains in the showcase tree"),
                    );
                    self.runtime.invalidate_layout();
                }
                Err(error) => eprintln!("Could not open {}: {error}", path.display()),
            },
        }
    }

    #[cfg(all(feature = "media", not(feature = "audio")))]
    #[allow(clippy::unused_self)] // mirrors the audio-enabled signature
    fn import_media(&mut self, _: MediaImport) {
        eprintln!(
            "Audio playback is disabled. Run `cargo run -p drawen --example style_showcase --features audio`."
        );
    }
}

impl WindowApplication for StyleShowcase {
    fn window_options(&self) -> WindowOptions {
        WindowOptions::new("Drawen — Style Lab / Complete Gallery", self.size)
            .with_decorations(false)
            .with_resizable(true)
            .with_min_size(valid_size(720.0, 520.0))
    }

    fn attach_clipboard(&mut self, clipboard: Box<dyn Clipboard>) {
        self.runtime.set_clipboard(clipboard);
    }

    fn attach_waker(&mut self, waker: WindowWaker) {
        let (dispatcher, sender) = UiDispatcher::new(move || waker.wake());
        *self.task_demo.sender.borrow_mut() = Some(sender);
        self.task_dispatcher = Some(dispatcher);
    }

    fn window_event(&mut self, event: WindowEvent) -> WindowAction {
        match event {
            WindowEvent::CloseRequested => WindowAction::Exit,
            WindowEvent::Resized(size) => {
                self.layout(size);
                WindowAction::RequestRedraw
            }
            WindowEvent::Maximized(maximized) => {
                self.chrome.set_maximized(maximized);
                WindowAction::RequestRedraw
            }
            WindowEvent::Fullscreen(fullscreen) => {
                self.chrome.set_fullscreen(fullscreen);
                WindowAction::RequestRedraw
            }
            WindowEvent::Input(event) => {
                let _ = self.runtime.dispatch_event(&event);
                if let Some(index) = self.page_request.replace(None) {
                    self.mount_page(index);
                }
                #[cfg(feature = "cubex")]
                let cubex_started = self.start_cubex_demo();
                if self.drain_tasks()
                    && let Some(id) = self.task_status_id
                {
                    self.runtime.invalidate_paint_for(id);
                }
                let toasts: Vec<ToastTone> = self.toast_queue.borrow_mut().drain(..).collect();
                if !toasts.is_empty() {
                    for tone in toasts {
                        let message = match tone {
                            ToastTone::Info => "Queued · informational",
                            ToastTone::Success => "Queued · success",
                            ToastTone::Warning => "Queued · warning",
                            ToastTone::Danger => "Queued · danger",
                        };
                        if let Some(handle) = &self.toast_handle {
                            let _ = handle.show(Toast::new(message).with_tone(tone));
                        }
                    }
                    self.runtime.invalidate_paint();
                }
                #[cfg(feature = "agent")]
                if self.agent_observation_requested.replace(false) {
                    let snapshot = drawen::agent::Agent::new().observe(&self.runtime);
                    *self.agent_report.borrow_mut() = compact_agent_report(&snapshot.to_text());
                    self.runtime.invalidate_layout();
                }
                #[cfg(feature = "media")]
                if let Some(import) = self.media_import_requested.replace(None) {
                    self.import_media(import);
                }
                if self.dirty.replace(false) {
                    let animations = self.appearance.borrow().animations;
                    self.runtime
                        .environment_mut()
                        .set_animations_enabled(animations);
                    self.runtime.invalidate_layout();
                }
                self.capture_diagnostics();
                let mut action = WindowAction::None;
                for command in self.runtime.take_window_commands() {
                    action = action_for(command);
                }
                if action == WindowAction::None
                    && (self.runtime.take_redraw_request() || {
                        #[cfg(feature = "cubex")]
                        {
                            cubex_started
                        }
                        #[cfg(not(feature = "cubex"))]
                        {
                            false
                        }
                    })
                {
                    WindowAction::RequestRedraw
                } else {
                    action
                }
            }
            // Lifecycle changes only affect whether a frame is worth drawing;
            // the platform asks for a redraw when content becomes visible.
            WindowEvent::Visible(_) | WindowEvent::Minimized(_) | WindowEvent::Occluded(_) => {
                WindowAction::None
            }
            WindowEvent::RedrawRequested => {
                if self.drain_tasks() {
                    if let Some(id) = self.task_status_id {
                        self.runtime.invalidate_paint_for(id);
                    }
                    WindowAction::RequestRedraw
                } else {
                    WindowAction::None
                }
            }
        }
    }

    fn presentation_changed(&mut self, presentation: WindowPresentation) -> WindowAction {
        self.runtime.set_presented(presentation.is_presented());
        WindowAction::None
    }

    fn frame_damage(&self) -> FrameDamage {
        self.runtime.frame_damage()
    }

    fn redraw(&mut self) -> DisplayList {
        let mut display_list = DisplayList::new();
        let _ = self
            .runtime
            .paint(&mut PaintContext::new(&mut display_list));
        display_list
    }

    fn tick(&mut self, delta: Duration) -> bool {
        self.runtime.tick(delta)
    }

    fn next_frame_in(&self) -> Option<Duration> {
        self.runtime.next_frame_in()
    }
}

/// Menu bar, quick actions, tabbed content area, and status bar.
struct ShowcaseShell;

impl Widget for ShowcaseShell {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let size = available_size(constraints);
        let (menu, menu_layout) = context
            .layout_child(
                0,
                Constraints::loose(valid_size(size.width(), MENU_MAX_HEIGHT)),
            )
            .expect("shell menu exists");
        let menu_height = menu_layout.size().height().min(size.height());

        let remaining = (size.height() - menu_height).max(0.0);
        let (toolbar, toolbar_layout) = context
            .layout_child(
                1,
                Constraints::loose(valid_size(size.width(), TOOLBAR_MAX_HEIGHT.min(remaining))),
            )
            .expect("shell toolbar exists");
        let toolbar_height = toolbar_layout.size().height().min(remaining);

        let status_height =
            STATUS_HEIGHT.min((size.height() - menu_height - toolbar_height).max(0.0));
        let tabs_height = (size.height() - menu_height - toolbar_height - status_height).max(0.0);
        let (tabs, _) = context
            .layout_child(2, tight(size.width(), tabs_height))
            .expect("shell tabs exists");
        let (status, _) = context
            .layout_child(3, tight(size.width(), status_height))
            .expect("shell status exists");

        LayoutResult::with_children(
            size,
            vec![
                ChildLayout::new(menu, Point::ZERO),
                ChildLayout::new(toolbar, Point::new(0.0, menu_height)),
                ChildLayout::new(tabs, Point::new(0.0, menu_height + toolbar_height)),
                ChildLayout::new(
                    status,
                    Point::new(0.0, menu_height + toolbar_height + tabs_height),
                ),
            ],
        )
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        context.fill_rect(bounds, environment.theme().background());
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

/// A tab page that paints the theme background behind a scrollable child.
struct Page;

impl Widget for Page {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let size = available_size(constraints);
        let mut children = Vec::new();
        if context.child_count() == 1 {
            let (id, _) = context
                .layout_child(0, tight(size.width(), size.height()))
                .expect("page scroll child exists");
            children.push(ChildLayout::new(id, Point::ZERO));
        }
        LayoutResult::with_children(size, children)
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        context.fill_rect(bounds, environment.theme().background());
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }

    fn clips_children(&self) -> bool {
        true
    }
}

/// Vertical stack with a real gap and optional padding.
struct VStack {
    gap: f32,
    padding: Insets,
}

impl VStack {
    const fn new(gap: f32) -> Self {
        Self {
            gap,
            padding: Insets::ZERO,
        }
    }

    const fn with_padding(mut self, padding: Insets) -> Self {
        self.padding = padding;
        self
    }
}

impl Widget for VStack {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let max = constraints.max();
        let width = if max.width().is_finite() {
            max.width()
        } else {
            constraints.min().width()
        };
        let inner_width = (width - self.padding.horizontal()).max(0.0);
        let mut cursor = self.padding.top;
        let mut widest: f32 = 0.0;
        let mut children = Vec::with_capacity(context.child_count());
        for index in 0..context.child_count() {
            let (id, layout) = context
                .layout_child(
                    index,
                    Constraints::loose(valid_size(inner_width, f32::INFINITY)),
                )
                .expect("stack child index is valid");
            let child = layout.size();
            children.push(ChildLayout::new(id, Point::new(self.padding.left, cursor)));
            cursor += child.height();
            if index + 1 < context.child_count() {
                cursor += self.gap;
            }
            widest = widest.max(child.width());
        }
        let content_width = if max.width().is_finite() {
            width
        } else {
            widest + self.padding.horizontal()
        };
        let height = (cursor + self.padding.bottom).max(0.0);
        LayoutResult::with_children(
            constraints.constrain(valid_size(content_width, height)),
            children,
        )
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

/// Horizontal stack. In `equal` mode children receive equal-width cells.
struct HStack {
    gap: f32,
    equal: bool,
}

impl HStack {
    const fn new(gap: f32) -> Self {
        Self { gap, equal: false }
    }

    const fn with_equal(mut self, equal: bool) -> Self {
        self.equal = equal;
        self
    }
}

impl Widget for HStack {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let max = constraints.max();
        let count = context.child_count();
        let total_gap = if count > 1 {
            self.gap * usize_to_f32(count - 1)
        } else {
            0.0
        };
        let width = if max.width().is_finite() {
            max.width()
        } else {
            constraints.min().width()
        };
        let cell_width = if self.equal && count > 0 {
            ((width - total_gap) / usize_to_f32(count)).max(0.0)
        } else {
            width
        };
        let mut children = Vec::with_capacity(count);
        let mut cursor = 0.0;
        let mut height: f32 = 0.0;
        for index in 0..count {
            let child_width = if self.equal { cell_width } else { width };
            let (id, layout) = context
                .layout_child(
                    index,
                    Constraints::loose(valid_size(child_width, f32::INFINITY)),
                )
                .expect("row child index is valid");
            let origin_x = if self.equal {
                usize_to_f32(index) * (cell_width + self.gap)
            } else {
                cursor
            };
            children.push(ChildLayout::new(id, Point::new(origin_x, 0.0)));
            height = height.max(layout.size().height());
            cursor = origin_x + layout.size().width() + self.gap;
        }
        let content_width = if self.equal || max.width().is_finite() {
            width
        } else {
            (cursor - self.gap).max(0.0)
        };
        LayoutResult::with_children(
            constraints.constrain(valid_size(content_width, height)),
            children,
        )
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

/// A theme-aware card surface with one content child.
struct SurfaceCard {
    elevation: f32,
}

impl SurfaceCard {
    const fn new(elevation: f32) -> Self {
        Self { elevation }
    }
}

impl Widget for SurfaceCard {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let theme = context.environment().theme();
        let padding = (theme.spacing() * 2.0).max(14.0);
        let max = constraints.max();
        let child_max = valid_size(
            (max.width() - padding * 2.0).max(0.0),
            (max.height() - padding * 2.0).max(0.0),
        );
        let mut children = Vec::new();
        let child_size = if context.child_count() == 1 {
            let (id, layout) = context
                .layout_child(0, Constraints::loose(child_max))
                .expect("card content exists");
            children.push(ChildLayout::new(id, Point::new(padding, padding)));
            layout.size()
        } else {
            Size::ZERO
        };
        LayoutResult::with_children(
            constraints.constrain(valid_size(
                child_size.width() + padding * 2.0,
                child_size.height() + padding * 2.0,
            )),
            children,
        )
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        let theme = environment.theme();
        let radius = theme.corner_radius() * 1.35;
        context.draw_shadow(bounds, radius, self.elevation, theme.shadow());
        context.fill_rounded_rect(bounds, radius, theme.raised_surface());
        context.stroke_rounded_rect(bounds, radius, 1.0, theme.border());
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

/// Gives its single child a fixed height while keeping the available width.
struct FixedHeight {
    height: f32,
}

impl FixedHeight {
    const fn new(height: f32) -> Self {
        Self { height }
    }
}

impl Widget for FixedHeight {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let max = constraints.max();
        let width = if max.width().is_finite() {
            max.width()
        } else {
            constraints.min().width().max(320.0)
        };
        let height = self.height.min(max.height().max(0.0));
        let mut children = Vec::new();
        if context.child_count() == 1 {
            let (id, _) = context
                .layout_child(0, tight(width, height))
                .expect("fixed-height child exists");
            children.push(ChildLayout::new(id, Point::ZERO));
        }
        LayoutResult::with_children(valid_size(width, height), children)
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

/// Caps the width offered to its single child, keeping the child's natural
/// height. Used to keep calendars and numeric fields at a readable size.
struct MaxWidth {
    width: f32,
}

impl MaxWidth {
    const fn new(width: f32) -> Self {
        Self { width }
    }
}

impl Widget for MaxWidth {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let max = constraints.max();
        let width = if max.width().is_finite() {
            max.width().min(self.width)
        } else {
            self.width
        };
        let mut children = Vec::new();
        let child_size = if context.child_count() == 1 {
            let (id, layout) = context
                .layout_child(0, Constraints::loose(valid_size(width, max.height())))
                .expect("max-width child exists");
            children.push(ChildLayout::new(id, Point::ZERO));
            layout.size()
        } else {
            Size::ZERO
        };
        LayoutResult::with_children(constraints.constrain(child_size), children)
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Theme-aware text and presentation widgets
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextTone {
    Primary,
    Muted,
    OnAccent,
}

impl TextTone {
    fn color(self, environment: &UiEnvironment) -> Color {
        let theme = environment.theme();
        match self {
            Self::Primary => theme.foreground(),
            Self::Muted => theme.muted_foreground(),
            Self::OnAccent => theme.accent_foreground(),
        }
    }
}

/// Text that reshapes against the live theme font and color during layout.
struct Label {
    text: String,
    scale: f32,
    tone: TextTone,
    layout: Option<TextLayout>,
}

impl Label {
    fn new(text: impl Into<String>, scale: f32, tone: TextTone) -> Self {
        Self {
            text: text.into(),
            scale,
            tone,
            layout: None,
        }
    }
}

impl Widget for Label {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let environment = context.environment();
        let max = constraints.max();
        let wrap = max.width().is_finite().then_some(max.width());
        let layout = layout_text(
            environment,
            &self.text,
            self.scale,
            self.tone.color(environment),
            wrap,
        );
        let size = layout.size();
        self.layout = Some(layout);
        LayoutResult::new(constraints.constrain(size))
    }

    fn paint(&self, context: &mut PaintContext<'_>, _: &UiEnvironment, bounds: Rect) {
        if let Some(layout) = &self.layout {
            context.draw_text(layout.clone(), bounds.origin);
        }
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

/// Live, clipped report text. It owns only a string, never the UI tree.
struct LiveText {
    text: Rc<RefCell<String>>,
    layout: Option<TextLayout>,
}

impl LiveText {
    fn new(text: Rc<RefCell<String>>) -> Self {
        Self { text, layout: None }
    }
}

impl Widget for LiveText {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let size = available_size(constraints);
        self.layout = Some(layout_text(
            context.environment(),
            &self.text.borrow(),
            0.82,
            context.environment().theme().foreground(),
            Some((size.width() - 24.0).max(0.0)),
        ));
        LayoutResult::new(size)
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        context.fill_rounded_rect(
            bounds,
            environment.theme().corner_radius(),
            environment.theme().surface(),
        );
        context.stroke_rounded_rect(
            bounds,
            environment.theme().corner_radius(),
            1.0,
            environment.theme().border(),
        );
        if let Some(layout) = &self.layout {
            context.draw_text(
                layout.clone(),
                Point::new(bounds.origin.x + 12.0, bounds.origin.y + 12.0),
            );
        }
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }

    fn clips_children(&self) -> bool {
        true
    }
}

/// A row of rounded pill chips with theme-tinted accent fills.
struct Chips {
    labels: Vec<&'static str>,
    layouts: Vec<TextLayout>,
    chip_widths: Vec<f32>,
    height: f32,
}

impl Chips {
    fn new(labels: &[&'static str]) -> Self {
        Self {
            labels: labels.to_vec(),
            layouts: Vec::new(),
            chip_widths: Vec::new(),
            height: 0.0,
        }
    }
}

impl Widget for Chips {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let environment = context.environment();
        let theme = environment.theme();
        let padding = theme.spacing() * 1.4;
        let gap = theme.spacing() * 0.75;
        self.layouts.clear();
        self.chip_widths.clear();
        let mut total = 0.0;
        for label in &self.labels {
            let layout = layout_text(environment, label, 0.84, theme.accent(), None);
            let width = layout.size().width() + padding;
            total += width + gap;
            self.chip_widths.push(width);
            self.layouts.push(layout);
        }
        if !self.labels.is_empty() {
            total -= gap;
        }
        self.height = (theme.font_size() * 1.9).max(24.0);
        let max = constraints.max();
        let width = if max.width().is_finite() {
            max.width()
        } else {
            total
        };
        LayoutResult::new(constraints.constrain(valid_size(width, self.height)))
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        let theme = environment.theme();
        let gap = theme.spacing() * 0.75;
        let radius = self.height / 2.0;
        let mut cursor = 0.0;
        for ((layout, width), _) in self.layouts.iter().zip(&self.chip_widths).zip(&self.labels) {
            let chip = Rect::new(
                Point::new(bounds.origin.x + cursor, bounds.origin.y),
                valid_size(*width, self.height),
            );
            context.fill_rounded_rect(chip, radius, theme.accent().with_alpha(0.14));
            context.stroke_rounded_rect(chip, radius, 1.0, theme.accent().with_alpha(0.34));
            let text_height = layout.size().height();
            context.draw_text(
                layout.clone(),
                Point::new(
                    chip.origin.x + (chip.size.width() - layout.size().width()) / 2.0,
                    chip.origin.y + (self.height - text_height) / 2.0,
                ),
            );
            cursor += width + gap;
        }
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

/// A compact dashboard metric.
struct StatCard {
    value: &'static str,
    caption: &'static str,
    value_layout: Option<TextLayout>,
    caption_layout: Option<TextLayout>,
}

impl StatCard {
    const fn new(value: &'static str, caption: &'static str) -> Self {
        Self {
            value,
            caption,
            value_layout: None,
            caption_layout: None,
        }
    }
}

impl Widget for StatCard {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let environment = context.environment();
        let theme = environment.theme();
        let max = constraints.max();
        let width = if max.width().is_finite() {
            max.width()
        } else {
            180.0
        };
        let height = (theme.font_size() * 4.4).max(86.0);
        self.value_layout = Some(layout_text(
            environment,
            self.value,
            1.9,
            theme.accent(),
            Some((width - 28.0).max(0.0)),
        ));
        self.caption_layout = Some(layout_text(
            environment,
            self.caption,
            0.84,
            theme.muted_foreground(),
            Some((width - 28.0).max(0.0)),
        ));
        LayoutResult::new(constraints.constrain(valid_size(width, height)))
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        let theme = environment.theme();
        let radius = theme.corner_radius() * 1.2;
        context.fill_rounded_rect(bounds, radius, theme.surface());
        context.stroke_rounded_rect(bounds, radius, 1.0, theme.border());
        let padding = 14.0;
        if let Some(layout) = &self.value_layout {
            context.draw_text(
                layout.clone(),
                Point::new(bounds.origin.x + padding, bounds.origin.y + padding),
            );
        }
        if let Some(layout) = &self.caption_layout {
            context.draw_text(
                layout.clone(),
                Point::new(
                    bounds.origin.x + padding,
                    bounds.origin.y + bounds.size.height() - padding - layout.size().height(),
                ),
            );
        }
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

/// Live palette preview painted from the current semantic theme.
struct PalettePreview;

impl Widget for PalettePreview {
    fn layout(&mut self, _: &mut LayoutContext<'_>, constraints: Constraints) -> LayoutResult {
        let max = constraints.max();
        let width = if max.width().is_finite() {
            max.width()
        } else {
            360.0
        };
        let height = constraints.constrain(valid_size(width, 96.0)).height();
        LayoutResult::new(valid_size(width, height))
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        let theme = environment.theme();
        let radius = theme.corner_radius();
        context.fill_rounded_rect(bounds, radius, theme.surface());
        context.stroke_rounded_rect(bounds, radius, 1.0, theme.border());

        let entries = [
            ("background", theme.background()),
            ("surface", theme.surface()),
            ("raised", theme.raised_surface()),
            ("control", theme.control_background()),
            ("accent", theme.accent()),
            ("text", theme.foreground()),
        ];
        let gap = 8.0;
        let inset = 12.0;
        let available = (bounds.size.width() - inset * 2.0 - gap * 5.0).max(0.0);
        let cell = (available / 6.0).max(0.0);
        for (index, (label, color)) in entries.iter().enumerate() {
            let x = bounds.origin.x + inset + usize_to_f32(index) * (cell + gap);
            let swatch = Rect::new(
                Point::new(x, bounds.origin.y + inset),
                valid_size(cell, 42.0),
            );
            context.fill_rounded_rect(swatch, (radius * 0.7).max(4.0), *color);
            context.stroke_rounded_rect(swatch, (radius * 0.7).max(4.0), 1.0, theme.border());
            let text = layout_text(
                environment,
                label,
                0.72,
                theme.muted_foreground(),
                Some(cell),
            );
            context.draw_text(
                text.clone(),
                Point::new(
                    x + (cell - text.size().width()) / 2.0,
                    bounds.origin.y + inset + 48.0,
                ),
            );
        }
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Settings row
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Metric {
    FontSize,
    Spacing,
    Radius,
    Accent,
    Preset,
    Animation,
}

impl Metric {
    fn text(self, appearance: &Appearance) -> String {
        match self {
            Self::FontSize => format!("{:.1} px", BASE_FONT_SIZE * appearance.font_scale),
            Self::Spacing => format!("{:.1} px", BASE_SPACING * appearance.spacing_scale),
            Self::Radius => format!("{:.0} px", appearance.radius),
            Self::Accent => format!(
                "{} · {:.0}°",
                accent_name(appearance.accent_hue),
                appearance.accent_hue
            ),
            Self::Preset => appearance.preset.label().to_owned(),
            Self::Animation => {
                if appearance.animations {
                    "Smooth".to_owned()
                } else {
                    "Instant".to_owned()
                }
            }
        }
    }

    fn swatch(self, appearance: &Appearance) -> Option<Color> {
        match self {
            Self::Accent => Some(accent_color(
                appearance.preset.is_dark(),
                appearance.accent_hue,
            )),
            _ => None,
        }
    }
}

/// Title / description / live value header above one control.
struct FormRow {
    title: String,
    description: Option<String>,
    metric: Option<Metric>,
    static_metric: Option<String>,
    appearance: Option<Rc<RefCell<Appearance>>>,
    align_end: bool,
    stretch: bool,
    title_layout: Option<TextLayout>,
    description_layout: Option<TextLayout>,
    metric_layout: Option<TextLayout>,
    metric_swatch: Option<Color>,
    title_height: f32,
    description_y: f32,
    metric_x: f32,
}

impl FormRow {
    fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: None,
            metric: None,
            static_metric: None,
            appearance: None,
            align_end: false,
            stretch: true,
            title_layout: None,
            description_layout: None,
            metric_layout: None,
            metric_swatch: None,
            title_height: 0.0,
            description_y: 0.0,
            metric_x: 0.0,
        }
    }

    fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    fn with_metric(mut self, metric: Metric, appearance: Rc<RefCell<Appearance>>) -> Self {
        self.metric = Some(metric);
        self.appearance = Some(appearance);
        self
    }

    fn with_static_metric(mut self, metric: impl Into<String>) -> Self {
        self.static_metric = Some(metric.into());
        self
    }

    const fn align_end(mut self) -> Self {
        self.align_end = true;
        self
    }

    const fn without_stretch(mut self) -> Self {
        self.stretch = false;
        self
    }
}

impl Widget for FormRow {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let environment = context.environment();
        let theme = environment.theme();
        let max = constraints.max();
        let width = if max.width().is_finite() {
            max.width()
        } else {
            constraints.min().width().max(320.0)
        };

        let appearance = self.appearance.as_ref().map(|value| *value.borrow());
        let metric_text = self
            .metric
            .zip(appearance)
            .map(|(metric, appearance)| metric.text(&appearance))
            .or_else(|| self.static_metric.clone());

        let mut metric_width = 0.0;
        self.metric_layout = None;
        self.metric_swatch = None;
        if let Some(text) = metric_text {
            let layout = layout_text(environment, &text, 0.86, theme.accent(), None);
            metric_width = layout.size().width();
            self.metric_swatch = self
                .metric
                .zip(appearance)
                .and_then(|(metric, appearance)| metric.swatch(&appearance));
            self.metric_layout = Some(layout);
        }
        let metric_space = if self.metric_layout.is_some() {
            metric_width + 26.0
        } else {
            0.0
        };
        self.metric_x = (width - metric_width).max(0.0);

        let title_wrap = (width - metric_space).max(0.0);
        let title = layout_text(
            environment,
            &self.title,
            1.0,
            theme.foreground(),
            Some(title_wrap),
        );
        self.title_height = title.size().height();
        self.title_layout = Some(title);

        self.description_y = self.title_height;
        self.description_layout = None;
        if let Some(description) = &self.description {
            let layout = layout_text(
                environment,
                description,
                0.84,
                theme.muted_foreground(),
                Some(width),
            );
            self.description_y += 3.0;
            self.title_height += 3.0 + layout.size().height();
            self.description_layout = Some(layout);
        }

        let gap = theme.spacing() * 0.9;
        let child_y = self.title_height + gap;
        let child_max_height = if max.height().is_finite() {
            (max.height() - child_y).max(0.0)
        } else {
            f32::INFINITY
        };
        let child_constraints = if self.stretch {
            Constraints::new(valid_size(width, 0.0), valid_size(width, child_max_height))
                .expect("stretched form constraints are valid")
        } else {
            Constraints::loose(valid_size(width, child_max_height))
        };
        let mut children = Vec::new();
        let mut total = self.title_height;
        if context.child_count() == 1 {
            let (id, layout) = context
                .layout_child(0, child_constraints)
                .expect("form control exists");
            let child = layout.size();
            let x = if self.align_end && width.is_finite() {
                (width - child.width()).max(0.0)
            } else {
                0.0
            };
            children.push(ChildLayout::new(id, Point::new(x, child_y)));
            total = child_y + child.height();
        }
        LayoutResult::with_children(constraints.constrain(valid_size(width, total)), children)
    }

    fn paint(&self, context: &mut PaintContext<'_>, _: &UiEnvironment, bounds: Rect) {
        if let Some(layout) = &self.title_layout {
            context.draw_text(layout.clone(), bounds.origin);
        }
        if let Some(layout) = &self.metric_layout {
            let mut x = bounds.origin.x + self.metric_x;
            if let Some(color) = self.metric_swatch {
                let swatch = Rect::new(
                    Point::new(x - 18.0, bounds.origin.y + (self.title_height - 11.0) / 2.0),
                    valid_size(11.0, 11.0),
                );
                context.fill_rounded_rect(swatch, 3.0, color);
                x += 0.0;
            }
            context.draw_text(layout.clone(), Point::new(x, bounds.origin.y));
        }
        if let Some(layout) = &self.description_layout {
            context.draw_text(
                layout.clone(),
                Point::new(bounds.origin.x, bounds.origin.y + self.description_y),
            );
        }
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Page content: overview
// ---------------------------------------------------------------------------

fn append_overview_page(runtime: &mut UiRuntime, tabs: drawen::WidgetId) {
    let stack = append_scroll_page(runtime, tabs);

    let hero = runtime
        .append_child(stack, SurfaceCard::new(12.0))
        .expect("overview stack exists");
    let hero_column = runtime
        .append_child(hero, VStack::new(12.0))
        .expect("hero card exists");
    append_label(
        runtime,
        hero_column,
        "Drawen UI · Style Lab",
        2.2,
        TextTone::Primary,
    );
    append_label(
        runtime,
        hero_column,
        "The complete laboratory: every standard widget, the live semantic theme, DSS styling, virtualized data, docking, media, software 3D, overlays, background tasks, and frame diagnostics — all in one retained tree.",
        1.02,
        TextTone::Muted,
    );
    runtime
        .append_child(
            hero_column,
            Chips::new(&[
                "Live palette",
                "Retained tree",
                "Frame damage",
                "EN / RU input",
                "Headless snapshots",
            ]),
        )
        .expect("hero column exists");
    append_divider(runtime, hero_column);

    let stats = runtime
        .append_child(hero_column, HStack::new(14.0).with_equal(true))
        .expect("hero column exists");
    for (value, caption) in [
        ("14", "showcase pages"),
        ("60+", "widgets & primitives"),
        ("3", "live palettes"),
    ] {
        runtime
            .append_child(stats, StatCard::new(value, caption))
            .expect("stats row exists");
    }

    let palette_card = append_card(
        runtime,
        stack,
        "Palette",
        "Every color below is read from the current theme. Open the \"Appearance\" page to change them.",
    );
    runtime
        .append_child(palette_card, PalettePreview)
        .expect("palette card exists");

    let map_card = append_card(
        runtime,
        stack,
        "Capability map",
        "Each page demonstrates one area of the library. Everything below is live — click, type, drag, and scroll.",
    );
    for line in [
        "Components — text editing, selectable labels, choice controls, buttons, status widgets, alerts, expanders.",
        "Entry — NumberInput, RangeSlider, Autocomplete, DatePicker, and one-way Property bindings.",
        "Data — TreeView, VirtualList (10,000 rows), VirtualTable, ListView, and the zoomable ImageViewer.",
        "Layout — Splitter, Row / Column / Stack / Align / Padding / SizedBox, and horizontal scrolling.",
        "Workspace — DockArea with a serializable panel model, tabs, and draggable splits.",
        "Media — Video and AudioPlayer with configurable controls, plus frame-based AnimatedImage.",
        "3D — CubeXView: a deterministic software rasterizer presenting a rotating lit scene.",
        "Appearance — the live semantic theme: palette, accent hue, typography, density, radius, motion.",
        "DSS — scoped stylesheet: custom properties, calc(), combinators, pseudo-classes, !important.",
        "Overlays — Tooltip, Popover, Menu, Modal, CommandPalette, SplitButton, and queued toasts.",
        "Runtime — TaskPool background work with a UiDispatcher, plus live frame-damage diagnostics.",
        "Agents — the semantic-tree inspector for LLM-driven UI observation and control.",
        "Servo — the shared-texture browser interop contract.",
    ] {
        append_label(runtime, map_card, line, 0.9, TextTone::Muted);
    }

    let atlas = append_card(
        runtime,
        stack,
        "Raster and typography",
        "The image is drawn through the display list, and labels are reshaped when the theme changes.",
    );
    runtime
        .append_child(atlas, Image::with_size(drawen_logo(), 72.0, 72.0))
        .expect("atlas card exists");
    append_label(
        runtime,
        atlas,
        "Logo · 16×16 RGBA · scales without losing paint commands",
        0.86,
        TextTone::Muted,
    );
}

// ---------------------------------------------------------------------------
// Page content: components
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
fn append_components_page(
    runtime: &mut UiRuntime,
    subscriptions: &mut Vec<Subscription>,
    tabs: drawen::WidgetId,
) {
    let stack = append_scroll_page(runtime, tabs);

    let text_card = append_card(
        runtime,
        stack,
        "Text",
        "Single-line and multiline editing, plus selectable read-only labels. Type in any language — missing scripts fall back to the system fonts.",
    );
    let field = TextField::with_text("Hello, Drawen! Edit this text.");
    subscriptions.push(
        field
            .changed()
            .subscribe(|text| println!("TextField: {text}")),
    );
    append_form(
        runtime,
        text_card,
        FormRow::new("Single-line field").with_description(
            "Click the field and start typing — the caret, selection, and IME work as expected.",
        ),
        field,
    );

    let editor = TextEdit::with_text(
        "This is a multiline TextEdit.\nCheck line wrapping, selection, and scrolling.\nEnglish text: The quick brown fox jumps over the lazy dog.\nUnicode text: Ελληνικά · 日本語 · العربية · हिन्दी · 한국어.",
    );
    subscriptions.push(
        editor
            .changed()
            .subscribe(|text| println!("TextEdit: {} characters", text.chars().count())),
    );
    append_form(
        runtime,
        text_card,
        FormRow::new("Multiline editor").with_description(
            "TextEdit reuses TextField's grapheme navigation, undo, and clipboard behavior.",
        ),
        editor,
    );
    append_form(
        runtime,
        text_card,
        FormRow::new("Selectable label").with_description(
            "Drag to select, double-click a word, Ctrl+C copies to the OS clipboard, Ctrl+A selects all.",
        ),
        Text::styled(
            "Drawen keeps a retained widget tree: select this sentence and copy it with Ctrl+C — the text reaches the native clipboard.",
            TextStyle::new(BASE_FONT_SIZE, Color::rgba8(148, 163, 184, 255)),
        )
        .selectable(),
    );

    let choice_card = append_card(
        runtime,
        stack,
        "Choice",
        "Checkboxes, switches, lists, segments, and sliders — every element reads the same theme.",
    );
    let checkbox = append_form(
        runtime,
        choice_card,
        FormRow::new("Checkbox")
            .with_description("A binary state with focus and keyboard activation."),
        Checkbox::with_checked(true),
    );
    append_label(
        runtime,
        checkbox,
        "Show hover hints",
        0.96,
        TextTone::Primary,
    );
    append_form(
        runtime,
        choice_card,
        FormRow::new("Switch")
            .with_description("Applies a setting immediately.")
            .align_end()
            .without_stretch(),
        Switch::with_checked(true),
    );
    let mut dropdown = Dropdown::new(["Auto", "Compact", "Roomy"]);
    dropdown.set_selected(Some(0));
    subscriptions.push(
        dropdown
            .changed()
            .subscribe(|index| println!("Dropdown: selected option {index}")),
    );
    append_form(
        runtime,
        choice_card,
        FormRow::new("Dropdown").with_description("The list opens inside the retained tree."),
        dropdown,
    );
    let segments = SegmentedControl::new(["Left", "Center", "Right"]).with_selected(1);
    subscriptions.push(
        segments
            .changed()
            .subscribe(|index| println!("SegmentedControl: segment {index}")),
    );
    append_form(
        runtime,
        choice_card,
        FormRow::new("Segmented control")
            .with_description("Mutually exclusive options on one line."),
        segments,
    );
    let slider = Slider::with_value(0.0, 100.0, 64.0);
    subscriptions.push(
        slider
            .changed()
            .subscribe(|value| println!("Slider: {value:.0}%")),
    );
    append_form(
        runtime,
        choice_card,
        FormRow::new("Slider")
            .with_description("A continuous range with keyboard control.")
            .with_static_metric("64%"),
        slider,
    );

    let radio_card = append_card(
        runtime,
        stack,
        "Radio buttons",
        "RadioGroup stores the selected value separately from the widgets and emits a signal.",
    );
    let group = RadioGroup::new();
    group.set_selected(&"All");
    subscriptions.push(
        group
            .changed()
            .subscribe(|value| println!("RadioGroup: {}", *value)),
    );
    let radio_row = runtime
        .append_child(
            radio_card,
            FormRow::new("Notifications")
                .with_description("Select one option — the others reset automatically."),
        )
        .expect("radio card exists");
    let radios = runtime
        .append_child(radio_row, VStack::new(7.0))
        .expect("radio row exists");
    for option in ["All", "Important", "None"] {
        let radio = runtime
            .append_child(radios, RadioButton::new(group.clone(), option))
            .expect("radio stack exists");
        append_label(runtime, radio, option, 0.96, TextTone::Primary);
    }

    let action_card = append_card(
        runtime,
        stack,
        "Actions and status",
        "Standard buttons, indicators, badges, avatars, and semantic alerts.",
    );
    let actions = runtime
        .append_child(action_card, HStack::new(10.0))
        .expect("action card exists");
    for (label, message) in [
        ("Primary", "Primary action"),
        ("Secondary", "Secondary action"),
        ("Danger", "Dangerous action"),
    ] {
        append_button(runtime, subscriptions, actions, label, message);
    }
    append_form(
        runtime,
        action_card,
        FormRow::new("Progress").with_static_metric("72%"),
        ProgressBar::with_progress(0.72),
    );
    let indicators = runtime
        .append_child(action_card, HStack::new(18.0))
        .expect("action card exists");
    runtime
        .append_child(indicators, Spinner::new().with_diameter(28.0))
        .expect("indicator row exists");
    runtime
        .append_child(indicators, Skeleton::new().with_size(170.0, 18.0))
        .expect("indicator row exists");
    runtime
        .append_child(
            indicators,
            Avatar::new()
                .with_diameter(42.0)
                .with_initials("TR")
                .with_status(AvatarStatus::Online)
                .with_label("Drawen"),
        )
        .expect("indicator row exists");

    let badges = runtime
        .append_child(action_card, HStack::new(8.0))
        .expect("action card exists");
    for (tone, text) in [
        (BadgeTone::Neutral, "Neutral"),
        (BadgeTone::Accent, "Accent"),
        (BadgeTone::Success, "Success"),
        (BadgeTone::Warning, "Warning"),
        (BadgeTone::Danger, "Error"),
    ] {
        append_badge(runtime, badges, tone, text);
    }

    let variants_card = append_card(
        runtime,
        stack,
        "Button variants",
        "Buttons resolve background, hover, and press colors from the theme; local builders override geometry and state without breaking the palette.",
    );
    let variants = runtime
        .append_child(variants_card, HStack::new(10.0))
        .expect("variants card exists");
    let disabled = Button::new().with_enabled(false);
    let disabled = runtime
        .append_child(variants, disabled)
        .expect("variants row exists");
    append_label(runtime, disabled, "Disabled", 0.96, TextTone::Muted);
    let pill = Button::new().with_corner_radius(20.0);
    subscriptions.push(pill.activated().subscribe(|()| println!("Pill button.")));
    let pill = runtime
        .append_child(variants, pill)
        .expect("variants row exists");
    append_label(runtime, pill, "Pill radius", 0.96, TextTone::Primary);
    let roomy = Button::new().with_padding(Insets::new(12.0, 26.0, 12.0, 26.0));
    subscriptions.push(roomy.activated().subscribe(|()| println!("Roomy button.")));
    let roomy = runtime
        .append_child(variants, roomy)
        .expect("variants row exists");
    append_label(runtime, roomy, "Roomy padding", 0.96, TextTone::Primary);

    let surfaces_card = append_card(
        runtime,
        stack,
        "Card surfaces",
        "Card is the standard raised container: rounded surface, border, padding, and a soft multi-layer shadow. CardStyle overrides elevation and padding locally.",
    );
    let surfaces = runtime
        .append_child(surfaces_card, HStack::new(14.0).with_equal(true))
        .expect("surfaces card exists");
    let plain = runtime
        .append_child(surfaces, Card::new())
        .expect("surfaces row exists");
    append_label(
        runtime,
        plain,
        "Default card · elevation 6 · padding 12",
        0.9,
        TextTone::Muted,
    );
    let raised = runtime
        .append_child(
            surfaces,
            Card::new().with_style(CardStyle {
                surface: SurfaceStyle {
                    padding: Some(Insets::all(16.0)),
                    ..SurfaceStyle::default()
                },
                elevation: 16.0,
                shadow_color: None,
            }),
        )
        .expect("surfaces row exists");
    append_label(
        runtime,
        raised,
        "CardStyle · elevation 16 · padding 16",
        0.9,
        TextTone::Muted,
    );

    let alert_card = append_card(
        runtime,
        stack,
        "Notifications",
        "Alert uses the theme's semantic colors: accent, success, warning, and danger.",
    );
    for (tone, title, message) in [
        (
            AlertTone::Info,
            "Information",
            "Theme tokens pick up the new accent automatically.",
        ),
        (
            AlertTone::Success,
            "Done",
            "Appearance changes apply without restarting the application.",
        ),
        (
            AlertTone::Warning,
            "Warning",
            "A very small radius can reduce the readability of borders.",
        ),
        (
            AlertTone::Danger,
            "Error",
            "This is how a destructive action looks in the current palette.",
        ),
    ] {
        runtime
            .append_child(
                alert_card,
                Alert::new()
                    .with_tone(tone)
                    .with_title(title)
                    .with_message(message),
            )
            .expect("alert card exists");
    }

    let expander_card = append_card(
        runtime,
        stack,
        "Expandable sections",
        "Expander keeps its body in the tree but excludes it from layout, paint, and hit testing while collapsed.",
    );
    let expander = runtime
        .append_child(
            expander_card,
            Expander::new("Advanced options").with_expanded(true),
        )
        .expect("expander card exists");
    let body = runtime
        .append_child(expander, VStack::new(8.0))
        .expect("expander exists");
    append_label(
        runtime,
        body,
        "A section can contain any child widgets: forms, lists, and tables.",
        0.92,
        TextTone::Muted,
    );
    append_label(
        runtime,
        body,
        "Space and Enter toggle the state, as in native dialogs.",
        0.92,
        TextTone::Muted,
    );
}

// ---------------------------------------------------------------------------
// Page content: data entry
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
fn append_entry_page(
    runtime: &mut UiRuntime,
    subscriptions: &mut Vec<Subscription>,
    bindings: &mut Vec<Binding>,
    tabs: drawen::WidgetId,
) {
    let stack = append_scroll_page(runtime, tabs);

    let numeric_card = append_card(
        runtime,
        stack,
        "Numeric entry",
        "NumberInput clamps to its range and steps with the arrows; RangeSlider keeps two thumbs ordered on one track.",
    );
    let number = NumberInput::new()
        .with_range(0.0, 240.0)
        .with_step(0.5)
        .with_value(42.0);
    subscriptions.push(
        number
            .changed()
            .subscribe(|value| println!("NumberInput: {value:.1}")),
    );
    let number_row = runtime
        .append_child(
            numeric_card,
            FormRow::new("NumberInput")
                .with_description("Type a value, use the steppers, or press ArrowUp/ArrowDown. Out-of-range input clamps on commit. Range 0–240, step 0.5.")
                .without_stretch(),
        )
        .expect("numeric card exists");
    let number_width = runtime
        .append_child(number_row, MaxWidth::new(220.0))
        .expect("quantity row exists");
    runtime
        .append_child(number_width, number)
        .expect("max-width wrapper exists");

    let range = RangeSlider::with_value(0.0, 100.0, 20.0, 80.0);
    subscriptions.push(range.changed().subscribe(|(lower, upper)| {
        println!("RangeSlider: {lower:.0} – {upper:.0}");
    }));
    append_form(
        runtime,
        numeric_card,
        FormRow::new("RangeSlider")
            .with_description("Drag either thumb; it stops at the other. ArrowDown/Up pick a thumb, ArrowLeft/Right step it."),
        range,
    );

    let autocomplete_card = append_card(
        runtime,
        stack,
        "Autocomplete",
        "An editable combobox: typing filters options into a floating suggestion list measured only while open.",
    );
    let cities = Autocomplete::new([
        "Amsterdam",
        "Berlin",
        "Lisbon",
        "London",
        "Paris",
        "Prague",
        "Seoul",
        "Singapore",
        "Stockholm",
        "Sydney",
        "Tokyo",
        "Toronto",
        "Vienna",
        "Warsaw",
        "Zurich",
        "Москва",
        "Новосибирск",
        "Санкт-Петербург",
    ])
    .with_placeholder("Search cities…");
    subscriptions.push(cities.selected().subscribe(|index| {
        println!("Autocomplete: committed option {index}");
    }));
    let popup = cities.popup();
    let cities = append_form(
        runtime,
        autocomplete_card,
        FormRow::new("City").with_description(
            "ArrowUp/ArrowDown move the highlight; Enter or a click commits; Escape closes.",
        ),
        cities,
    );
    runtime
        .append_child(cities, popup)
        .expect("autocomplete exists");

    let calendar_card = append_card(
        runtime,
        stack,
        "Calendar",
        "DatePicker never reads the platform clock — the application supplies the initial date. Weeks start on Monday.",
    );
    let picker = DatePicker::with_month(2026, 11)
        .with_value(Date::new(2026, 11, 27).expect("demo date is valid"));
    subscriptions.push(picker.changed().subscribe(|date| {
        println!(
            "DatePicker: {}",
            date.map_or_else(|| "<none>".to_owned(), |date| date.to_string())
        );
    }));
    let picker_row = runtime
        .append_child(
            calendar_card,
            FormRow::new("Deadline")
                .with_description("Header arrows and PageUp/PageDown page months; arrow keys walk days and weeks; Enter selects.")
                .without_stretch(),
        )
        .expect("calendar card exists");
    let picker_width = runtime
        .append_child(picker_row, MaxWidth::new(340.0))
        .expect("deadline row exists");
    runtime
        .append_child(picker_width, picker)
        .expect("max-width wrapper exists");

    let binding_card = append_card(
        runtime,
        stack,
        "Live bindings",
        "Property + runtime.bind is a thin one-way copy that wakes redraws — no callbacks, no dependency graph. Drag the slider: the progress bar follows through a binding alone.",
    );
    let drive = Slider::with_value(0.0, 100.0, 64.0);
    let drive_value = drive.value();
    append_form(
        runtime,
        binding_card,
        FormRow::new("Drive").with_description("The source property."),
        drive,
    );
    let follower = ProgressBar::with_progress(0.64);
    bindings.push(runtime.bind(&follower.progress(), &drive_value));
    append_form(
        runtime,
        binding_card,
        FormRow::new("Follower")
            .with_description("ProgressBar.progress() bound to the slider's value."),
        follower,
    );

    let source = NumberInput::new()
        .with_range(0.0, 1_000.0)
        .with_step(1.0)
        .with_value(128.0);
    let source_value = source.value();
    let source_row = runtime
        .append_child(
            binding_card,
            FormRow::new("Source")
                .with_description("Edit this field…")
                .without_stretch(),
        )
        .expect("binding card exists");
    let source_width = runtime
        .append_child(source_row, MaxWidth::new(220.0))
        .expect("source row exists");
    runtime
        .append_child(source_width, source)
        .expect("max-width wrapper exists");
    let mirror = NumberInput::new()
        .with_range(0.0, 1_000.0)
        .with_step(1.0)
        .with_value(128.0);
    bindings.push(runtime.bind(&mirror.value(), &source_value));
    let mirror_row = runtime
        .append_child(
            binding_card,
            FormRow::new("Mirror (one-way)")
                .with_description("…and the mirror updates. Editing the mirror does not write back: bindings are one-way by design.")
                .without_stretch(),
        )
        .expect("binding card exists");
    let mirror_width = runtime
        .append_child(mirror_row, MaxWidth::new(220.0))
        .expect("mirror row exists");
    runtime
        .append_child(mirror_width, mirror)
        .expect("max-width wrapper exists");
}

// ---------------------------------------------------------------------------
// Page content: data views
// ---------------------------------------------------------------------------

/// Ten thousand log rows backed by plain data, never by widgets.
struct LogSource;

impl VirtualDataSource for LogSource {
    fn row_count(&self) -> usize {
        10_000
    }

    fn row_key(&self, row: usize) -> u64 {
        row as u64
    }

    fn row_text(&self, row: usize) -> String {
        format!("[{row:05}] event=frame.commit channel=render severity=info")
    }

    fn label(&self) -> Option<String> {
        Some("Render log".to_owned())
    }
}

/// Five thousand table rows with four typed columns.
struct SceneTable;

impl VirtualTableSource for SceneTable {
    fn row_count(&self) -> usize {
        5_000
    }

    fn row_key(&self, row: usize) -> u64 {
        row as u64
    }

    fn column_count(&self) -> usize {
        4
    }

    fn column_title(&self, column: usize) -> String {
        ["ID", "Name", "Kind", "Score"][column].to_owned()
    }

    fn column_width(&self, column: usize) -> f32 {
        [80.0, 240.0, 180.0, 100.0][column]
    }

    fn cell_text(&self, row: usize, column: usize) -> String {
        match column {
            0 => format!("{row:05}"),
            1 => format!("node-{:04}", row % 1_000),
            2 => ["Mesh", "Light", "Camera", "Material"][row % 4].to_owned(),
            _ => format!("{}", (row * 37) % 100),
        }
    }

    fn label(&self) -> Option<String> {
        Some("Scene graph".to_owned())
    }
}

#[allow(clippy::too_many_lines)]
fn append_data_page(
    runtime: &mut UiRuntime,
    subscriptions: &mut Vec<Subscription>,
    tabs: drawen::WidgetId,
) {
    let stack = append_scroll_page(runtime, tabs);

    let tree_card = append_card(
        runtime,
        stack,
        "TreeView",
        "Hierarchical data with expandable branches. Selection addresses stable node ids, so it survives expanding, collapsing, and replacing the roots.",
    );
    let tree = TreeView::new().with_roots(vec![
        TreeNode::new("assets", "assets")
            .child(TreeNode::new("textures", "textures").child(TreeNode::new("logo", "logo.png")))
            .child(TreeNode::new("audio", "audio")),
        TreeNode::new("scenes", "scenes").child(TreeNode::new("main", "main.graph")),
        TreeNode::new("src", "src")
            .child(TreeNode::new("lib", "lib.rs"))
            .child(TreeNode::new("main_rs", "main.rs")),
        TreeNode::new("docs", "docs"),
    ]);
    subscriptions.push(tree.changed().subscribe(|id| {
        println!("TreeView: selected {}", id.as_deref().unwrap_or("<none>"));
    }));
    subscriptions.push(
        tree.activated()
            .subscribe(|id| println!("TreeView: open {id}")),
    );
    subscriptions.push(tree.toggled().subscribe(|(id, expanded)| {
        println!("TreeView: {id} expanded={expanded}");
    }));
    let tree_frame = runtime
        .append_child(tree_card, FixedHeight::new(280.0))
        .expect("tree card exists");
    runtime
        .append_child(tree_frame, tree)
        .expect("tree frame exists");
    append_label(
        runtime,
        tree_card,
        "Click a twisty to expand, a row to select; double-click or Enter activates. Arrow keys navigate, expand, and collapse.",
        0.86,
        TextTone::Muted,
    );

    let list_card = append_card(
        runtime,
        stack,
        "VirtualList · 10,000 rows",
        "Only visible rows plus overscan are measured and painted. Ctrl-click and Shift-click extend the selection; the data source is plain Rust, not widgets.",
    );
    let mut log = VirtualList::new(LogSource, 24.0);
    log.selection_mut().set_mode(SelectionMode::Multiple);
    subscriptions.push(log.selection_changed().subscribe(|rows| {
        println!("VirtualList: {} rows selected", rows.len());
    }));
    subscriptions.push(
        log.activated()
            .subscribe(|row| println!("VirtualList: activated row {row}")),
    );
    let log_frame = runtime
        .append_child(list_card, FixedHeight::new(260.0))
        .expect("virtual list card exists");
    runtime
        .append_child(log_frame, log)
        .expect("virtual list frame exists");

    let table_card = append_card(
        runtime,
        stack,
        "VirtualTable · 5,000 rows",
        "Column headers, stable row keys, and the same selection model as VirtualList. Double-click a cell to activate it.",
    );
    let table = VirtualTable::new(
        SceneTable,
        vec![
            drawen::TableColumn::new("ID", 80.0),
            drawen::TableColumn::new("Name", 240.0),
            drawen::TableColumn::new("Kind", 180.0),
            drawen::TableColumn::new("Score", 100.0),
        ],
    );
    subscriptions.push(table.selection_changed().subscribe(|rows| {
        println!("VirtualTable: {} rows selected", rows.len());
    }));
    subscriptions.push(table.activated().subscribe(|(row, column)| {
        println!("VirtualTable: activated cell ({row}, {column})");
    }));
    let table_frame = runtime
        .append_child(table_card, FixedHeight::new(260.0))
        .expect("table card exists");
    runtime
        .append_child(table_frame, table)
        .expect("table frame exists");

    let custom_card = append_card(
        runtime,
        stack,
        "ListView",
        "The offset-virtualized list for custom row widgets: only visible rows plus overscan are measured and painted.",
    );
    let list_frame = runtime
        .append_child(custom_card, FixedHeight::new(286.0))
        .expect("list card exists");
    let list = runtime
        .append_child(list_frame, ListView::new(46.0))
        .expect("list frame exists");
    for index in 0..36 {
        runtime
            .append_child(list, ListRow { index })
            .expect("list view exists");
    }

    let viewer_card = append_card(
        runtime,
        stack,
        "ImageViewer",
        "Drag to pan, wheel to zoom. The zoom is a Property, so application state can drive or follow it.",
    );
    let viewer = ImageViewer::new(showcase_texture())
        .with_label("Procedural 128×128 texture · drag to pan · wheel to zoom");
    subscriptions.push(
        viewer
            .zoom_changed()
            .subscribe(|zoom| println!("ImageViewer: zoom {zoom:.2}×")),
    );
    let viewer_frame = runtime
        .append_child(viewer_card, FixedHeight::new(280.0))
        .expect("viewer card exists");
    runtime
        .append_child(viewer_frame, viewer)
        .expect("viewer frame exists");
}

// ---------------------------------------------------------------------------
// Page content: appearance
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
fn append_appearance_page(
    runtime: &mut UiRuntime,
    subscriptions: &mut Vec<Subscription>,
    appearance: &Rc<RefCell<Appearance>>,
    dirty: &Rc<Cell<bool>>,
    tabs: drawen::WidgetId,
) {
    let stack = append_scroll_page(runtime, tabs);

    let card = append_card(
        runtime,
        stack,
        "Appearance",
        "Tune the interface. The values below change the semantic theme used by every standard widget.",
    );

    let preset =
        SegmentedControl::new(Preset::LABELS).with_selected(appearance.borrow().preset.index());
    subscriptions.push(preset.changed().subscribe({
        let appearance = Rc::clone(appearance);
        let dirty = Rc::clone(dirty);
        move |index| {
            if let Some(next) = Preset::ALL.get(*index) {
                appearance.borrow_mut().preset = *next;
                dirty.set(true);
            }
        }
    }));
    append_form(
        runtime,
        card,
        FormRow::new("Palette")
            .with_description("Base surfaces, contrast, and shadows.")
            .with_metric(Metric::Preset, Rc::clone(appearance)),
        preset,
    );

    let accent = Slider::with_value(0.0, 360.0, appearance.borrow().accent_hue);
    subscriptions.push(accent.changed().subscribe({
        let appearance = Rc::clone(appearance);
        let dirty = Rc::clone(dirty);
        move |value| {
            let mut settings = appearance.borrow_mut();
            settings.accent_hue = *value;
            dirty.set(true);
        }
    }));
    append_form(
        runtime,
        card,
        FormRow::new("Accent color")
            .with_description("Hue in HSL space; saturation and lightness adapt to the palette.")
            .with_metric(Metric::Accent, Rc::clone(appearance)),
        accent,
    );

    let font = Slider::with_value(0.8, 1.6, appearance.borrow().font_scale);
    subscriptions.push(font.changed().subscribe({
        let appearance = Rc::clone(appearance);
        let dirty = Rc::clone(dirty);
        move |value| {
            let mut settings = appearance.borrow_mut();
            settings.font_scale = *value;
            dirty.set(true);
        }
    }));
    append_form(
        runtime,
        card,
        FormRow::new("Font size")
            .with_description(
                "The theme's base size. Every standard control recalculates text and height.",
            )
            .with_metric(Metric::FontSize, Rc::clone(appearance)),
        font,
    );

    let spacing = Slider::with_value(0.6, 1.8, appearance.borrow().spacing_scale);
    subscriptions.push(spacing.changed().subscribe({
        let appearance = Rc::clone(appearance);
        let dirty = Rc::clone(dirty);
        move |value| {
            let mut settings = appearance.borrow_mut();
            settings.spacing_scale = *value;
            dirty.set(true);
        }
    }));
    append_form(
        runtime,
        card,
        FormRow::new("Density")
            .with_description("Scales spacing, gaps, and control padding.")
            .with_metric(Metric::Spacing, Rc::clone(appearance)),
        spacing,
    );

    let radius = Slider::with_value(0.0, 22.0, appearance.borrow().radius);
    subscriptions.push(radius.changed().subscribe({
        let appearance = Rc::clone(appearance);
        let dirty = Rc::clone(dirty);
        move |value| {
            let mut settings = appearance.borrow_mut();
            settings.radius = *value;
            dirty.set(true);
        }
    }));
    append_form(
        runtime,
        card,
        FormRow::new("Corner radius")
            .with_description(
                "Radius of controls and cards. Zero produces a strict rectangular interface.",
            )
            .with_metric(Metric::Radius, Rc::clone(appearance)),
        radius,
    );

    let animations = Switch::with_checked(appearance.borrow().animations);
    subscriptions.push(animations.toggled().subscribe({
        let appearance = Rc::clone(appearance);
        let dirty = Rc::clone(dirty);
        move |enabled| {
            appearance.borrow_mut().animations = *enabled;
            dirty.set(true);
        }
    }));
    append_form(
        runtime,
        card,
        FormRow::new("Animations")
            .with_description(
                "Smooth hover, press, and focus transitions. Disable for reduced motion.",
            )
            .with_metric(Metric::Animation, Rc::clone(appearance))
            .align_end()
            .without_stretch(),
        animations,
    );

    append_divider(runtime, card);

    append_label(runtime, card, "Preview", 1.12, TextTone::Primary);
    runtime
        .append_child(card, PalettePreview)
        .expect("appearance card exists");

    let hint_card = append_card(
        runtime,
        stack,
        "How it works",
        "Drawen separates the semantic theme from DSS styles. The showcase changes only the theme, so every standard widget picks up the change through environment.theme().",
    );
    append_label(
        runtime,
        hint_card,
        "1. A control changes Appearance through a callback.\n2. The showcase sets a dirty flag and calls invalidate_layout().\n3. The next frame recalculates layout and resolves theme tokens again.",
        0.92,
        TextTone::Muted,
    );
}

// ---------------------------------------------------------------------------
// Page content: layout
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
fn append_layout_page(runtime: &mut UiRuntime, tabs: drawen::WidgetId) {
    let stack = append_scroll_page(runtime, tabs);

    let card = append_card(
        runtime,
        stack,
        "Splitter",
        "Drag the vertical splitter. Both panels receive new tight constraints instead of simply shifting.",
    );
    let frame = runtime
        .append_child(card, FixedHeight::new(310.0))
        .expect("layout card exists");
    let splitter = runtime
        .append_child(frame, Splitter::new(SplitAxis::Horizontal, 0.46))
        .expect("fixed splitter frame exists");
    runtime
        .append_child(
            splitter,
            PanelCard::new(
                "First panel",
                "Panels paint their background from theme.surface() with a slight accent tint, so switching palettes is immediately visible.",
                0.07,
            ),
        )
        .expect("splitter exists");
    runtime
        .append_child(
            splitter,
            PanelCard::new(
                "Second panel",
                "Splitter works with the pointer and keyboard while keeping focus inside the retained tree.",
                0.13,
            ),
        )
        .expect("splitter exists");

    let primitives_card = append_card(
        runtime,
        stack,
        "Layout primitives",
        "Row, Column, Stack, Align, Padding, and SizedBox are the standard building blocks; the showcase's own VStack/HStack above are built from the same Widget contract.",
    );
    let demos = runtime
        .append_child(primitives_card, HStack::new(16.0).with_equal(true))
        .expect("primitives card exists");

    let row_demo = runtime
        .append_child(demos, VStack::new(8.0))
        .expect("demos row exists");
    let row = runtime
        .append_child(row_demo, Row::new())
        .expect("row demo exists");
    for (width, color) in [
        (44.0, Color::rgba8(94, 234, 212, 255)),
        (60.0, Color::rgba8(167, 139, 250, 255)),
        (80.0, Color::rgba8(251, 191, 36, 255)),
    ] {
        let cell = runtime
            .append_child(
                row,
                DrawenBox::new()
                    .with_background(color)
                    .with_corner_radius(8.0),
            )
            .expect("row exists");
        runtime
            .append_child(cell, SizedBox::new(Some(width), Some(44.0)))
            .expect("colored box exists");
    }
    append_label(
        runtime,
        row_demo,
        "Row · sums child widths",
        0.8,
        TextTone::Muted,
    );

    let column_demo = runtime
        .append_child(demos, VStack::new(8.0))
        .expect("demos row exists");
    let column = runtime
        .append_child(column_demo, drawen::Column::new())
        .expect("column demo exists");
    for color in [
        Color::rgba8(94, 234, 212, 255),
        Color::rgba8(167, 139, 250, 255),
    ] {
        let cell = runtime
            .append_child(
                column,
                DrawenBox::new()
                    .with_background(color)
                    .with_corner_radius(6.0),
            )
            .expect("column exists");
        runtime
            .append_child(cell, SizedBox::new(Some(120.0), Some(26.0)))
            .expect("colored box exists");
    }
    append_label(
        runtime,
        column_demo,
        "Column · stacks children",
        0.8,
        TextTone::Muted,
    );

    let stack_demo = runtime
        .append_child(demos, VStack::new(8.0))
        .expect("demos row exists");
    let overlay = runtime
        .append_child(stack_demo, Stack::new())
        .expect("stack demo exists");
    let base = runtime
        .append_child(
            overlay,
            DrawenBox::new()
                .with_background(Color::rgba8(94, 234, 212, 70))
                .with_corner_radius(8.0),
        )
        .expect("stack exists");
    runtime
        .append_child(base, SizedBox::new(Some(110.0), Some(66.0)))
        .expect("base layer box exists");
    let inset = runtime
        .append_child(overlay, Padding::new(Insets::new(18.0, 0.0, 0.0, 26.0)))
        .expect("stack exists");
    let top = runtime
        .append_child(
            inset,
            DrawenBox::new()
                .with_background(Color::rgba8(167, 139, 250, 255))
                .with_corner_radius(6.0),
        )
        .expect("padding exists");
    runtime
        .append_child(top, SizedBox::new(Some(64.0), Some(36.0)))
        .expect("top layer box exists");
    append_label(
        runtime,
        stack_demo,
        "Stack · overlays at the origin",
        0.8,
        TextTone::Muted,
    );

    let align_demo = runtime
        .append_child(demos, VStack::new(8.0))
        .expect("demos row exists");
    let align_width = runtime
        .append_child(align_demo, MaxWidth::new(220.0))
        .expect("align demo exists");
    let align_frame = runtime
        .append_child(align_width, FixedHeight::new(88.0))
        .expect("max width exists");
    let align = runtime
        .append_child(align_frame, Align::new(Alignment::End, Alignment::Center))
        .expect("fixed frame exists");
    let aligned = runtime
        .append_child(
            align,
            DrawenBox::new()
                .with_background(Color::rgba8(251, 191, 36, 255))
                .with_corner_radius(8.0),
        )
        .expect("align exists");
    runtime
        .append_child(aligned, SizedBox::new(Some(40.0), Some(40.0)))
        .expect("aligned box exists");
    append_label(
        runtime,
        align_demo,
        "Align · end / center",
        0.8,
        TextTone::Muted,
    );

    let padding_demo = runtime
        .append_child(demos, VStack::new(8.0))
        .expect("demos row exists");
    let outer = runtime
        .append_child(
            padding_demo,
            DrawenBox::new()
                .with_background(Color::rgba8(128, 140, 160, 60))
                .with_corner_radius(8.0),
        )
        .expect("padding demo exists");
    let padded = runtime
        .append_child(outer, Padding::new(Insets::all(14.0)))
        .expect("outer box exists");
    let inner = runtime
        .append_child(
            padded,
            DrawenBox::new()
                .with_background(Color::rgba8(94, 234, 212, 255))
                .with_corner_radius(6.0),
        )
        .expect("padding exists");
    runtime
        .append_child(inner, SizedBox::new(Some(72.0), Some(40.0)))
        .expect("inner box exists");
    append_label(
        runtime,
        padding_demo,
        "Padding · insets around a child",
        0.8,
        TextTone::Muted,
    );

    let scroll_card = append_card(
        runtime,
        stack,
        "Horizontal scrolling",
        "ScrollView::horizontal() gives its child unbounded width; the Row below is wider than the page, so the wheel and drag scroll it.",
    );
    let scroll_frame = runtime
        .append_child(scroll_card, FixedHeight::new(150.0))
        .expect("scroll card exists");
    let scroll = runtime
        .append_child(scroll_frame, ScrollView::horizontal())
        .expect("scroll frame exists");
    let tiles = runtime
        .append_child(scroll, Row::new())
        .expect("horizontal scroll exists");
    for index in 0..16 {
        let tile = runtime
            .append_child(tiles, Padding::new(Insets::new(6.0, 12.0, 6.0, 0.0)))
            .expect("tiles row exists");
        let column = runtime
            .append_child(tile, VStack::new(6.0))
            .expect("tile exists");
        runtime
            .append_child(column, Image::with_size(showcase_texture(), 88.0, 88.0))
            .expect("tile column exists");
        append_label(
            runtime,
            column,
            &format!("Tile {:02}", index + 1),
            0.8,
            TextTone::Muted,
        );
    }
}

// ---------------------------------------------------------------------------
// Page content: dock workspace
// ---------------------------------------------------------------------------

fn append_workspace_page(runtime: &mut UiRuntime, tabs: drawen::WidgetId) {
    let page = runtime
        .append_child(tabs, WorkspacePage)
        .expect("tab bar exists");
    let layout = Rc::new(RefCell::new(
        WorkspaceLayout::new(LayoutNode::split(
            DockAxis::Horizontal,
            0.25,
            LayoutNode::Panel {
                id: PanelId::from("navigator"),
            },
            LayoutNode::tabs(vec![PanelId::from("scene"), PanelId::from("inspector")]),
        ))
        .expect("showcase workspace layout is valid"),
    ));
    let mut dock = DockArea::new(layout);
    dock.register_panel(PanelId::from("navigator"))
        .expect("panel is unique");
    dock.register_panel(PanelId::from("scene"))
        .expect("panel is unique");
    dock.register_panel(PanelId::from("inspector"))
        .expect("panel is unique");
    let dock = runtime
        .append_child(page, dock)
        .expect("workspace page exists");
    for panel in [
        PanelCard::new(
            "Navigator",
            "DockArea projects the stable WorkspaceLayout model onto retained children.",
            0.05,
        ),
        PanelCard::new(
            "Scene",
            "Switch tabs and drag the splitter between groups.",
            0.10,
        ),
        PanelCard::new(
            "Inspector",
            "DockArea keeps the UI separate from the serializable panel model.",
            0.15,
        ),
    ] {
        runtime.append_child(dock, panel).expect("dock area exists");
    }
}

struct WorkspacePage;

impl Widget for WorkspacePage {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let size = available_size(constraints);
        let inset = PAGE_PADDING
            .min(size.width() / 12.0)
            .min(size.height() / 12.0);
        let mut children = Vec::new();
        if context.child_count() == 1 {
            let (id, _) = context
                .layout_child(
                    0,
                    tight(
                        (size.width() - inset * 2.0).max(0.0),
                        (size.height() - inset * 2.0).max(0.0),
                    ),
                )
                .expect("workspace dock exists");
            children.push(ChildLayout::new(id, Point::new(inset, inset)));
        }
        LayoutResult::with_children(size, children)
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        context.fill_rect(bounds, environment.theme().background());
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }

    fn clips_children(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Page content: media
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
#[cfg_attr(not(feature = "media"), allow(unused_variables, clippy::ptr_arg))]
fn append_media_page(
    runtime: &mut UiRuntime,
    subscriptions: &mut Vec<Subscription>,
    #[cfg(feature = "media")] media_import_requested: &Rc<Cell<Option<MediaImport>>>,
    tabs: drawen::WidgetId,
) -> (
    drawen::WidgetId,
    drawen::WidgetId,
    drawen::WidgetId,
    drawen::WidgetId,
) {
    let stack = append_scroll_page(runtime, tabs);

    #[cfg(feature = "media")]
    {
        let anim_card = append_card(
            runtime,
            stack,
            "AnimatedImage",
            "Frame-based animation on the media clock: each frame is a RasterImage with a duration. from_gif() decodes real GIF bytes; this loop is generated procedurally.",
        );
        let anim_row = runtime
            .append_child(anim_card, HStack::new(18.0))
            .expect("animated image card exists");
        runtime
            .append_child(
                anim_row,
                AnimatedImage::new(animation_frames())
                    .with_size(120.0, 120.0)
                    .with_label("Procedural 12-frame loop"),
            )
            .expect("animated row exists");
        append_label(
            runtime,
            anim_row,
            "The animation advances through UiRuntime::tick and freezes while the window is hidden or the page is offscreen — no forced frames.",
            0.9,
            TextTone::Muted,
        );

        let media_card = append_card(
            runtime,
            stack,
            "Media players",
            "Video and AudioPlayer expose configurable native controls. These previews use local frames and manual clocks, so they need no decoder or external media file.",
        );
        let video_frame = runtime
            .append_child(media_card, FixedHeight::new(210.0))
            .expect("media card exists");
        let media_preview = runtime
            .append_child(
                video_frame,
                Video::with_frame(procedural_texture(0.35))
                    .with_title("Drawen release overview")
                    .with_subtitle("Manual frame · minimal overlay")
                    .with_duration(Duration::from_secs(215))
                    .with_position(Duration::from_secs(74))
                    .with_control_set(VideoControls::minimal())
                    .with_behavior(PlayerBehavior::new().with_auto_hide(None))
                    .with_theme(PlayerTheme::new().with_accent(Color::rgba8(94, 234, 212, 255))),
            )
            .expect("video frame exists");
        let full_frame = runtime
            .append_child(media_card, FixedHeight::new(210.0))
            .expect("media card exists");
        runtime
            .append_child(
                full_frame,
                Video::with_frame(showcase_texture())
                    .with_title("Full control set")
                    .with_subtitle("VideoControls::new() · volume, rate, fullscreen")
                    .with_duration(Duration::from_secs(180))
                    .with_position(Duration::from_secs(42))
                    .with_control_set(VideoControls::new()),
            )
            .expect("full-control frame exists");
        let import_video = Button::new();
        subscriptions.push(import_video.activated().subscribe({
            let media_import_requested = Rc::clone(media_import_requested);
            move |()| media_import_requested.set(Some(MediaImport::Video))
        }));
        let import_video = runtime
            .append_child(media_card, import_video)
            .expect("media card exists");
        append_label(
            runtime,
            import_video,
            "Import video with audio…",
            0.96,
            TextTone::Primary,
        );
        append_label(
            runtime,
            media_card,
            "Run with `--features audio` to play the chosen video's audio track. The video player then follows the audio-mastered clock.",
            0.86,
            TextTone::Muted,
        );
        let audio_preview = runtime
            .append_child(
                media_card,
                AudioPlayer::new()
                    .with_title("Night drive")
                    .with_subtitle("AudioPlayer · inline layout")
                    .with_duration(Duration::from_secs(214))
                    .with_position(Duration::from_secs(86))
                    .with_volume(0.7)
                    .with_cover(procedural_texture(0.6)),
            )
            .expect("media card exists");
        let import_audio = Button::new();
        subscriptions.push(import_audio.activated().subscribe({
            let media_import_requested = Rc::clone(media_import_requested);
            move |()| media_import_requested.set(Some(MediaImport::Audio))
        }));
        let import_audio = runtime
            .append_child(media_card, import_audio)
            .expect("media card exists");
        append_label(
            runtime,
            import_audio,
            "Import audio…",
            0.96,
            TextTone::Primary,
        );
        runtime
            .append_child(
                media_card,
                AudioPlayer::new()
                    .with_title("Compact queue item")
                    .with_duration(Duration::from_secs(182))
                    .with_position(Duration::from_secs(35))
                    .with_layout(AudioPlayerLayout::Compact),
            )
            .expect("media card exists");

        (video_frame, media_preview, media_card, audio_preview)
    }
    #[cfg(not(feature = "media"))]
    {
        let card = append_card(
            runtime,
            stack,
            "Media",
            "Media playback is disabled in this build. Enable Drawen's `media` feature for Video, AudioPlayer, and AnimatedImage; add `audio` for audio-mastered playback.",
        );
        append_label(
            runtime,
            card,
            "cargo run -p drawen --example style_showcase --features audio",
            0.92,
            TextTone::Muted,
        );
        (stack, stack, stack, stack)
    }
}

// ---------------------------------------------------------------------------
// Page content: software 3D
// ---------------------------------------------------------------------------

#[cfg(feature = "cubex")]
fn append_cubex_page(
    runtime: &mut UiRuntime,
    subscriptions: &mut Vec<Subscription>,
    start_requested: &Rc<Cell<bool>>,
    tabs: drawen::WidgetId,
) -> drawen::WidgetId {
    let stack = append_scroll_page(runtime, tabs);
    let card = append_card(
        runtime,
        stack,
        "CubeX · software 3D",
        "Experimental CPU renderer. It rasterizes an indexed scene into a cached image and can noticeably load the system while rotating.",
    );
    runtime
        .append_child(
            card,
            Chips::new(&[
                "Deterministic rasterizer",
                "CPU rendering",
                "Starts on demand",
                "Cached frame",
            ]),
        )
        .expect("cubex card exists");

    append_label(
        runtime,
        card,
        "The scene and its CPU buffers are not created until you choose to run this interactive example.",
        0.88,
        TextTone::Muted,
    );
    let start = Button::new();
    subscriptions.push(start.activated().subscribe({
        let start_requested = Rc::clone(start_requested);
        move |()| start_requested.set(true)
    }));
    let start = runtime
        .append_child(card, start)
        .expect("cubex card exists");
    append_label(
        runtime,
        start,
        "Run interactive 3D example",
        0.96,
        TextTone::Primary,
    );
    runtime
        .append_child(card, VStack::new(0.0))
        .expect("cubex card exists")
}

#[cfg(feature = "cubex")]
fn cubex_demo_view() -> CubeXView {
    let mut scene = Scene::new()
        .with_background(SceneColor::from_srgb8(16, 18, 24, 255))
        .with_ambient(SceneColor::linear_rgb(0.12, 0.13, 0.16));
    scene.add_light(
        DirectionalLight::new(Vec3::new(-0.4, 0.9, 0.6), SceneColor::WHITE).with_intensity(1.1),
    );
    scene.add(
        primitives::plane(12.0),
        Transform::from_translation(Vec3::new(0.0, -1.2, 0.0)),
        Material::new(SceneColor::from_srgb8(48, 54, 66, 255)),
    );
    scene.add(
        primitives::cube(1.4),
        Transform::from_rotation(Quat::from_axis_angle(Vec3::Y, 0.5)),
        Material::new(SceneColor::from_srgb8(230, 126, 48, 255)),
    );
    scene.add(
        primitives::sphere(0.7, 32, 16),
        Transform::from_translation(Vec3::new(1.8, -0.5, 0.8)),
        Material::new(SceneColor::from_srgb8(80, 150, 230, 255)),
    );
    scene.add(
        primitives::cube(0.6),
        Transform::from_translation(Vec3::new(-1.9, -0.9, 0.6)),
        Material::new(SceneColor::from_srgb8(94, 234, 212, 255)),
    );

    CubeXView::new(
        scene,
        Camera::orbiting(Vec3::new(0.0, 0.0, 0.0), 5.5, 0.0, 0.28),
    )
    .with_resolution(1_360, 470)
    .with_auto_rotate(0.35)
    .with_label("Rotating CubeX scene")
}

#[cfg(not(feature = "cubex"))]
fn append_cubex_page(
    runtime: &mut UiRuntime,
    _: &mut Vec<Subscription>,
    tabs: drawen::WidgetId,
) -> drawen::WidgetId {
    let stack = append_scroll_page(runtime, tabs);
    let card = append_card(
        runtime,
        stack,
        "CubeX · software 3D",
        "The cubex feature is disabled in this build. Enable Drawen's `cubex` feature to present deterministic software-rasterized 3D scenes through CubeXView.",
    );
    append_label(
        runtime,
        card,
        "cargo run -p drawen --example style_showcase --features cubex",
        0.92,
        TextTone::Muted,
    )
}

// ---------------------------------------------------------------------------
// Page content: DSS
// ---------------------------------------------------------------------------

fn append_dss_page(runtime: &mut UiRuntime, tabs: drawen::WidgetId) {
    let stack = append_scroll_page(runtime, tabs);

    let card = append_card(
        runtime,
        stack,
        "Drawen Style Sheets",
        "This isolated sample is styled by SHOWCASE_DSS. The rest of the lab continues to use the live semantic theme, so the two styling layers stay easy to compare.",
    );
    append_label(
        runtime,
        card,
        "Custom properties · calc() · modern rgb()/hsl() · descendant and child combinators · structural and semantic states · !important",
        0.9,
        TextTone::Muted,
    );

    let sample = runtime
        .append_child(card, Styled::new(DrawenBox::new()).with_class("dss-sample"))
        .expect("DSS card exists");
    let direct_child = runtime
        .append_child(sample, DrawenBox::new())
        .expect("DSS sample exists");
    let controls = runtime
        .append_child(direct_child, VStack::new(8.0))
        .expect("DSS direct child exists");
    let buttons = runtime
        .append_child(controls, VStack::new(8.0))
        .expect("DSS controls exist");

    append_dss_button(runtime, buttons, "First child: :not(.emphasis)", None);
    append_dss_button(
        runtime,
        buttons,
        "Second child: :nth-child(2) + :is(.emphasis)",
        Some("emphasis"),
    );
    append_dss_button(runtime, buttons, "Last child: !important wins", None);

    let checkbox = runtime
        .append_child(controls, Checkbox::with_checked(true))
        .expect("DSS controls exist");
    runtime
        .append_child(
            checkbox,
            Text::styled(
                "Toggle me: checkbox:checked changes the accent",
                TextStyle::new(BASE_FONT_SIZE, Color::WHITE),
            ),
        )
        .expect("DSS checkbox exists");

    let explanation = append_card(
        runtime,
        stack,
        "What to inspect",
        "The outer panel obtains its background, border, radius, and padding from `box.dss-sample`. Its direct child demonstrates `>`. The buttons are descendants: the middle rule wins through :nth-child and :is(), while the final purple background wins because it is !important. Toggle the checkbox to observe the semantic :checked selector.",
    );
    append_label(
        runtime,
        explanation,
        "All custom properties are global stylesheet tokens by design; undefined tokens and invalid expressions fail when parsing the sheet rather than being ignored at paint time.",
        0.9,
        TextTone::Muted,
    );
}

fn append_dss_button(
    runtime: &mut UiRuntime,
    parent: drawen::WidgetId,
    label: &str,
    class: Option<&str>,
) {
    let button = match class {
        Some(class) => Styled::new(Button::new()).with_class(class),
        None => Styled::new(Button::new()),
    };
    let button = runtime
        .append_child(parent, button)
        .expect("DSS controls exist");
    runtime
        .append_child(
            button,
            Text::styled(label, TextStyle::new(BASE_FONT_SIZE, Color::WHITE)),
        )
        .expect("DSS button exists");
}

// ---------------------------------------------------------------------------
// Page content: overlays
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
fn append_overlay_page(
    runtime: &mut UiRuntime,
    subscriptions: &mut Vec<Subscription>,
    dirty: &Rc<Cell<bool>>,
    toast_queue: &Rc<RefCell<Vec<ToastTone>>>,
    tabs: drawen::WidgetId,
) -> ToastHandle {
    let stack = append_scroll_page(runtime, tabs);
    let card = append_card(
        runtime,
        stack,
        "Floating surfaces",
        "Tooltips, menus, dialogs, palettes, and toast notifications use the runtime overlay layer: they float above siblings without changing the parent layout.",
    );

    let controls = runtime
        .append_child(card, HStack::new(10.0))
        .expect("overlay card exists");
    let tooltip = runtime
        .append_child(controls, Tooltip::new("A delayed floating hint"))
        .expect("overlay controls exist");
    let tooltip_button = runtime
        .append_child(tooltip, Button::new())
        .expect("tooltip exists");
    append_label(
        runtime,
        tooltip_button,
        "Hover for tooltip",
        0.94,
        TextTone::Primary,
    );

    let popover = runtime
        .append_child(
            controls,
            Popover::new().with_trigger(PopoverTrigger::Primary),
        )
        .expect("overlay controls exist");
    let menu_button = runtime
        .append_child(popover, Button::new())
        .expect("popover exists");
    append_label(runtime, menu_button, "Open menu", 0.94, TextTone::Primary);
    let menu = Menu::new([
        MenuEntry::item("Duplicate").with_shortcut("Ctrl+D"),
        MenuEntry::item("Rename").with_shortcut("F2"),
        MenuEntry::separator(),
        MenuEntry::item("Archive").with_shortcut("Del"),
    ]);
    subscriptions.push(
        menu.selected()
            .subscribe(|index| println!("Popover menu selected row {index}.")),
    );
    runtime.append_child(popover, menu).expect("popover exists");

    let toast_card = append_card(
        runtime,
        stack,
        "Toast notifications",
        "ToastHost stacks transient notifications in a window corner. Each tone resolves a semantic theme color; the queue is drained on the UI thread after the input event.",
    );
    let toast_host = ToastHost::new();
    let toast_handle = toast_host.handle();
    let tones = runtime
        .append_child(toast_card, HStack::new(10.0))
        .expect("toast card exists");
    for (tone, label) in [
        (ToastTone::Info, "Info toast"),
        (ToastTone::Success, "Success toast"),
        (ToastTone::Warning, "Warning toast"),
        (ToastTone::Danger, "Danger toast"),
    ] {
        let button = Button::new();
        subscriptions.push(button.activated().subscribe({
            let toast_queue = Rc::clone(toast_queue);
            move |()| toast_queue.borrow_mut().push(tone)
        }));
        let button = runtime
            .append_child(tones, button)
            .expect("toast tone row exists");
        append_label(runtime, button, label, 0.94, TextTone::Primary);
    }
    let clear = Button::new();
    subscriptions.push(clear.activated().subscribe({
        let toast_handle = toast_handle.clone();
        move |()| toast_handle.clear()
    }));
    let clear = runtime
        .append_child(tones, clear)
        .expect("toast tone row exists");
    append_label(runtime, clear, "Clear all", 0.94, TextTone::Primary);

    let corner_card = append_card(
        runtime,
        stack,
        "Toast corners",
        "ToastHost::with_corner picks any window corner. This second host stacks to the top-left; its handle shows a longer-lived toast directly.",
    );
    let corner_host = ToastHost::new().with_corner(drawen::ToastCorner::TopLeft);
    let corner_handle = corner_host.handle();
    let corner_button = Button::new();
    subscriptions.push(corner_button.activated().subscribe({
        let corner_handle = corner_handle.clone();
        move |()| {
            let _ = corner_handle.show(
                Toast::new("Top-left corner · 8 s lifetime")
                    .with_tone(ToastTone::Success)
                    .with_duration(Duration::from_secs(8)),
            );
        }
    }));
    let corner_button = runtime
        .append_child(corner_card, corner_button)
        .expect("corner card exists");
    append_label(
        runtime,
        corner_button,
        "Show top-left toast",
        0.94,
        TextTone::Primary,
    );

    let modal_card = append_card(
        runtime,
        stack,
        "Modal dialog and command palette",
        "Both surfaces trap sequential focus and close through the overlay layer on Escape or a scrim click.",
    );

    let dialog_modal = Modal::new();
    let dialog_handle = dialog_modal.handle();
    let dialog_modal = runtime
        .append_child(modal_card, dialog_modal)
        .expect("modal card exists");
    let dialog = runtime
        .append_child(dialog_modal, drawen::Dialog::new())
        .expect("modal exists");
    let dialog_content = runtime
        .append_child(dialog, VStack::new(10.0))
        .expect("dialog exists");
    append_label(
        runtime,
        dialog_content,
        "Discard draft?",
        1.18,
        TextTone::Primary,
    );
    append_label(
        runtime,
        dialog_content,
        "This dialog is centered by the runtime and receives keyboard focus before the underlying page.",
        0.9,
        TextTone::Muted,
    );
    let close_dialog = Button::new();
    subscriptions.push(close_dialog.activated().subscribe({
        let dialog_handle = dialog_handle.clone();
        let dirty = Rc::clone(dirty);
        move |()| {
            dialog_handle.close();
            dirty.set(true);
        }
    }));
    let close_dialog = runtime
        .append_child(dialog_content, close_dialog)
        .expect("dialog exists");
    append_label(
        runtime,
        close_dialog,
        "Keep editing",
        0.92,
        TextTone::Primary,
    );

    let open_dialog = Button::new();
    subscriptions.push(open_dialog.activated().subscribe({
        let dialog_handle = dialog_handle.clone();
        let dirty = Rc::clone(dirty);
        move |()| {
            dialog_handle.open();
            dirty.set(true);
        }
    }));
    let open_dialog = runtime
        .append_child(modal_card, open_dialog)
        .expect("modal card exists");
    append_label(runtime, open_dialog, "Open dialog", 0.94, TextTone::Primary);

    let palette_modal = Modal::new();
    let palette_handle = palette_modal.handle();
    let palette_modal = runtime
        .append_child(modal_card, palette_modal)
        .expect("modal card exists");
    let palette = CommandPalette::new([
        PaletteCommand::new("Open component gallery").with_hint("Ctrl+O"),
        PaletteCommand::new("Save appearance preset").with_hint("Ctrl+S"),
        PaletteCommand::new("Toggle reduced motion"),
    ]);
    subscriptions.push(
        palette
            .executed()
            .subscribe(|index| println!("Command palette executed command {index}.")),
    );
    runtime
        .append_child(palette_modal, palette)
        .expect("palette modal exists");
    let open_palette = Button::new();
    subscriptions.push(open_palette.activated().subscribe({
        let palette_handle = palette_handle.clone();
        let dirty = Rc::clone(dirty);
        move |()| {
            palette_handle.open();
            dirty.set(true);
        }
    }));
    let open_palette = runtime
        .append_child(modal_card, open_palette)
        .expect("modal card exists");
    append_label(
        runtime,
        open_palette,
        "Open command palette",
        0.94,
        TextTone::Primary,
    );

    let split_card = append_card(
        runtime,
        stack,
        "Split button",
        "The main segment runs the default action; the arrow opens a floating Menu child.",
    );
    let split = SplitButton::new("Save");
    subscriptions.push(split.activated().subscribe(|()| println!("Saved draft.")));
    let split = runtime
        .append_child(split_card, split)
        .expect("split card exists");
    let split_menu = Menu::new([
        MenuEntry::item("Save as…").with_shortcut("Ctrl+Shift+S"),
        MenuEntry::item("Save all"),
    ]);
    subscriptions.push(
        split_menu
            .selected()
            .subscribe(|index| println!("Split button selected action {index}.")),
    );
    runtime
        .append_child(split, split_menu)
        .expect("split button exists");

    runtime
        .append_child(stack, toast_host)
        .expect("overlay stack exists");
    runtime
        .append_child(stack, corner_host)
        .expect("overlay stack exists");
    toast_handle
}

// ---------------------------------------------------------------------------
// Page content: runtime, tasks, diagnostics
// ---------------------------------------------------------------------------

fn append_runtime_page(
    runtime: &mut UiRuntime,
    subscriptions: &mut Vec<Subscription>,
    task_demo: &Rc<TaskDemo>,
    diagnostics: &Rc<RefCell<String>>,
    bindings: &mut Vec<Binding>,
    tabs: drawen::WidgetId,
) -> (drawen::WidgetId, drawen::WidgetId) {
    let stack = append_scroll_page(runtime, tabs);

    let task_card = append_card(
        runtime,
        stack,
        "Background tasks",
        "TaskPool runs the job on a worker thread; a UiSender delivers progress and the result to a UiDispatcher, and the UI thread drains them in window_event. Workers never touch widgets, signals, or subscriptions.",
    );
    let actions = runtime
        .append_child(task_card, HStack::new(10.0))
        .expect("task card exists");
    let run = Button::new();
    subscriptions.push(run.activated().subscribe({
        let task_demo = Rc::clone(task_demo);
        move |()| task_demo.start()
    }));
    let run = runtime
        .append_child(actions, run)
        .expect("task actions exist");
    append_label(runtime, run, "Run checksum job", 0.94, TextTone::Primary);
    let cancel = Button::new();
    subscriptions.push(cancel.activated().subscribe({
        let task_demo = Rc::clone(task_demo);
        move |()| task_demo.cancel()
    }));
    let cancel = runtime
        .append_child(actions, cancel)
        .expect("task actions exist");
    append_label(runtime, cancel, "Cancel", 0.94, TextTone::Primary);

    let bar = ProgressBar::with_progress(0.0);
    bindings.push(runtime.bind(&bar.progress(), &task_demo.progress));
    append_form(
        runtime,
        task_card,
        FormRow::new("Progress").with_description(
            "Bound to the task's progress Property — no callbacks in the widget.",
        ),
        bar,
    );
    let status_frame = runtime
        .append_child(task_card, FixedHeight::new(56.0))
        .expect("task card exists");
    let task_status_id = runtime
        .append_child(status_frame, LiveText::new(Rc::clone(&task_demo.status)))
        .expect("status frame exists");

    let diag_card = append_card(
        runtime,
        stack,
        "Frame diagnostics",
        "UiRuntime::stats reports layout/paint passes, timings, command counts, and cache reuse. The readout below is sampled after every input event and only invalidates its own region when the text changes.",
    );
    let diag_frame = runtime
        .append_child(diag_card, FixedHeight::new(120.0))
        .expect("diagnostics card exists");
    let diagnostics_id = runtime
        .append_child(diag_frame, LiveText::new(Rc::clone(diagnostics)))
        .expect("diagnostics frame exists");

    let damage_card = append_card(
        runtime,
        stack,
        "Frame damage contract",
        "The showcase forwards UiRuntime::frame_damage from WindowApplication::frame_damage, so the backend repaints only the damaged region.",
    );
    for line in [
        "Full — rasterize the whole frame; the conservative default.",
        "Region(rect) — only this logical-pixel region changed; the rest keeps its previous pixels.",
        "Unchanged — the frame is identical; the backend still presents the retained frame.",
        "Local changes use invalidate_paint_for(id) / invalidate_rect(rect); geometry changes invalidate layout.",
    ] {
        append_label(runtime, damage_card, line, 0.9, TextTone::Muted);
    }

    (task_status_id, diagnostics_id)
}

// ---------------------------------------------------------------------------
// Page content: agent bridge
// ---------------------------------------------------------------------------

#[cfg(feature = "agent")]
fn append_agent_page(
    runtime: &mut UiRuntime,
    subscriptions: &mut Vec<Subscription>,
    observation_requested: &Rc<Cell<bool>>,
    report: &Rc<RefCell<String>>,
    tabs: drawen::WidgetId,
) {
    let stack = append_scroll_page(runtime, tabs);
    let card = append_card(
        runtime,
        stack,
        "Semantic UI inspector",
        "drawen-agent reads the same accessibility tree used by assistive technology. It returns owned data, so an LLM can inspect and drive a UI without screenshots or GPU access.",
    );
    let button = Button::new();
    subscriptions.push(button.activated().subscribe({
        let observation_requested = Rc::clone(observation_requested);
        move |()| observation_requested.set(true)
    }));
    let button = runtime
        .append_child(card, button)
        .expect("agent card exists");
    append_label(
        runtime,
        button,
        "Capture semantic tree",
        0.96,
        TextTone::Primary,
    );
    let frame = runtime
        .append_child(card, FixedHeight::new(260.0))
        .expect("agent card exists");
    runtime
        .append_child(frame, LiveText::new(Rc::clone(report)))
        .expect("agent report frame exists");
    append_label(
        runtime,
        card,
        "The report is intentionally compact. Agent::execute can then target a current widget by id, name, or role and receives a fresh snapshot after every action.",
        0.88,
        TextTone::Muted,
    );
}

#[cfg(not(feature = "agent"))]
fn append_agent_page(runtime: &mut UiRuntime, _: &mut Vec<Subscription>, tabs: drawen::WidgetId) {
    let stack = append_scroll_page(runtime, tabs);
    let card = append_card(
        runtime,
        stack,
        "Semantic UI inspector",
        "The agent bridge is opt-in, keeping ordinary applications free of serde and transport dependencies.",
    );
    append_label(
        runtime,
        card,
        "Run `cargo run -p drawen --example style_showcase --features agent` to activate the live semantic-tree inspector. Use `agent-serve` when an external process should use the loopback JSON-lines bridge.",
        0.92,
        TextTone::Muted,
    );
}

// ---------------------------------------------------------------------------
// Page content: Servo browser interop
// ---------------------------------------------------------------------------

fn append_servo_page(runtime: &mut UiRuntime, tabs: drawen::WidgetId) {
    let stack = append_scroll_page(runtime, tabs);
    let card = append_card(
        runtime,
        stack,
        "Servo browser surface",
        "Drawen composites a browser frame through a platform shared texture instead of copying pixels through the CPU.",
    );
    runtime
        .append_child(
            card,
            Chips::new(&[
                "Servo producer",
                "SharedTexture",
                "wgpu consumer",
                "BrowserView",
            ]),
        )
        .expect("Servo card exists");
    append_label(
        runtime,
        card,
        "1. The Servo adapter starts a producer frame on BrowserSurface.  2. Servo renders into the native shared texture and signals it ready.  3. Drawen waits, composites BrowserView in the display list, then releases the texture back to Servo.",
        0.92,
        TextTone::Muted,
    );

    #[cfg(feature = "browser")]
    append_label(
        runtime,
        card,
        &format!(
            "This build exposes the platform-selected interop surface as {}. A real browser embedding must supply Servo's native producer context; the showcase intentionally does not fabricate one.",
            std::any::type_name::<drawen::browser::BrowserSurface>(),
        ),
        0.88,
        TextTone::Muted,
    );
    #[cfg(not(feature = "browser"))]
    append_label(
        runtime,
        card,
        "Browser interop is disabled in this build. Enable Drawen's `browser` feature to expose BrowserSurface, SharedTexture, and BrowserView.",
        0.88,
        TextTone::Muted,
    );

    let contract = append_card(
        runtime,
        stack,
        "Frame ownership contract",
        "The hand-off is explicit so producer and consumer never write or sample the texture at the same time.",
    );
    for text in [
        "begin_producer_frame() → Servo owns the native producer texture.",
        "signal_producer_ready() → makes the completed frame available to Drawen.",
        "begin_consumer_frame() → Drawen waits before submitting BrowserView.",
        "finish_consumer_frame() → returns ownership after the final GPU submission.",
    ] {
        append_label(runtime, contract, text, 0.9, TextTone::Primary);
    }
}

/// A theme-tinted information panel used by the splitter and dock areas.
struct PanelCard {
    title: &'static str,
    body: &'static str,
    tint: f32,
    title_layout: Option<TextLayout>,
    body_layout: Option<TextLayout>,
}

impl PanelCard {
    const fn new(title: &'static str, body: &'static str, tint: f32) -> Self {
        Self {
            title,
            body,
            tint,
            title_layout: None,
            body_layout: None,
        }
    }
}

impl Widget for PanelCard {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let environment = context.environment();
        let theme = environment.theme();
        let size = available_size(constraints);
        let wrap = (size.width() - 36.0).max(0.0);
        self.title_layout = Some(layout_text(
            environment,
            self.title,
            1.28,
            theme.foreground(),
            Some(wrap),
        ));
        self.body_layout = Some(layout_text(
            environment,
            self.body,
            0.94,
            theme.muted_foreground(),
            Some(wrap),
        ));
        LayoutResult::new(size)
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        let theme = environment.theme();
        let radius = theme.corner_radius() * 1.2;
        let background = theme.surface().mix(theme.accent(), self.tint);
        context.fill_rounded_rect(bounds, radius, background);
        context.stroke_rounded_rect(bounds, radius, 1.0, theme.border());
        context.fill_rounded_rect(
            Rect::new(
                Point::new(bounds.origin.x + 18.0, bounds.origin.y + 18.0),
                valid_size(52.0, 4.0),
            ),
            2.0,
            theme.accent(),
        );
        if let Some(layout) = &self.title_layout {
            context.draw_text(
                layout.clone(),
                Point::new(bounds.origin.x + 18.0, bounds.origin.y + 32.0),
            );
        }
        if let Some(layout) = &self.body_layout {
            let title_height = self
                .title_layout
                .as_ref()
                .map_or(0.0, |layout| layout.size().height());
            context.draw_text(
                layout.clone(),
                Point::new(
                    bounds.origin.x + 18.0,
                    bounds.origin.y + 34.0 + title_height,
                ),
            );
        }
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

/// A row painted by `ListView`.
struct ListRow {
    index: usize,
}

impl Widget for ListRow {
    fn layout(&mut self, _: &mut LayoutContext<'_>, constraints: Constraints) -> LayoutResult {
        LayoutResult::new(available_size(constraints))
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        let theme = environment.theme();
        let background = if self.index.is_multiple_of(2) {
            theme.surface()
        } else {
            theme.raised_surface()
        };
        context.fill_rect(bounds, background);
        context.fill_rect(
            Rect::new(
                Point::new(
                    bounds.origin.x,
                    bounds.origin.y + bounds.size.height() - 1.0,
                ),
                valid_size(bounds.size.width(), 1.0),
            ),
            theme.border(),
        );
        let text = format!(
            "{:02}    virtualized row · retained tree · display list",
            self.index + 1
        );
        let layout = layout_text(
            environment,
            &text,
            0.96,
            theme.foreground(),
            Some((bounds.size.width() - 36.0).max(0.0)),
        );
        context.draw_text(
            layout.clone(),
            Point::new(
                bounds.origin.x + 18.0,
                bounds.origin.y + (bounds.size.height() - layout.size().height()) / 2.0,
            ),
        );
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

struct StatusBar {
    appearance: Rc<RefCell<Appearance>>,
}

impl StatusBar {
    const fn new(appearance: Rc<RefCell<Appearance>>) -> Self {
        Self { appearance }
    }
}

impl Widget for StatusBar {
    fn layout(&mut self, _: &mut LayoutContext<'_>, constraints: Constraints) -> LayoutResult {
        let max = constraints.max();
        LayoutResult::new(constraints.constrain(valid_size(
            if max.width().is_finite() {
                max.width()
            } else {
                640.0
            },
            STATUS_HEIGHT,
        )))
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        let theme = environment.theme();
        let appearance = *self.appearance.borrow();
        context.fill_rect(bounds, theme.surface());
        context.fill_rect(
            Rect::new(bounds.origin, valid_size(bounds.size.width(), 1.0)),
            theme.border(),
        );
        let left_text = format!(
            "Drawen Style Lab · {} · accent {} · {:.1} px · density {:.2}×",
            appearance.preset.label(),
            accent_name(appearance.accent_hue),
            BASE_FONT_SIZE * appearance.font_scale,
            appearance.spacing_scale
        );
        let right_text = "Tab: focus · Enter: activate";
        let left = layout_text(environment, &left_text, 0.8, theme.muted_foreground(), None);
        let right = layout_text(environment, right_text, 0.8, theme.muted_foreground(), None);
        context.draw_text(
            left.clone(),
            Point::new(
                bounds.origin.x + 14.0,
                bounds.origin.y + (bounds.size.height() - left.size().height()) / 2.0,
            ),
        );
        context.draw_text(
            right.clone(),
            Point::new(
                bounds.origin.x + bounds.size.width() - right.size().width() - 14.0,
                bounds.origin.y + (bounds.size.height() - right.size().height()) / 2.0,
            ),
        );
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

/// Fills a page host with a scrollable stack. The host itself is created up
/// front so tab indices stay stable while content mounts lazily.
fn append_scroll_page(runtime: &mut UiRuntime, page: drawen::WidgetId) -> drawen::WidgetId {
    let scroll = runtime
        .append_child(page, ScrollView::vertical())
        .expect("page host exists");
    runtime
        .append_child(
            scroll,
            VStack::new(18.0).with_padding(Insets::all(PAGE_PADDING)),
        )
        .expect("scroll view exists")
}

fn append_card(
    runtime: &mut UiRuntime,
    parent: drawen::WidgetId,
    title: &str,
    description: &str,
) -> drawen::WidgetId {
    let surface = runtime
        .append_child(parent, SurfaceCard::new(8.0))
        .expect("page stack exists");
    let column = runtime
        .append_child(surface, VStack::new(12.0))
        .expect("card surface exists");
    append_label(runtime, column, title, 1.42, TextTone::Primary);
    append_label(runtime, column, description, 0.92, TextTone::Muted);
    append_divider(runtime, column);
    column
}

fn append_label(
    runtime: &mut UiRuntime,
    parent: drawen::WidgetId,
    text: &str,
    scale: f32,
    tone: TextTone,
) -> drawen::WidgetId {
    runtime
        .append_child(parent, Label::new(text, scale, tone))
        .expect("label parent exists")
}

fn append_divider(runtime: &mut UiRuntime, parent: drawen::WidgetId) {
    runtime
        .append_child(parent, Divider::new())
        .expect("divider parent exists");
}

fn append_form<W>(
    runtime: &mut UiRuntime,
    parent: drawen::WidgetId,
    row: FormRow,
    control: W,
) -> drawen::WidgetId
where
    W: Widget + 'static,
{
    let row = runtime
        .append_child(parent, row)
        .expect("form parent exists");
    runtime.append_child(row, control).expect("form row exists")
}

fn append_button(
    runtime: &mut UiRuntime,
    subscriptions: &mut Vec<Subscription>,
    parent: drawen::WidgetId,
    text: &str,
    message: &'static str,
) {
    let button = Button::new();
    subscriptions.push(
        button
            .activated()
            .subscribe(move |()| println!("{message}")),
    );
    let button = runtime
        .append_child(parent, button)
        .expect("button parent exists");
    append_label(runtime, button, text, 0.98, TextTone::Primary);
}

fn append_tool_button(
    runtime: &mut UiRuntime,
    subscriptions: &mut Vec<Subscription>,
    toolbar: drawen::WidgetId,
    text: &'static str,
    message: &'static str,
) {
    let button = Button::new();
    subscriptions.push(
        button
            .activated()
            .subscribe(move |()| println!("{message}")),
    );
    let button = runtime
        .append_child(toolbar, button)
        .expect("toolbar exists");
    append_label(runtime, button, text, 0.94, TextTone::Primary);
}

fn append_badge(runtime: &mut UiRuntime, parent: drawen::WidgetId, tone: BadgeTone, text: &str) {
    let badge = runtime
        .append_child(parent, Badge::new().with_tone(tone))
        .expect("badge parent exists");
    let label_tone = match tone {
        BadgeTone::Accent | BadgeTone::Success | BadgeTone::Warning | BadgeTone::Danger => {
            TextTone::OnAccent
        }
        BadgeTone::Neutral => TextTone::Primary,
    };
    append_label(runtime, badge, text, 0.78, label_tone);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn layout_text(
    environment: &UiEnvironment,
    text: &str,
    scale: f32,
    color: Color,
    wrap_width: Option<f32>,
) -> TextLayout {
    let mut font = environment.default_font();
    font.set_pixel_size(environment.theme().font_size() * scale);
    environment
        .text_shaper()
        .layout(text, &TextStyle::new_with_font(font, color), wrap_width)
}

fn drawen_logo() -> RasterImage {
    let mut pixels = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            let color = if x == 7 || y == 7 || (x > 3 && x < 12 && y > 3 && y < 12) {
                Color::rgba8(94, 234, 212, 255)
            } else if (x + y) % 3 == 0 {
                Color::rgba8(167, 139, 250, 255)
            } else {
                Color::rgba8(26, 36, 62, 255)
            };
            pixels.extend_from_slice(&[
                color_component(color.red),
                color_component(color.green),
                color_component(color.blue),
                color_component(color.alpha),
            ]);
        }
    }
    RasterImage::new(16, 16, pixels).expect("built-in logo has valid RGBA pixels")
}

/// A procedural 128×128 texture for the image viewer and scroll tiles.
fn showcase_texture() -> RasterImage {
    procedural_texture(0.0)
}

/// A procedural texture whose palette rotates with `phase` (0..1), so media
/// previews and tiles can show distinct frames without external assets.
fn procedural_texture(phase: f32) -> RasterImage {
    const SIZE: usize = 128;
    let mut pixels = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let u = usize_to_f32(x) / 127.0;
            let v = usize_to_f32(y) / 127.0;
            let diagonal = f32::midpoint(u, v);
            let grid = if x % 16 == 0 || y % 16 == 0 {
                0.16
            } else {
                0.0
            };
            let wave = (diagonal + phase).rem_euclid(1.0);
            let color = Color::rgb(
                0.10 + 0.27 * wave + grid,
                0.14 + 0.78 * diagonal * (1.0 - v * 0.4) + grid,
                0.24 + 0.59 * (1.0 - wave) + grid,
            );
            pixels.extend_from_slice(&[
                color_component(color.red),
                color_component(color.green),
                color_component(color.blue),
                255,
            ]);
        }
    }
    RasterImage::new(128, 128, pixels).expect("procedural texture has valid RGBA pixels")
}

/// Twelve procedural frames for the `AnimatedImage` demo loop.
#[cfg(feature = "media")]
fn animation_frames() -> Vec<AnimationFrame> {
    const SIZE: usize = 64;
    (0..12)
        .map(|frame| {
            let angle = usize_to_f32(frame) / 12.0 * std::f32::consts::TAU;
            let center_x = 32.0 + angle.cos() * 18.0;
            let center_y = 32.0 + angle.sin() * 18.0;
            let hue = usize_to_f32(frame) * 30.0;
            let dot = hsl(hue, 0.7, 0.6);
            let mut pixels = Vec::with_capacity(SIZE * SIZE * 4);
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let dx = usize_to_f32(x) - center_x;
                    let dy = usize_to_f32(y) - center_y;
                    let distance = (dx * dx + dy * dy).sqrt();
                    let color = if distance < 9.0 {
                        dot
                    } else if distance < 11.0 {
                        dot.with_alpha(0.4)
                    } else {
                        Color::rgba8(19, 24, 35, 255)
                    };
                    pixels.extend_from_slice(&[
                        color_component(color.red),
                        color_component(color.green),
                        color_component(color.blue),
                        color_component(color.alpha),
                    ]);
                }
            }
            let image =
                RasterImage::new(64, 64, pixels).expect("animation frame has valid RGBA pixels");
            AnimationFrame::new(image, Duration::from_millis(90))
        })
        .collect()
}

fn color_component(value: f32) -> u8 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    }
}

#[cfg(feature = "agent")]
fn compact_agent_report(snapshot: &str) -> String {
    const MAX_LINES: usize = 14;

    let mut lines = snapshot.lines();
    let mut report = lines
        .by_ref()
        .take(MAX_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    if lines.next().is_some() {
        report.push_str("\n… capture a subtree by id for the remaining nodes");
    }
    report
}

fn available_size(constraints: Constraints) -> Size {
    let max = constraints.max();
    let min = constraints.min();
    let width = if max.width().is_finite() {
        max.width()
    } else {
        min.width()
    };
    let height = if max.height().is_finite() {
        max.height()
    } else {
        min.height()
    };
    constraints.constrain(valid_size(width, height))
}

fn tight(width: f32, height: f32) -> Constraints {
    Constraints::tight(valid_size(width, height)).expect("showcase child bounds are valid")
}

fn valid_size(width: f32, height: f32) -> Size {
    Size::new(width.max(0.0), height.max(0.0)).expect("showcase dimensions are valid")
}

fn usize_to_f32(value: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    {
        value as f32
    }
}
