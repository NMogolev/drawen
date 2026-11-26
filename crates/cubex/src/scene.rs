//! Colors, materials, lights, instances, and the scene container.

use std::sync::Arc;

use crate::math::{Transform, Vec3};
use crate::mesh::Mesh;

/// A linear RGB color with alpha.
///
/// The renderer shades in linear space and converts to sRGB when it writes a
/// frame. [`Color::from_srgb8`] and [`Color::to_srgb8`] are the boundary for
/// colors authored as 8-bit sRGB values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    /// Linear red component.
    pub red: f32,
    /// Linear green component.
    pub green: f32,
    /// Linear blue component.
    pub blue: f32,
    /// Linear alpha component.
    pub alpha: f32,
}

impl Color {
    /// Opaque white.
    pub const WHITE: Self = Self::linear_rgba(1.0, 1.0, 1.0, 1.0);

    /// Opaque black.
    pub const BLACK: Self = Self::linear_rgba(0.0, 0.0, 0.0, 1.0);

    /// Transparent black.
    pub const TRANSPARENT: Self = Self::linear_rgba(0.0, 0.0, 0.0, 0.0);

    /// Creates an opaque linear color.
    #[must_use]
    pub const fn linear_rgb(red: f32, green: f32, blue: f32) -> Self {
        Self::linear_rgba(red, green, blue, 1.0)
    }

    /// Creates a linear color with alpha.
    #[must_use]
    pub const fn linear_rgba(red: f32, green: f32, blue: f32, alpha: f32) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    /// Converts an 8-bit sRGB color to linear components.
    #[must_use]
    pub fn from_srgb8(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red: srgb_to_linear(red),
            green: srgb_to_linear(green),
            blue: srgb_to_linear(blue),
            alpha: f32::from(alpha) / 255.0,
        }
    }

    /// Converts this linear color to unpremultiplied 8-bit sRGB.
    #[must_use]
    pub fn to_srgb8(self) -> [u8; 4] {
        [
            linear_to_srgb(self.red),
            linear_to_srgb(self.green),
            linear_to_srgb(self.blue),
            linear_to_unorm8(self.alpha),
        ]
    }

    /// Returns the component-wise product of two colors.
    #[must_use]
    pub fn multiply(self, other: Self) -> Self {
        Self {
            red: self.red * other.red,
            green: self.green * other.green,
            blue: self.blue * other.blue,
            alpha: self.alpha * other.alpha,
        }
    }

    /// Returns the color with components clamped to `0.0..=1.0`.
    #[must_use]
    pub fn clamped(self) -> Self {
        Self {
            red: self.red.clamp(0.0, 1.0),
            green: self.green.clamp(0.0, 1.0),
            blue: self.blue.clamp(0.0, 1.0),
            alpha: self.alpha.clamp(0.0, 1.0),
        }
    }

    /// Returns whether every component is finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.red.is_finite()
            && self.green.is_finite()
            && self.blue.is_finite()
            && self.alpha.is_finite()
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::WHITE
    }
}

fn srgb_to_linear(channel: u8) -> f32 {
    let value = f32::from(channel) / 255.0;
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    linear_to_unorm8(encoded)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn linear_to_unorm8(value: f32) -> u8 {
    let scaled = (value.clamp(0.0, 1.0) * 255.0).round();
    scaled as u8
}

/// How a surface responds to light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    base_color: Color,
    unlit: bool,
    double_sided: bool,
}

impl Material {
    /// Creates a lit, single-sided material with `base_color`.
    #[must_use]
    pub const fn new(base_color: Color) -> Self {
        Self {
            base_color,
            unlit: false,
            double_sided: false,
        }
    }

    /// Returns this material with lighting enabled or disabled.
    ///
    /// An unlit material writes its base color directly and ignores ambient and
    /// directional light.
    #[must_use]
    pub const fn with_unlit(mut self, unlit: bool) -> Self {
        self.unlit = unlit;
        self
    }

    /// Returns this material with back-face culling enabled or disabled.
    ///
    /// Double-sided materials flip the interpolated normal when the viewer sees
    /// the back side, so both faces receive plausible lighting.
    #[must_use]
    pub const fn with_double_sided(mut self, double_sided: bool) -> Self {
        self.double_sided = double_sided;
        self
    }

    /// Returns the base color.
    #[must_use]
    pub const fn base_color(self) -> Color {
        self.base_color
    }

