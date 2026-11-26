//! Vector, matrix, quaternion, and transform types used by `CubeX`.
//!
//! The types are deliberately small value types. Matrices are column-major and
//! use the right-handed convention: a camera looks down its local negative Z
//! axis, and clip-space depth is mapped to `0.0..=1.0` (the same convention
//! `wgpu` uses).

use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

/// A small tolerance used when inverting or normalizing a value.
const EPSILON: f32 = 1.0e-6;

/// A two-dimensional vector.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    /// Horizontal component.
    pub x: f32,
    /// Vertical component.
    pub y: f32,
}

impl Vec2 {
    /// The zero vector.
    pub const ZERO: Self = Self::new(0.0, 0.0);

    /// The vector with both components set to one.
    pub const ONE: Self = Self::new(1.0, 1.0);

    /// Creates a vector from its components.
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Creates a vector with both components set to `value`.
    #[must_use]
    pub const fn splat(value: f32) -> Self {
        Self::new(value, value)
    }

    /// Returns the dot product of two vectors.
    #[must_use]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    /// Returns the squared length.
    #[must_use]
    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    /// Returns the length.
    #[must_use]
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Returns a unit vector, or `None` when the vector is zero or non-finite.
    #[must_use]
    pub fn try_normalize(self) -> Option<Self> {
        let length = self.length();
        if length.is_finite() && length > EPSILON {
            Some(Self::new(self.x / length, self.y / length))
        } else {
            None
        }
    }

    /// Returns a unit vector, or the zero vector when normalization is not
    /// possible.
    #[must_use]
    pub fn normalize_or_zero(self) -> Self {
        self.try_normalize().unwrap_or(Self::ZERO)
    }

    /// Linearly interpolates from `self` to `other` by `t`.
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
        )
    }

    /// Returns whether both components are finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Add for Vec2 {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, other: Self) {
        self.x += other.x;
        self.y += other.y;
    }
}

impl Sub for Vec2 {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, other: Self) {
        self.x -= other.x;
        self.y -= other.y;
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;

    fn mul(self, scalar: f32) -> Self {
        Self::new(self.x * scalar, self.y * scalar)
    }
}

impl MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, scalar: f32) {
        self.x *= scalar;
        self.y *= scalar;
    }
}

impl Div<f32> for Vec2 {
    type Output = Self;

    fn div(self, scalar: f32) -> Self {
        Self::new(self.x / scalar, self.y / scalar)
    }
}

impl DivAssign<f32> for Vec2 {
    fn div_assign(&mut self, scalar: f32) {
        self.x /= scalar;
        self.y /= scalar;
    }
}

impl Neg for Vec2 {
    type Output = Self;

    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

/// A three-dimensional vector.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
}

impl Vec3 {
    /// The zero vector.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    /// The vector with every component set to one.
    pub const ONE: Self = Self::new(1.0, 1.0, 1.0);

    /// The positive X unit vector.
    pub const X: Self = Self::new(1.0, 0.0, 0.0);

    /// The positive Y unit vector.
    pub const Y: Self = Self::new(0.0, 1.0, 0.0);

    /// The positive Z unit vector.
    pub const Z: Self = Self::new(0.0, 0.0, 1.0);

    /// The negative X unit vector.
    pub const NEG_X: Self = Self::new(-1.0, 0.0, 0.0);

    /// The negative Y unit vector.
    pub const NEG_Y: Self = Self::new(0.0, -1.0, 0.0);

    /// The negative Z unit vector.
    pub const NEG_Z: Self = Self::new(0.0, 0.0, -1.0);

    /// Creates a vector from its components.
    #[must_use]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Creates a vector with every component set to `value`.
    #[must_use]
    pub const fn splat(value: f32) -> Self {
        Self::new(value, value, value)
    }

