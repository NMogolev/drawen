//! A Drawen widget that presents a `CubeX` scene as a cached sRGBA image.

use std::sync::Arc;
use std::time::Duration;

use cubex::{Camera, RenderError, Renderer, Scene};
use drawen_core::{
    Color, Constraints, InputEvent, Point, PointerButton, PointerId, Rect, Size, WheelDelta,
};
use drawen_render::{PaintContext, RasterImage};
use drawen_ui::{
    Accessibility, AccessibilityRole, DssStyle, EventContext, EventPhase, EventStatus,
    LayoutContext, LayoutResult, StyleTarget, UiEnvironment, Widget,
};

use crate::image::{fitted_rect, image_size};
use crate::{Alignment, ImageFit};

const DEFAULT_RENDER_WIDTH: u32 = 640;
const DEFAULT_RENDER_HEIGHT: u32 = 360;
const DEFAULT_LOGICAL_WIDTH: f32 = 480.0;
const DEFAULT_LOGICAL_HEIGHT: f32 = 270.0;
const ORBIT_RADIANS_PER_PIXEL: f32 = 0.01;
const WHEEL_LINE_PIXELS: f32 = 40.0;
const ZOOM_SENSITIVITY: f32 = 0.0015;
const MINIMUM_DISTANCE: f32 = 0.5;
const MAXIMUM_DISTANCE: f32 = 50.0;
const MAXIMUM_PITCH: f32 = core::f32::consts::FRAC_PI_2 - 0.01;

/// A widget that renders a [`Scene`] from a [`Camera`] with `CubeX`.
///
/// The widget renders into an offscreen frame at a fixed resolution and caches
/// the resulting [`RasterImage`]. Painting therefore only records an image draw;
/// rendering happens when the scene, camera, resolution, or orbit changes.
///
/// Pointer drags orbit the camera around its target and the wheel moves it
/// closer or farther away. The orbit controls assume a Y-up camera. Enable
/// [`Self::with_auto_rotate`] for a continuously turning view; the runtime then
/// schedules frames through [`Widget::advance_animation`] and
/// [`Widget::is_animating`].
///
/// The widget never opens a window, spawns a thread, or calls a platform API.
/// It is a normal retained widget, so the application owns the event loop and
/// the scene data.
pub struct CubeXView {
    renderer: Renderer,
    scene: Scene,
    camera: Camera,
    orbit: Orbit,
    image: Arc<RasterImage>,
    fit: ImageFit,
    background: Option<Color>,
    label: Option<String>,
    auto_rotate: Option<f32>,
    dragging: Option<PointerId>,
    drag_origin: Point,
    orbit_origin: Orbit,
}

impl CubeXView {
    /// Creates a view of `scene` from `camera`.
    ///
    /// The default render resolution is 640 by 360 pixels.
    ///
    /// # Panics
    ///
    /// Panics only when the default frame cannot be allocated, which cannot
    /// happen with the constant default dimensions.
    #[must_use]
    pub fn new(scene: Scene, camera: Camera) -> Self {
        let mut renderer = Renderer::new(DEFAULT_RENDER_WIDTH, DEFAULT_RENDER_HEIGHT)
            .expect("the default CubeX frame size is valid");
        renderer.render(&scene, &camera);
        let image = Arc::new(frame_image(renderer.frame()));
        Self {
            renderer,
            scene,
            camera,
            orbit: Orbit::from_camera(&camera),
            image,
            fit: ImageFit::Contain,
            background: None,
            label: None,
            auto_rotate: None,
            dragging: None,
            drag_origin: Point::ZERO,
            orbit_origin: Orbit::from_camera(&camera),
        }
    }

    /// Returns this view with a different offscreen resolution.
    ///
    /// # Panics
    ///
    /// Panics when either dimension is zero or when the frame cannot be
    /// allocated.
    #[must_use]
    pub fn with_resolution(mut self, width: u32, height: u32) -> Self {
        assert!(
            width > 0 && height > 0,
            "CubeX view resolution must be non-zero"
        );
        self.renderer
            .resize(width, height)
            .expect("the requested CubeX resolution is allocatable");
        self.refresh();
        self
    }

