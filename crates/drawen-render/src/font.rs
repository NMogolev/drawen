//! Font requests, face metadata, and deterministic font matching.
//!
//! The types in this module mirror the parts of Qt's font model that affect
//! layout: a font is a *request* (family preferences, size, weight, and style)
//! that a [`FontDatabase`] resolves to concrete [`FontFace`] resources. Text
//! shaping consumes the resolved faces, so measuring and painting always agree
//! on which font was selected.

use std::{
    collections::{HashMap, VecDeque},
    fmt,
    sync::{Arc, Mutex, OnceLock},
};

use fontdue::{Font as FontdueFont, FontSettings};

/// The logical dots per inch used when a point size is resolved without an
/// explicit DPI value.
pub const DEFAULT_DPI: f32 = 96.0;

const GLYPH_RASTER_CACHE_CAPACITY: usize = 512;
const SYSTEM_FACE_CACHE_CAPACITY: usize = 24;

/// A font family name.
///
/// Families are cheap to clone because the name is shared. Matching against a
/// [`FontDatabase`] is case-insensitive.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FontFamily(Arc<str>);

impl FontFamily {
    /// Creates a family name from any string-like value.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(Arc::from(name.into()))
    }

    /// Returns the family name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FontFamily {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl From<&str> for FontFamily {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl From<String> for FontFamily {
    fn from(name: String) -> Self {
        Self(Arc::from(name))
    }
}

impl From<&String> for FontFamily {
    fn from(name: &String) -> Self {
        Self::new(name.as_str())
    }
}

impl AsRef<str> for FontFamily {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// A CSS-compatible numeric font weight.
///
/// Common values are available as constants such as [`Self::NORMAL`] and
/// [`Self::BOLD`]. Values from 100 through 900 are the interoperable range.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FontWeight(u16);

impl FontWeight {
    /// Weight 100.
    pub const THIN: Self = Self(100);
    /// Weight 200.
    pub const EXTRA_LIGHT: Self = Self(200);
    /// Weight 300.
    pub const LIGHT: Self = Self(300);
    /// Weight 400.
    pub const NORMAL: Self = Self(400);
    /// Weight 500.
    pub const MEDIUM: Self = Self(500);
    /// Weight 600.
    pub const SEMI_BOLD: Self = Self(600);
    /// Weight 700.
    pub const BOLD: Self = Self(700);
    /// Weight 800.
    pub const EXTRA_BOLD: Self = Self(800);
    /// Weight 900.
    pub const BLACK: Self = Self(900);

    /// Creates a weight from a numeric value.
    ///
    /// Returns `None` outside the inclusive `1..=1000` range.
    #[must_use]
    pub const fn new(weight: u16) -> Option<Self> {
        if weight >= 1 && weight <= 1000 {
            Some(Self(weight))
        } else {
            None
        }
    }

    /// Returns the numeric weight.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }

    /// Returns whether this weight is at least bold.
    #[must_use]
    pub const fn is_bold(self) -> bool {
        self.0 >= Self::BOLD.0
    }
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::NORMAL
    }
}

/// The slant of a font face.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum FontStyle {
    /// Upright text.
    #[default]
    Normal,
    /// Cursive text with a dedicated italic face.
    Italic,
    /// Slanted text; some families provide an oblique face distinct from italic.
    Oblique,
}

impl FontStyle {
    /// Returns whether this style is slanted.
    #[must_use]
    pub const fn is_slanted(self) -> bool {
        matches!(self, Self::Italic | Self::Oblique)
    }
}

/// A requested font size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FontSize {
    /// A size in logical pixels.
    Pixels(f32),
    /// A size in typographic points. Resolved with [`FontSize::pixels_at`].
    Points(f32),
}

impl FontSize {
    /// Creates a size in logical pixels.
    #[must_use]
    pub const fn pixels(pixels: f32) -> Self {
        Self::Pixels(pixels)
    }

    /// Creates a size in typographic points.
    #[must_use]
    pub const fn points(points: f32) -> Self {
        Self::Points(points)
    }

    /// Resolves this size to logical pixels at `dpi`.
    ///
    /// Invalid or negative values resolve to zero rather than producing NaN
    /// geometry. A non-finite or non-positive `dpi` falls back to
    /// [`DEFAULT_DPI`].
    #[must_use]
    pub fn pixels_at(self, dpi: f32) -> f32 {
        let dpi = if dpi.is_finite() && dpi > 0.0 {
            dpi
        } else {
            DEFAULT_DPI
        };
        let pixels = match self {
            Self::Pixels(pixels) => pixels,
            Self::Points(points) => points * dpi / 72.0,
        };
        if pixels.is_finite() {
            pixels.max(0.0)
        } else {
            0.0
        }
    }

    /// Returns whether this size resolves to a positive pixel value at `dpi`.
    #[must_use]
    pub fn is_visible_at(self, dpi: f32) -> bool {
        self.pixels_at(dpi) > 0.0
    }
}

/// A font request, analogous to `QFont`.
///
/// A request lists family preferences in priority order. When no registered
/// face matches a requested family, the [`FontDatabase`] uses its default face
/// and then other registered faces as character fallbacks. This mirrors Qt's
/// behavior of never failing to produce a usable font when any font exists.
#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    families: Vec<FontFamily>,
    size: FontSize,
    weight: FontWeight,
    style: FontStyle,
}

impl Font {
    /// Creates a font request using the default size, weight, and style.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a font request for `family` with the default size.
    #[must_use]
    pub fn from_family(family: impl Into<FontFamily>) -> Self {
        Self::new().with_family(family)
    }

    /// Creates a font request with a size in logical pixels.
    #[must_use]
    pub fn from_pixel_size(pixels: f32) -> Self {
        Self::new().with_pixel_size(pixels)
    }

    /// Creates a font request with a size in typographic points.
    #[must_use]
    pub fn from_point_size(points: f32) -> Self {
        Self::new().with_point_size(points)
    }

    /// Returns the family preferences in priority order.
    #[must_use]
    pub fn families(&self) -> &[FontFamily] {
        &self.families
    }

