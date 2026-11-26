//! A perspective camera with an orbit-friendly constructor.

use crate::math::{Mat4, Vec3};

const DEFAULT_VERTICAL_FOV: f32 = core::f32::consts::PI / 3.0;
const DEFAULT_NEAR: f32 = 0.1;
const DEFAULT_FAR: f32 = 1_000.0;

/// A right-handed perspective camera.
///
/// The camera looks from `position` toward `target` with `up` as the preferred
/// vertical axis. The default vertical field of view is 60 degrees and the
/// default clip range is `0.1..=1000`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    position: Vec3,
    target: Vec3,
    up: Vec3,
    vertical_fov: f32,
    near: f32,
    far: f32,
}

impl Camera {
    /// Creates a camera at `position` looking at `target`.
    #[must_use]
    pub const fn new(position: Vec3, target: Vec3) -> Self {
        Self {
            position,
            target,
            up: Vec3::Y,
            vertical_fov: DEFAULT_VERTICAL_FOV,
            near: DEFAULT_NEAR,
            far: DEFAULT_FAR,
        }
    }

    /// Creates a camera orbiting `target` at `distance`.
    ///
    /// `yaw` rotates around the Y axis and `pitch` moves the camera above or
    /// below the target. Both angles are in radians. A pitch of zero places the
    /// camera on the positive Z axis.
    ///
    /// # Panics
    ///
    /// Panics when `distance` is not finite and positive, or when either angle
    /// is not finite.
    #[must_use]
    pub fn orbiting(target: Vec3, distance: f32, yaw: f32, pitch: f32) -> Self {
        assert!(
            distance.is_finite() && distance > 0.0,
            "orbit distance must be finite and positive"
        );
        assert!(
            yaw.is_finite() && pitch.is_finite(),
            "orbit angles must be finite"
        );
        let horizontal = pitch.cos();
        let offset = Vec3::new(horizontal * yaw.sin(), pitch.sin(), horizontal * yaw.cos());
        Self::new(target + offset * distance, target)
    }

    /// Returns this camera with a different up axis.
    #[must_use]
    pub const fn with_up(mut self, up: Vec3) -> Self {
        self.up = up;
        self
    }

    /// Returns this camera with a vertical field of view in radians.
    ///
    /// # Panics
    ///
    /// Panics when `radians` is not finite or not in `(0, pi)`.
    #[must_use]
    pub fn with_vertical_fov(mut self, radians: f32) -> Self {
        assert!(
            radians.is_finite() && radians > 0.0 && radians < core::f32::consts::PI,
            "camera field of view must be finite and between zero and pi"
        );
        self.vertical_fov = radians;
        self
    }

    /// Returns this camera with a vertical field of view in degrees.
    ///
    /// # Panics
    ///
    /// Panics when `degrees` is not finite or not in `(0, 180)`.
    #[must_use]
    pub fn with_vertical_fov_degrees(self, degrees: f32) -> Self {
        self.with_vertical_fov(degrees.to_radians())
    }

    /// Returns this camera with new clip planes.
    ///
    /// # Panics
    ///
    /// Panics when the planes are not finite or do not satisfy
    /// `0 < near < far`.
    #[must_use]
    pub fn with_clip_planes(mut self, near: f32, far: f32) -> Self {
        assert!(
            near.is_finite() && far.is_finite() && near > 0.0 && far > near,
            "camera clip planes must satisfy 0 < near < far"
        );
        self.near = near;
        self.far = far;
        self
    }

    /// Returns the camera position.
    #[must_use]
    pub const fn position(self) -> Vec3 {
        self.position
    }

    /// Returns the point the camera looks at.
    #[must_use]
    pub const fn target(self) -> Vec3 {
        self.target
    }

    /// Returns the preferred up axis.
    #[must_use]
    pub const fn up(self) -> Vec3 {
        self.up
    }

    /// Returns the vertical field of view in radians.
    #[must_use]
    pub const fn vertical_fov(self) -> f32 {
        self.vertical_fov
    }

    /// Returns the near clip distance.
    #[must_use]
    pub const fn near(self) -> f32 {
        self.near
    }

    /// Returns the far clip distance.
    #[must_use]
    pub const fn far(self) -> f32 {
        self.far
    }

    /// Returns the distance from the camera to its target.
    #[must_use]
    pub fn distance(self) -> f32 {
        self.position.distance(self.target)
    }

    /// Returns the view matrix.
    #[must_use]
    pub fn view_matrix(self) -> Mat4 {
        Mat4::look_at(self.position, self.target, self.up)
    }

    /// Returns the perspective projection matrix for `aspect`.
    ///
    /// # Panics
    ///
    /// Panics when `aspect` is not finite and positive.
    #[must_use]
    pub fn projection_matrix(self, aspect: f32) -> Mat4 {
        Mat4::perspective(self.vertical_fov, aspect, self.near, self.far)
    }

    /// Returns the combined projection and view matrix.
    ///
    /// # Panics
    ///
    /// Panics when `aspect` is not finite and positive.
    #[must_use]
    pub fn view_projection(self, aspect: f32) -> Mat4 {
        self.projection_matrix(aspect) * self.view_matrix()
    }

    /// Returns whether every camera parameter is finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.position.is_finite() && self.target.is_finite() && self.up.is_finite()
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self::new(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::Camera;
    use crate::math::Vec3;
    use core::f32::consts::FRAC_PI_2;

    #[test]
    fn orbit_constructor_keeps_the_requested_distance() {
        let camera = Camera::orbiting(Vec3::ZERO, 5.0, 0.0, 0.0);
        assert!((camera.position().z - 5.0).abs() <= 1.0e-5);
        assert!((camera.distance() - 5.0).abs() <= 1.0e-5);
    }

    #[test]
    fn view_matrix_places_the_target_ahead_of_the_camera() {
        let camera = Camera::new(Vec3::new(2.0, 3.0, 4.0), Vec3::ZERO);
        let target = camera.view_matrix().transform_point(Vec3::ZERO);
        assert!(target.z < 0.0);
        assert!(target.x.abs() <= 1.0e-5);
        assert!(target.y.abs() <= 1.0e-5);
    }

    #[test]
    fn pitch_moves_the_camera_above_the_target() {
        let camera = Camera::orbiting(Vec3::ZERO, 5.0, 0.0, FRAC_PI_2);
        assert!((camera.position().y - 5.0).abs() <= 1.0e-5);
        assert!(camera.position().z.abs() <= 1.0e-5);
    }

    #[test]
    fn field_of_view_degrees_are_converted() {
        let camera = Camera::default().with_vertical_fov_degrees(90.0);
        assert!((camera.vertical_fov() - FRAC_PI_2).abs() <= 1.0e-5);
    }
}