    /// Returns the dot product of two vectors.
    #[must_use]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Returns the cross product of two vectors.
    #[must_use]
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Returns the squared length.
    #[must_use]
    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    /// Returns the length.
    #[must_use]
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Returns the distance between two points.
    #[must_use]
    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }

    /// Returns a unit vector, or `None` when the vector is zero or non-finite.
    #[must_use]
    pub fn try_normalize(self) -> Option<Self> {
        let length = self.length();
        if length.is_finite() && length > EPSILON {
            Some(Self::new(self.x / length, self.y / length, self.z / length))
        } else {
            None
        }
    }

    /// Returns a unit vector, or the zero vector when normalization is not
    /// possible.
    #[must_use]
    pub fn normalize_or_zero(self) -> Self {
        self.try_normalize().unwrap_or(Self::ZERO)
    }

    /// Returns the component-wise absolute value.
    #[must_use]
    pub fn abs(self) -> Self {
        Self::new(self.x.abs(), self.y.abs(), self.z.abs())
    }

    /// Returns the component-wise minimum.
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self::new(
            self.x.min(other.x),
            self.y.min(other.y),
            self.z.min(other.z),
        )
    }

    /// Returns the component-wise maximum.
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self::new(
            self.x.max(other.x),
            self.y.max(other.y),
            self.z.max(other.z),
        )
    }

    /// Linearly interpolates from `self` to `other` by `t`.
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
            self.z + (other.z - self.z) * t,
        )
    }

    /// Returns whether every component is finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl Add for Vec3 {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, other: Self) {
        self.x += other.x;
        self.y += other.y;
        self.z += other.z;
    }
}

impl Sub for Vec3 {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl SubAssign for Vec3 {
    fn sub_assign(&mut self, other: Self) {
        self.x -= other.x;
        self.y -= other.y;
        self.z -= other.z;
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;

    fn mul(self, scalar: f32) -> Self {
        Self::new(self.x * scalar, self.y * scalar, self.z * scalar)
    }
}

impl MulAssign<f32> for Vec3 {
    fn mul_assign(&mut self, scalar: f32) {
        self.x *= scalar;
        self.y *= scalar;
        self.z *= scalar;
    }
}

impl Mul for Vec3 {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        Self::new(self.x * other.x, self.y * other.y, self.z * other.z)
    }
}

impl Div<f32> for Vec3 {
    type Output = Self;

    fn div(self, scalar: f32) -> Self {
        Self::new(self.x / scalar, self.y / scalar, self.z / scalar)
    }
}

impl DivAssign<f32> for Vec3 {
    fn div_assign(&mut self, scalar: f32) {
        self.x /= scalar;
        self.y /= scalar;
        self.z /= scalar;
    }
}

impl Neg for Vec3 {
    type Output = Self;

    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

/// A four-dimensional vector, used for homogeneous coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec4 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
    /// W component.
    pub w: f32,
}

impl Vec4 {
    /// The zero vector.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0, 0.0);

    /// Creates a vector from its components.
    #[must_use]
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    /// Creates a vector with every component set to `value`.
    #[must_use]
    pub const fn splat(value: f32) -> Self {
        Self::new(value, value, value, value)
    }

    /// Returns the first three components.
    #[must_use]
    pub const fn truncate(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }

    /// Returns the dot product of two vectors.
    #[must_use]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
    }

    /// Returns whether every component is finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite() && self.w.is_finite()
    }
}

impl Add for Vec4 {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(
            self.x + other.x,
            self.y + other.y,
            self.z + other.z,
            self.w + other.w,
        )
    }
}

impl Sub for Vec4 {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self::new(
            self.x - other.x,
            self.y - other.y,
            self.z - other.z,
            self.w - other.w,
        )
    }
}

impl Mul<f32> for Vec4 {
    type Output = Self;

    fn mul(self, scalar: f32) -> Self {
        Self::new(
            self.x * scalar,
            self.y * scalar,
            self.z * scalar,
            self.w * scalar,
        )
    }
}

impl Neg for Vec4 {
    type Output = Self;

    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, -self.w)
    }
}

/// A three-by-three matrix stored in column-major order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat3 {
    cols: [Vec3; 3],
}

impl Mat3 {
    /// The identity matrix.
    pub const IDENTITY: Self = Self::from_cols(Vec3::X, Vec3::Y, Vec3::Z);