    /// Returns the first family preference, if any.
    #[must_use]
    pub fn family(&self) -> Option<&FontFamily> {
        self.families.first()
    }

    /// Replaces the family preferences with a single family.
    pub fn set_family(&mut self, family: impl Into<FontFamily>) {
        self.families.clear();
        self.families.push(family.into());
    }

    /// Replaces the family preferences.
    pub fn set_families(&mut self, families: impl IntoIterator<Item = FontFamily>) {
        self.families = families.into_iter().collect();
    }

    /// Appends a family preference if it is not already present.
    pub fn add_family(&mut self, family: impl Into<FontFamily>) {
        let family = family.into();
        if !self.families.contains(&family) {
            self.families.push(family);
        }
    }

    /// Returns this request with a single family preference.
    #[must_use]
    pub fn with_family(mut self, family: impl Into<FontFamily>) -> Self {
        self.set_family(family);
        self
    }

    /// Returns this request with the supplied family preferences.
    #[must_use]
    pub fn with_families(mut self, families: impl IntoIterator<Item = FontFamily>) -> Self {
        self.set_families(families);
        self
    }

    /// Returns the requested size.
    #[must_use]
    pub const fn size(&self) -> FontSize {
        self.size
    }

    /// Replaces the requested size.
    pub fn set_size(&mut self, size: FontSize) {
        self.size = size;
    }

    /// Replaces the requested size with logical pixels.
    pub fn set_pixel_size(&mut self, pixels: f32) {
        self.size = FontSize::Pixels(pixels);
    }

    /// Replaces the requested size with typographic points.
    pub fn set_point_size(&mut self, points: f32) {
        self.size = FontSize::Points(points);
    }

    /// Returns this request with the supplied size.
    #[must_use]
    pub fn with_size(mut self, size: FontSize) -> Self {
        self.size = size;
        self
    }

    /// Returns this request with a size in logical pixels.
    #[must_use]
    pub fn with_pixel_size(mut self, pixels: f32) -> Self {
        self.set_pixel_size(pixels);
        self
    }

    /// Returns this request with a size in typographic points.
    #[must_use]
    pub fn with_point_size(mut self, points: f32) -> Self {
        self.set_point_size(points);
        self
    }

    /// Resolves the requested size to logical pixels at `dpi`.
    #[must_use]
    pub fn pixel_size(&self, dpi: f32) -> f32 {
        self.size.pixels_at(dpi)
    }

    /// Returns the requested weight.
    #[must_use]
    pub const fn weight(&self) -> FontWeight {
        self.weight
    }

    /// Replaces the requested weight.
    pub fn set_weight(&mut self, weight: FontWeight) {
        self.weight = weight;
    }

    /// Returns this request with the supplied weight.
    #[must_use]
    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    /// Returns the requested style.
    #[must_use]
    pub const fn style(&self) -> FontStyle {
        self.style
    }

    /// Replaces the requested style.
    pub fn set_style(&mut self, style: FontStyle) {
        self.style = style;
    }

    /// Returns this request with the supplied style.
    #[must_use]
    pub fn with_style(mut self, style: FontStyle) -> Self {
        self.style = style;
        self
    }
}

impl Default for Font {
    fn default() -> Self {
        Self {
            families: Vec::new(),
            size: FontSize::Pixels(14.0),
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        }
    }
}

/// Metadata used by [`FontDatabase`] to match a concrete face.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FontDescriptor {
    family: FontFamily,
    weight: FontWeight,
    style: FontStyle,
}

impl FontDescriptor {
    /// Creates a descriptor for a face.
    #[must_use]
    pub fn new(family: impl Into<FontFamily>, weight: FontWeight, style: FontStyle) -> Self {
        Self {
            family: family.into(),
            weight,
            style,
        }
    }

    /// Returns the face's family.
    #[must_use]
    pub const fn family(&self) -> &FontFamily {
        &self.family
    }

    /// Returns the face's weight.
    #[must_use]
    pub const fn weight(&self) -> FontWeight {
        self.weight
    }

    /// Returns the face's style.
    #[must_use]
    pub const fn style(&self) -> FontStyle {
        self.style
    }
}

impl Default for FontDescriptor {
    fn default() -> Self {
        Self {
            family: FontFamily::new("Sans"),
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        }
    }
}

/// An immutable font resource used by shaped glyph runs.
#[derive(Clone)]
pub struct FontFace {
    inner: Arc<FontdueFont>,
    descriptor: FontDescriptor,
    data: Arc<[u8]>,
    index: u32,
    glyph_rasters: Arc<Mutex<GlyphRasterCache>>,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct GlyphRasterKey {
    glyph_id: u16,
    font_size: u32,
}

#[derive(Default)]
struct GlyphRasterCache {
    entries: HashMap<GlyphRasterKey, Arc<GlyphBitmap>>,
    insertion_order: VecDeque<GlyphRasterKey>,
}

impl FontFace {
    /// Parses a TrueType or OpenType font from its complete binary contents.
    ///
    /// The face family is taken from the font's full name when available, with
    /// normal weight and style. Use [`Self::from_bytes_with_descriptor`] to
    /// register a family, weight, and style explicitly.
    ///
    /// # Errors
    ///
    /// Returns [`FontError`] when `bytes` do not contain a supported font.
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Result<Self, FontError> {
        Self::from_bytes_with_index(bytes, 0)
    }

    /// Parses one face from a font collection and infers its family name.
    ///
    /// `index` selects a face inside a TrueType/OpenType collection (`.ttc`).
    /// Plain font files use index `0`. Use
    /// [`Self::from_bytes_with_descriptor_and_index`] to register a family,
    /// weight, and style explicitly.
    ///
    /// # Errors
    ///
    /// Returns [`FontError`] when `bytes` do not contain a supported font or
    /// the collection does not contain `index`.
    pub fn from_bytes_with_index(bytes: impl Into<Vec<u8>>, index: u32) -> Result<Self, FontError> {
        let data: Vec<u8> = bytes.into();
        let font = parse_font(&data, index)?;
        let family = font
            .name()
            .map_or_else(|| FontFamily::new("Sans"), FontFamily::new);
        let descriptor = FontDescriptor::new(family, FontWeight::NORMAL, FontStyle::Normal);
        Ok(Self::from_parts(font, descriptor, data, index))
    }

