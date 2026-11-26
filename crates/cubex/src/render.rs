//! A deterministic CPU rasterizer that writes sRGBA frames.
//!
//! The renderer is intentionally small: it transforms indexed triangles into
//! view space, clips them against the near and far planes, culls back faces,
//! rasterizes with a depth buffer, and applies one Lambert term per directional
//! light plus ambient light. There is no GPU, no window, and no hidden state
//! beyond the color and depth buffers, so the same scene always produces the
//! same frame on the same platform.

use core::fmt;

use crate::camera::Camera;
use crate::math::{Mat4, Vec3};
use crate::scene::{Color, DirectionalLight, Material, Scene};

const INFINITY: f32 = f32::INFINITY;
const EPSILON: f32 = 1.0e-6;

/// Why a renderer could not be created or resized.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenderError {
    /// A width or height of zero cannot be rendered.
    ZeroDimension,
    /// The dimensions overflow the addressable pixel or byte count.
    TooLarge,
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDimension => formatter.write_str("render dimensions must be non-zero"),
            Self::TooLarge => formatter.write_str("render dimensions are too large"),
        }
    }
}

impl std::error::Error for RenderError {}

/// An owned, unpremultiplied 8-bit sRGBA frame in row-major order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Frame {
    fn allocate(width: u32, height: u32) -> Result<Self, RenderError> {
        if width == 0 || height == 0 {
            return Err(RenderError::ZeroDimension);
        }
        let pixel_count = usize::try_from(u64::from(width) * u64::from(height))
            .map_err(|_| RenderError::TooLarge)?;
        let byte_count = pixel_count.checked_mul(4).ok_or(RenderError::TooLarge)?;
        Ok(Self {
            width,
            height,
            pixels: vec![0; byte_count],
        })
    }

    fn pixel_count(&self) -> usize {
        self.pixels.len() / 4
    }

    /// Returns the frame width in pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the frame height in pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns the sRGBA pixels in row-major order.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Returns one pixel, or `None` when the coordinates are outside the frame.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let width = usize::try_from(self.width).ok()?;
        let index = usize::try_from(y).ok()? * width + usize::try_from(x).ok()?;
        let start = index.checked_mul(4)?;
        let pixel = self.pixels.get(start..start + 4)?;
        Some([pixel[0], pixel[1], pixel[2], pixel[3]])
    }
}

/// A software renderer with reusable color and depth buffers.
///
/// Construct one renderer and reuse it across frames. [`Renderer::render`]
/// clears the buffers, so the renderer keeps no scene history.
#[derive(Clone, Debug)]
pub struct Renderer {
    frame: Frame,
    depth: Vec<f32>,
}

impl Renderer {
    /// Creates a renderer with a `width` by `height` frame.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::ZeroDimension`] for a zero dimension and
    /// [`RenderError::TooLarge`] when the buffer size is not addressable.
    pub fn new(width: u32, height: u32) -> Result<Self, RenderError> {
        let frame = Frame::allocate(width, height)?;
        let depth = vec![INFINITY; frame.pixel_count()];
        Ok(Self { frame, depth })
    }

    /// Resizes the color and depth buffers.
    ///
    /// The previous frame is discarded. When allocation fails the renderer
    /// keeps its current size.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Renderer::new`].
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), RenderError> {
        let frame = Frame::allocate(width, height)?;
        self.depth.clear();
        self.depth.resize(frame.pixel_count(), INFINITY);
        self.frame = frame;
        Ok(())
    }

    /// Returns the current frame size.
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.frame.width, self.frame.height)
    }

    /// Renders `scene` from `camera`.
    ///
    /// The camera's vertical field of view maps to the frame height and the
    /// projection uses the frame aspect ratio.
    pub fn render(&mut self, scene: &Scene, camera: &Camera) {
        let width = self.frame.width;
        let height = self.frame.height;
        #[allow(clippy::cast_precision_loss)]
        let width_f = width as f32;
        #[allow(clippy::cast_precision_loss)]
        let height_f = height as f32;

        let background = scene.background().to_srgb8();
        for pixel in self.frame.pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&background);
        }
        self.depth.fill(INFINITY);

        let aspect = width_f / height_f;
        let view = camera.view_matrix();
        let projection = camera.projection_matrix(aspect);

        for instance in scene.instances() {
            let model = instance.transform().matrix();
            let model_view = view * model;
            let normal_matrix = model.normal_matrix();
            let material = instance.material();

            for triangle in instance.mesh().triangles() {
                let view_vertices = [
                    ViewVertex {
                        position: model_view.transform_point(triangle[0].position).truncate(),
                        normal: normal_matrix.transform_vector(triangle[0].normal),
                    },
                    ViewVertex {
                        position: model_view.transform_point(triangle[1].position).truncate(),
                        normal: normal_matrix.transform_vector(triangle[1].normal),
                    },
                    ViewVertex {
                        position: model_view.transform_point(triangle[2].position).truncate(),
                        normal: normal_matrix.transform_vector(triangle[2].normal),
                    },
                ];

                let polygon = clip_triangle(view_vertices, camera.near(), camera.far());
                for index in 1..polygon.len.saturating_sub(1) {
                    rasterize(
                        &projection,
                        polygon.vertices[0],
                        polygon.vertices[index],
                        polygon.vertices[index + 1],
                        width_f,
                        height_f,
                        material,
                        scene.ambient(),
                        scene.lights(),
                        &mut self.frame.pixels,
                        &mut self.depth,
                    );
                }
            }
        }
    }

    /// Returns the most recently rendered frame.
    #[must_use]
    pub const fn frame(&self) -> &Frame {
        &self.frame
    }

    /// Consumes the renderer and returns its frame.
    #[must_use]
    pub fn into_frame(self) -> Frame {
        self.frame
    }
}

