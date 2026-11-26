//! Triangle meshes, vertices, bounds, and validation.

use core::fmt;

use crate::math::{Transform, Vec2, Vec3};

/// A single mesh vertex.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vertex {
    /// Position in the mesh's local space.
    pub position: Vec3,
    /// Unit normal in the mesh's local space.
    pub normal: Vec3,
    /// Texture coordinate. The software rasterizer does not sample textures
    /// yet, but the attribute is carried through the pipeline for later
    /// backends.
    pub uv: Vec2,
}

impl Vertex {
    /// Creates a vertex with a position and normal and a zero texture
    /// coordinate.
    #[must_use]
    pub const fn new(position: Vec3, normal: Vec3) -> Self {
        Self {
            position,
            normal,
            uv: Vec2::ZERO,
        }
    }

    /// Returns this vertex with a texture coordinate.
    #[must_use]
    pub const fn with_uv(mut self, uv: Vec2) -> Self {
        self.uv = uv;
        self
    }

    /// Returns whether every attribute is finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.position.is_finite() && self.normal.is_finite() && self.uv.is_finite()
    }
}

/// An axis-aligned bounding box in local space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    /// Minimum corner.
    pub min: Vec3,
    /// Maximum corner.
    pub max: Vec3,
}

impl Bounds {
    /// Creates bounds from two corners.
    #[must_use]
    pub const fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    /// Returns the center of the box.
    #[must_use]
    pub fn center(self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Returns the size of the box on each axis.
    #[must_use]
    pub fn size(self) -> Vec3 {
        self.max - self.min
    }

    /// Returns whether `point` is inside the box.
    #[must_use]
    pub fn contains(self, point: Vec3) -> bool {
        point.x >= self.min.x
            && point.y >= self.min.y
            && point.z >= self.min.z
            && point.x <= self.max.x
            && point.y <= self.max.y
            && point.z <= self.max.z
    }
}

/// Why constructing a [`Mesh`] failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MeshError {
    /// The mesh has no vertices.
    Empty,
    /// The index buffer is not a whole number of triangles.
    IndexCountNotMultipleOfThree {
        /// Number of supplied indices.
        count: usize,
    },
    /// An index points past the end of the vertex buffer.
    IndexOutOfBounds {
        /// The invalid index.
        index: u32,
        /// Number of vertices in the mesh.
        vertex_count: usize,
    },
    /// A vertex attribute is not finite.
    NonFiniteVertex {
        /// Index of the invalid vertex.
        index: usize,
    },
}

impl fmt::Display for MeshError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("a mesh needs at least one vertex"),
            Self::IndexCountNotMultipleOfThree { count } => {
                write!(
                    formatter,
                    "mesh index count {count} is not a multiple of three"
                )
            }
            Self::IndexOutOfBounds {
                index,
                vertex_count,
            } => write!(
                formatter,
                "mesh index {index} is outside the {vertex_count} vertices"
            ),
            Self::NonFiniteVertex { index } => {
                write!(
                    formatter,
                    "mesh vertex {index} contains a non-finite attribute"
                )
            }
        }
    }
}

impl std::error::Error for MeshError {}

/// An indexed triangle mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}

impl Mesh {
    /// Creates a mesh from vertices and triangle indices.
    ///
    /// Every index must refer to a vertex, the index count must be a multiple
    /// of three, and every vertex attribute must be finite.
    ///
    /// # Errors
    ///
    /// Returns [`MeshError`] when the buffers violate those invariants.
    pub fn new(vertices: Vec<Vertex>, indices: Vec<u32>) -> Result<Self, MeshError> {
        if vertices.is_empty() {
            return Err(MeshError::Empty);
        }
        if !indices.len().is_multiple_of(3) {
            return Err(MeshError::IndexCountNotMultipleOfThree {
                count: indices.len(),
            });
        }
        let vertex_count = vertices.len();
        for (index, vertex) in vertices.iter().enumerate() {
            if !vertex.is_finite() {
                return Err(MeshError::NonFiniteVertex { index });
            }
        }
        for &index in &indices {
            let in_bounds = usize::try_from(index).is_ok_and(|index| index < vertex_count);
            if !in_bounds {
                return Err(MeshError::IndexOutOfBounds {
                    index,
                    vertex_count,
                });
            }
        }
        Ok(Self { vertices, indices })
    }