    /// Parses a font and registers it under an explicit descriptor.
    ///
    /// # Errors
    ///
    /// Returns [`FontError`] when `bytes` do not contain a supported font.
    pub fn from_bytes_with_descriptor(
        bytes: impl Into<Vec<u8>>,
        descriptor: FontDescriptor,
    ) -> Result<Self, FontError> {
        Self::from_bytes_with_descriptor_and_index(bytes, descriptor, 0)
    }

    /// Parses one face from a font collection under an explicit descriptor.
    ///
    /// # Errors
    ///
    /// Returns [`FontError`] when `bytes` do not contain a supported font or
    /// the collection does not contain `index`.
    pub fn from_bytes_with_descriptor_and_index(
        bytes: impl Into<Vec<u8>>,
        descriptor: FontDescriptor,
        index: u32,
    ) -> Result<Self, FontError> {
        let data: Vec<u8> = bytes.into();
        let font = parse_font(&data, index)?;
        Ok(Self::from_parts(font, descriptor, data, index))
    }

    fn from_parts(
        font: FontdueFont,
        descriptor: FontDescriptor,
        data: Vec<u8>,
        index: u32,
    ) -> Self {
        Self {
            inner: Arc::new(font),
            descriptor,
            data: Arc::from(data),
            index,
            glyph_rasters: Arc::new(Mutex::new(GlyphRasterCache::default())),
        }
    }

    /// Returns Drawen's bundled Ubuntu Light font.
    ///
    /// The font is licensed under the Ubuntu Font Licence 1.0; its full text is
    /// included alongside the font asset in this crate.
    ///
    /// # Panics
    ///
    /// Panics if the bundled font asset is corrupt. This is a build-time
    /// invariant maintained with the crate source.
    #[must_use]
    pub fn ubuntu_light() -> Self {
        static UBUNTU_LIGHT: OnceLock<FontFace> = OnceLock::new();

        UBUNTU_LIGHT
            .get_or_init(|| {
                Self::from_bytes_with_descriptor(
                    include_bytes!("../assets/Ubuntu-Light.ttf").as_slice(),
                    FontDescriptor::new("Ubuntu", FontWeight::LIGHT, FontStyle::Normal),
                )
                .expect("the bundled Ubuntu Light font must be valid")
            })
            .clone()
    }

    /// Returns the face's registration metadata.
    #[must_use]
    pub const fn descriptor(&self) -> &FontDescriptor {
        &self.descriptor
    }

    /// Returns the face's family.
    #[must_use]
    pub const fn family(&self) -> &FontFamily {
        self.descriptor.family()
    }

    /// Returns the face's weight.
    #[must_use]
    pub const fn weight(&self) -> FontWeight {
        self.descriptor.weight()
    }

    /// Returns the face's style.
    #[must_use]
    pub const fn style(&self) -> FontStyle {
        self.descriptor.style()
    }

    /// Returns the face index inside its source font collection.
    ///
    /// This is `0` for ordinary single-face files.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// Returns whether this face contains a glyph for `character`.
    #[must_use]
    pub fn has_glyph(&self, character: char) -> bool {
        self.inner.has_glyph(character)
    }

    /// Rasterizes one glyph to an 8-bit coverage bitmap.
    #[must_use]
    pub fn rasterize(&self, glyph_id: u16, font_size: f32) -> GlyphBitmap {
        self.rasterize_cached(glyph_id, font_size).as_ref().clone()
    }

    /// Rasterizes one glyph and shares the cached coverage bitmap between
    /// repeated draws at the same physical pixel size.
    ///
    /// The cache is shared by cloned handles for this font face and retains up
    /// to 512 glyph-size pairs. Its bounded FIFO eviction prevents arbitrary
    /// text or scale changes from growing renderer memory without limit.
    #[must_use]
    pub fn rasterize_cached(&self, glyph_id: u16, font_size: f32) -> Arc<GlyphBitmap> {
        let key = GlyphRasterKey {
            glyph_id,
            font_size: font_size.to_bits(),
        };
        let mut cache = self
            .glyph_rasters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(bitmap) = cache.entries.get(&key) {
            return Arc::clone(bitmap);
        }
        let (metrics, coverage) = self.inner.rasterize_indexed(glyph_id, font_size);
        let bitmap = Arc::new(GlyphBitmap {
            width: metrics.width,
            height: metrics.height,
            coverage,
        });
        if cache.entries.len() == GLYPH_RASTER_CACHE_CAPACITY
            && let Some(oldest) = cache.insertion_order.pop_front()
        {
            cache.entries.remove(&oldest);
        }
        cache.insertion_order.push_back(key);
        cache.entries.insert(key, Arc::clone(&bitmap));
        bitmap
    }

    /// Returns the horizontal line metrics for `font_size` logical pixels.
    ///
    /// Returns `None` for a non-positive or non-finite size, or when the font
    /// does not provide horizontal line metrics.
    #[must_use]
    pub fn metrics(&self, font_size: f32) -> Option<FontMetrics> {
        if !font_size.is_finite() || font_size <= 0.0 {
            return None;
        }
        self.inner
            .horizontal_line_metrics(font_size)
            .map(|metrics| FontMetrics {
                ascent: metrics.ascent,
                descent: -metrics.descent,
                leading: metrics.line_gap,
                height: metrics.new_line_size,
            })
    }

    /// Returns whether both handles refer to the same face of the same file.
    fn same_face(&self, other: &Self) -> bool {
        self.index == other.index && self.inner.file_hash() == other.inner.file_hash()
    }

    pub(crate) fn font(&self) -> &FontdueFont {
        &self.inner
    }