    /// Returns this view with a different image fit.
    #[must_use]
    pub const fn with_fit(mut self, fit: ImageFit) -> Self {
        self.fit = fit;
        self
    }

    /// Returns this view with a background painted behind the frame.
    #[must_use]
    pub const fn with_background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// Returns this view with an accessible name.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Returns this view with automatic rotation at `radians_per_second`.
    ///
    /// A non-finite speed is ignored. Automatic rotation pauses while the user
    /// drags the camera.
    #[must_use]
    pub fn with_auto_rotate(mut self, radians_per_second: f32) -> Self {
        if radians_per_second.is_finite() {
            self.auto_rotate = Some(radians_per_second);
        }
        self
    }

    /// Returns the scene.
    #[must_use]
    pub const fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Returns the camera.
    #[must_use]
    pub const fn camera(&self) -> &Camera {
        &self.camera
    }

    /// Returns the cached frame.
    #[must_use]
    pub fn image(&self) -> &RasterImage {
        &self.image
    }

    /// Returns the offscreen frame resolution.
    #[must_use]
    pub const fn resolution(&self) -> (u32, u32) {
        self.renderer.size()
    }

    /// Returns the automatic rotation speed, if enabled.
    #[must_use]
    pub const fn auto_rotate(&self) -> Option<f32> {
        self.auto_rotate
    }

    /// Replaces the scene and re-renders the cached frame.
    pub fn set_scene(&mut self, scene: Scene) {
        self.scene = scene;
        self.refresh();
    }

    /// Replaces the camera and re-renders the cached frame.
    ///
    /// The orbit state is resynchronized from the new camera.
    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
        self.orbit = Orbit::from_camera(&camera);
        self.refresh();
    }

    /// Mutates the scene and camera, then re-renders the cached frame.
    ///
    /// This is the canonical way to change a view after it has been added to a
    /// runtime, because the widget owns both values and the cache must be
    /// rebuilt in the same step.
    pub fn update(&mut self, edit: impl FnOnce(&mut Scene, &mut Camera)) {
        edit(&mut self.scene, &mut self.camera);
        self.orbit = Orbit::from_camera(&self.camera);
        self.refresh();
    }

    /// Changes the offscreen resolution and re-renders the cached frame.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] when a dimension is zero or the frame cannot be
    /// allocated. The previous frame and resolution are kept on failure.
    pub fn set_resolution(&mut self, width: u32, height: u32) -> Result<(), RenderError> {
        self.renderer.resize(width, height)?;
        self.refresh();
        Ok(())
    }

    /// Re-renders the cached frame from the current scene and camera.
    ///
    /// [`Self::update`], [`Self::set_scene`], [`Self::set_camera`], and
    /// [`Self::set_resolution`] call this automatically.
    pub fn refresh(&mut self) {
        self.renderer.render(&self.scene, &self.camera);
        self.image = Arc::new(frame_image(self.renderer.frame()));
    }

    fn apply_orbit(&mut self) {
        let horizontal = self.orbit.pitch.cos();
        let offset = cubex::Vec3::new(
            horizontal * self.orbit.yaw.sin(),
            self.orbit.pitch.sin(),
            horizontal * self.orbit.yaw.cos(),
        ) * self.orbit.distance;
        self.camera = self.camera.with_position(self.orbit.target + offset);
        self.refresh();
    }

    fn zoom(&mut self, delta: f32) {
        if delta.abs() <= f32::EPSILON {
            return;
        }
        let factor = (-delta * ZOOM_SENSITIVITY).exp();
        self.orbit.distance =
            (self.orbit.distance * factor).clamp(MINIMUM_DISTANCE, MAXIMUM_DISTANCE);
        self.apply_orbit();
    }
}