    /// Returns the vertices.
    #[must_use]
    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    /// Returns the triangle indices.
    #[must_use]
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Returns the number of triangles.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Returns the number of vertices.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Returns the local-space bounding box.
    ///
    /// The box is always valid because a mesh has at least one vertex.
    #[must_use]
    pub fn bounds(&self) -> Bounds {
        let mut min = self.vertices[0].position;
        let mut max = min;
        for vertex in &self.vertices[1..] {
            min = min.min(vertex.position);
            max = max.max(vertex.position);
        }
        Bounds::new(min, max)
    }

    /// Iterates over triangles, copying each vertex.
    #[must_use]
    pub fn triangles(&self) -> Triangles<'_> {
        Triangles {
            mesh: self,
            offset: 0,
        }
    }

    /// Returns a copy transformed by `transform`.
    ///
    /// Normals use the inverse transpose of the transform's upper-left matrix,
    /// so non-uniform scale stays correct.
    #[must_use]
    pub fn transformed(&self, transform: Transform) -> Self {
        let matrix = transform.matrix();
        let normal_matrix = matrix.normal_matrix();
        let vertices = self
            .vertices
            .iter()
            .map(|vertex| Vertex {
                position: matrix.transform_point(vertex.position).truncate(),
                normal: normal_matrix
                    .transform_vector(vertex.normal)
                    .normalize_or_zero(),
                uv: vertex.uv,
            })
            .collect();
        Self {
            vertices,
            indices: self.indices.clone(),
        }
    }

    /// Replaces every normal with the area-weighted average of its adjacent
    /// triangle normals.
    ///
    /// Degenerate triangles contribute no normal. Vertices that end up without
    /// a usable normal keep a zero normal, which the renderer treats as
    /// unlit-only.
    pub fn compute_normals(&mut self) {
        for vertex in &mut self.vertices {
            vertex.normal = Vec3::ZERO;
        }
        for offset in (0..self.indices.len()).step_by(3) {
            let Some(&index0) = self.indices.get(offset) else {
                break;
            };
            let Some(&index1) = self.indices.get(offset + 1) else {
                break;
            };
            let Some(&index2) = self.indices.get(offset + 2) else {
                break;
            };
            let Some(index0) = usize::try_from(index0).ok() else {
                continue;
            };
            let Some(index1) = usize::try_from(index1).ok() else {
                continue;
            };
            let Some(index2) = usize::try_from(index2).ok() else {
                continue;
            };
            let (Some(vertex0), Some(vertex1), Some(vertex2)) = (
                self.vertices.get(index0),
                self.vertices.get(index1),
                self.vertices.get(index2),
            ) else {
                continue;
            };
            let normal =
                (vertex1.position - vertex0.position).cross(vertex2.position - vertex0.position);
            if normal.length_squared() <= f32::EPSILON {
                continue;
            }
            for index in [index0, index1, index2] {
                if let Some(vertex) = self.vertices.get_mut(index) {
                    vertex.normal += normal;
                }
            }
        }
        for vertex in &mut self.vertices {
            vertex.normal = vertex.normal.normalize_or_zero();
        }
    }
}

/// An iterator over copied mesh triangles.
#[derive(Clone, Debug)]
pub struct Triangles<'a> {
    mesh: &'a Mesh,
    offset: usize,
}

