//! Position, rate, and duration arithmetic for the player.

use std::time::Duration;

/// The lowest rate a player accepts.
pub(crate) const MIN_RATE: f64 = 0.05;
/// The highest rate a player accepts.
pub(crate) const MAX_RATE: f64 = 16.0;

/// The playback timeline without any playback state.
///
/// Keeping the arithmetic separate makes it testable without a file, and keeps
/// the audio-master-clock and wall-clock paths on the same clamping rules.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Timeline {
    position: Duration,
    duration: Option<Duration>,
    rate: f64,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            position: Duration::ZERO,
            duration: None,
            rate: 1.0,
        }
    }
}

impl Timeline {
    pub(crate) const fn position(&self) -> Duration {
        self.position
    }

    pub(crate) const fn duration(&self) -> Option<Duration> {
        self.duration
    }

    pub(crate) const fn rate(&self) -> f64 {
        self.rate
    }

    pub(crate) fn set_duration(&mut self, duration: Option<Duration>) {
        self.duration = duration.filter(|duration| !duration.is_zero());
        self.position = self.clamp(self.position);
    }

    /// Sets the rate, returning whether it changed.
    pub(crate) fn set_rate(&mut self, rate: f64) -> bool {
        let rate = clamp_rate(rate);
        if (self.rate - rate).abs() <= f64::EPSILON {
            return false;
        }
        self.rate = rate;
        true
    }

    /// Moves to `position`, clamped to the known duration.
    pub(crate) fn seek(&mut self, position: Duration) {
        self.position = self.clamp(position);
    }

    /// Moves to a position produced by the audio device.
    pub(crate) fn synchronize(&mut self, position: Duration) {
        self.position = self.clamp(position);
    }

    /// Advances by one wall-clock delta at the current rate.
    pub(crate) fn advance(&mut self, delta: Duration) {
        if delta.is_zero() {
            return;
        }
        self.position = self.clamp(self.position.saturating_add(delta.mul_f64(self.rate)));
    }

    /// Returns whether the playhead has reached the known end.
    pub(crate) fn is_at_duration(&self) -> bool {
        self.duration
            .is_some_and(|duration| self.position >= duration)
    }

    const fn clamp(&self, position: Duration) -> Duration {
        match self.duration {
            Some(duration) => {
                if position.as_nanos() > duration.as_nanos() {
                    duration
                } else {
                    position
                }
            }
            None => position,
        }
    }
}

/// Clamps a requested rate to the supported range, mapping non-finite values to
/// `1.0`.
pub(crate) fn clamp_rate(rate: f64) -> f64 {
    if rate.is_finite() {
        rate.clamp(MIN_RATE, MAX_RATE)
    } else {
        1.0
    }
}

/// Returns whether `rate` is the normal playback rate.
///
/// The device clock only matches the media clock at this rate, so every audio
/// path keys off it rather than comparing floats at each call site.
pub(crate) fn is_realtime(rate: f64) -> bool {
    (rate - 1.0).abs() < f64::EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advancing_scales_with_the_rate() {
        let mut timeline = Timeline::default();
        timeline.set_rate(2.0);
        timeline.advance(Duration::from_millis(200));
        assert_eq!(timeline.position(), Duration::from_millis(400));
    }

    #[test]
    fn positions_stay_inside_the_duration() {
        let mut timeline = Timeline::default();
        timeline.set_duration(Some(Duration::from_millis(500)));
        timeline.seek(Duration::from_secs(9));
        assert_eq!(timeline.position(), Duration::from_millis(500));
        assert!(timeline.is_at_duration());

        timeline.advance(Duration::from_millis(100));
        assert_eq!(timeline.position(), Duration::from_millis(500));
    }

    #[test]
    fn zero_and_unknown_durations_do_not_clamp() {
        let mut timeline = Timeline::default();
        timeline.set_duration(Some(Duration::ZERO));
        timeline.seek(Duration::from_secs(3));
        assert_eq!(timeline.position(), Duration::from_secs(3));
        assert!(!timeline.is_at_duration());
    }

    #[test]
    fn rates_are_clamped_and_non_finite_values_fall_back() {
        let close = |actual: f64, expected: f64| {
            assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
        };
        close(clamp_rate(0.0), MIN_RATE);
        close(clamp_rate(1_000.0), MAX_RATE);
        close(clamp_rate(f64::NAN), 1.0);
        close(clamp_rate(-4.0), MIN_RATE);
        assert!(is_realtime(clamp_rate(1.0)));
        assert!(!is_realtime(clamp_rate(0.5)));
    }

    #[test]
    fn a_rate_change_is_reported_once() {
        let mut timeline = Timeline::default();
        assert!(timeline.set_rate(1.5));
        assert!(!timeline.set_rate(1.5));
        assert!((timeline.rate() - 1.5).abs() < f64::EPSILON);
    }
}