impl Widget for CubeXView {
    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("cubex-view")
    }

    fn layout(&mut self, _: &mut LayoutContext<'_>, constraints: Constraints) -> LayoutResult {
        let maximum = constraints.max();
        let width = if maximum.width().is_finite() {
            maximum.width()
        } else {
            DEFAULT_LOGICAL_WIDTH
        };
        let height = if maximum.height().is_finite() {
            maximum.height()
        } else {
            DEFAULT_LOGICAL_HEIGHT
        };
        LayoutResult::new(
            constraints
                .constrain(Size::new(width, height).expect("CubeX view sizes are non-negative")),
        )
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
        let background = self
            .background
            .or(style.background)
            .unwrap_or_else(|| environment.theme().control_background());
        context.fill_rect(bounds, background);
        if bounds.is_empty() || self.image.width() == 0 || self.image.height() == 0 {
            return;
        }
        let destination = fitted_rect(
            bounds,
            image_size(&self.image),
            self.fit,
            Alignment::Center,
            Alignment::Center,
        );
        if contains(bounds, destination) {
            context.draw_image(self.image.clone(), destination);
        } else {
            context.with_clip(bounds, |context| {
                context.draw_image(self.image.clone(), destination);
            });
        }
    }

    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        if context.phase() != EventPhase::Target {
            return EventStatus::Ignored;
        }
        match event {
            InputEvent::PointerDown(pointer) if pointer.button == Some(PointerButton::Primary) => {
                self.dragging = Some(pointer.pointer_id);
                self.drag_origin = pointer.position;
                self.orbit_origin = self.orbit;
                context.capture_pointer(pointer.pointer_id);
                context.request_redraw();
                EventStatus::Handled
            }
            InputEvent::PointerMove(pointer) if self.dragging == Some(pointer.pointer_id) => {
                let delta_x = pointer.position.x - self.drag_origin.x;
                let delta_y = pointer.position.y - self.drag_origin.y;
                self.orbit.yaw = self.orbit_origin.yaw - delta_x * ORBIT_RADIANS_PER_PIXEL;
                self.orbit.pitch = (self.orbit_origin.pitch + delta_y * ORBIT_RADIANS_PER_PIXEL)
                    .clamp(-MAXIMUM_PITCH, MAXIMUM_PITCH);
                self.apply_orbit();
                context.request_redraw();
                EventStatus::Handled
            }
            InputEvent::PointerUp(pointer) if self.dragging == Some(pointer.pointer_id) => {
                self.dragging = None;
                context.release_pointer(pointer.pointer_id);
                EventStatus::Handled
            }
            InputEvent::PointerCancel(pointer) if self.dragging == Some(pointer.pointer_id) => {
                self.dragging = None;
                EventStatus::Handled
            }
            InputEvent::Wheel(wheel) => {
                let delta = match wheel.delta {
                    WheelDelta::Pixels(delta) => delta.y,
                    WheelDelta::Lines(delta) => delta.y * WHEEL_LINE_PIXELS,
                };
                if delta.abs() <= f32::EPSILON {
                    return EventStatus::Ignored;
                }
                self.zoom(delta);
                context.request_redraw();
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn advance_animation(&mut self, delta: Duration, settle: bool) -> bool {
        let Some(speed) = self.auto_rotate else {
            return false;
        };
        if settle || self.dragging.is_some() || speed.abs() <= f32::EPSILON {
            return false;
        }
        self.orbit.yaw += speed * delta.as_secs_f32();
        self.apply_orbit();
        true
    }

    fn is_animating(&self) -> bool {
        self.dragging.is_none()
            && self
                .auto_rotate
                .is_some_and(|speed| speed.abs() > f32::EPSILON)
    }

    fn accessibility(&self) -> Accessibility {
        Accessibility::new(AccessibilityRole::Image)
            .with_label(self.label.clone().unwrap_or_else(|| "3D scene".to_owned()))
    }
}

fn frame_image(frame: &cubex::Frame) -> RasterImage {
    RasterImage::new(frame.width(), frame.height(), frame.pixels().to_vec())
        .expect("a CubeX frame always contains four bytes per pixel")
}

fn contains(outer: Rect, inner: Rect) -> bool {
    inner.origin.x >= outer.origin.x
        && inner.origin.y >= outer.origin.y
        && inner.origin.x + inner.size.width() <= outer.origin.x + outer.size.width()
        && inner.origin.y + inner.size.height() <= outer.origin.y + outer.size.height()
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Orbit {
    target: cubex::Vec3,
    distance: f32,
    yaw: f32,
    pitch: f32,
}

impl Orbit {
    fn from_camera(camera: &Camera) -> Self {
        let target = camera.target();
        let offset = camera.position() - target;
        let distance = offset.length();
        if !distance.is_finite() || distance <= f32::EPSILON {
            return Self {
                target,
                distance: 1.0,
                yaw: 0.0,
                pitch: 0.0,
            };
        }
        Self {
            target,
            distance,
            yaw: offset.x.atan2(offset.z),
            pitch: (offset.y / distance).clamp(-1.0, 1.0).asin(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CubeXView;
    use cubex::{Camera, Material, Scene, Transform, Vec3, primitives};
    use drawen_core::{Constraints, Size};
    use drawen_render::{DisplayCommand, DisplayList, PaintContext};
    use drawen_ui::{UiRuntime, Widget};
    use std::time::Duration;

    fn scene() -> Scene {
        let mut scene = Scene::new();
        scene.add(
            primitives::cube(1.0),
            Transform::IDENTITY,
            Material::new(cubex::Color::from_srgb8(220, 120, 40, 255)),
        );
        scene
    }

    fn camera() -> Camera {
        Camera::new(Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO)
    }

    #[test]
    fn new_renders_the_default_resolution() {
        let view = CubeXView::new(scene(), camera());
        assert_eq!(view.resolution(), (640, 360));
        assert_eq!(view.image().width(), 640);
        assert_eq!(view.image().height(), 360);
        assert!(view.image().pixels().iter().any(|&channel| channel != 0));
    }

    #[test]
    fn resolution_builder_resizes_the_cached_frame() {
        let view = CubeXView::new(scene(), camera()).with_resolution(160, 90);
        assert_eq!(view.resolution(), (160, 90));
        assert_eq!(view.image().width(), 160);
        assert_eq!(view.image().height(), 90);
    }

    #[test]
    fn layout_fills_the_available_viewport() {
        let view = CubeXView::new(scene(), camera());
        let mut runtime = UiRuntime::new(view);
        let expected = Size::new(100.0, 80.0).expect("valid size");
        let result = runtime
            .layout(Constraints::tight(expected).expect("valid constraints"))
            .expect("CubeX view layout succeeds");
        assert_eq!(result.size(), expected);
    }

    #[test]
    fn paint_records_one_image_draw() {
        let view = CubeXView::new(scene(), camera());
        let mut runtime = UiRuntime::new(view);
        let mut list = DisplayList::new();
        runtime
            .layout(
                Constraints::tight(Size::new(320.0, 180.0).expect("valid size"))
                    .expect("valid constraints"),
            )
            .expect("CubeX view layout succeeds");
        runtime
            .paint(&mut PaintContext::new(&mut list))
            .expect("CubeX view paint succeeds");
        assert!(
            list.commands()
                .iter()
                .any(|command| { matches!(command, DisplayCommand::DrawImage { .. }) })
        );
    }

    #[test]
    fn auto_rotation_advances_the_camera_and_invalidates_paint() {
        let mut view = CubeXView::new(scene(), camera()).with_auto_rotate(1.0);
        let before = *view.camera();
        assert!(view.advance_animation(Duration::from_millis(250), false));
        assert_ne!(*view.camera(), before);
        assert!(!view.advance_animation(Duration::from_millis(250), true));
    }

    #[test]
    fn update_replaces_scene_and_camera_together() {
        let mut view = CubeXView::new(scene(), camera());
        view.update(|scene, camera| {
            scene.clear_instances();
            *camera = Camera::new(Vec3::new(0.0, 1.0, 3.0), Vec3::ZERO);
        });
        assert!(view.scene().is_empty());
        assert_eq!(view.camera().position(), Vec3::new(0.0, 1.0, 3.0));
        let (width, height) = view.resolution();
        assert_eq!(
            view.image().pixels().len(),
            width as usize * height as usize * 4
        );
    }

    #[test]
    fn set_resolution_rejects_zero_dimensions() {
        let mut view = CubeXView::new(scene(), camera());
        assert!(view.set_resolution(0, 10).is_err());
        assert_eq!(view.resolution(), (640, 360));
    }
}
