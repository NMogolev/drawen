//! A native window that presents a rotating `CubeX` scene.
//!
//! Run it with `cargo run -p drawen --example cubex_showcase`. On Linux, an X11
//! or Wayland session must be available to winit.

use drawen::{
    Box as DrawenBox, Constraints, CubeXView, Size, UiRuntime,
    cubex::{
        Camera, Color as SceneColor, DirectionalLight, Material, Quat, Scene, Transform, Vec3,
        primitives,
    },
    platform::{WindowAction, WindowApplication, WindowEvent, WindowOptions, WindowPresentation},
    render::{DisplayList, PaintContext},
};

fn main() -> Result<(), drawen_platform_winit::RunError> {
    drawen_platform_winit::run(CubeXShowcase::new())
}

struct CubeXShowcase {
    runtime: UiRuntime,
    size: Size,
}

impl CubeXShowcase {
    fn new() -> Self {
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

        let view = CubeXView::new(
            scene,
            Camera::orbiting(Vec3::new(0.0, 0.0, 0.0), 5.5, 0.0, 0.28),
        )
        .with_resolution(960, 540)
        .with_auto_rotate(0.35)
        .with_label("Rotating 3D scene");

        let size = Size::new(960.0, 540.0).expect("initial window size is valid");
        let mut runtime = UiRuntime::new(DrawenBox::new());
        runtime
            .append_child(runtime.root(), view)
            .expect("root exists");
        runtime
            .layout(Constraints::tight(size).expect("initial constraints are valid"))
            .expect("the showcase lays out");

        Self { runtime, size }
    }

    fn layout(&mut self, size: Size) {
        self.size = size;
        if let Ok(constraints) = Constraints::tight(size) {
            let _ = self.runtime.layout(constraints);
        }
    }
}

impl WindowApplication for CubeXShowcase {
    fn window_options(&self) -> WindowOptions {
        WindowOptions::new("CubeX", self.size)
    }

    fn window_event(&mut self, event: WindowEvent) -> WindowAction {
        match event {
            WindowEvent::CloseRequested => WindowAction::Exit,
            WindowEvent::Resized(size) => {
                self.layout(size);
                WindowAction::RequestRedraw
            }
            WindowEvent::Input(event) => {
                let _ = self.runtime.dispatch_event(&event);
                if self.runtime.take_redraw_request() {
                    WindowAction::RequestRedraw
                } else {
                    WindowAction::None
                }
            }
            WindowEvent::Visible(_) | WindowEvent::Minimized(_) | WindowEvent::Occluded(_) => {
                WindowAction::None
            }
            WindowEvent::Maximized(_) | WindowEvent::Fullscreen(_) => WindowAction::RequestRedraw,
            WindowEvent::RedrawRequested => WindowAction::None,
        }
    }

    fn presentation_changed(&mut self, presentation: WindowPresentation) -> WindowAction {
        self.runtime.set_presented(presentation.is_presented());
        WindowAction::None
    }

    fn redraw(&mut self) -> DisplayList {
        let mut display_list = DisplayList::new();
        let _ = self
            .runtime
            .paint(&mut PaintContext::new(&mut display_list));
        display_list
    }

    fn tick(&mut self, delta: std::time::Duration) -> bool {
        self.runtime.tick(delta)
    }

    fn next_frame_in(&self) -> Option<std::time::Duration> {
        self.runtime.next_frame_in()
    }
}
