//! `HEVCDecoderConfigurationRecord` parsing and NAL framing conversion.
//!
//! ISO/MP4 and Matroska store HEVC access units with length-prefixed NAL units
//! ("HVCC" framing), while the decoder consumes Annex-B start codes. The
//! `hvcC` record in the track header provides both the NAL length size and the
//! VPS/SPS/PPS parameter sets that every random-access point needs.

use crate::error::{Error, Result};

/// A four-byte Annex-B start code. Three-byte start codes are accepted on
/// input, but every NAL this crate writes uses the four-byte form.
pub(crate) const START_CODE: [u8; 4] = [0, 0, 0, 1];

/// NAL unit types of the parameter sets that length-prefixed packets leave out
/// of band.
const NAL_TYPE_VPS: u8 = 32;
const NAL_TYPE_SPS: u8 = 33;
const NAL_TYPE_PPS: u8 = 34;

/// HEVC NAL unit types 16 through 21 are the intra random access points:
/// BLA with leading pictures, BLA without them, IDR, and CRA.
const NAL_TYPE_IRAP_START: u8 = 16;
const NAL_TYPE_IRAP_END: u8 = 21;

/// The parts of an `HEVCDecoderConfigurationRecord` that a decoder needs.
#[derive(Clone, Debug)]
pub(crate) struct HevcConfig {
    nal_length_size: usize,
    parameter_sets: Box<[u8]>,
}

impl HevcConfig {
    /// Parses an `HEVCDecoderConfigurationRecord`, as stored in an `hvcC` box
    /// or in Matroska `CodecPrivate`.
    ///
    /// Every array is walked so malformed records are rejected, but only the
    /// VPS/SPS/PPS arrays are kept: sequence-parameter arrays carry nothing the
    /// decoder needs before the first picture.
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 23 {
            return Err(Error::InvalidConfig("record is shorter than 23 bytes"));
        }
        if data[0] != 1 {
            return Err(Error::InvalidConfig("unsupported configuration version"));
        }

        // Byte 21: `constantFrameRate`, `numTemporalLayers`, `temporalIdNested`,
        // then the 2-bit `lengthSizeMinusOne`.
        let nal_length_size = usize::from(data[21] & 0x03) + 1;
        let array_count = usize::from(data[22]);

        let mut parameter_sets = Vec::new();
        let mut offset = 23;
        for _ in 0..array_count {
            let header = *data
                .get(offset)
                .ok_or(Error::InvalidConfig("truncated array header"))?;
            let nal_type = header & 0x3f;
            offset += 1;
            let count = read_u16(data, &mut offset)?;
            for _ in 0..count {
                let length = usize::from(read_u16(data, &mut offset)?);
                let end = offset
                    .checked_add(length)
                    .filter(|end| *end <= data.len())
                    .ok_or(Error::InvalidConfig("NAL unit exceeds the record"))?;
                if matches!(nal_type, NAL_TYPE_VPS | NAL_TYPE_SPS | NAL_TYPE_PPS) {
                    parameter_sets.extend_from_slice(&START_CODE);
                    parameter_sets.extend_from_slice(&data[offset..end]);
                }
                offset = end;
            }
        }

        Ok(Self {
            nal_length_size,
            parameter_sets: parameter_sets.into_boxed_slice(),
        })
    }

    /// Rewrites one packet into Annex-B framing in `out`.
    ///
    /// When `prepend_parameter_sets` is set, the VPS/SPS/PPS from the record
    /// are emitted first, which is required at every random-access point
    /// because length-prefixed packets do not repeat them in-band.
    ///
    /// Packets are length-prefixed by contract. A packet that does not form a
    /// consistent sequence of length-prefixed NAL units is accepted as
    /// Annex-B instead, because some muxers keep the stream in that form even
    /// when the track declares an `hvcC` record.
    pub fn to_annex_b(
        &self,
        packet: &[u8],
        out: &mut Vec<u8>,
        prepend_parameter_sets: bool,
    ) -> Result<()> {
        out.clear();
        let parameters_len = if prepend_parameter_sets {
            out.extend_from_slice(&self.parameter_sets);
            self.parameter_sets.len()
        } else {
            0
        };

        if self.append_framed(packet, out) {
            return Ok(());
        }

        out.truncate(parameters_len);
        if annex_b_nal_header(packet).is_some() {
            out.extend_from_slice(packet);
            return Ok(());
        }

        Err(Error::InvalidPacket("NAL length exceeds the packet"))
    }

    /// Reports whether the packet contains an intra random access point.
    pub fn is_irap(&self, packet: &[u8]) -> bool {
        if let Some(irap) = self.framed_is_irap(packet) {
            return irap;
        }
        annex_b_is_irap(packet)
    }

    /// Appends the packet's length-prefixed NAL units as Annex-B.
    ///
    /// Returns `false` unless the prefixes consume the packet exactly and at
    /// least one NAL is non-empty. This disambiguates a four-byte length prefix
    /// such as `00 00 01 b0` (432) from a three-byte Annex-B start code followed
    /// by a NAL header. Nothing is rolled back here; the caller truncates `out`.
    fn append_framed(&self, packet: &[u8], out: &mut Vec<u8>) -> bool {
        let mut offset = 0;
        let mut count = 0;
        while offset < packet.len() {
            let Some((start, length)) = self.next_nal(packet, &mut offset) else {
                return false;
            };
            if length > 0 {
                count += 1;
                out.extend_from_slice(&START_CODE);
                out.extend_from_slice(&packet[start..start + length]);
            }
        }
        count > 0
    }

    /// Returns whether the length-prefixed packet contains a random-access
    /// picture, or `None` when its framing is not a consistent sequence of NAL
    /// units.
    fn framed_is_irap(&self, packet: &[u8]) -> Option<bool> {
        let mut offset = 0;
        let mut count = 0;
        let mut irap = false;
        while offset < packet.len() {
            let (start, length) = self.next_nal(packet, &mut offset)?;
            if length > 0 {
                count += 1;
                irap |= packet
                    .get(start)
                    .is_some_and(|byte| is_irap_type(byte >> 1 & 0x3f));
            }
        }
        (count > 0).then_some(irap)
    }

    /// Reads one NAL length prefix and returns the payload offset and length.
    fn next_nal(&self, packet: &[u8], offset: &mut usize) -> Option<(usize, usize)> {
        let length_size = self.nal_length_size;
        let header_end = offset.checked_add(length_size)?;
        if header_end > packet.len() {
            return None;
        }
        let mut length = 0_usize;
        for byte in &packet[*offset..header_end] {
            length = (length << 8) | usize::from(*byte);
        }
        let end = header_end.checked_add(length)?;
        if end > packet.len() {
            return None;
        }
        *offset = end;
        Some((header_end, length))
    }
}