    /// Returns whether lighting is disabled.
    #[must_use]
    pub const fn is_unlit(self) -> bool {
        self.unlit
    }

    /// Returns whether back faces are rendered.
    #[must_use]
    pub const fn is_double_sided(self) -> bool {
        self.double_sided
    }
}

impl Default for Material {
    fn default() -> Self {
        Self::new(Color::WHITE)
    }
}

/// A directional light, such as the sun.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectionalLight {
    direction: Vec3,
    color: Color,
    intensity: f32,
}

impl DirectionalLight {
    /// Creates a white light with intensity `1.0`.
    ///
    /// `direction` points from the surface toward the light and is normalized
    /// when the renderer uses it.
    #[must_use]
    pub const fn new(direction: Vec3, color: Color) -> Self {
        Self {
            direction,
            color,
            intensity: 1.0,
        }
    }

    /// Returns this light with a new intensity.
    ///
    /// # Panics
    ///
    /// Panics when `intensity` is not finite and non-negative.
    #[must_use]
    pub fn with_intensity(mut self, intensity: f32) -> Self {
        assert!(
            intensity.is_finite() && intensity >= 0.0,
            "light intensity must be finite and non-negative"
        );
        self.intensity = intensity;
        self
    }

    /// Returns the direction toward the light.
    #[must_use]
    pub const fn direction(self) -> Vec3 {
        self.direction
    }

    /// Returns the light color.
    #[must_use]
    pub const fn color(self) -> Color {
        self.color
    }

    /// Returns the light intensity.
    #[must_use]
    pub const fn intensity(self) -> f32 {
        self.intensity
    }
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self::new(Vec3::new(0.45, 0.8, 0.55), Color::WHITE)
    }
}

/// One mesh placed in a scene.
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    mesh: Arc<Mesh>,
    transform: Transform,
    material: Material,
}

impl Instance {
    /// Creates an instance at the identity transform.
    #[must_use]
    pub fn new(mesh: Mesh, material: Material) -> Self {
        Self {
            mesh: Arc::new(mesh),
            transform: Transform::IDENTITY,
            material,
        }
    }

    /// Creates an instance from a shared mesh.
    #[must_use]
    pub fn from_shared(mesh: Arc<Mesh>, material: Material) -> Self {
        Self {
            mesh,
            transform: Transform::IDENTITY,
            material,
        }
    }

    /// Returns this instance with a different transform.
    #[must_use]
    pub const fn with_transform(mut self, transform: Transform) -> Self {
        self.transform = transform;
        self
    }

    /// Returns this instance with a different material.
    #[must_use]
    pub const fn with_material(mut self, material: Material) -> Self {
        self.material = material;
        self
    }

    /// Returns the mesh.
    #[must_use]
    pub fn mesh(&self) -> &Mesh {
        &self.mesh
    }

    /// Returns the shared mesh handle.
    #[must_use]
    pub const fn shared_mesh(&self) -> &Arc<Mesh> {
        &self.mesh
    }

    /// Returns the transform.
    #[must_use]
    pub const fn transform(&self) -> Transform {
        self.transform
    }

    /// Returns the material.
    #[must_use]
    pub const fn material(&self) -> Material {
        self.material
    }

    /// Replaces the transform.
    pub const fn set_transform(&mut self, transform: Transform) {
        self.transform = transform;
    }

    /// Replaces the material.
    pub const fn set_material(&mut self, material: Material) {
        self.material = material;
    }
}

/// A background, ambient light, directional lights, and mesh instances.
///
/// The scene is a flat list for now. Hierarchical nodes, multiple material
/// passes, and skeletal animation can be layered on top without changing the
/// renderer contract: a scene always yields instances with a mesh, a transform,
/// and a material.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    background: Color,
    ambient: Color,
    lights: Vec<DirectionalLight>,
    instances: Vec<Instance>,
}

impl Scene {
    /// Creates an empty scene with a dark background, soft ambient light, and
    /// one default key light.
    #[must_use]
    pub fn new() -> Self {
        Self {
            background: Color::from_srgb8(18, 20, 26, 255),
            ambient: Color::linear_rgb(0.12, 0.13, 0.16),
            lights: vec![DirectionalLight::default()],
            instances: Vec::new(),
        }
    }

    /// Returns this scene with a different background.
    #[must_use]
    pub const fn with_background(mut self, color: Color) -> Self {
        self.background = color;
        self
    }