#[derive(Clone, Copy, Default)]
struct ViewVertex {
    position: Vec3,
    normal: Vec3,
}

#[derive(Clone, Copy)]
struct Polygon {
    vertices: [ViewVertex; 8],
    len: usize,
}

impl Polygon {
    fn from_triangle(vertices: [ViewVertex; 3]) -> Self {
        let mut polygon = Self {
            vertices: [ViewVertex::default(); 8],
            len: 0,
        };
        for vertex in vertices {
            polygon.push(vertex);
        }
        polygon
    }

    fn push(&mut self, vertex: ViewVertex) {
        if let Some(slot) = self.vertices.get_mut(self.len) {
            *slot = vertex;
            self.len += 1;
        }
    }
}

fn clip_triangle(vertices: [ViewVertex; 3], near: f32, far: f32) -> Polygon {
    let polygon = Polygon::from_triangle(vertices);
    let polygon = clip_plane(&polygon, -near, true);
    clip_plane(&polygon, -far, false)
}

fn clip_plane(input: &Polygon, plane_z: f32, keep_below: bool) -> Polygon {
    let mut output = Polygon {
        vertices: [ViewVertex::default(); 8],
        len: 0,
    };
    if input.len < 3 {
        return output;
    }
    for index in 0..input.len {
        let current = input.vertices[index];
        let next = input.vertices[(index + 1) % input.len];
        let current_inside = inside(current.position.z, plane_z, keep_below);
        let next_inside = inside(next.position.z, plane_z, keep_below);
        if current_inside {
            output.push(current);
        }
        if current_inside != next_inside {
            let delta = next.position.z - current.position.z;
            let t = if delta.abs() > EPSILON {
                (plane_z - current.position.z) / delta
            } else {
                0.0
            };
            output.push(ViewVertex {
                position: current.position.lerp(next.position, t),
                normal: current.normal.lerp(next.normal, t),
            });
        }
    }
    output
}

fn inside(value: f32, plane: f32, keep_below: bool) -> bool {
    if keep_below {
        value <= plane
    } else {
        value >= plane
    }
}

#[derive(Clone, Copy, Default)]
struct ScreenVertex {
    x: f32,
    y: f32,
    depth: f32,
    inverse_w: f32,
    normal: Vec3,
}