    /// The zero matrix.
    pub const ZERO: Self = Self::from_cols(Vec3::ZERO, Vec3::ZERO, Vec3::ZERO);

    /// Creates a matrix from three column vectors.
    #[must_use]
    pub const fn from_cols(x: Vec3, y: Vec3, z: Vec3) -> Self {
        Self { cols: [x, y, z] }
    }

    /// Returns column `index`.
    ///
    /// # Panics
    ///
    /// Panics when `index` is greater than two.
    #[must_use]
    pub const fn col(self, index: usize) -> Vec3 {
        self.cols[index]
    }

    /// Returns the transpose.
    #[must_use]
    pub fn transpose(self) -> Self {
        Self::from_cols(
            Vec3::new(self.cols[0].x, self.cols[1].x, self.cols[2].x),
            Vec3::new(self.cols[0].y, self.cols[1].y, self.cols[2].y),
            Vec3::new(self.cols[0].z, self.cols[1].z, self.cols[2].z),
        )
    }

    /// Returns the determinant.
    #[must_use]
    pub fn determinant(self) -> f32 {
        self.cols[0].dot(self.cols[1].cross(self.cols[2]))
    }

    /// Returns the inverse, or `None` when the matrix is singular or
    /// non-finite.
    #[must_use]
    pub fn inverse(self) -> Option<Self> {
        let determinant = self.determinant();
        if !determinant.is_finite() || determinant.abs() <= EPSILON {
            return None;
        }
        let inverse_determinant = 1.0 / determinant;
        let row0 = self.cols[1].cross(self.cols[2]) * inverse_determinant;
        let row1 = self.cols[2].cross(self.cols[0]) * inverse_determinant;
        let row2 = self.cols[0].cross(self.cols[1]) * inverse_determinant;
        Some(Self::from_cols(
            Vec3::new(row0.x, row1.x, row2.x),
            Vec3::new(row0.y, row1.y, row2.y),
            Vec3::new(row0.z, row1.z, row2.z),
        ))
    }

    /// Returns the inverse transpose, used to transform normals.
    ///
    /// Returns `None` when the matrix is singular or non-finite.
    #[must_use]
    pub fn inverse_transpose(self) -> Option<Self> {
        self.inverse().map(Self::transpose)
    }

    /// Transforms `vector` as a direction.
    #[must_use]
    pub fn transform_vector(self, vector: Vec3) -> Vec3 {
        self.cols[0] * vector.x + self.cols[1] * vector.y + self.cols[2] * vector.z
    }

    /// Returns whether every component is finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.cols.iter().all(|column| column.is_finite())
    }
}

impl Default for Mat3 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// A four-by-four matrix stored in column-major order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4 {
    cols: [Vec4; 4],
}

impl Mat4 {
    /// The identity matrix.
    pub const IDENTITY: Self = Self::from_cols(
        Vec4::new(1.0, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 1.0, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 1.0, 0.0),
        Vec4::new(0.0, 0.0, 0.0, 1.0),
    );

    /// The zero matrix.
    pub const ZERO: Self = Self::from_cols(Vec4::ZERO, Vec4::ZERO, Vec4::ZERO, Vec4::ZERO);

    /// Creates a matrix from four column vectors.
    #[must_use]
    pub const fn from_cols(x: Vec4, y: Vec4, z: Vec4, w: Vec4) -> Self {
        Self { cols: [x, y, z, w] }
    }

    /// Returns column `index`.
    ///
    /// # Panics
    ///
    /// Panics when `index` is greater than three.
    #[must_use]
    pub const fn col(self, index: usize) -> Vec4 {
        self.cols[index]
    }

    /// Returns row `index`.
    ///
    /// # Panics
    ///
    /// Panics when `index` is greater than three.
    #[must_use]
    pub fn row(self, index: usize) -> Vec4 {
        let [x, y, z, w] = self.cols;
        match index {
            0 => Vec4::new(x.x, y.x, z.x, w.x),
            1 => Vec4::new(x.y, y.y, z.y, w.y),
            2 => Vec4::new(x.z, y.z, z.z, w.z),
            3 => Vec4::new(x.w, y.w, z.w, w.w),
            _ => panic!("matrix row index must be less than four"),
        }
    }

