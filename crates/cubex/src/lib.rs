//! `CubeX` is Drawen's pure-Rust 3D layer.
//!
//! It is not a game engine. It is the 3D counterpart to [Paleo]: a small,
//! deterministic rendering core that Drawen widgets can present, that agents
//! can run headlessly, and that later GPU or scene-graph backends can replace
//! without changing application code.
//!
//! The first slice has four layers:
//!
//! ```text
//! math       Vec2 / Vec3 / Vec4 / Mat3 / Mat4 / Quat / Transform
//!    |
//! mesh       Vertex, Mesh, Bounds, and cube / plane / sphere primitives
//!    |
//! scene      Color, Material, DirectionalLight, Instance, Scene, Camera
//!    |
//! render     Renderer -> Frame (sRGBA, depth-tested, clipped, lit)
//! ```
//!
//! Rendering is CPU-only and deterministic: no window, GPU, or driver is
//! required. The same `Scene` and `Camera` can be rendered in a test, shown by
//! the `CubeXView` widget, or inspected by an agent. The renderer keeps only a
//! color buffer and a depth buffer, and [`Renderer::render`] clears both, so a
//! frame never depends on earlier frames.
//!
//! # Quick start
//!
//! ```
//! use cubex::{Camera, Color, Material, Renderer, Scene, Transform, Vec3, primitives};
//!
//! let mut scene = Scene::new();
//! scene.add(
//!     primitives::cube(1.0),
//!     Transform::IDENTITY,
//!     Material::new(Color::from_srgb8(220, 120, 40, 255)),
//! );
//!
//! let camera = Camera::new(Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO);
//! let mut renderer = Renderer::new(160, 120)?;
//! renderer.render(&scene, &camera);
//!
//! assert_eq!(renderer.frame().pixels().len(), 160 * 120 * 4);
//! # Ok::<(), cubex::RenderError>(())
//! ```
//!
//! # Scope
//!
//! `CubeX` currently renders opaque triangles with a depth buffer, near/far
//! clipping, back-face culling, ambient light, and any number of directional
//! lights. Textures, transparency, shadows, animation, and a hierarchical scene
//! graph are deliberately left for later layers. A game can be built on top,
//! but the crate does not impose a game loop, an entity system, or a window.
//!
//! [Paleo]: https://docs.rs/paleo

#![forbid(unsafe_code)]

mod camera;
mod math;
mod mesh;
pub mod primitives;
mod render;
mod scene;

pub use camera::Camera;
pub use math::{Mat3, Mat4, Quat, Transform, Vec2, Vec3, Vec4};
pub use mesh::{Bounds, Mesh, MeshError, Triangles, Vertex};
pub use render::{Frame, RenderError, Renderer};
pub use scene::{Color, DirectionalLight, Instance, Material, Scene};
