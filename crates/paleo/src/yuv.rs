//! Deterministic YUV to RGBA conversion shared by video backends.

/// The luma/chroma matrix used to convert YUV to RGB.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ColorMatrix {
    /// BT.601 for standard definition, BT.709 for 720 lines and above.
    #[default]
    Auto,
    /// BT.601, the matrix used by most standard-definition video.
    Bt601,
    /// BT.709, the matrix used by most high-definition video.
    Bt709,
}

impl ColorMatrix {
    fn resolve(self, height: usize) -> Coefficients {
        let high_definition = match self {
            Self::Bt601 => false,
            Self::Bt709 => true,
            Self::Auto => height >= 720,
        };
        if high_definition {
            Coefficients::BT_709_LIMITED
        } else {
            Coefficients::BT_601_LIMITED
        }
    }
}

/// The sample range used by a YUV stream.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum YuvRange {
    /// Studio/video range: luma `16..=235`, chroma `16..=240`.
    #[default]
    Limited,
    /// Full range: luma and chroma use the whole sample range.
    Full,
}

/// One plane of a decoded picture.
///
/// `data` holds `height` rows of `stride` samples. Samples are `u8` for 8-bit
/// pictures and little-endian `u16` for 10- and 12-bit pictures. `stride` is
/// always measured in samples, not bytes.
#[derive(Clone, Copy, Debug)]
pub struct YuvPlane<'a> {
    data: &'a [u8],
    stride: usize,
    x: usize,
    y: usize,
}

impl<'a> YuvPlane<'a> {
    /// Creates a plane view over tightly described sample data.
    #[must_use]
    pub const fn new(data: &'a [u8], stride: usize) -> Self {
        Self {
            data,
            stride,
            x: 0,
            y: 0,
        }
    }

    /// Creates a plane view whose first visible sample is at `(x, y)`.
    ///
    /// This is how a backend presents the display area of a picture that was
    /// decoded at a larger coded size, which HEVC allows through its cropping
    /// window.
    #[must_use]
    pub const fn with_offset(data: &'a [u8], stride: usize, x: usize, y: usize) -> Self {
        Self { data, stride, x, y }
    }

    /// Returns the backing bytes.
    #[must_use]
    pub const fn data(self) -> &'a [u8] {
        self.data
    }

    /// Returns the row stride in samples.
    #[must_use]
    pub const fn stride(self) -> usize {
        self.stride
    }

    fn sample(self, x: usize, y: usize, bit_depth: u8) -> Option<i32> {
        if self.stride == 0 {
            return None;
        }
        let index = y
            .checked_add(self.y)?
            .checked_mul(self.stride)?
            .checked_add(x.checked_add(self.x)?)?;
        if bit_depth > 8 {
            let offset = index.checked_mul(2)?;
            let bytes = self.data.get(offset..offset + 2)?;
            Some(i32::from(u16::from_le_bytes([bytes[0], bytes[1]])))
        } else {
            self.data.get(index).map(|sample| i32::from(*sample))
        }
    }
}

/// One plane of a decoded picture whose samples are already `u16`.
///
/// Decoders that keep every bit depth in `u16` storage — HEVC, for example —
/// use this view to avoid narrowing an 8-bit picture into a byte buffer before
/// conversion. `stride` is measured in samples, not bytes, at every bit depth.
#[derive(Clone, Copy, Debug)]
pub struct YuvPlane16<'a> {
    data: &'a [u16],
    stride: usize,
    x: usize,
    y: usize,
}

impl<'a> YuvPlane16<'a> {
    /// Creates a plane view over `u16` sample data.
    #[must_use]
    pub const fn new(data: &'a [u16], stride: usize) -> Self {
        Self {
            data,
            stride,
            x: 0,
            y: 0,
        }
    }

    /// Creates a plane view whose first visible sample is at `(x, y)`.
    #[must_use]
    pub const fn with_offset(data: &'a [u16], stride: usize, x: usize, y: usize) -> Self {
        Self { data, stride, x, y }
    }

    /// Returns the backing samples.
    #[must_use]
    pub const fn data(self) -> &'a [u16] {
        self.data
    }

    /// Returns the row stride in samples.
    #[must_use]
    pub const fn stride(self) -> usize {
        self.stride
    }

    fn sample(self, x: usize, y: usize, _bit_depth: u8) -> Option<i32> {
        if self.stride == 0 {
            return None;
        }
        let index = y
            .checked_add(self.y)?
            .checked_mul(self.stride)?
            .checked_add(x.checked_add(self.x)?)?;
        self.data.get(index).map(|sample| i32::from(*sample))
    }
}

/// Fixed-point coefficients scaled by 1024.
struct Coefficients {
    luma: i32,
    r_v: i32,
    g_u: i32,
    g_v: i32,
    b_u: i32,
}

impl Coefficients {
    // Limited-range BT.601: R = 1.164(Y-16) + 1.596(V-128), and so on.
    const BT_601_LIMITED: Self = Self {
        luma: 1192,
        r_v: 1634,
        g_u: 400,
        g_v: 833,
        b_u: 2066,
    };