    /// Returns the transpose.
    #[must_use]
    pub fn transpose(self) -> Self {
        Self::from_cols(self.row(0), self.row(1), self.row(2), self.row(3))
    }

    /// Creates a translation matrix.
    #[must_use]
    pub const fn translation(offset: Vec3) -> Self {
        Self::from_cols(
            Vec4::new(1.0, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 1.0, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 1.0, 0.0),
            Vec4::new(offset.x, offset.y, offset.z, 1.0),
        )
    }

    /// Creates a scale matrix.
    #[must_use]
    pub const fn scale(factor: Vec3) -> Self {
        Self::from_cols(
            Vec4::new(factor.x, 0.0, 0.0, 0.0),
            Vec4::new(0.0, factor.y, 0.0, 0.0),
            Vec4::new(0.0, 0.0, factor.z, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        )
    }

    /// Creates a rotation matrix around the X axis.
    #[must_use]
    pub fn rotation_x(radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        Self::from_cols(
            Vec4::new(1.0, 0.0, 0.0, 0.0),
            Vec4::new(0.0, cos, sin, 0.0),
            Vec4::new(0.0, -sin, cos, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        )
    }

    /// Creates a rotation matrix around the Y axis.
    #[must_use]
    pub fn rotation_y(radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        Self::from_cols(
            Vec4::new(cos, 0.0, -sin, 0.0),
            Vec4::new(0.0, 1.0, 0.0, 0.0),
            Vec4::new(sin, 0.0, cos, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        )
    }

    /// Creates a rotation matrix around the Z axis.
    #[must_use]
    pub fn rotation_z(radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        Self::from_cols(
            Vec4::new(cos, sin, 0.0, 0.0),
            Vec4::new(-sin, cos, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 1.0, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        )
    }

    /// Creates a rotation matrix from a unit quaternion.
    #[must_use]
    pub fn from_quat(rotation: Quat) -> Self {
        rotation.to_mat4()
    }

    /// Creates a right-handed perspective projection with clip depth in
    /// `0.0..=1.0`.
    ///
    /// # Panics
    ///
    /// Panics when `vertical_fov` is not in `(0, pi)`, when `aspect` is not
    /// positive, or when the clip planes are not finite and ordered as
    /// `0 < near < far`.
    #[must_use]
    pub fn perspective(vertical_fov: f32, aspect: f32, near: f32, far: f32) -> Self {
        assert!(
            vertical_fov.is_finite() && vertical_fov > 0.0 && vertical_fov < core::f32::consts::PI,
            "perspective field of view must be finite and between zero and pi"
        );
        assert!(
            aspect.is_finite() && aspect > 0.0,
            "perspective aspect ratio must be finite and positive"
        );
        assert!(
            near.is_finite() && far.is_finite() && near > 0.0 && far > near,
            "perspective clip planes must satisfy 0 < near < far"
        );

        let focal = 1.0 / (vertical_fov * 0.5).tan();
        let depth_scale = far / (near - far);
        Self::from_cols(
            Vec4::new(focal / aspect, 0.0, 0.0, 0.0),
            Vec4::new(0.0, focal, 0.0, 0.0),
            Vec4::new(0.0, 0.0, depth_scale, -1.0),
            Vec4::new(0.0, 0.0, near * depth_scale, 0.0),
        )
    }

    /// Creates a right-handed orthographic projection with clip depth in
    /// `0.0..=1.0`.
    ///
    /// # Panics
    ///
    /// Panics when the bounds are not finite or ordered.
    #[must_use]
    pub fn orthographic(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Self {
        assert!(
            left.is_finite()
                && right.is_finite()
                && bottom.is_finite()
                && top.is_finite()
                && left < right
                && bottom < top,
            "orthographic bounds must be finite and ordered"
        );
        assert!(
            near.is_finite() && far.is_finite() && far > near,
            "orthographic clip planes must be finite and ordered"
        );

        let depth_scale = 1.0 / (near - far);
        Self::from_cols(
            Vec4::new(2.0 / (right - left), 0.0, 0.0, 0.0),
            Vec4::new(0.0, 2.0 / (top - bottom), 0.0, 0.0),
            Vec4::new(0.0, 0.0, depth_scale, 0.0),
            Vec4::new(
                -(right + left) / (right - left),
                -(top + bottom) / (top - bottom),
                near * depth_scale,
                1.0,
            ),
        )
    }

    /// Creates a right-handed view matrix looking from `eye` toward `target`.
    ///
    /// When `eye` and `target` coincide, or when `up` is parallel to the view
    /// direction, the matrix falls back to a stable axis instead of producing
    /// non-finite values.
    #[must_use]
    pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Self {
        let forward = (target - eye).try_normalize().unwrap_or(Vec3::NEG_Z);
        let right = forward.cross(up).try_normalize().unwrap_or(Vec3::X);
        let up = right.cross(forward);
        Self::from_cols(
            Vec4::new(right.x, up.x, -forward.x, 0.0),
            Vec4::new(right.y, up.y, -forward.y, 0.0),
            Vec4::new(right.z, up.z, -forward.z, 0.0),
            Vec4::new(-right.dot(eye), -up.dot(eye), forward.dot(eye), 1.0),
        )
    }

    /// Transforms a point and returns the homogeneous result.
    #[must_use]
    pub fn transform_point(self, point: Vec3) -> Vec4 {
        self.transform_vec4(Vec4::new(point.x, point.y, point.z, 1.0))
    }

    /// Transforms a direction, ignoring translation.
    #[must_use]
    pub fn transform_vector(self, vector: Vec3) -> Vec3 {
        self.transform_vec4(Vec4::new(vector.x, vector.y, vector.z, 0.0))
            .truncate()
    }

    /// Transforms a homogeneous vector.
    #[must_use]
    pub fn transform_vec4(self, vector: Vec4) -> Vec4 {
        self.cols[0] * vector.x
            + self.cols[1] * vector.y
            + self.cols[2] * vector.z
            + self.cols[3] * vector.w
    }

    /// Returns the upper-left three-by-three matrix.
    #[must_use]
    pub fn upper_left(self) -> Mat3 {
        Mat3::from_cols(
            self.cols[0].truncate(),
            self.cols[1].truncate(),
            self.cols[2].truncate(),
        )
    }

    /// Returns the matrix that transforms normals with this matrix.
    ///
    /// The result is the inverse transpose of the upper-left three-by-three
    /// block. A singular matrix yields [`Mat3::ZERO`].
    #[must_use]
    pub fn normal_matrix(self) -> Mat3 {
        self.upper_left().inverse_transpose().unwrap_or(Mat3::ZERO)
    }

    /// Returns whether every component is finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.cols.iter().all(|column| column.is_finite())
    }
}

impl Default for Mat4 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mul for Mat4 {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        let [b0, b1, b2, b3] = other.cols;
        Self::from_cols(
            self.transform_vec4(b0),
            self.transform_vec4(b1),
            self.transform_vec4(b2),
            self.transform_vec4(b3),
        )
    }
}

impl Mul<Vec4> for Mat4 {
    type Output = Vec4;

    fn mul(self, vector: Vec4) -> Vec4 {
        self.transform_vec4(vector)
    }
}

/// A unit quaternion representing a rotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    /// X component of the vector part.
    pub x: f32,
    /// Y component of the vector part.
    pub y: f32,
    /// Z component of the vector part.
    pub z: f32,
    /// Scalar component.
    pub w: f32,
}

impl Quat {
    /// The identity rotation.
    pub const IDENTITY: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    /// Creates a quaternion from its components.
    #[must_use]
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    /// Creates a rotation of `radians` around `axis`.
    ///
    /// A zero axis or a non-finite angle produces the identity rotation.
    #[must_use]
    pub fn from_axis_angle(axis: Vec3, radians: f32) -> Self {
        if !radians.is_finite() {
            return Self::IDENTITY;
        }
        let Some(axis) = axis.try_normalize() else {
            return Self::IDENTITY;
        };
        let (sin, cos) = (radians * 0.5).sin_cos();
        Self::new(axis.x * sin, axis.y * sin, axis.z * sin, cos)
    }

    /// Returns the dot product of two quaternions.
    #[must_use]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
    }

    /// Returns the conjugate, which is the inverse for a unit quaternion.
    #[must_use]
    pub const fn conjugate(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, self.w)
    }