fn read_u16(data: &[u8], offset: &mut usize) -> Result<u16> {
    let bytes = data
        .get(*offset..*offset + 2)
        .ok_or(Error::InvalidConfig("truncated NAL unit length"))?;
    *offset += 2;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

const fn is_irap_type(nal_type: u8) -> bool {
    nal_type >= NAL_TYPE_IRAP_START && nal_type <= NAL_TYPE_IRAP_END
}

/// Returns whether raw Annex-B data contains an intra random access point.
pub(crate) fn annex_b_is_irap(data: &[u8]) -> bool {
    annex_b_has_nal_type(data, is_irap_type)
}

/// Returns whether `data` starts with an Annex-B start code and a plausible
/// HEVC NAL header.
pub(crate) fn annex_b_nal_header_is_valid(data: &[u8]) -> bool {
    annex_b_nal_header(data).is_some()
}

/// Returns the Annex-B NAL header bytes when `data` begins with a start code
/// and a plausible two-byte HEVC NAL header.
///
/// The forbidden zero bit must be clear and `nuh_temporal_id_plus1` must be
/// non-zero (H.265 §7.3.1.2). This check keeps a four-byte length prefix such
/// as `00 00 01 b0` from being mistaken for a three-byte start code followed by
/// a NAL header.
fn annex_b_nal_header(data: &[u8]) -> Option<[u8; 2]> {
    let offset = if data.starts_with(&START_CODE) {
        4
    } else if data.starts_with(&[0, 0, 1]) {
        3
    } else {
        return None;
    };
    let header = data.get(offset..offset + 2)?;
    let valid = header[0] & 0x80 == 0 && header[1] & 0x07 != 0;
    valid.then(|| [header[0], header[1]])
}

fn annex_b_has_nal_type(data: &[u8], matches: impl Fn(u8) -> bool) -> bool {
    let mut offset = 0;
    while offset + 3 <= data.len() {
        if data[offset] == 0 && data[offset + 1] == 0 && data[offset + 2] == 1 {
            let header = offset + 3;
            if let Some(byte) = data.get(header)
                && byte & 0x80 == 0
                && matches(byte >> 1 & 0x3f)
            {
                return true;
            }
            offset = header + 1;
        } else {
            offset += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal `hvcC` with one array per parameter set type. The
    /// payloads are not real bitstreams; the parser does not inspect them.
    fn record(nal_length_size_minus_one: u8, sets: &[(u8, &[u8])]) -> Vec<u8> {
        let mut data = vec![1; 23];
        data[21] = nal_length_size_minus_one;
        data[22] = u8::try_from(sets.len()).unwrap();
        for (nal_type, payload) in sets {
            data.push(*nal_type);
            data.extend_from_slice(&1_u16.to_be_bytes());
            data.extend_from_slice(&u16::try_from(payload.len()).unwrap().to_be_bytes());
            data.extend_from_slice(payload);
        }
        data
    }

    fn parameter_sets() -> Vec<(u8, &'static [u8])> {
        vec![
            (NAL_TYPE_VPS, &[0x40, 0x01, 0xaa][..]),
            (NAL_TYPE_SPS, &[0x42, 0x01, 0xbb][..]),
            (NAL_TYPE_PPS, &[0x44, 0x01, 0xcc][..]),
        ]
    }

    #[test]
    fn parses_parameter_sets() {
        let config = HevcConfig::parse(&record(3, &parameter_sets())).unwrap();
        assert_eq!(config.nal_length_size, 4);
        let expected = [
            START_CODE.as_slice(),
            &[0x40, 0x01, 0xaa],
            &START_CODE,
            &[0x42, 0x01, 0xbb],
            &START_CODE,
            &[0x44, 0x01, 0xcc],
        ]
        .concat();
        assert_eq!(config.parameter_sets.as_ref(), expected);
    }

    #[test]
    fn keeps_other_arrays_out_of_the_parameter_sets() {
        // A prefix SEI array (type 39) must be walked but not replayed.
        let sets = [(39_u8, &[0x4e, 0x01, 0xdd][..])];
        let config = HevcConfig::parse(&record(3, &sets)).unwrap();
        assert!(config.parameter_sets.is_empty());
    }

    #[test]
    fn rejects_short_and_mismatched_records() {
        assert!(matches!(
            HevcConfig::parse(&[1, 2, 3]),
            Err(Error::InvalidConfig(_))
        ));
        let mut data = record(3, &parameter_sets());
        data[0] = 2;
        assert!(matches!(
            HevcConfig::parse(&data),
            Err(Error::InvalidConfig(_))
        ));
        let mut data = record(3, &parameter_sets());
        data.truncate(data.len() - 2);
        assert!(matches!(
            HevcConfig::parse(&data),
            Err(Error::InvalidConfig(_))
        ));
    }

    #[test]
    fn converts_length_prefixed_nals() {
        let config = HevcConfig::parse(&record(3, &parameter_sets())).unwrap();
        // An IDR_W_RADL slice (type 19) followed by a suffix SEI (type 40).
        let packet = [0, 0, 0, 3, 0x26, 0x01, 0xaa, 0, 0, 0, 2, 0x50, 0x01];
        let mut out = Vec::new();
        config.to_annex_b(&packet, &mut out, false).unwrap();
        assert_eq!(
            out,
            [0, 0, 0, 1, 0x26, 0x01, 0xaa, 0, 0, 0, 1, 0x50, 0x01].to_vec()
        );
        assert!(config.is_irap(&packet));

        config.to_annex_b(&packet, &mut out, true).unwrap();
        assert!(out.ends_with(&[0, 0, 0, 1, 0x26, 0x01, 0xaa, 0, 0, 0, 1, 0x50, 0x01]));
        assert!(out.starts_with(&START_CODE));
    }

    #[test]
    fn every_irap_type_is_a_keyframe() {
        let config = HevcConfig::parse(&record(3, &parameter_sets())).unwrap();
        for nal_type in 16..=21_u8 {
            let packet = [0, 0, 0, 2, nal_type << 1, 0x01];
            assert!(config.is_irap(&packet), "type {nal_type}");
        }
        // A trail picture (type 1) and a parameter-set-only packet are not.
        assert!(!config.is_irap(&[0, 0, 0, 2, 0x02, 0x01]));
        assert!(!config.is_irap(&[0, 0, 0, 3, 0x40, 0x01, 0xaa]));
    }

    #[test]
    fn does_not_confuse_a_nal_length_with_a_start_code() {
        let config = HevcConfig::parse(&record(3, &parameter_sets())).unwrap();
        // A four-byte NAL length of 432 is `00 00 01 b0`, which begins with
        // the same three bytes as a three-byte Annex-B start code. The filler
        // byte starts a TRAIL_R NAL (type 1), not a random access point.
        let mut packet = vec![0, 0, 1, 0xb0];
        packet.resize(4 + 432, 0x02);
        let mut out = Vec::new();
        config.to_annex_b(&packet, &mut out, false).unwrap();
        assert_eq!(out.len(), 4 + 432);
        assert_eq!(&out[..4], &START_CODE);
        assert!(out[4..].iter().all(|byte| *byte == 0x02));
        assert!(!config.is_irap(&packet));
    }

    #[test]
    fn passes_annex_b_through() {
        let config = HevcConfig::parse(&record(3, &parameter_sets())).unwrap();
        let packet = [0, 0, 1, 0x26, 0x01, 0x11];
        let mut out = Vec::new();
        config.to_annex_b(&packet, &mut out, false).unwrap();
        assert_eq!(out, packet.to_vec());
        assert!(config.is_irap(&packet));
        assert!(annex_b_is_irap(&packet));
        assert!(!annex_b_is_irap(&[0, 0, 1, 0x02, 0x01, 0x11]));
    }

    #[test]
    fn detects_truncated_packets() {
        let config = HevcConfig::parse(&record(3, &parameter_sets())).unwrap();
        let mut out = Vec::new();
        assert!(matches!(
            config.to_annex_b(&[0, 0, 0, 8, 0x26, 0x01], &mut out, false),
            Err(Error::InvalidPacket(_))
        ));
        assert!(matches!(
            config.to_annex_b(&[0, 0], &mut out, false),
            Err(Error::InvalidPacket(_))
        ));
    }
}
