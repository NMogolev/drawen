//! The device half of playback.
//!
//! [`Output`] is a thin wrapper that keeps the player free of two versions of
//! every audio call: a build with the `device` feature owns a real
//! [`paleo_audio::AudioOutput`], and a build without it has an uninhabited type
//! whose methods can never run.

use std::time::Duration;

use crate::{PlayerOptions, Result};

/// A running audio device, or an empty type in a build without one.
#[cfg(feature = "device")]
pub(crate) struct Output(paleo_audio::AudioOutput);

#[cfg(feature = "device")]
impl Output {
    /// Opens the device described by `options`.
    pub(crate) fn open(options: &PlayerOptions) -> Result<Self> {
        let mut audio = paleo_audio::AudioOptions::new();
        if let Some(device) = &options.audio_device {
            audio = audio.with_device(device.clone());
        }
        if let Some(latency) = options.audio_latency {
            audio = audio.with_latency(latency);
        }
        Ok(Self(paleo_audio::AudioOutput::open_with(&audio)?))
    }

    /// Returns the PCM format the device consumes.
    pub(crate) const fn format(&self) -> paleo_audio::PcmFormat {
        self.0.format()
    }

    /// Returns how much audio is waiting in the device ring.
    pub(crate) fn buffered(&self) -> Duration {
        self.0.buffered()
    }

    /// Returns the ring capacity as media time.
    pub(crate) fn capacity(&self) -> Duration {
        self.0.capacity()
    }

    /// Returns the number of complete PCM frames that fit in the device ring.
    pub(crate) fn vacant_frames(&self) -> usize {
        self.0.vacant_frames()
    }

    /// Returns the media position the device has played.
    pub(crate) fn position(&self) -> Duration {
        self.0.position()
    }

    /// Starts the device clock.
    pub(crate) fn play(&mut self) -> Result<()> {
        Ok(self.0.play()?)
    }

    /// Stops the device clock.
    pub(crate) fn pause(&mut self) {
        self.0.pause();
    }

    /// Discards queued audio and anchors the clock at `position`.
    pub(crate) fn reset(&mut self, position: Duration) {
        self.0.reset(position);
    }

    /// Queues audio at the device format and returns the dropped frame count.
    pub(crate) fn queue(&mut self, buffer: &paleo_audio::PcmBuffer) -> Result<usize> {
        Ok(self.0.queue(buffer)?.skipped_frames())
    }

    /// Sets the linear volume, clamped by the device to `0.0..=1.0`.
    pub(crate) fn set_volume(&mut self, volume: f32) {
        self.0.set_volume(volume);
    }

    /// Mutes or unmutes the device without changing the stored volume.
    pub(crate) fn set_muted(&mut self, muted: bool) {
        self.0.set_muted(muted);
    }
}

/// A build without an output device: the type cannot be constructed, so every
/// method below is unreachable.
#[cfg(not(feature = "device"))]
pub(crate) enum Output {}

#[cfg(not(feature = "device"))]
impl Output {
    pub(crate) fn open(options: &PlayerOptions) -> Result<Self> {
        let _ = (&options.audio_device, options.audio_latency);
        Err(crate::Error::State(
            "this build has no audio device support".to_owned(),
        ))
    }

    pub(crate) fn format(&self) -> paleo_audio::PcmFormat {
        match *self {}
    }

    pub(crate) fn buffered(&self) -> Duration {
        match *self {}
    }

    pub(crate) fn capacity(&self) -> Duration {
        match *self {}
    }

    pub(crate) fn vacant_frames(&self) -> usize {
        match *self {}
    }

    pub(crate) fn position(&self) -> Duration {
        match *self {}
    }

    pub(crate) fn play(&mut self) -> Result<()> {
        match *self {}
    }

    pub(crate) fn pause(&mut self) {
        match *self {}
    }

    pub(crate) fn reset(&mut self, position: Duration) {
        let _ = position;
        match *self {}
    }

    pub(crate) fn queue(&mut self, buffer: &paleo_audio::PcmBuffer) -> Result<usize> {
        let _ = buffer;
        match *self {}
    }

    pub(crate) fn set_volume(&mut self, volume: f32) {
        let _ = volume;
        match *self {}
    }

    pub(crate) fn set_muted(&mut self, muted: bool) {
        let _ = muted;
        match *self {}
    }
}