impl Iterator for Triangles<'_> {
    type Item = [Vertex; 3];

    fn next(&mut self) -> Option<Self::Item> {
        let indices = self.mesh.indices.get(self.offset..self.offset + 3)?;
        let mut vertices = [Vertex::default(); 3];
        for (slot, &index) in vertices.iter_mut().zip(indices) {
            *slot = *self.mesh.vertices.get(usize::try_from(index).ok()?)?;
        }
        self.offset += 3;
        Some(vertices)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.mesh.triangle_count().saturating_sub(self.offset / 3);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Triangles<'_> {}

#[cfg(test)]
mod tests {
    use super::{Bounds, Mesh, MeshError, Vertex};
    use crate::math::{Quat, Transform, Vec2, Vec3};
    use core::f32::consts::FRAC_PI_2;

    fn triangle() -> Mesh {
        Mesh::new(
            vec![
                Vertex::new(Vec3::new(0.0, 0.0, 0.0), Vec3::Z),
                Vertex::new(Vec3::new(1.0, 0.0, 0.0), Vec3::Z),
                Vertex::new(Vec3::new(0.0, 1.0, 0.0), Vec3::Z),
            ],
            vec![0, 1, 2],
        )
        .expect("a valid triangle")
    }

    #[test]
    fn rejects_an_empty_mesh() {
        assert_eq!(Mesh::new(Vec::new(), Vec::new()), Err(MeshError::Empty));
    }

    #[test]
    fn rejects_incomplete_triangles_and_bad_indices() {
        let vertices = vec![Vertex::new(Vec3::ZERO, Vec3::Y)];
        assert_eq!(
            Mesh::new(vertices.clone(), vec![0, 0]),
            Err(MeshError::IndexCountNotMultipleOfThree { count: 2 })
        );
        assert_eq!(
            Mesh::new(vertices, vec![0, 1, 0]),
            Err(MeshError::IndexOutOfBounds {
                index: 1,
                vertex_count: 1,
            })
        );
    }

    #[test]
    fn rejects_non_finite_vertices() {
        let vertices = vec![Vertex::new(Vec3::new(f32::NAN, 0.0, 0.0), Vec3::Y)];
        assert_eq!(
            Mesh::new(vertices, vec![0, 0, 0]),
            Err(MeshError::NonFiniteVertex { index: 0 })
        );
    }

    #[test]
    fn iterates_triangles_in_index_order() {
        let mesh = triangle();
        let triangles: Vec<_> = mesh.triangles().collect();
        assert_eq!(triangles.len(), 1);
        assert_eq!(triangles[0][0].position, Vec3::ZERO);
        assert_eq!(triangles[0][1].position, Vec3::X);
        assert_eq!(triangles[0][2].position, Vec3::Y);
    }

    #[test]
    fn bounds_cover_every_vertex() {
        let mesh = triangle();
        assert_eq!(
            mesh.bounds(),
            Bounds::new(Vec3::ZERO, Vec3::new(1.0, 1.0, 0.0))
        );
        assert!(mesh.bounds().contains(Vec3::new(0.25, 0.25, 0.0)));
        assert!(!mesh.bounds().contains(Vec3::new(1.5, 0.0, 0.0)));
    }

    #[test]
    fn transforming_rotates_positions_and_normals() {
        let transformed = triangle().transformed(Transform::from_rotation(Quat::from_axis_angle(
            Vec3::Z,
            FRAC_PI_2,
        )));
        let first = transformed.vertices()[1].position;
        assert!((first.x - 0.0).abs() <= 1.0e-5);
        assert!((first.y - 1.0).abs() <= 1.0e-5);
        assert!((transformed.vertices()[0].normal.z - 1.0).abs() <= 1.0e-5);
    }

    #[test]
    fn computing_normals_rebuilds_a_flat_triangle() {
        let mut mesh = Mesh::new(
            vec![
                Vertex::new(Vec3::ZERO, Vec3::ZERO),
                Vertex::new(Vec3::X, Vec3::ZERO),
                Vertex::new(Vec3::Y, Vec3::ZERO),
            ],
            vec![0, 1, 2],
        )
        .expect("a valid triangle");
        mesh.compute_normals();
        for vertex in mesh.vertices() {
            assert_eq!(vertex.normal, Vec3::Z);
        }
    }

    #[test]
    fn texture_coordinates_survive_validation() {
        let mesh = Mesh::new(
            vec![Vertex::new(Vec3::ZERO, Vec3::Y).with_uv(Vec2::new(0.25, 0.75))],
            vec![],
        )
        .expect("a valid point mesh");
        assert_eq!(mesh.vertices()[0].uv, Vec2::new(0.25, 0.75));
        assert_eq!(mesh.triangle_count(), 0);
    }
}