    // Limited-range BT.709.
    const BT_709_LIMITED: Self = Self {
        luma: 1192,
        r_v: 1836,
        g_u: 218,
        g_v: 546,
        b_u: 2163,
    };

    // Full-range BT.601/709 share these coefficients.
    const FULL: Self = Self {
        luma: 1024,
        r_v: 1436,
        g_u: 352,
        g_v: 731,
        b_u: 1815,
    };
}

/// Converts one YUV picture into tightly packed RGBA.
///
/// `subsampling_x` and `subsampling_y` describe the chroma layout (`true`,
/// `true` is 4:2:0). An empty U or V plane is treated as neutral chroma, which
/// makes monochrome streams render as gray.
///
/// `out` is resized to `width * height * 4` bytes and fully overwritten.
/// Returns `false` when the picture cannot be represented with the supplied
/// planes; `out` is then left empty.
#[must_use]
// A conversion needs the geometry, the layout, the colour description, the
// planes, and the destination. Grouping them behind a builder would not make
// the call site easier to read.
#[allow(clippy::too_many_arguments)]
pub fn rgba_from_yuv(
    width: usize,
    height: usize,
    bit_depth: u8,
    subsampling_x: bool,
    subsampling_y: bool,
    planes: [YuvPlane<'_>; 3],
    matrix: ColorMatrix,
    range: YuvRange,
    out: &mut Vec<u8>,
) -> bool {
    convert(
        width,
        height,
        bit_depth,
        subsampling_x,
        subsampling_y,
        planes,
        matrix,
        range,
        out,
    )
}

/// Converts one `u16` YUV picture into tightly packed RGBA.
///
/// This is [`rgba_from_yuv`] for decoders that store every bit depth as `u16`
/// samples, including 8-bit pictures. `bit_depth` selects the sample range, so
/// an 8-bit picture still converts with 8-bit offsets.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn rgba_from_yuv16(
    width: usize,
    height: usize,
    bit_depth: u8,
    subsampling_x: bool,
    subsampling_y: bool,
    planes: [YuvPlane16<'_>; 3],
    matrix: ColorMatrix,
    range: YuvRange,
    out: &mut Vec<u8>,
) -> bool {
    convert(
        width,
        height,
        bit_depth,
        subsampling_x,
        subsampling_y,
        planes,
        matrix,
        range,
        out,
    )
}

/// Sample access shared by the byte and `u16` plane views.
trait PlaneSamples: Copy {
    fn sample(self, x: usize, y: usize, bit_depth: u8) -> Option<i32>;
    fn is_empty(self) -> bool;
}

impl PlaneSamples for YuvPlane<'_> {
    fn sample(self, x: usize, y: usize, bit_depth: u8) -> Option<i32> {
        YuvPlane::sample(self, x, y, bit_depth)
    }

    fn is_empty(self) -> bool {
        self.data.is_empty()
    }
}

impl PlaneSamples for YuvPlane16<'_> {
    fn sample(self, x: usize, y: usize, bit_depth: u8) -> Option<i32> {
        YuvPlane16::sample(self, x, y, bit_depth)
    }

    fn is_empty(self) -> bool {
        self.data.is_empty()
    }
}

#[allow(clippy::too_many_arguments)]
fn convert<P: PlaneSamples>(
    width: usize,
    height: usize,
    bit_depth: u8,
    subsampling_x: bool,
    subsampling_y: bool,
    planes: [P; 3],
    matrix: ColorMatrix,
    range: YuvRange,
    out: &mut Vec<u8>,
) -> bool {
    out.clear();
    if width == 0 || height == 0 || !(8..=16).contains(&bit_depth) {
        return false;
    }
    let Some(pixels) = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
    else {
        return false;
    };
    out.resize(pixels, 0);

    let coefficients = match range {
        YuvRange::Full => Coefficients::FULL,
        YuvRange::Limited => matrix.resolve(height),
    };
    let shift = u32::from(bit_depth.saturating_sub(8));
    let luma_offset = match range {
        YuvRange::Limited => 16_i32 << shift,
        YuvRange::Full => 0,
    };
    let chroma_offset = 128_i32 << shift;
    let chroma_width = if subsampling_x {
        width.div_ceil(2)
    } else {
        width
    };
    let chroma_height = if subsampling_y {
        height.div_ceil(2)
    } else {
        height
    };
    let monochrome = planes[1].is_empty() || planes[2].is_empty();

    for row in 0..height {
        let Some(out_row) = out.get_mut(row * width * 4..(row + 1) * width * 4) else {
            out.clear();
            return false;
        };
        let chroma_row = if subsampling_y { row / 2 } else { row };
        if chroma_row >= chroma_height {
            out.clear();
            return false;
        }
        for column in 0..width {
            let Some(y) = planes[0].sample(column, row, bit_depth) else {
                out.clear();
                return false;
            };
            let chroma_column = if subsampling_x { column / 2 } else { column };
            if chroma_column >= chroma_width {
                out.clear();
                return false;
            }
            let (u, v) = if monochrome {
                (chroma_offset, chroma_offset)
            } else {
                let Some(u) = planes[1].sample(chroma_column, chroma_row, bit_depth) else {
                    out.clear();
                    return false;
                };
                let Some(v) = planes[2].sample(chroma_column, chroma_row, bit_depth) else {
                    out.clear();
                    return false;
                };
                (u, v)
            };

            let luma = (y - luma_offset) >> shift;
            let u = (u - chroma_offset) >> shift;
            let v = (v - chroma_offset) >> shift;
            let scaled_luma = coefficients.luma * luma;
            let pixel = &mut out_row[column * 4..column * 4 + 4];
            pixel[0] = clamp8((scaled_luma + coefficients.r_v * v + 512) >> 10);
            pixel[1] =
                clamp8((scaled_luma - coefficients.g_u * u - coefficients.g_v * v + 512) >> 10);
            pixel[2] = clamp8((scaled_luma + coefficients.b_u * u + 512) >> 10);
            pixel[3] = 255;
        }
    }
    true
}