    /// Returns the squared length.
    #[must_use]
    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    /// Returns a unit quaternion, or `None` when the quaternion is zero or
    /// non-finite.
    #[must_use]
    pub fn try_normalize(self) -> Option<Self> {
        let length = self.length_squared().sqrt();
        if length.is_finite() && length > EPSILON {
            let inverse = 1.0 / length;
            Some(Self::new(
                self.x * inverse,
                self.y * inverse,
                self.z * inverse,
                self.w * inverse,
            ))
        } else {
            None
        }
    }

    /// Returns a unit quaternion, or the identity when normalization is not
    /// possible.
    #[must_use]
    pub fn normalize_or_identity(self) -> Self {
        self.try_normalize().unwrap_or(Self::IDENTITY)
    }

    /// Rotates `vector` by this quaternion.
    #[must_use]
    pub fn rotate_vector(self, vector: Vec3) -> Vec3 {
        let vector_part = Vec3::new(self.x, self.y, self.z);
        let doubled = vector_part.cross(vector) * 2.0;
        vector + doubled * self.w + vector_part.cross(doubled)
    }

    /// Converts this quaternion to a rotation matrix.
    #[must_use]
    pub fn to_mat4(self) -> Mat4 {
        let Self { x, y, z, w } = self.normalize_or_identity();
        let x2 = x + x;
        let y2 = y + y;
        let z2 = z + z;
        let xx = x * x2;
        let xy = x * y2;
        let xz = x * z2;
        let yy = y * y2;
        let yz = y * z2;
        let zz = z * z2;
        let wx = w * x2;
        let wy = w * y2;
        let wz = w * z2;

        Mat4::from_cols(
            Vec4::new(1.0 - (yy + zz), xy + wz, xz - wy, 0.0),
            Vec4::new(xy - wz, 1.0 - (xx + zz), yz + wx, 0.0),
            Vec4::new(xz + wy, yz - wx, 1.0 - (xx + yy), 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        )
    }

    /// Spherically interpolates from `self` to `other` by `t`.
    #[must_use]
    pub fn slerp(self, other: Self, t: f32) -> Self {
        let mut cosine = self.dot(other);
        let mut end = other;
        if cosine < 0.0 {
            cosine = -cosine;
            end = -other;
        }
        if cosine > 0.999_5 {
            return self.lerp(end, t).normalize_or_identity();
        }
        let angle = cosine.clamp(-1.0, 1.0).acos();
        let sine = angle.sin();
        if !sine.is_finite() || sine.abs() <= EPSILON {
            return self.normalize_or_identity();
        }
        let start_weight = ((1.0 - t) * angle).sin() / sine;
        let end_weight = (t * angle).sin() / sine;
        Self::new(
            self.x * start_weight + end.x * end_weight,
            self.y * start_weight + end.y * end_weight,
            self.z * start_weight + end.z * end_weight,
            self.w * start_weight + end.w * end_weight,
        )
        .normalize_or_identity()
    }

    /// Linearly interpolates the components without normalizing.
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
            self.z + (other.z - self.z) * t,
            self.w + (other.w - self.w) * t,
        )
    }

    /// Returns whether every component is finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite() && self.w.is_finite()
    }
}