#[allow(
    clippy::too_many_arguments,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn rasterize(
    projection: &Mat4,
    vertex0: ViewVertex,
    vertex1: ViewVertex,
    vertex2: ViewVertex,
    width: f32,
    height: f32,
    material: Material,
    ambient: Color,
    lights: &[DirectionalLight],
    pixels: &mut [u8],
    depth_buffer: &mut [f32],
) {
    let face = (vertex1.position - vertex0.position).cross(vertex2.position - vertex0.position);
    let facing = face.dot(-vertex0.position);
    if facing.abs() <= EPSILON {
        return;
    }
    let double_sided = material.is_double_sided();
    if facing < 0.0 && !double_sided {
        return;
    }
    let flip_normal = facing < 0.0;

    let a = project(vertex0, projection, width, height);
    let mut b = project(vertex1, projection, width, height);
    let mut c = project(vertex2, projection, width, height);

    let mut area = edge(a.x, a.y, b.x, b.y, c.x, c.y);
    if area.abs() <= EPSILON {
        return;
    }
    if area < 0.0 {
        core::mem::swap(&mut b, &mut c);
        area = -area;
    }

    let min_x = a.x.min(b.x).min(c.x).floor().max(0.0);
    let max_x = a.x.max(b.x).max(c.x).ceil().min(width - 1.0);
    let min_y = a.y.min(b.y).min(c.y).floor().max(0.0);
    let max_y = a.y.max(b.y).max(c.y).ceil().min(height - 1.0);
    if max_x < min_x || max_y < min_y {
        return;
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let start_x = min_x as usize;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let end_x = max_x as usize;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let start_y = min_y as usize;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let end_y = max_y as usize;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let frame_width = width as usize;

    for y in start_y..=end_y {
        let sample_y = y as f32 + 0.5;
        for x in start_x..=end_x {
            let sample_x = x as f32 + 0.5;
            let weight0 = edge(b.x, b.y, c.x, c.y, sample_x, sample_y);
            let weight1 = edge(c.x, c.y, a.x, a.y, sample_x, sample_y);
            let weight2 = edge(a.x, a.y, b.x, b.y, sample_x, sample_y);
            if weight0 < 0.0 || weight1 < 0.0 || weight2 < 0.0 {
                continue;
            }
            let inverse_area = 1.0 / area;
            let barycentric0 = weight0 * inverse_area;
            let barycentric1 = weight1 * inverse_area;
            let barycentric2 = weight2 * inverse_area;
            let depth = barycentric0 * a.depth + barycentric1 * b.depth + barycentric2 * c.depth;
            let index = y * frame_width + x;
            let Some(stored_depth) = depth_buffer.get_mut(index) else {
                continue;
            };
            if depth >= *stored_depth {
                continue;
            }

            let denominator = barycentric0 * a.inverse_w
                + barycentric1 * b.inverse_w
                + barycentric2 * c.inverse_w;
            if denominator.abs() <= EPSILON {
                continue;
            }
            let inverse_denominator = 1.0 / denominator;
            let perspective0 = barycentric0 * a.inverse_w * inverse_denominator;
            let perspective1 = barycentric1 * b.inverse_w * inverse_denominator;
            let perspective2 = barycentric2 * c.inverse_w * inverse_denominator;
            let mut normal =
                a.normal * perspective0 + b.normal * perspective1 + c.normal * perspective2;
            normal = normal.normalize_or_zero();
            if flip_normal {
                normal = -normal;
            }

            let color = shade(material, normal, ambient, lights).to_srgb8();
            let Some(pixel) = pixels.get_mut(index * 4..index * 4 + 4) else {
                continue;
            };
            pixel.copy_from_slice(&color);
            *stored_depth = depth;
        }
    }
}

fn project(vertex: ViewVertex, projection: &Mat4, width: f32, height: f32) -> ScreenVertex {
    let clip = projection.transform_point(vertex.position);
    let inverse_w = 1.0 / clip.w;
    let normalized_x = clip.x * inverse_w;
    let normalized_y = clip.y * inverse_w;
    let normalized_z = clip.z * inverse_w;
    ScreenVertex {
        x: (normalized_x * 0.5 + 0.5) * width,
        y: (0.5 - normalized_y * 0.5) * height,
        depth: normalized_z,
        inverse_w,
        normal: vertex.normal,
    }
}

fn edge(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (bx - ax) * (py - ay) - (by - ay) * (px - ax)
}

fn shade(material: Material, normal: Vec3, ambient: Color, lights: &[DirectionalLight]) -> Color {
    let base = material.base_color();
    if material.is_unlit() {
        return base;
    }
    let mut illumination = Vec3::new(ambient.red, ambient.green, ambient.blue);
    for light in lights {
        let direction = light.direction().normalize_or_zero();
        let lambert = normal.dot(direction).max(0.0);
        let color = light.color();
        illumination +=
            Vec3::new(color.red, color.green, color.blue) * (light.intensity() * lambert);
    }
    Color::linear_rgba(
        base.red * illumination.x,
        base.green * illumination.y,
        base.blue * illumination.z,
        base.alpha,
    )
}

#[cfg(test)]
mod tests {
    use super::{Frame, RenderError, Renderer};
    use crate::camera::Camera;
    use crate::math::{Transform, Vec3};
    use crate::primitives;
    use crate::scene::{Color, Material, Scene};

    fn background() -> Color {
        Color::from_srgb8(10, 20, 30, 255)
    }

    fn unlit(red: u8, green: u8, blue: u8) -> Material {
        Material::new(Color::from_srgb8(red, green, blue, 255)).with_unlit(true)
    }

    fn render(scene: &Scene, camera: &Camera, size: u32) -> Renderer {
        let mut renderer = Renderer::new(size, size).expect("a valid test renderer");
        renderer.render(scene, camera);
        renderer
    }

    #[test]
    fn rejects_zero_dimensions() {
        assert_eq!(Renderer::new(0, 10).err(), Some(RenderError::ZeroDimension));
        assert_eq!(Renderer::new(10, 0).err(), Some(RenderError::ZeroDimension));
    }

    #[test]
    fn empty_scene_clears_to_the_background() {
        let scene = Scene::new().with_background(background());
        let renderer = render(&scene, &Camera::default(), 4);
        let expected = background().to_srgb8();
        for pixel in renderer.frame().pixels().chunks_exact(4) {
            assert_eq!(pixel, expected);
        }
    }

    #[test]
    fn near_instance_occludes_a_far_one_regardless_of_order() {
        let camera = Camera::new(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO);

        let mut scene = Scene::new();
        scene.add(
            primitives::cube(1.0),
            Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
            unlit(255, 0, 0),
        );
        scene.add(
            primitives::cube(1.0),
            Transform::from_translation(Vec3::new(0.0, 0.0, -2.0)),
            unlit(0, 255, 0),
        );
        let renderer = render(&scene, &camera, 32);
        assert_eq!(renderer.frame().pixel(16, 16), Some([255, 0, 0, 255]));

        let mut reversed = Scene::new();
        reversed.add(
            primitives::cube(1.0),
            Transform::from_translation(Vec3::new(0.0, 0.0, -2.0)),
            unlit(0, 255, 0),
        );
        reversed.add(
            primitives::cube(1.0),
            Transform::from_translation(Vec3::ZERO),
            unlit(255, 0, 0),
        );
        let renderer = render(&reversed, &camera, 32);
        assert_eq!(renderer.frame().pixel(16, 16), Some([255, 0, 0, 255]));
    }

    #[test]
    fn back_faces_are_culled_unless_the_material_is_double_sided() {
        let camera = Camera::new(Vec3::new(0.0, -3.0, 0.0), Vec3::ZERO);
        let mut scene = Scene::new().with_background(background());
        scene.add(
            primitives::plane(10.0),
            Transform::IDENTITY,
            unlit(200, 100, 50),
        );

        let culled = render(&scene, &camera, 16);
        assert_eq!(culled.frame().pixel(8, 8), Some(background().to_srgb8()));

        let mut double_sided = Scene::new().with_background(background());
        double_sided.add(
            primitives::plane(10.0),
            Transform::IDENTITY,
            unlit(200, 100, 50).with_double_sided(true),
        );
        let visible = render(&double_sided, &camera, 16);
        assert_eq!(visible.frame().pixel(8, 8), Some([200, 100, 50, 255]));
    }

    #[test]
    fn lighting_is_brighter_toward_the_key_light() {
        let camera = Camera::new(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO);
        let mut scene = Scene::new()
            .with_background(background())
            .with_ambient(Color::linear_rgb(0.05, 0.05, 0.05));
        scene.clear_lights();
        scene.add_light(crate::scene::DirectionalLight::new(
            Vec3::new(0.0, 0.0, 1.0),
            Color::WHITE,
        ));
        scene.add(
            primitives::cube(1.0),
            Transform::IDENTITY,
            Material::new(Color::WHITE),
        );
        let renderer = render(&scene, &camera, 32);
        let center = renderer.frame().pixel(16, 16).expect("center is inside");
        assert!(center[0] > 200, "the front face should be lit: {center:?}");
    }

    #[test]
    fn rendering_is_deterministic() {
        let camera = Camera::new(Vec3::new(2.0, 2.0, 4.0), Vec3::ZERO);
        let mut scene = Scene::new();
        scene.add(
            primitives::sphere(1.0, 16, 8),
            Transform::IDENTITY,
            Material::new(Color::from_srgb8(220, 120, 40, 255)),
        );
        let first = render(&scene, &camera, 48);
        let second = render(&scene, &camera, 48);
        assert_eq!(first.frame(), second.frame());
    }

    #[test]
    fn clipping_survives_a_camera_inside_the_geometry() {
        let camera = Camera::new(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0));
        let mut scene = Scene::new();
        scene.add(
            primitives::cube(4.0),
            Transform::IDENTITY,
            Material::new(Color::WHITE).with_double_sided(true),
        );
        let renderer = render(&scene, &camera, 32);
        assert_eq!(renderer.frame().width(), 32);
    }

    #[test]
    fn resize_preserves_the_requested_size() {
        let mut renderer = Renderer::new(4, 3).expect("valid renderer");
        assert_eq!(renderer.size(), (4, 3));
        renderer.resize(8, 6).expect("valid resize");
        assert_eq!(renderer.size(), (8, 6));
        assert_eq!(renderer.frame().pixels().len(), 8 * 6 * 4);
    }

    #[test]
    fn frame_pixel_access_is_bounds_checked() {
        let frame = Frame::allocate(2, 1).expect("valid frame");
        assert_eq!(frame.pixel(0, 0), Some([0, 0, 0, 0]));
        assert_eq!(frame.pixel(2, 0), None);
        assert_eq!(frame.pixel(0, 1), None);
    }
}
