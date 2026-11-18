//! The video decoder contract shared by Paleo backends.

use crate::Result;
use crate::frame::VideoFrame;
use crate::video::VideoPacket;

/// A stateful video decoder driven by demuxed [`VideoPacket`] values.
///
/// A backend owns one codec instance and turns compressed packets into RGBA
/// pictures. Implementations are synchronous and single-threaded: the caller
/// decides where decoding runs, and the decoder never spawns threads or queues
/// packets internally.
pub trait VideoDecoder {
    /// Decodes one packet and returns the pictures that are ready to present.
    ///
    /// A packet can produce no picture (while a reorder window fills, or before
    /// the next random-access point after [`Self::reset`]) and buffered codecs
    /// can return more than one.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Decode`] when the packet is malformed or uses a
    /// tool that this build cannot decode.
    fn decode(&mut self, packet: &VideoPacket) -> Result<Vec<VideoFrame>>;

    /// Emits every buffered picture at end of stream.
    fn flush(&mut self) -> Vec<VideoFrame>;

    /// Drops decoder state and waits for the next random-access point.
    ///
    /// Call this after seeking. Packets before the next keyframe are skipped
    /// because inter pictures cannot be decoded from an arbitrary point.
    fn reset(&mut self);

    /// Reports whether the packet is a random-access point.
    #[must_use]
    fn is_keyframe(&self, packet: &VideoPacket) -> bool {
        self.is_keyframe_data(packet.data())
    }

    /// Reports whether raw packet data starts a random-access sequence.
    ///
    /// This is the byte-level form used while scanning for the keyframe before
    /// a seek target.
    #[must_use]
    fn is_keyframe_data(&self, data: &[u8]) -> bool;

    /// Reports whether the decoder is waiting for the next keyframe.
    #[must_use]
    fn is_awaiting_keyframe(&self) -> bool;
}