impl Default for Quat {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mul for Quat {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        Self::new(
            self.w * other.x + self.x * other.w + self.y * other.z - self.z * other.y,
            self.w * other.y - self.x * other.z + self.y * other.w + self.z * other.x,
            self.w * other.z + self.x * other.y - self.y * other.x + self.z * other.w,
            self.w * other.w - self.x * other.x - self.y * other.y - self.z * other.z,
        )
    }
}

impl Neg for Quat {
    type Output = Self;

    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, -self.w)
    }
}

/// A translation, rotation, and scale in three-dimensional space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// Translation applied after rotation and scale.
    pub translation: Vec3,
    /// Rotation applied before translation.
    pub rotation: Quat,
    /// Per-axis scale applied before rotation.
    pub scale: Vec3,
}

impl Transform {
    /// The identity transform.
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    /// Creates a transform from its parts.
    #[must_use]
    pub const fn new(translation: Vec3, rotation: Quat, scale: Vec3) -> Self {
        Self {
            translation,
            rotation,
            scale,
        }
    }

    /// Creates a translation-only transform.
    #[must_use]
    pub const fn from_translation(translation: Vec3) -> Self {
        Self {
            translation,
            ..Self::IDENTITY
        }
    }

    /// Creates a rotation-only transform.
    #[must_use]
    pub const fn from_rotation(rotation: Quat) -> Self {
        Self {
            rotation,
            ..Self::IDENTITY
        }
    }

