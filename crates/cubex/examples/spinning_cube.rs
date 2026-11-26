//! Renders a short orbit around a lit cube and writes a PPM sequence.
//!
//! Run with:
//!
//! ```text
//! cargo run -p cubex --example spinning_cube
//! ```
//!
//! The frames land in `target/cubex-cube/` and need no window, GPU, or image
//! dependency.

use std::error::Error;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use cubex::{
    Camera, Color, DirectionalLight, Frame, Material, Quat, Renderer, Scene, Transform, Vec3,
    primitives,
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;
const FRAMES: u32 = 12;

fn main() -> Result<(), Box<dyn Error>> {
    let output = Path::new("target/cubex-cube");
    fs::create_dir_all(output)?;

    let mut scene = Scene::new()
        .with_background(Color::from_srgb8(18, 20, 26, 255))
        .with_ambient(Color::linear_rgb(0.10, 0.11, 0.14));
    scene.add_light(
        DirectionalLight::new(Vec3::new(0.45, 0.8, 0.55), Color::WHITE).with_intensity(1.0),
    );
    scene.add(
        primitives::plane(8.0),
        Transform::from_translation(Vec3::new(0.0, -1.0, 0.0)),
        Material::new(Color::from_srgb8(52, 58, 70, 255)),
    );

    let mut renderer = Renderer::new(WIDTH, HEIGHT)?;
    for index in 0..FRAMES {
        #[allow(clippy::cast_precision_loss)]
        let phase = index as f32 / FRAMES as f32 * core::f32::consts::TAU;
        let rotation = Quat::from_axis_angle(Vec3::new(0.35, 1.0, 0.0), phase);
        scene.clear_instances();
        scene.add(
            primitives::cube(1.4),
            Transform::from_translation(Vec3::new(0.0, 0.1, 0.0)).with_rotation(rotation),
            Material::new(Color::from_srgb8(224, 122, 48, 255)),
        );

        let camera = Camera::orbiting(Vec3::new(0.0, 0.2, 0.0), 4.5, phase * 0.5, 0.32);
        renderer.render(&scene, &camera);
        let path = output.join(format!("frame-{index:03}.ppm"));
        write_ppm(&path, renderer.frame())?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

fn write_ppm(path: &Path, frame: &Frame) -> Result<(), Box<dyn Error>> {
    let width = usize::try_from(frame.width())?;
    let mut writer = BufWriter::new(File::create(path)?);
    writeln!(writer, "P6")?;
    writeln!(writer, "{} {}", frame.width(), frame.height())?;
    writeln!(writer, "255")?;
    let mut row = Vec::with_capacity(width * 3);
    for y in 0..frame.height() {
        row.clear();
        for x in 0..frame.width() {
            let [red, green, blue, _] = frame.pixel(x, y).expect("the pixel is inside the frame");
            row.extend_from_slice(&[red, green, blue]);
        }
        writer.write_all(&row)?;
    }
    writer.flush()?;
    Ok(())
}
