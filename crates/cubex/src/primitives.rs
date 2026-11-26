//! Built-in meshes: a cube, a ground plane, and a UV sphere.

use core::f32::consts::{PI, TAU};

use crate::math::{Vec2, Vec3};
use crate::mesh::{Mesh, Vertex};

/// Creates an axis-aligned cube centered at the origin.
///
/// The cube has separate vertices per face, so its normals stay flat.
///
/// # Panics
///
/// Panics when `size` is not finite and positive.
#[must_use]
pub fn cube(size: f32) -> Mesh {
    assert!(
        size.is_finite() && size > 0.0,
        "cube size must be finite and positive"
    );
    let half = size * 0.5;
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    let faces = [
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
        (Vec3::X, Vec3::NEG_Z, Vec3::Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::NEG_Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
    ];
    for (normal, right, up) in faces {
        let center = normal * half;
        push_quad(
            &mut vertices,
            &mut indices,
            [
                center - right * half - up * half,
                center + right * half - up * half,
                center + right * half + up * half,
                center - right * half + up * half,
            ],
            normal,
        );
    }
    Mesh::new(vertices, indices).expect("cube indices are generated in bounds")
}

/// Creates a square plane on the XZ axis facing positive Y.
///
/// # Panics
///
/// Panics when `size` is not finite and positive.
#[must_use]
pub fn plane(size: f32) -> Mesh {
    assert!(
        size.is_finite() && size > 0.0,
        "plane size must be finite and positive"
    );
    let half = size * 0.5;
    let mut vertices = Vec::with_capacity(4);
    let mut indices = Vec::with_capacity(6);
    push_quad(
        &mut vertices,
        &mut indices,
        [
            Vec3::new(-half, 0.0, half),
            Vec3::new(half, 0.0, half),
            Vec3::new(half, 0.0, -half),
            Vec3::new(-half, 0.0, -half),
        ],
        Vec3::Y,
    );
    Mesh::new(vertices, indices).expect("plane indices are generated in bounds")
}

/// Creates a UV sphere centered at the origin.
///
/// `segments` is the number of divisions around the Y axis and `rings` is the
/// number of divisions from pole to pole.
///
/// # Panics
///
/// Panics when `radius` is not finite and positive, when `segments` is less
/// than three, or when `rings` is less than two.
#[must_use]
pub fn sphere(radius: f32, segments: u32, rings: u32) -> Mesh {
    assert!(
        radius.is_finite() && radius > 0.0,
        "sphere radius must be finite and positive"
    );
    assert!(segments >= 3, "a sphere needs at least three segments");
    assert!(rings >= 2, "a sphere needs at least two rings");

    let row = segments + 1;
    let mut vertices = Vec::with_capacity(
        usize::try_from((rings + 1) * row).expect("sphere vertex count fits in memory"),
    );
    for ring in 0..=rings {
        #[allow(clippy::cast_precision_loss)]
        let vertical = ring as f32 / rings as f32;
        let theta = vertical * PI;
        let (sin_theta, cos_theta) = theta.sin_cos();
        for segment in 0..=segments {
            #[allow(clippy::cast_precision_loss)]
            let horizontal = segment as f32 / segments as f32;
            let phi = horizontal * TAU;
            let (sin_phi, cos_phi) = phi.sin_cos();
            let normal = Vec3::new(sin_theta * cos_phi, cos_theta, sin_theta * sin_phi);
            vertices.push(
                Vertex::new(normal * radius, normal).with_uv(Vec2::new(horizontal, vertical)),
            );
        }
    }

    let mut indices = Vec::with_capacity(
        usize::try_from(rings * segments * 6).expect("sphere index count fits in memory"),
    );
    for ring in 0..rings {
        for segment in 0..segments {
            let top_left = ring * row + segment;
            let top_right = top_left + 1;
            let bottom_left = top_left + row;
            let bottom_right = bottom_left + 1;
            if ring != 0 {
                indices.extend_from_slice(&[top_left, top_right, bottom_left]);
            }
            if ring + 1 != rings {
                indices.extend_from_slice(&[top_right, bottom_right, bottom_left]);
            }
        }
    }

    Mesh::new(vertices, indices).expect("sphere indices are generated in bounds")
}

fn push_quad(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>, corners: [Vec3; 4], normal: Vec3) {
    let base = u32::try_from(vertices.len()).expect("mesh vertex count fits in a u32");
    for (corner, uv) in corners.into_iter().zip([
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ]) {
        vertices.push(Vertex::new(corner, normal).with_uv(uv));
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

#[cfg(test)]
mod tests {
    use super::{cube, plane, sphere};
    use crate::math::Vec3;

    #[test]
    fn cube_has_flat_faces_and_centered_bounds() {
        let mesh = cube(2.0);
        assert_eq!(mesh.vertex_count(), 24);
        assert_eq!(mesh.triangle_count(), 12);
        assert_eq!(
            mesh.bounds(),
            crate::mesh::Bounds::new(Vec3::splat(-1.0), Vec3::splat(1.0))
        );
    }

    #[test]
    fn cube_triangles_face_outward() {
        let mesh = cube(1.0);
        for triangle in mesh.triangles() {
            let face = (triangle[1].position - triangle[0].position)
                .cross(triangle[2].position - triangle[0].position);
            assert!(
                face.dot(triangle[0].normal) > 0.0,
                "a cube triangle wound inward"
            );
        }
    }

    #[test]
    fn plane_faces_positive_y() {
        let mesh = plane(4.0);
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.triangle_count(), 2);
        for triangle in mesh.triangles() {
            let face = (triangle[1].position - triangle[0].position)
                .cross(triangle[2].position - triangle[0].position);
            assert!(face.dot(Vec3::Y) > 0.0);
        }
    }

    #[test]
    fn sphere_normals_are_unit_length() {
        let mesh = sphere(2.0, 12, 8);
        assert_eq!(mesh.triangle_count(), 12 * 8 * 2 - 12 * 2);
        for vertex in mesh.vertices() {
            assert!((vertex.normal.length() - 1.0).abs() <= 1.0e-5);
            assert!((vertex.position.length() - 2.0).abs() <= 1.0e-5);
        }
    }

    #[test]
    fn sphere_triangles_face_outward() {
        let mesh = sphere(1.0, 10, 6);
        for triangle in mesh.triangles() {
            let face = (triangle[1].position - triangle[0].position)
                .cross(triangle[2].position - triangle[0].position);
            let center = (triangle[0].position + triangle[1].position + triangle[2].position) / 3.0;
            assert!(face.dot(center) > 0.0, "a sphere triangle wound inward");
        }
    }
}