    /// Creates a uniform-scale transform.
    #[must_use]
    pub const fn from_scale(scale: f32) -> Self {
        Self {
            scale: Vec3::splat(scale),
            ..Self::IDENTITY
        }
    }

    /// Returns this transform with a new translation.
    #[must_use]
    pub const fn with_translation(mut self, translation: Vec3) -> Self {
        self.translation = translation;
        self
    }

    /// Returns this transform with a new rotation.
    #[must_use]
    pub const fn with_rotation(mut self, rotation: Quat) -> Self {
        self.rotation = rotation;
        self
    }

    /// Returns this transform with a new scale.
    #[must_use]
    pub const fn with_scale(mut self, scale: Vec3) -> Self {
        self.scale = scale;
        self
    }

    /// Returns the model matrix for this transform.
    #[must_use]
    pub fn matrix(self) -> Mat4 {
        Mat4::translation(self.translation) * self.rotation.to_mat4() * Mat4::scale(self.scale)
    }

    /// Transforms a point.
    #[must_use]
    pub fn transform_point(self, point: Vec3) -> Vec3 {
        self.rotation.rotate_vector(point * self.scale) + self.translation
    }

    /// Transforms a direction, ignoring translation.
    #[must_use]
    pub fn transform_vector(self, vector: Vec3) -> Vec3 {
        self.rotation.rotate_vector(vector * self.scale)
    }