    /// Returns the raw OpenType file used to build this face.
    ///
    /// Shaping consumes the same bytes so `HarfRust` and `fontdue` always agree
    /// on the face's glyph identifiers and metrics.
    pub(crate) fn data(&self) -> &[u8] {
        &self.data
    }
}

impl fmt::Debug for FontFace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FontFace")
            .field("descriptor", &self.descriptor)
            .field("index", &self.index)
            .field("name", &self.inner.name())
            .finish_non_exhaustive()
    }
}

impl PartialEq for FontFace {
    fn eq(&self, other: &Self) -> bool {
        self.descriptor == other.descriptor && self.same_face(other)
    }
}

fn parse_font(data: &[u8], index: u32) -> Result<FontdueFont, FontError> {
    FontdueFont::from_bytes(
        data,
        FontSettings {
            collection_index: index,
            ..FontSettings::default()
        },
    )
    .map_err(FontError)
}

/// A font parsing failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontError(&'static str);

impl fmt::Display for FontError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "could not parse font: {}", self.0)
    }
}

impl std::error::Error for FontError {}

/// Horizontal metrics for one line of text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontMetrics {
    ascent: f32,
    descent: f32,
    leading: f32,
    height: f32,
}

impl FontMetrics {
    /// Returns the distance above the baseline reserved by the font.
    #[must_use]
    pub const fn ascent(self) -> f32 {
        self.ascent
    }

    /// Returns the distance below the baseline reserved by the font.
    #[must_use]
    pub const fn descent(self) -> f32 {
        self.descent
    }

    /// Returns the font-recommended gap between lines.
    #[must_use]
    pub const fn leading(self) -> f32 {
        self.leading
    }

    /// Returns the total line height.
    #[must_use]
    pub const fn height(self) -> f32 {
        self.height
    }

    /// Returns ascent plus descent plus leading.
    #[must_use]
    pub fn line_spacing(self) -> f32 {
        self.ascent + self.descent + self.leading
    }
}

/// A rasterized glyph's alpha coverage, arranged in row-major order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlyphBitmap {
    width: usize,
    height: usize,
    coverage: Vec<u8>,
}

impl GlyphBitmap {
    /// Returns the bitmap width in pixels.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Returns the bitmap height in pixels.
    #[must_use]
    pub const fn height(&self) -> usize {
        self.height
    }

    /// Returns row-major 8-bit glyph coverage, from transparent to opaque.
    #[must_use]
    pub fn coverage(&self) -> &[u8] {
        &self.coverage
    }
}

/// A registry of font faces with Qt-style family and style matching.
///
/// Faces are matched case-insensitively by family. Within a family the closest
/// style and weight are selected; when no family matches, the database default
/// is used. Every registered face is also available as a per-character
/// fallback.
///
/// A database can additionally keep a lazy index of the operating system's
/// installed faces. Those faces are parsed only when a character needs them,
/// which makes multilingual fallback practical without paying the cost of
/// loading every installed font up front. See [`Self::discover_system_fallbacks`].
#[derive(Clone, Debug, Default)]
pub struct FontDatabase {
    faces: Vec<FontFace>,
    default_face: Option<usize>,
    system: Option<Arc<SystemFontSet>>,
}

impl PartialEq for FontDatabase {
    fn eq(&self, other: &Self) -> bool {
        self.faces == other.faces
            && self.default_face == other.default_face
            && self.system.is_some() == other.system.is_some()
    }
}

struct SystemFontSet {
    database: fontdb::Database,
    faces: Vec<SystemFace>,
    cache: Mutex<SystemFallbackCache>,
}

