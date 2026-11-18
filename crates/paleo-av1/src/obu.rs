//! AV1 open bitstream unit scanning for random-access detection.
//!
//! Paleo's demuxer hands out one temporal unit per packet, but the packet is a
//! sequence of self-delimiting OBUs, so a seek needs a helper that reads the
//! frame header and reports whether the picture is a keyframe.

/// The OBU types that carry a frame header.
const OBU_FRAME_HEADER: u8 = 3;
const OBU_FRAME: u8 = 6;

/// The largest `obu_size` field the specification allows.
const MAX_LEB128_BYTES: usize = 8;

/// Reports whether the packet contains an AV1 keyframe.
///
/// A keyframe is the only picture that can start decoding from an arbitrary
/// point of the stream. Packets that do not parse as OBUs, or that carry only
/// sequence headers and metadata, report `false`.
pub(crate) fn is_keyframe(data: &[u8]) -> bool {
    let mut offset = 0;
    while offset < data.len() {
        let Some(obu) = next_obu(data, &mut offset) else {
            return false;
        };
        if matches!(obu.kind, OBU_FRAME_HEADER | OBU_FRAME)
            && data
                .get(obu.payload.clone())
                .is_some_and(is_keyframe_header)
        {
            return true;
        }
    }
    false
}

/// One parsed OBU header: its type and the byte range of its payload.
struct Obu {
    kind: u8,
    payload: std::ops::Range<usize>,
}

/// Reads the next OBU header and returns it with the payload range.
///
/// An OBU without a size field extends to the end of the data, as the
/// specification requires of the last OBU in a low-overhead temporal unit.
fn next_obu(data: &[u8], offset: &mut usize) -> Option<Obu> {
    let header = *data.get(*offset)?;
    // `forbidden_bit` must be zero for a well-formed OBU.
    if header & 0x80 != 0 {
        return None;
    }
    let kind = header >> 3 & 0x0f;
    let has_extension = header & 0x04 != 0;
    let has_size = header & 0x02 != 0;

    let mut cursor = offset.checked_add(1)?;
    if has_extension {
        cursor = cursor.checked_add(1)?;
    }
    data.get(cursor..)?;

    let payload = if has_size {
        let (size, next) = leb128(data, cursor)?;
        let end = next.checked_add(size)?;
        if end > data.len() {
            return None;
        }
        *offset = end;
        next..end
    } else {
        *offset = data.len();
        cursor..data.len()
    };
    Some(Obu { kind, payload })
}

/// Reads an unsigned LEB128 value and returns it with the following offset.
fn leb128(data: &[u8], mut offset: usize) -> Option<(usize, usize)> {
    let mut value = 0_usize;
    for byte_index in 0..MAX_LEB128_BYTES {
        let byte = *data.get(offset)?;
        offset += 1;
        let shift = byte_index * 7;
        value |= usize::from(byte & 0x7f).checked_shl(u32::try_from(shift).ok()?)?;
        if byte & 0x80 == 0 {
            return Some((value, offset));
        }
    }
    None
}

/// Reads the start of a `frame_header_obu` and reports whether it codes a
/// keyframe.
fn is_keyframe_header(payload: &[u8]) -> bool {
    let mut bits = Bits::new(payload);
    // `show_existing_frame` re-displays an earlier picture, which cannot start
    // a sequence on its own.
    if bits.read(1) != Some(0) {
        return false;
    }
    // `frame_type` is 0 for a keyframe and 1 for an inter frame.
    bits.read(2) == Some(0)
}

/// An MSB-first bit reader over an AV1 payload.
struct Bits<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> Bits<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self { data, bit: 0 }
    }

    /// Reads up to 8 bits, returning `None` when the payload ends first.
    fn read(&mut self, count: usize) -> Option<u32> {
        let mut value = 0_u32;
        for _ in 0..count {
            let byte = *self.data.get(self.bit / 8)?;
            let shift = 7 - self.bit % 8;
            value = value << 1 | u32::from(byte >> shift & 1);
            self.bit += 1;
        }
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an OBU with a size field.
    fn obu(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut data = vec![kind << 3 | 0x02];
        data.push(u8::try_from(payload.len()).unwrap());
        data.extend_from_slice(payload);
        data
    }

    /// A frame header with `show_existing_frame = 0` and `frame_type`.
    fn frame_header(show_existing: bool, frame_type: u8) -> Vec<u8> {
        let mut byte = u8::from(show_existing) << 7;
        byte |= frame_type << 5;
        vec![byte]
    }

    #[test]
    fn recognises_a_keyframe() {
        let mut packet = obu(1, &[0x0a, 0x0b]); // sequence header
        packet.extend(obu(OBU_FRAME, &frame_header(false, 0)));
        assert!(is_keyframe(&packet));
    }

    #[test]
    fn rejects_inter_and_shown_pictures() {
        assert!(!is_keyframe(&obu(OBU_FRAME, &frame_header(false, 1))));
        assert!(!is_keyframe(&obu(OBU_FRAME, &frame_header(true, 0))));
    }

    #[test]
    fn rejects_metadata_only_packets() {
        assert!(!is_keyframe(&[]));
        assert!(!is_keyframe(&obu(1, &[0x0a, 0x0b])));
        assert!(!is_keyframe(&obu(2, &[]))); // temporal delimiter
    }

    #[test]
    fn reads_a_frame_header_obu() {
        let packet = obu(OBU_FRAME_HEADER, &frame_header(false, 0));
        assert!(is_keyframe(&packet));
    }

    #[test]
    fn skips_a_temporal_delimiter() {
        let mut packet = obu(2, &[]);
        packet.extend(obu(1, &[0x0a]));
        packet.extend(obu(OBU_FRAME, &frame_header(false, 0)));
        assert!(is_keyframe(&packet));
    }

    #[test]
    fn rejects_truncated_and_oversized_obus() {
        // A declared size larger than the packet.
        assert!(!is_keyframe(&[0x0a, 0x7f, 0x00]));
        // A size continuation that never ends.
        assert!(!is_keyframe(&[
            0x0a, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80
        ]));
        // The forbidden bit is set.
        assert!(!is_keyframe(&[0x8a, 0x00]));
    }

    #[test]
    fn accepts_a_low_overhead_obu_without_a_size_field() {
        // OBU_FRAME with `obu_has_size_field` clear; the payload runs to the
        // end of the packet.
        let mut packet = vec![OBU_FRAME << 3];
        packet.extend(frame_header(false, 0));
        assert!(is_keyframe(&packet));
    }

    #[test]
    fn reads_multi_byte_leb128_sizes() {
        // A 200-byte payload needs a two-byte LEB128 size field.
        let packet = |header_byte: u8| {
            let mut payload = vec![header_byte];
            payload.resize(200, 0);
            let mut data = vec![OBU_FRAME << 3 | 0x02];
            data.extend_from_slice(&[0xc8, 0x01]);
            data.extend_from_slice(&payload);
            data
        };
        assert!(!is_keyframe(&packet(frame_header(false, 1)[0])));
        assert!(is_keyframe(&packet(frame_header(false, 0)[0])));
    }
}