fn clamp8(value: i32) -> u8 {
    u8::try_from(value.clamp(0, 255)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 4x2 picture: a luma ramp, neutral chroma.
    fn planes_8bit() -> (Vec<u8>, Vec<u8>) {
        let luma = vec![16, 48, 80, 112, 144, 176, 208, 235];
        let chroma = vec![128, 128, 128, 128];
        (luma, chroma)
    }

    #[test]
    fn u16_planes_match_byte_planes_at_eight_bits() {
        let (luma, chroma) = planes_8bit();
        let mut bytes = Vec::new();
        assert!(rgba_from_yuv(
            4,
            2,
            8,
            true,
            true,
            [
                YuvPlane::new(&luma, 4),
                YuvPlane::new(&chroma, 2),
                YuvPlane::new(&chroma, 2),
            ],
            ColorMatrix::Bt709,
            YuvRange::Limited,
            &mut bytes,
        ));

        let luma16: Vec<u16> = luma.iter().map(|&sample| u16::from(sample)).collect();
        let chroma16: Vec<u16> = chroma.iter().map(|&sample| u16::from(sample)).collect();
        let mut wide = Vec::new();
        assert!(rgba_from_yuv16(
            4,
            2,
            8,
            true,
            true,
            [
                YuvPlane16::new(&luma16, 4),
                YuvPlane16::new(&chroma16, 2),
                YuvPlane16::new(&chroma16, 2),
            ],
            ColorMatrix::Bt709,
            YuvRange::Limited,
            &mut wide,
        ));
        assert_eq!(bytes, wide);
    }

    #[test]
    fn ten_bit_samples_use_the_matching_offset() {
        // Mid-grey in 10-bit limited range is 512.
        let luma = [512_u16; 4];
        let chroma = [512_u16; 2];
        let mut pixels = Vec::new();
        assert!(rgba_from_yuv16(
            2,
            2,
            10,
            true,
            true,
            [
                YuvPlane16::new(&luma, 2),
                YuvPlane16::new(&chroma, 1),
                YuvPlane16::new(&chroma, 1),
            ],
            ColorMatrix::Bt709,
            YuvRange::Limited,
            &mut pixels,
        ));
        for pixel in pixels.as_chunks::<4>().0 {
            assert!(pixel[0].abs_diff(pixel[1]) <= 1);
            assert!(pixel[1].abs_diff(pixel[2]) <= 1);
            assert!((120..=140).contains(&pixel[1]));
            assert_eq!(pixel[3], 255);
        }
    }

    #[test]
    fn plane_offsets_select_the_crop_window() {
        // A 4x2 luma plane where only the right 2x2 block is visible.
        let luma = [0_u16, 0, 235, 235, 0, 0, 235, 235];
        let mut pixels = Vec::new();
        assert!(rgba_from_yuv16(
            2,
            2,
            8,
            true,
            true,
            [
                YuvPlane16::with_offset(&luma, 4, 2, 0),
                YuvPlane16::new(&[], 0),
                YuvPlane16::new(&[], 0),
            ],
            ColorMatrix::Bt709,
            YuvRange::Limited,
            &mut pixels,
        ));
        assert!(pixels.iter().step_by(4).all(|white| *white == 255));
    }

    #[test]
    fn malformed_planes_are_reported() {
        let mut pixels = Vec::new();
        assert!(!rgba_from_yuv(
            2,
            2,
            8,
            true,
            true,
            [
                YuvPlane::new(&[16, 16], 2),
                YuvPlane::new(&[], 0),
                YuvPlane::new(&[], 0),
            ],
            ColorMatrix::Auto,
            YuvRange::Limited,
            &mut pixels,
        ));
        assert_eq!(pixels, []);
    }
}