impl fmt::Debug for SystemFontSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SystemFontSet")
            .field("faces", &self.faces.len())
            .field("cached_faces", &self.lock_cache().loaded.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
struct SystemFace {
    id: fontdb::ID,
    family: FontFamily,
    family_key: String,
    weight: FontWeight,
    style: FontStyle,
}

#[derive(Debug, Default)]
struct SystemFallbackCache {
    loaded: HashMap<fontdb::ID, FontFace>,
    loaded_order: VecDeque<fontdb::ID>,
    coverage: HashMap<char, Option<fontdb::ID>>,
}

impl SystemFontSet {
    fn discover() -> Option<Self> {
        let mut database = fontdb::Database::new();
        database.load_system_fonts();
        let faces: Vec<SystemFace> = database
            .faces()
            .filter_map(|face| {
                let family = face
                    .families
                    .first()
                    .map_or_else(|| face.post_script_name.clone(), |(name, _)| name.clone());
                if family.is_empty() {
                    return None;
                }
                Some(SystemFace {
                    id: face.id,
                    family: FontFamily::new(family.clone()),
                    family_key: family.to_lowercase(),
                    weight: FontWeight::new(face.weight.0).unwrap_or(FontWeight::NORMAL),
                    style: system_style(face.style),
                })
            })
            .collect();
        if faces.is_empty() {
            return None;
        }
        Some(Self {
            database,
            faces,
            cache: Mutex::new(SystemFallbackCache::default()),
        })
    }

    fn face_for_char(
        &self,
        character: char,
        weight: FontWeight,
        style: FontStyle,
    ) -> Option<FontFace> {
        let cached = self.lock_cache().coverage.get(&character).copied();
        match cached {
            Some(Some(id)) => {
                if let Some(face) = self.loaded_face(id) {
                    return Some(face);
                }
                // The parsed face was evicted; fall through and load it again.
            }
            Some(None) => return None,
            None => {}
        }

        let group = script_group(character);
        let mut order: Vec<usize> = (0..self.faces.len()).collect();
        order.sort_by_key(|&index| {
            let face = &self.faces[index];
            (
                family_script_score(&face.family_key, group),
                style_distance(face.style, style),
                weight_distance(face.weight, weight),
                index,
            )
        });

        let mut found = None;
        for index in order {
            let face = &self.faces[index];
            if let Some(loaded) = self.loaded_face(face.id) {
                if loaded.has_glyph(character) {
                    found = Some((face.id, loaded));
                    break;
                }
                continue;
            }
            let Some((bytes, face_index)) = self
                .database
                .with_face_data(face.id, |data, index| (data.to_vec(), index))
            else {
                continue;
            };
            let descriptor = FontDescriptor::new(face.family.clone(), face.weight, face.style);
            let Ok(loaded) =
                FontFace::from_bytes_with_descriptor_and_index(bytes, descriptor, face_index)
            else {
                continue;
            };
            let covers = loaded.has_glyph(character);
            self.store_loaded(face.id, loaded.clone());
            if covers {
                found = Some((face.id, loaded));
                break;
            }
        }

        let mut cache = self.lock_cache();
        cache
            .coverage
            .insert(character, found.as_ref().map(|(id, _)| *id));
        drop(cache);
        found.map(|(_, face)| face)
    }

    fn loaded_face(&self, id: fontdb::ID) -> Option<FontFace> {
        self.lock_cache().loaded.get(&id).cloned()
    }

    fn store_loaded(&self, id: fontdb::ID, face: FontFace) {
        let mut cache = self.lock_cache();
        if let Some(existing) = cache.loaded.get_mut(&id) {
            *existing = face;
            return;
        }
        if cache.loaded.len() == SYSTEM_FACE_CACHE_CAPACITY
            && let Some(oldest) = cache.loaded_order.pop_front()
        {
            cache.loaded.remove(&oldest);
        }
        cache.loaded_order.push_back(id);
        cache.loaded.insert(id, face);
    }

    fn lock_cache(&self) -> std::sync::MutexGuard<'_, SystemFallbackCache> {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn system_style(style: fontdb::Style) -> FontStyle {
    match style {
        fontdb::Style::Normal => FontStyle::Normal,
        fontdb::Style::Italic => FontStyle::Italic,
        fontdb::Style::Oblique => FontStyle::Oblique,
    }
}

impl FontDatabase {
    /// Creates an empty database.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a database containing Drawen's bundled Ubuntu Light face.
    #[must_use]
    pub fn bundled() -> Self {
        let mut database = Self::new();
        database.add_face(FontFace::ubuntu_light());
        database
    }

    /// Enumerates the operating system's installed fonts and registers the
    /// faces that Drawen can shape and rasterize.
    ///
    /// Enumeration is delegated to `fontdb`, which scans the platform's
    /// standard font directories without calling into a particular windowing
    /// toolkit. Every face of a font collection is registered with its
    /// collection index.
    ///
    /// The returned database uses the first discovered face as its default.
    /// Applications that want Drawen's deterministic bundled face should call
    /// [`Self::set_default_face`] afterwards.
    #[must_use]
    pub fn load_system_fonts() -> Self {
        let mut database = Self::new();
        database.discover_system_fonts();
        database
    }

    /// Adds every suitable installed system font to this database.
    ///
    /// Existing faces and the current default selection are preserved.
    pub fn discover_system_fonts(&mut self) {
        let mut system = fontdb::Database::new();
        system.load_system_fonts();
        for face in system.faces() {
            let Some(bytes) = system.with_face_data(face.id, |data, _| data.to_vec()) else {
                continue;
            };
            let family = face
                .families
                .first()
                .map_or_else(|| face.post_script_name.clone(), |(name, _)| name.clone());
            let weight = FontWeight::new(face.weight.0).unwrap_or(FontWeight::NORMAL);
            let descriptor = FontDescriptor::new(family, weight, system_style(face.style));
            let _ = self.add_font_with_index(descriptor, bytes, face.index);
        }
    }

    /// Enables lazy per-character fallback through the operating system's
    /// installed fonts.
    ///
    /// Discovery records family, weight, style, and collection index only;
    /// font files are opened and parsed on demand when a character is not
    /// covered by the registered faces. Candidates are ranked by script, then
    /// by style and weight proximity, so common scripts resolve without
    /// touching unrelated fonts.
    ///
    /// Unlike [`Self::discover_system_fonts`], this does not change the
    /// registered faces or the default face.
    pub fn discover_system_fallbacks(&mut self) {
        self.system = SystemFontSet::discover().map(Arc::new);
    }

    /// Returns whether lazy system fallback is enabled.
    #[must_use]
    pub fn has_system_fallbacks(&self) -> bool {
        self.system.is_some()
    }

    /// Returns a lazily loaded system face that covers `character`.
    ///
    /// The face is cached for later calls. Returns `None` when system fallback
    /// is disabled, no installed face covers the character, or the system font
    /// cannot be parsed by Drawen's rasterizer.
    #[must_use]
    pub fn fallback_face(
        &self,
        character: char,
        weight: FontWeight,
        style: FontStyle,
    ) -> Option<FontFace> {
        self.system
            .as_ref()?
            .face_for_char(character, weight, style)
    }

    /// Creates a database whose default face is `face`.
    #[must_use]
    pub fn with_default(face: FontFace) -> Self {
        let mut database = Self::new();
        database.add_face(face);
        database
    }

    /// Registers `face` and returns its index.
    ///
    /// The first registered face becomes the default.
    pub fn add_face(&mut self, face: FontFace) -> usize {
        let index = self.faces.len();
        self.faces.push(face);
        if self.default_face.is_none() {
            self.default_face = Some(index);
        }
        index
    }

    /// Parses and registers a face under `descriptor`.
    ///
    /// # Errors
    ///
    /// Returns [`FontError`] when `bytes` do not contain a supported font.
    pub fn add_font(
        &mut self,
        descriptor: FontDescriptor,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<FontFace, FontError> {
        self.add_font_with_index(descriptor, bytes, 0)
    }

    /// Parses and registers one face of a font collection.
    ///
    /// `index` selects the face inside a TrueType/OpenType collection (`.ttc`).
    ///
    /// # Errors
    ///
    /// Returns [`FontError`] when `bytes` do not contain a supported font or
    /// the collection does not contain `index`.
    pub fn add_font_with_index(
        &mut self,
        descriptor: FontDescriptor,
        bytes: impl Into<Vec<u8>>,
        index: u32,
    ) -> Result<FontFace, FontError> {
        let face = FontFace::from_bytes_with_descriptor_and_index(bytes, descriptor, index)?;
        self.add_face(face.clone());
        Ok(face)
    }

    /// Returns the registered faces in insertion order.
    #[must_use]
    pub fn faces(&self) -> &[FontFace] {
        &self.faces
    }

    /// Returns the unique registered family names in insertion order.
    #[must_use]
    pub fn families(&self) -> Vec<&str> {
        let mut families = Vec::new();
        for face in &self.faces {
            let name = face.family().as_str();
            if !families
                .iter()
                .any(|existing: &&str| existing.eq_ignore_ascii_case(name))
            {
                families.push(name);
            }
        }
        families
    }

    /// Returns the faces registered for `family`, case-insensitively.
    pub fn faces_for_family<'a>(
        &'a self,
        family: &'a str,
    ) -> impl Iterator<Item = &'a FontFace> + 'a {
        self.faces
            .iter()
            .filter(move |face| face.family().as_str().eq_ignore_ascii_case(family))
    }

    /// Returns the default face used when no family matches.
    #[must_use]
    pub fn default_face(&self) -> Option<&FontFace> {
        self.default_face.and_then(|index| self.faces.get(index))
    }

    /// Replaces the default face by index.
    ///
    /// Returns `false` when `index` does not name a registered face.
    pub fn set_default_face(&mut self, index: usize) -> bool {
        if index < self.faces.len() {
            self.default_face = Some(index);
            true
        } else {
            false
        }
    }

    /// Returns whether no faces are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// Resolves `font` to an ordered list of faces.
    ///
    /// Requested families come first, followed by the default face when no
    /// family matched, and finally every other registered face as a
    /// per-character fallback. The returned list is never empty when the
    /// database contains a face.
    #[must_use]
    pub fn resolve(&self, font: &Font) -> Vec<FontFace> {
        let mut resolved = Vec::new();
        for family in font.families() {
            if let Some(face) = self.best_face(family, font.weight(), font.style()) {
                push_unique(&mut resolved, face);
            }
        }
        if resolved.is_empty()
            && let Some(face) = self.default_face()
        {
            push_unique(&mut resolved, face.clone());
        }
        for face in &self.faces {
            push_unique(&mut resolved, face.clone());
        }
        resolved
    }

    /// Resolves `font` and extends the result with system fallback faces that
    /// cover characters missing from the registered faces.
    ///
    /// This is [`Self::resolve`] plus lazy coverage-driven fallback for the
    /// characters that actually occur in `text`. Characters that no installed
    /// face covers are left to the shaper's replacement policy.
    #[must_use]
    pub fn resolve_for_text(&self, font: &Font, text: &str) -> Vec<FontFace> {
        let mut faces = self.resolve(font);
        if self.system.is_none() {
            return faces;
        }

        let mut missing = Vec::<char>::new();
        for character in text.chars() {
            if is_ignorable_for_coverage(character) || missing.contains(&character) {
                continue;
            }
            if faces.iter().any(|face| face.has_glyph(character)) {
                continue;
            }
            missing.push(character);
        }
        for character in missing {
            if let Some(face) = self.fallback_face(character, font.weight(), font.style()) {
                push_unique(&mut faces, face);
            }
        }
        faces
    }

    fn best_face(
        &self,
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
    ) -> Option<FontFace> {
        self.faces
            .iter()
            .filter(|face| face.family().as_str().eq_ignore_ascii_case(family.as_str()))
            .min_by_key(|face| {
                (
                    style_distance(face.style(), style),
                    weight_distance(face.weight(), weight),
                )
            })
            .cloned()
    }
}

fn push_unique(faces: &mut Vec<FontFace>, face: FontFace) {
    if !faces.iter().any(|existing| existing.same_face(&face)) {
        faces.push(face);
    }
}

/// Returns whether `character` should be ignored when checking a font's
/// coverage.
///
/// Controls, format characters, and variation selectors do not have visible
/// glyphs of their own, so requiring a face to cover them would reject the
/// fonts that shape the surrounding cluster correctly.
pub(crate) fn is_ignorable_for_coverage(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{00AD}'
                | '\u{034F}'
                | '\u{061C}'
                | '\u{115F}'..='\u{1160}'
                | '\u{17B4}'..='\u{17B5}'
                | '\u{180B}'..='\u{180F}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{3164}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{FEFF}'
                | '\u{FFA0}'
                | '\u{FFF0}'..='\u{FFF8}'
                | '\u{1BCA0}'..='\u{1BCA3}'
                | '\u{1D173}'..='\u{1D17A}'
                | '\u{E0000}'..='\u{E0FFF}'
        )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScriptGroup {
    Default,
    Han,
    Japanese,
    Korean,
    Arabic,
    Hebrew,
    Indic,
    Thai,
    Emoji,
}

fn script_group(character: char) -> ScriptGroup {
    match character {
        '\u{0600}'..='\u{06FF}'
        | '\u{0750}'..='\u{077F}'
        | '\u{08A0}'..='\u{08FF}'
        | '\u{FB50}'..='\u{FDFF}'
        | '\u{FE70}'..='\u{FEFF}' => ScriptGroup::Arabic,
        '\u{0590}'..='\u{05FF}' | '\u{FB1D}'..='\u{FB4F}' => ScriptGroup::Hebrew,
        '\u{0900}'..='\u{0DFF}' | '\u{0F00}'..='\u{0FFF}' => ScriptGroup::Indic,
        '\u{0E00}'..='\u{0EFF}' => ScriptGroup::Thai,
        '\u{3040}'..='\u{309F}'
        | '\u{30A0}'..='\u{30FF}'
        | '\u{31F0}'..='\u{31FF}'
        | '\u{FF66}'..='\u{FF9D}' => ScriptGroup::Japanese,
        '\u{1100}'..='\u{11FF}'
        | '\u{3130}'..='\u{318F}'
        | '\u{A960}'..='\u{A97F}'
        | '\u{AC00}'..='\u{D7AF}' => ScriptGroup::Korean,
        '\u{3000}'..='\u{303F}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{20000}'..='\u{2FA1F}' => ScriptGroup::Han,
        '\u{2600}'..='\u{27BF}' | '\u{1F000}'..='\u{1FAFF}' | '\u{FE0F}' => ScriptGroup::Emoji,
        _ => ScriptGroup::Default,
    }
}

fn family_script_score(family: &str, group: ScriptGroup) -> u8 {
    if group != ScriptGroup::Emoji && is_symbol_family(family) {
        return 3;
    }
    if keywords_for(group)
        .iter()
        .any(|keyword| family.contains(keyword))
    {
        return 0;
    }
    if group != ScriptGroup::Default
        && keywords_for(ScriptGroup::Default)
            .iter()
            .any(|keyword| family.contains(keyword))
    {
        return 1;
    }
    2
}

fn is_symbol_family(family: &str) -> bool {
    [
        "emoji",
        "symbol",
        "dingbat",
        "wingdings",
        "webdings",
        "marlett",
    ]
    .iter()
    .any(|keyword| family.contains(keyword))
}

const DEFAULT_FAMILY_KEYWORDS: &[&str] = &[
    "segoe ui",
    "arial",
    "helvetica",
    "noto sans",
    "noto serif",
    "dejavu sans",
    "liberation sans",
    "ubuntu",
    "roboto",
    "cantarell",
    "inter",
    "tahoma",
    "verdana",
    "calibri",
    "system-ui",
];
const HAN_FAMILY_KEYWORDS: &[&str] = &[
    "yahei",
    "jhenghei",
    "simsun",
    "simhei",
    "nsimsun",
    "song",
    "ming",
    "kai",
    "fangsong",
    "cjk",
    "source han",
    "noto sans sc",
    "noto sans tc",
    "noto sans hk",
    "wenquanyi",
    "droid sans fallback",
    "hiragino",
    "pingfang",
    "yu gothic",
    "meiryo",
    "ms gothic",
    "ms mincho",
    "malgun",
    "gulim",
    "dotum",
    "batang",
    "noto sans kr",
    "nanum",
    "apple sd gothic",
];
const JAPANESE_FAMILY_KEYWORDS: &[&str] = &[
    "yu gothic",
    "yu mincho",
    "meiryo",
    "ms gothic",
    "ms mincho",
    "hiragino",
    "osaka",
    "noto sans jp",
    "noto serif jp",
    "cjk jp",
    "pingfang",
];
const KOREAN_FAMILY_KEYWORDS: &[&str] = &[
    "malgun",
    "gulim",
    "dotum",
    "batang",
    "gungsuh",
    "apple sd gothic",
    "noto sans kr",
    "noto serif kr",
    "cjk kr",
    "nanum",
];
const ARABIC_FAMILY_KEYWORDS: &[&str] = &[
    "arabic",
    "segoe ui",
    "tahoma",
    "arial",
    "geeza",
    "amiri",
    "scheherazade",
    "noto naskh",
    "noto kufi",
    "dejavu sans",
];
const HEBREW_FAMILY_KEYWORDS: &[&str] = &[
    "hebrew",
    "segoe ui",
    "arial",
    "tahoma",
    "david",
    "gisha",
    "frankruehl",
    "noto rashi",
];
const INDIC_FAMILY_KEYWORDS: &[&str] = &[
    "devanagari",
    "bengali",
    "gurmukhi",
    "gujarati",
    "tamil",
    "telugu",
    "kannada",
    "malayalam",
    "sinhala",
    "oriya",
    "odia",
    "nirmala",
    "mangal",
    "kokila",
    "utsaah",
    "aparajita",
    "sanskrit",
    "lohit",
    "samyak",
    "noto sans",
    "noto serif",
];
const THAI_FAMILY_KEYWORDS: &[&str] = &[
    "thai",
    "leelawadee",
    "tahoma",
    "angsana",
    "browallia",
    "loma",
    "garuda",
    "norasi",
    "kinnari",
];
const EMOJI_FAMILY_KEYWORDS: &[&str] = &[
    "emoji",
    "symbola",
    "segoe ui symbol",
    "apple color emoji",
    "noto color emoji",
    "twemoji",
];

fn keywords_for(group: ScriptGroup) -> &'static [&'static str] {
    match group {
        ScriptGroup::Default => DEFAULT_FAMILY_KEYWORDS,
        ScriptGroup::Han => HAN_FAMILY_KEYWORDS,
        ScriptGroup::Japanese => JAPANESE_FAMILY_KEYWORDS,
        ScriptGroup::Korean => KOREAN_FAMILY_KEYWORDS,
        ScriptGroup::Arabic => ARABIC_FAMILY_KEYWORDS,
        ScriptGroup::Hebrew => HEBREW_FAMILY_KEYWORDS,
        ScriptGroup::Indic => INDIC_FAMILY_KEYWORDS,
        ScriptGroup::Thai => THAI_FAMILY_KEYWORDS,
        ScriptGroup::Emoji => EMOJI_FAMILY_KEYWORDS,
    }
}

