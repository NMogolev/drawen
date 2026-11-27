# CubeX

CubeX is Drawen's pure-Rust 3D layer. It is not a game engine and not a GPU
abstraction: it is the 3D counterpart to [Paleo](PALEO.md), a small deterministic
rendering core that Drawen widgets can present, that tests can render headlessly,
and that a later GPU or scene-graph backend can replace without changing
application code.

The first implementation renders indexed triangle meshes on the CPU:

```text
math       Vec2 / Vec3 / Vec4 / Mat3 / Mat4 / Quat / Transform
   |
mesh       Vertex, Mesh, Bounds, cube / plane / sphere primitives
   |
scene      Color, Material, DirectionalLight, Instance, Scene, Camera
   |
render     Renderer -> Frame (sRGBA, clipped, depth-tested, lit)
   |
widget     CubeXView -> cached RasterImage -> DrawList
```

CubeX lives in `crates/cubex` and has no dependencies. The facade re-exports it
as `drawen::cubex` and re-exports `CubeXView` behind the `cubex` feature, which
is part of the default feature set.

## Quick start

Rendering does not need a window, a GPU, or a driver:

```rust
use drawen::cubex::{
    Camera, Color, Material, Renderer, Scene, Transform, Vec3, primitives,
};

let mut scene = Scene::new();
scene.add(
    primitives::cube(1.0),
    Transform::IDENTITY,
    Material::new(Color::from_srgb8(224, 122, 48, 255)),
);

let camera = Camera::new(Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO);
let mut renderer = Renderer::new(640, 360)?;
renderer.render(&scene, &camera);

// sRGBA, row-major, four bytes per pixel.
let pixels = renderer.frame().pixels();
# Ok::<(), drawen::cubex::RenderError>(())
```

To show the same scene in a window, put a `CubeXView` in a normal widget tree:

```rust
use drawen::{
    Box as DrawenBox, CubeXView, Size, UiRuntime,
    cubex::{Camera, Color as SceneColor, Material, Scene, Transform, Vec3, primitives},
};

let mut scene = Scene::new()
    .with_background(SceneColor::from_srgb8(16, 18, 24, 255));
scene.add(
    primitives::cube(1.4),
    Transform::IDENTITY,
    Material::new(SceneColor::from_srgb8(230, 126, 48, 255)),
);

let view = CubeXView::new(
    scene,
    Camera::new(Vec3::new(3.0, 2.0, 5.0), Vec3::ZERO),
)
.with_resolution(960, 540)
.with_auto_rotate(0.35)
.with_label("Rotating 3D scene");

let mut runtime = UiRuntime::new(DrawenBox::new());
runtime.append_child(runtime.root(), view)?;
runtime.layout(drawen::Constraints::tight(
    Size::new(960.0, 540.0)?,
)?)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The complete native example is
`crates/drawen/examples/cubex_showcase.rs`; the headless PPM example is
`crates/cubex/examples/spinning_cube.rs`.

## Conventions

- Coordinates are right-handed and Y-up. A camera looks down its local negative
  Z axis.
- Clip-space depth is `0.0` at the near plane and `1.0` at the far plane, which
  matches the `wgpu` convention.
- Front faces wind counter-clockwise when viewed from outside. Back faces are
  culled unless a material is double-sided.
- `Color` stores linear RGB. `Color::from_srgb8` and `Color::to_srgb8` are the
  boundary for 8-bit sRGB values; shading happens in linear space.
- `Frame` pixels are unpremultiplied sRGBA, row-major, top-left origin.
- Matrices are column-major and multiply vectors on the right (`matrix * vector`).

## Scene model

A `Scene` is a flat list of `Instance`s plus a background, ambient color, and any
number of `DirectionalLight`s.

- `Instance` owns an `Arc<Mesh>`, a `Transform`, and a `Material`. Use
  `Scene::add` for a mesh value or `Scene::add_shared` to place one mesh many
  times without copying vertices.
- `Material` has a base color and two flags: `with_unlit(true)` writes the base
  color directly, and `with_double_sided(true)` keeps back faces and flips their
  interpolated normal.
- `DirectionalLight::new(direction, color)` points from the surface toward the
  light. Intensity defaults to `1.0`.
- `Camera` is built from a position and target, or with `Camera::orbiting`
  around a target. It exposes the vertical field of view, near and far planes,
  and the view/projection matrices.

Meshes are validated on construction: indices must be in bounds and the index
count must be a multiple of three. `Mesh::compute_normals` derives smooth
per-vertex normals for procedurally generated geometry.

## Renderer

`Renderer` owns a reusable color buffer and a depth buffer. `Renderer::render`
clears both, so a frame never depends on earlier frames.

For each triangle the renderer:

1. transforms positions and normals into view space with the instance transform
   and the camera view matrix;
2. clips against the near and far planes, generating a polygon when needed;
3. culls back faces unless the material is double-sided;
4. projects and rasterizes with a depth test;
5. interpolates normals with perspective correction;
6. applies ambient light plus one Lambert term per directional light.

The renderer is deterministic: the same `Scene` and `Camera` produce the same
frame on the same platform. There is no threading, sampling jitter, or hidden
state.

## The `CubeXView` widget

`CubeXView` renders the scene into an offscreen frame at a fixed resolution and
caches the result as a `RasterImage`. Painting records one image draw, so the
software rasterizer only runs when the scene, camera, or resolution changes.

- `CubeXView::new(scene, camera)` renders at 640 by 360 by default.
- `with_resolution`, `with_fit`, `with_background`, `with_label`, and
  `with_auto_rotate` are chainable builders.
- `update(|scene, camera| ...)` mutates both values and refreshes the cache in
  one step. Use it after the widget is in a runtime.
- Pointer drags orbit the camera around its target, and the wheel zooms between
  0.5 and 50 world units. Auto-rotation pauses while dragging.
- `advance_animation` drives auto-rotation through the normal runtime animation
  path, so hidden windows and reduced-motion handling behave like every other
  visual animation.

The widget never opens a window, spawns a thread, or calls a platform API. It is
UI-thread-only, like every other retained widget.

## Scope and limits

CubeX currently renders opaque triangles only. It has no textures, transparency,
shadows, skeletal animation, instancing fast path, or hierarchical scene graph.
The software rasterizer is tuned for correctness and small interactive scenes,
not for large scenes or high frame rates.

The quality bar is still low: the 3D layer is an early experiment. Flat
Lambert shading without antialiasing, CPU-only rasterization, and none of the
visual polish of the 2D stack put it far behind the rest of Drawen in both
looks and speed. Use it for small deterministic previews and tests, not
production rendering; a later GPU backend is the intended path out.

Those are layer boundaries, not game-engine decisions. A game can be built on
top of CubeX, but the crate deliberately does not provide a game loop, an entity
system, input mapping, physics, or asset loading. Later layers can add a GPU
backend, textures, transparency, and a scene graph without changing the
application-facing contract: a scene yields instances with a mesh, a transform,
and a material.

## Testing

- `cargo test -p cubex` covers math, meshes, primitives, cameras, and the
  rasterizer, including depth ordering, culling, clipping, lighting, and
  determinism.
- `cargo test -p drawen-widgets --features cubex` covers `CubeXView` layout,
  painting, resolution changes, and animation.
- `cargo run -p cubex --example spinning_cube` writes a PPM sequence to
  `target/cubex-cube/` without extra dependencies.
- `cargo run -p drawen --example cubex_showcase` opens the native window.