    /// Returns whether every component is finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.translation.is_finite() && self.rotation.is_finite() && self.scale.is_finite()
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[cfg(test)]
mod tests {
    use super::{Mat3, Mat4, Quat, Transform, Vec2, Vec3, Vec4};
    use core::f32::consts::{FRAC_PI_2, PI};

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 1.0e-5,
            "expected {expected}, got {actual}"
        );
    }

    fn assert_vec3_close(actual: Vec3, expected: Vec3) {
        assert_close(actual.x, expected.x);
        assert_close(actual.y, expected.y);
        assert_close(actual.z, expected.z);
    }

    #[test]
    fn vector_operations_follow_the_right_hand_rule() {
        assert_eq!(Vec3::X.cross(Vec3::Y), Vec3::Z);
        assert_eq!(Vec3::Y.cross(Vec3::Z), Vec3::X);
        assert_eq!(Vec3::Z.cross(Vec3::X), Vec3::Y);
        assert_close(Vec3::new(3.0, 4.0, 0.0).length(), 5.0);
        assert_close(Vec2::new(3.0, 4.0).length(), 5.0);
        assert_eq!(
            Vec4::new(1.0, 2.0, 3.0, 4.0).truncate(),
            Vec3::new(1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn normalization_handles_degenerate_input() {
        assert_eq!(Vec3::ZERO.normalize_or_zero(), Vec3::ZERO);
        assert!(Vec3::ZERO.try_normalize().is_none());
        assert!(Vec3::new(f32::NAN, 0.0, 0.0).try_normalize().is_none());
    }

    #[test]
    fn rotation_matrices_rotate_in_the_expected_direction() {
        let rotated = Mat4::rotation_z(FRAC_PI_2).transform_vector(Vec3::X);
        assert_vec3_close(rotated, Vec3::Y);

        let rotated = Mat4::rotation_x(FRAC_PI_2).transform_vector(Vec3::Y);
        assert_vec3_close(rotated, Vec3::Z);

        let rotated = Mat4::rotation_y(FRAC_PI_2).transform_vector(Vec3::Z);
        assert_vec3_close(rotated, Vec3::X);
    }

    #[test]
    fn quaternion_rotation_matches_its_matrix() {
        let rotation = Quat::from_axis_angle(Vec3::new(1.0, 2.0, 3.0), 0.7);
        let vector = Vec3::new(-0.4, 0.2, 0.9);
        let from_quaternion = rotation.rotate_vector(vector);
        let from_matrix = rotation.to_mat4().transform_vector(vector);
        assert_vec3_close(from_quaternion, from_matrix);
    }

    #[test]
    fn quaternion_composition_applies_the_right_operand_first() {
        let first = Quat::from_axis_angle(Vec3::Z, FRAC_PI_2);
        let second = Quat::from_axis_angle(Vec3::X, FRAC_PI_2);
        let combined = second * first;
        assert_vec3_close(combined.rotate_vector(Vec3::X), Vec3::Z);
    }

    #[test]
    fn slerp_reaches_both_endpoints() {
        let start = Quat::IDENTITY;
        let end = Quat::from_axis_angle(Vec3::Y, PI);
        assert_eq!(start.slerp(end, 0.0), start);
        let finished = start.slerp(end, 1.0);
        assert_vec3_close(finished.rotate_vector(Vec3::X), end.rotate_vector(Vec3::X));
    }

    #[test]
    fn perspective_maps_the_near_and_far_planes_to_zero_and_one() {
        let projection = Mat4::perspective(FRAC_PI_2, 1.5, 0.1, 100.0);
        let near = projection.transform_point(Vec3::new(0.0, 0.0, -0.1));
        assert_close(near.z / near.w, 0.0);
        let far = projection.transform_point(Vec3::new(0.0, 0.0, -100.0));
        assert_close(far.z / far.w, 1.0);
    }

    #[test]
    fn look_at_places_the_target_on_the_negative_z_axis() {
        let view = Mat4::look_at(Vec3::new(3.0, 4.0, 5.0), Vec3::ZERO, Vec3::Y);
        let target = view.transform_point(Vec3::ZERO);
        assert_close(target.x, 0.0);
        assert_close(target.y, 0.0);
        assert!(target.z < 0.0);
    }

    #[test]
    fn transform_composes_scale_rotation_and_translation() {
        let transform = Transform::new(
            Vec3::new(1.0, 2.0, 3.0),
            Quat::from_axis_angle(Vec3::Z, FRAC_PI_2),
            Vec3::splat(2.0),
        );
        let point = transform.transform_point(Vec3::X);
        assert_vec3_close(point, Vec3::new(1.0, 4.0, 3.0));
        assert_vec3_close(
            transform.matrix().transform_point(Vec3::X).truncate(),
            point,
        );
    }

    #[test]
    fn mat3_inverse_round_trips_a_vector() {
        let matrix = Mat3::from_cols(
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.5, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 4.0),
        );
        let inverse = matrix.inverse().expect("the matrix is invertible");
        let vector = Vec3::new(1.0, -2.0, 3.0);
        assert_vec3_close(
            inverse.transform_vector(matrix.transform_vector(vector)),
            vector,
        );
    }

    #[test]
    fn singular_matrices_have_no_inverse() {
        let matrix = Mat3::from_cols(Vec3::X, Vec3::X, Vec3::Z);
        assert!(matrix.inverse().is_none());
        assert_eq!(
            Mat4::scale(Vec3::new(1.0, 0.0, 1.0)).normal_matrix(),
            Mat3::ZERO
        );
    }

    #[test]
    fn non_uniform_scale_keeps_normals_perpendicular() {
        let transform = Transform::from_scale(1.0).with_scale(Vec3::new(1.0, 3.0, 1.0));
        let normal = transform.matrix().normal_matrix().transform_vector(Vec3::Y);
        assert_vec3_close(normal.normalize_or_zero(), Vec3::Y);
    }
}