const fn style_distance(face: FontStyle, requested: FontStyle) -> u8 {
    match (face, requested) {
        (FontStyle::Normal, FontStyle::Normal)
        | (FontStyle::Italic, FontStyle::Italic)
        | (FontStyle::Oblique, FontStyle::Oblique) => 0,
        (FontStyle::Italic, FontStyle::Oblique) | (FontStyle::Oblique, FontStyle::Italic) => 1,
        _ => 2,
    }
}

const fn weight_distance(face: FontWeight, requested: FontWeight) -> u16 {
    face.value().abs_diff(requested.value())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        Font, FontDatabase, FontDescriptor, FontFace, FontFamily, FontSize, FontStyle, FontWeight,
        ScriptGroup, family_script_score, is_ignorable_for_coverage, script_group,
    };

    fn database() -> FontDatabase {
        let mut database = FontDatabase::with_default(
            FontFace::from_bytes_with_descriptor(
                include_bytes!("../assets/Ubuntu-Light.ttf").as_slice(),
                FontDescriptor::new("Ubuntu", FontWeight::LIGHT, FontStyle::Normal),
            )
            .expect("bundled font is valid"),
        );
        database
            .add_font(
                FontDescriptor::new("Test Sans", FontWeight::NORMAL, FontStyle::Normal),
                include_bytes!("../assets/Ubuntu-Light.ttf").as_slice(),
            )
            .expect("test font is valid");
        database
    }

    #[test]
    fn resolves_family_weight_and_style_by_closest_match() {
        let database = database();
        let font = Font::from_family("test sans")
            .with_weight(FontWeight::BOLD)
            .with_style(FontStyle::Italic);

        let faces = database.resolve(&font);
        assert_eq!(faces[0].family().as_str(), "Test Sans");
        assert_eq!(faces[0].weight(), FontWeight::NORMAL);
    }

    #[test]
    fn falls_back_to_the_default_face_for_unknown_families() {
        let database = database();
        let faces = database.resolve(&Font::from_family("Missing Family"));

        assert_eq!(faces[0].family().as_str(), "Ubuntu");
        assert_eq!(database.families(), vec!["Ubuntu", "Test Sans"]);
    }

    #[test]
    fn empty_family_list_uses_the_default_face_first() {
        let database = database();
        let faces = database.resolve(&Font::new());

        assert_eq!(faces[0].family().as_str(), "Ubuntu");
        // Both registered faces use the same physical font file, so the
        // fallback list collapses them into a single face.
        assert_eq!(faces.len(), 1);
    }

    #[test]
    fn fallback_lists_do_not_repeat_a_physical_font_file() {
        let database = database();
        let faces = database.resolve(&Font::from_family("Test Sans"));

        assert_eq!(faces[0].family().as_str(), "Test Sans");
        assert_eq!(faces.len(), 1);
    }

    #[test]
    fn converts_points_using_the_supplied_dpi() {
        assert!((FontSize::points(12.0).pixels_at(96.0) - 16.0).abs() < f32::EPSILON);
        assert!((FontSize::pixels(16.0).pixels_at(144.0) - 16.0).abs() < f32::EPSILON);
        assert!((FontSize::points(12.0).pixels_at(f32::NAN) - 16.0).abs() < f32::EPSILON);
    }

    #[test]
    fn reports_line_metrics_for_a_visible_size() {
        let face = FontFace::ubuntu_light();
        let metrics = face.metrics(16.0).expect("font exposes line metrics");

        assert!(metrics.ascent() > 0.0);
        assert!(metrics.descent() > 0.0);
        assert!(metrics.height() > 0.0);
        assert!(face.metrics(0.0).is_none());
    }

    #[test]
    fn caches_rasterized_glyphs_across_font_face_clones() {
        let face = FontFace::ubuntu_light();
        let clone = face.clone();
        let glyph_id = face.font().lookup_glyph_index('T');

        let first = face.rasterize_cached(glyph_id, 16.0);
        let second = clone.rasterize_cached(glyph_id, 16.0);

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn font_family_is_cheap_to_share_and_compares_by_name() {
        let family = FontFamily::new("Ubuntu");
        let copy = family.clone();

        assert_eq!(family, copy);
        assert_eq!(family.as_str(), "Ubuntu");
    }

    #[test]
    fn reports_the_collection_index_of_a_face() {
        let face = FontFace::from_bytes_with_index(
            include_bytes!("../assets/Ubuntu-Light.ttf").as_slice(),
            0,
        )
        .expect("bundled font is valid");

        assert_eq!(face.index(), 0);
        assert!(
            FontFace::from_bytes_with_index(
                include_bytes!("../assets/Ubuntu-Light.ttf").as_slice(),
                1,
            )
            .is_err()
        );
    }

    #[test]
    fn classifies_common_scripts_for_fallback_ranking() {
        assert_eq!(script_group('A'), ScriptGroup::Default);
        assert_eq!(script_group('Я'), ScriptGroup::Default);
        assert_eq!(script_group('Ω'), ScriptGroup::Default);
        assert_eq!(script_group('س'), ScriptGroup::Arabic);
        assert_eq!(script_group('ש'), ScriptGroup::Hebrew);
        assert_eq!(script_group('क'), ScriptGroup::Indic);
        assert_eq!(script_group('ก'), ScriptGroup::Thai);
        assert_eq!(script_group('あ'), ScriptGroup::Japanese);
        assert_eq!(script_group('ア'), ScriptGroup::Japanese);
        assert_eq!(script_group('한'), ScriptGroup::Korean);
        assert_eq!(script_group('日'), ScriptGroup::Han);
        assert_eq!(script_group('🦀'), ScriptGroup::Emoji);
    }

    #[test]
    fn ranks_script_specific_families_before_generic_faces() {
        assert!(
            family_script_score("microsoft yahei", ScriptGroup::Han)
                < family_script_score("arial", ScriptGroup::Han)
        );
        assert!(
            family_script_score("noto sans devanagari", ScriptGroup::Indic)
                < family_script_score("dejavu sans", ScriptGroup::Indic)
        );
        assert!(
            family_script_score("segoe ui emoji", ScriptGroup::Emoji)
                < family_script_score("segoe ui", ScriptGroup::Emoji)
        );
        assert!(
            family_script_score("wingdings", ScriptGroup::Default)
                > family_script_score("segoe ui", ScriptGroup::Default)
        );
    }

    #[test]
    fn ignores_format_controls_when_checking_glyph_coverage() {
        assert!(is_ignorable_for_coverage('\u{200D}'));
        assert!(is_ignorable_for_coverage('\u{FE0F}'));
        assert!(is_ignorable_for_coverage('\n'));
        assert!(!is_ignorable_for_coverage('a'));
        assert!(!is_ignorable_for_coverage('\u{0301}'));
    }

    #[test]
    fn text_resolution_is_plain_resolution_without_system_fallback() {
        let database = database();
        let font = Font::from_family("Ubuntu");
        let text = "Drawen Ελληνικά Кириллица 日本語 العربية";

        assert_eq!(
            database.resolve(&font),
            database.resolve_for_text(&font, text)
        );
    }
}