    /// Returns this scene with a different ambient light.
    #[must_use]
    pub const fn with_ambient(mut self, color: Color) -> Self {
        self.ambient = color;
        self
    }

    /// Returns this scene with one more directional light.
    #[must_use]
    pub fn with_light(mut self, light: DirectionalLight) -> Self {
        self.lights.push(light);
        self
    }

    /// Returns this scene with one more instance.
    #[must_use]
    pub fn with_instance(mut self, instance: Instance) -> Self {
        self.instances.push(instance);
        self
    }

    /// Sets the background color.
    pub const fn set_background(&mut self, color: Color) {
        self.background = color;
    }

    /// Returns the background color.
    #[must_use]
    pub const fn background(&self) -> Color {
        self.background
    }

    /// Sets the ambient light color.
    pub const fn set_ambient(&mut self, color: Color) {
        self.ambient = color;
    }

    /// Returns the ambient light color.
    #[must_use]
    pub const fn ambient(&self) -> Color {
        self.ambient
    }

    /// Appends a directional light.
    pub fn add_light(&mut self, light: DirectionalLight) {
        self.lights.push(light);
    }

    /// Removes every directional light.
    pub fn clear_lights(&mut self) {
        self.lights.clear();
    }

    /// Returns the directional lights.
    #[must_use]
    pub fn lights(&self) -> &[DirectionalLight] {
        &self.lights
    }

    /// Adds a mesh with a transform and material.
    pub fn add(&mut self, mesh: Mesh, transform: Transform, material: Material) {
        self.instances
            .push(Instance::new(mesh, material).with_transform(transform));
    }

    /// Adds a shared mesh with a transform and material.
    pub fn add_shared(&mut self, mesh: Arc<Mesh>, transform: Transform, material: Material) {
        self.instances
            .push(Instance::from_shared(mesh, material).with_transform(transform));
    }

    /// Appends an existing instance.
    pub fn add_instance(&mut self, instance: Instance) {
        self.instances.push(instance);
    }

    /// Removes every instance.
    pub fn clear_instances(&mut self) {
        self.instances.clear();
    }

    /// Returns the instances.
    #[must_use]
    pub fn instances(&self) -> &[Instance] {
        &self.instances
    }

    /// Returns whether the scene contains no instances.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Color, DirectionalLight, Material, Scene};
    use crate::math::{Transform, Vec3};
    use crate::primitives;

    #[test]
    fn srgb_round_trips_through_linear_space() {
        for channel in [0_u8, 32, 64, 128, 200, 255] {
            let color = Color::from_srgb8(channel, channel, channel, channel);
            assert_eq!(color.to_srgb8(), [channel, channel, channel, channel]);
        }
    }

    #[test]
    fn color_clamping_keeps_channels_in_range() {
        let color = Color::linear_rgba(2.0, -1.0, 0.5, 1.5).clamped();
        assert_eq!(color, Color::linear_rgba(1.0, 0.0, 0.5, 1.0));
    }

    #[test]
    fn material_builders_set_shading_flags() {
        let material = Material::new(Color::WHITE)
            .with_unlit(true)
            .with_double_sided(true);
        assert!(material.is_unlit());
        assert!(material.is_double_sided());
        assert_eq!(material.base_color(), Color::WHITE);
    }

    #[test]
    fn new_scene_has_a_key_light_and_dark_background() {
        let scene = Scene::new();
        assert_eq!(scene.lights().len(), 1);
        assert!(scene.is_empty());
        assert!(scene.background().red < 0.01);
    }

    #[test]
    fn instances_can_share_one_mesh() {
        let mesh = std::sync::Arc::new(primitives::cube(1.0));
        let mut scene = Scene::new();
        scene.add_shared(
            mesh.clone(),
            Transform::from_translation(Vec3::new(-1.0, 0.0, 0.0)),
            Material::default(),
        );
        scene.add_shared(
            mesh,
            Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
            Material::default(),
        );
        assert_eq!(scene.instances().len(), 2);
        assert!(std::sync::Arc::ptr_eq(
            scene.instances()[0].shared_mesh(),
            scene.instances()[1].shared_mesh()
        ));
    }

    #[test]
    fn lights_reject_negative_intensity() {
        let result = std::panic::catch_unwind(|| DirectionalLight::default().with_intensity(-1.0));
        assert!(result.is_err());
    }
}
