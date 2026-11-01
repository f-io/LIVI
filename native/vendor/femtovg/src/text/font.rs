use fnv::FnvHashMap;
use std::cell::{Ref, RefCell};
use std::collections::hash_map::Entry;
use std::fmt;
#[cfg(all(feature = "swash", not(feature = "textlayout")))]
use std::rc::Rc;
#[cfg(feature = "textlayout")]
use ttf_parser::{Face as TtfFont, GlyphId};

use crate::{paint::FontVariations, ErrorKind, Path};

/// Abstraction over the parsed font face, so callers don't need cfg blocks.
/// With `textlayout`, this wraps a `ttf_parser::Face`.
/// Otherwise, this is a zero-sized type.
#[cfg(feature = "textlayout")]
pub(crate) struct FontFaceRef<'a>(pub(crate) ttf_parser::Face<'a>);

#[cfg(not(feature = "textlayout"))]
pub(crate) struct FontFaceRef<'a>(std::marker::PhantomData<&'a ()>);

/// Information about a font variation axis (e.g. weight, width, optical size).
///
/// Returned by [`Canvas::font_variation_axes`](crate::Canvas::font_variation_axes)
/// in the order they appear in the font's OpenType `fvar` table. The position
/// of each axis in that vector determines the index of the corresponding
/// normalized coordinate (`i16` in F2DOT14 format) when calling
/// [`Canvas::fill_glyph_run`](crate::Canvas::fill_glyph_run) or
/// [`Canvas::stroke_glyph_run`](crate::Canvas::stroke_glyph_run).
#[derive(Clone, Debug)]
pub struct VariationAxisInfo {
    /// Four-byte axis tag (e.g. `*b"wght"` for weight).
    pub tag: [u8; 4],
    /// Minimum value for this axis.
    pub min_value: f32,
    /// Default value for this axis.
    pub def_value: f32,
    /// Maximum value for this axis.
    pub max_value: f32,
    /// Name table ID for this axis's name.
    pub name_id: u16,
    /// Whether this axis is hidden from the user.
    pub hidden: bool,
}

#[derive(Clone, Debug)]
pub struct GlyphMetrics {
    pub width: f32,
    pub height: f32,
    pub bearing_x: f32,
    pub bearing_y: f32,
}

#[derive(Debug)]
pub struct Glyph {
    pub path: Option<Path>, // None means render as image
    pub metrics: GlyphMetrics,
}

pub enum GlyphRendering<'a> {
    RenderAsPath(Ref<'a, Path>),
    #[cfg(feature = "image-loading")]
    RenderAsImage(image::DynamicImage),
}

#[derive(Copy, Clone, Default, Debug)]
struct FontFlags(u8);

impl FontFlags {
    fn new(regular: bool, italic: bool, bold: bool, oblique: bool, variable: bool) -> Self {
        let mut flags = 0;
        if regular {
            flags |= 0x1;
        }
        if italic {
            flags |= 0x2;
        }
        if bold {
            flags |= 0x4;
        }
        if oblique {
            flags |= 0x8;
        }
        if variable {
            flags |= 0x10;
        }
        Self(flags)
    }

    #[inline]
    fn regular(&self) -> bool {
        self.0 & 0x1 > 0
    }

    #[inline]
    fn italic(&self) -> bool {
        self.0 & 0x2 > 0
    }

    #[inline]
    fn bold(&self) -> bool {
        self.0 & 0x4 > 0
    }

    #[inline]
    fn oblique(&self) -> bool {
        self.0 & 0x8 > 0
    }

    #[inline]
    fn variable(&self) -> bool {
        self.0 & 0x10 > 0
    }
}

/// Conventional typographic approximations used when a font does not provide
/// the corresponding OS/2 value. Expressed as fractions of the em size and
/// shared by both font-parsing backends so they agree on fallbacks.
#[cfg(any(feature = "textlayout", feature = "swash"))]
mod fallback {
    /// Recommended sub/superscript glyph size: 65% of the em.
    pub const SCRIPT_SIZE: f32 = 0.65;
    /// Subscript baseline drop below the text baseline: 14% of the em.
    pub const SUBSCRIPT_DROP: f32 = 0.14;
    /// Superscript baseline rise above the text baseline: 48% of the em.
    pub const SUPERSCRIPT_RISE: f32 = 0.48;
    /// Height of a lowercase "x" above the baseline: half the em.
    pub const X_HEIGHT: f32 = 0.5;
    /// Height of an uppercase letter above the baseline: 70% of the em.
    pub const CAP_HEIGHT: f32 = 0.7;
}

/// Information about a font.
#[derive(Copy, Clone, Default, Debug)]
pub struct FontMetrics {
    ascender: f32,
    descender: f32,
    height: f32,
    underline_position: f32,
    underline_thickness: f32,
    strikeout_position: f32,
    strikeout_thickness: f32,
    subscript_size: (f32, f32),
    subscript_offset: (f32, f32),
    superscript_size: (f32, f32),
    superscript_offset: (f32, f32),
    x_height: f32,
    cap_height: f32,
    line_gap: f32,
    flags: FontFlags,
    weight: u16,
    width: u16,
}

impl FontMetrics {
    fn scale(&mut self, scale: f32) {
        self.ascender *= scale;
        self.descender *= scale;
        self.height *= scale;
        self.underline_position *= scale;
        self.underline_thickness *= scale;
        self.strikeout_position *= scale;
        self.strikeout_thickness *= scale;
        self.subscript_size.0 *= scale;
        self.subscript_size.1 *= scale;
        self.subscript_offset.0 *= scale;
        self.subscript_offset.1 *= scale;
        self.superscript_size.0 *= scale;
        self.superscript_size.1 *= scale;
        self.superscript_offset.0 *= scale;
        self.superscript_offset.1 *= scale;
        self.x_height *= scale;
        self.cap_height *= scale;
        self.line_gap *= scale;
    }

    /// Returns the distance from the baseline to the top of the highest glyph.
    pub fn ascender(&self) -> f32 {
        self.ascender
    }

    /// Returns the distance from the baseline to the bottom of the lowest descenders on the glyphs.
    pub fn descender(&self) -> f32 {
        self.descender
    }

    /// Returns the height of the font.
    pub fn height(&self) -> f32 {
        self.height.round()
    }

    /// Returns the position of the underline relative to the baseline.
    ///
    /// Follows the OpenType `post` table convention: the value is measured from
    /// the baseline with the positive y-axis pointing up, so it is typically
    /// negative (the underline sits below the baseline). Sourced from the font's
    /// `post` table when present, otherwise derived from the descender.
    pub fn underline_position(&self) -> f32 {
        self.underline_position
    }

    /// Returns the thickness of the underline.
    ///
    /// Sourced from the font's `post` table when present, otherwise derived from
    /// the font height.
    pub fn underline_thickness(&self) -> f32 {
        self.underline_thickness
    }

    /// Returns the position of the strikeout line relative to the baseline.
    ///
    /// Follows the OpenType OS/2 table convention: the value is measured from the
    /// baseline with the positive y-axis pointing up, so it is typically positive
    /// (the strikeout sits above the baseline, through the middle of the text).
    /// Sourced from the font's OS/2 table when present, otherwise derived from the
    /// ascender.
    pub fn strikeout_position(&self) -> f32 {
        self.strikeout_position
    }

    /// Returns the thickness of the strikeout line.
    ///
    /// Sourced from the font's OS/2 table when present, otherwise derived from the
    /// font height.
    pub fn strikeout_thickness(&self) -> f32 {
        self.strikeout_thickness
    }

    /// Returns the recommended horizontal and vertical size for subscript text,
    /// as an `(x, y)` pair.
    ///
    /// Sourced from the font's OS/2 table (`ySubscriptXSize`/`ySubscriptYSize`)
    /// when present, otherwise both components fall back to 0.65 × the em size,
    /// a conventional typographic approximation. Text layout engines can use
    /// this as the font size for `vertical-align: sub` runs.
    pub fn subscript_size(&self) -> (f32, f32) {
        self.subscript_size
    }

    /// Returns the recommended offset from the text origin to the subscript
    /// origin, as an `(x, y)` pair.
    ///
    /// Sourced from the font's OS/2 table (`ySubscriptXOffset`/`ySubscriptYOffset`)
    /// when present, otherwise falls back to `(0, 0.14 × em)`, a conventional
    /// typographic approximation. The y component follows the canvas convention
    /// (+y points down): it is typically positive, dropping the subscript
    /// baseline below the text baseline. This matches the raw OS/2 value, which
    /// is also expressed positive-down, but note it differs from
    /// [`Self::underline_position`], which keeps the +y-up `post` table
    /// convention.
    pub fn subscript_offset(&self) -> (f32, f32) {
        self.subscript_offset
    }

    /// Returns the recommended horizontal and vertical size for superscript
    /// text, as an `(x, y)` pair.
    ///
    /// Sourced from the font's OS/2 table (`ySuperscriptXSize`/`ySuperscriptYSize`)
    /// when present, otherwise both components fall back to 0.65 × the em size,
    /// a conventional typographic approximation. Text layout engines can use
    /// this as the font size for `vertical-align: super` runs.
    pub fn superscript_size(&self) -> (f32, f32) {
        self.superscript_size
    }

    /// Returns the recommended offset from the text origin to the superscript
    /// origin, as an `(x, y)` pair.
    ///
    /// Sourced from the font's OS/2 table (`ySuperscriptXOffset`/`ySuperscriptYOffset`)
    /// when present, otherwise falls back to `(0, -0.48 × em)`, a conventional
    /// typographic approximation. The y component follows the canvas convention
    /// (+y points down): it is typically negative, raising the superscript
    /// baseline above the text baseline. The raw OS/2 `ySuperscriptYOffset` is
    /// expressed positive-up; it is negated here so both script offsets share
    /// the same +y-down convention.
    pub fn superscript_offset(&self) -> (f32, f32) {
        self.superscript_offset
    }

    /// Returns the height of lowercase letters without ascenders or descenders
    /// (the height of a lowercase "x") above the baseline.
    ///
    /// Sourced from the font's OS/2 table (`sxHeight`, version 2 and up) when
    /// present, otherwise falls back to 0.5 × the em size, a conventional
    /// typographic approximation. Useful for implementing the CSS `ex` unit.
    pub fn x_height(&self) -> f32 {
        self.x_height
    }

    /// Returns the height of uppercase letters (the height of a capital "H")
    /// above the baseline.
    ///
    /// Sourced from the font's OS/2 table (`sCapHeight`, version 2 and up) when
    /// present, otherwise falls back to 0.7 × the em size, a conventional
    /// typographic approximation. Useful for implementing the CSS `cap` unit
    /// and for small-caps synthesis.
    pub fn cap_height(&self) -> f32 {
        self.cap_height
    }

    /// Returns the recommended additional space between lines of text, beyond
    /// ascender + descender.
    ///
    /// Sourced from the font's `hhea` table (or the OS/2 typographic line gap
    /// when the font opts into typographic metrics via `USE_TYPO_METRICS`).
    /// Many fonts legitimately report 0 here.
    pub fn line_gap(&self) -> f32 {
        self.line_gap
    }

    /// Returns if the font is regular.
    pub fn regular(&self) -> bool {
        self.flags.regular()
    }

    /// Returns if the font is italic.
    pub fn italic(&self) -> bool {
        self.flags.italic()
    }

    /// Returns if the font is bold.
    pub fn bold(&self) -> bool {
        self.flags.bold()
    }

    /// Returns if the font is oblique.
    pub fn oblique(&self) -> bool {
        self.flags.oblique()
    }

    /// Returns if the font is a variable font.
    pub fn variable(&self) -> bool {
        self.flags.variable()
    }

    /// Returns the weight of the font.
    pub fn weight(&self) -> u16 {
        self.weight
    }

    /// Returns the width of the font.
    pub fn width(&self) -> u16 {
        self.width
    }
}

impl fmt::Debug for Font {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Font")
            .field("data", &format_args!(".."))
            .field("face_index", &self.face_index)
            .field("units_per_em", &self.units_per_em)
            .field("metrics", &self.metrics)
            .field("glyphs", &self.glyphs)
            .finish()
    }
}

/// Key for the glyph cache: (glyph_id, normalized_coords_hash).
/// The hash covers all normalized variation coordinates, or 0 for defaults.
type GlyphCacheKey = (u16, u64);

pub struct Font {
    data: Box<dyn AsRef<[u8]>>,
    face_index: u32,
    units_per_em: u16,
    metrics: FontMetrics,
    glyphs: RefCell<FnvHashMap<GlyphCacheKey, Glyph>>,
    #[cfg(all(feature = "swash", not(feature = "textlayout")))]
    swash_scale_context: Rc<RefCell<swash::scale::ScaleContext>>,
}

impl Font {
    #[cfg(feature = "textlayout")]
    pub fn new_with_data<T: AsRef<[u8]> + 'static>(
        data: T,
        face_index: u32,
        _text_context: &super::TextContextImpl,
    ) -> Result<Self, ErrorKind> {
        let ttf_font = TtfFont::parse(data.as_ref(), face_index).map_err(|_| ErrorKind::FontParseError)?;

        let units_per_em = ttf_font.units_per_em();

        let ascender = ttf_font.ascender() as f32;
        let descender = ttf_font.descender() as f32;
        // The `post`/OS-2 tables are optional. When absent, fall back to values
        // derived from the ascender/descender so decorations still land sensibly:
        //   * underline a little below the baseline (descender follows +y up, so
        //     it is negative), at roughly 1/14 of the em as thickness.
        //   * strikeout through the middle of the lowercase x-height region,
        //     approximated as ~40% of the ascender above the baseline.
        // The typesetting metrics below fall back to the conventional fractions
        // of the em size documented in the `fallback` module.
        // A table that is present but holds a zero is treated like a missing
        // table, matching the swash backend's `!= 0` checks: a zero position
        // would draw the line on the baseline, a zero thickness would make it
        // invisible, and a zero script size would typeset invisible sub- or
        // superscript text, none of which a font can mean.
        let em = units_per_em as f32;
        let default_thickness = em / 14.0;
        let underline = ttf_font.underline_metrics();
        let strikeout = ttf_font.strikeout_metrics();
        let subscript = ttf_font.subscript_metrics().filter(|m| m.x_size != 0 && m.y_size != 0);
        let superscript = ttf_font
            .superscript_metrics()
            .filter(|m| m.x_size != 0 && m.y_size != 0);
        let fallback_script_size = (em * fallback::SCRIPT_SIZE, em * fallback::SCRIPT_SIZE);

        let metrics = FontMetrics {
            ascender,
            descender,
            height: ttf_font.height() as f32,
            underline_position: underline
                .map(|m| m.position)
                .filter(|&v| v != 0)
                .map_or(descender * 0.5, |v| v as f32),
            underline_thickness: underline
                .map(|m| m.thickness)
                .filter(|&v| v != 0)
                .map_or(default_thickness, |v| v as f32),
            strikeout_position: strikeout
                .map(|m| m.position)
                .filter(|&v| v != 0)
                .map_or(ascender * 0.4, |v| v as f32),
            strikeout_thickness: strikeout
                .map(|m| m.thickness)
                .filter(|&v| v != 0)
                .map_or(default_thickness, |v| v as f32),
            // The OS/2 script offsets are normalized to canvas +y-down
            // semantics: `ySubscriptYOffset` is defined positive-down and
            // passes through unchanged, while `ySuperscriptYOffset` is defined
            // positive-up and is negated.
            subscript_size: subscript.map_or(fallback_script_size, |m| (m.x_size as f32, m.y_size as f32)),
            subscript_offset: subscript.map_or((0.0, em * fallback::SUBSCRIPT_DROP), |m| {
                (m.x_offset as f32, m.y_offset as f32)
            }),
            superscript_size: superscript.map_or(fallback_script_size, |m| (m.x_size as f32, m.y_size as f32)),
            superscript_offset: superscript.map_or((0.0, -(em * fallback::SUPERSCRIPT_RISE)), |m| {
                (m.x_offset as f32, -(m.y_offset as f32))
            }),
            x_height: ttf_font
                .x_height()
                .filter(|&v| v != 0)
                .map_or(em * fallback::X_HEIGHT, |v| v as f32),
            cap_height: ttf_font
                .capital_height()
                .filter(|&v| v != 0)
                .map_or(em * fallback::CAP_HEIGHT, |v| v as f32),
            line_gap: ttf_font.line_gap() as f32,
            flags: FontFlags::new(
                ttf_font.is_regular(),
                ttf_font.is_italic(),
                ttf_font.is_bold(),
                ttf_font.is_oblique(),
                ttf_font.is_variable(),
            ),
            width: ttf_font.width().to_number(),
            weight: ttf_font.weight().to_number(),
        };

        Ok(Self {
            data: Box::new(data),
            face_index,
            units_per_em,
            metrics,
            glyphs: RefCell::default(),
        })
    }

    #[cfg(all(feature = "swash", not(feature = "textlayout")))]
    pub fn new_with_data<T: AsRef<[u8]> + 'static>(
        data: T,
        face_index: u32,
        text_context: &super::TextContextImpl,
    ) -> Result<Self, ErrorKind> {
        let font_ref =
            swash::FontRef::from_index(data.as_ref(), face_index as usize).ok_or(ErrorKind::FontParseError)?;

        let swash_metrics = font_ref.metrics(&[]);
        let attrs = font_ref.attributes();

        let units_per_em = swash_metrics.units_per_em;

        let weight = attrs.weight().0;
        let stretch = attrs.stretch();
        let style = attrs.style();

        let is_bold = weight >= 700;
        let is_italic = matches!(style, swash::Style::Italic);
        let is_oblique = matches!(style, swash::Style::Oblique(_));
        let is_variable = font_ref.variations().next().is_some();

        let is_regular = weight == 400 && matches!(style, swash::Style::Normal) && stretch == swash::Stretch::NORMAL;

        // Map swash::Stretch to usWidthClass values as per
        // https://learn.microsoft.com/en-us/typography/opentype/spec/os2#uswidthclass
        let width: u16 = match stretch {
            swash::Stretch::ULTRA_CONDENSED => 1,
            swash::Stretch::EXTRA_CONDENSED => 2,
            swash::Stretch::CONDENSED => 3,
            swash::Stretch::SEMI_CONDENSED => 4,
            swash::Stretch::NORMAL => 5,
            swash::Stretch::SEMI_EXPANDED => 6,
            swash::Stretch::EXPANDED => 7,
            swash::Stretch::EXTRA_EXPANDED => 8,
            swash::Stretch::ULTRA_EXPANDED => 9,
            _ => 5,
        };

        // swash reports underline/strikeout offsets from the baseline with +y up
        // (so underline_offset is typically negative), matching ttf-parser's
        // post/OS-2 convention; stroke_size is shared by both decorations. Fall
        // back to ascender/descender-derived values when the font omits them.
        let em = units_per_em as f32;
        let default_thickness = em / 14.0;
        let stroke_size = if swash_metrics.stroke_size > 0.0 {
            swash_metrics.stroke_size
        } else {
            default_thickness
        };

        // swash's `Metrics` does not surface the OS/2 sub/superscript metrics,
        // so this backend always uses the conventional em-fraction fallbacks
        // that the ttf-parser backend applies when a font omits them. x-height
        // and cap-height are reported as 0 when the font lacks them, in which
        // case the same fallbacks kick in, keeping both backends in agreement
        // on availability.
        let fallback_script_size = (em * fallback::SCRIPT_SIZE, em * fallback::SCRIPT_SIZE);

        let metrics = FontMetrics {
            ascender: swash_metrics.ascent,
            descender: -swash_metrics.descent,
            // swash ascent and descent are both positive (distance from baseline),
            // unlike ttf-parser where descent is negative, so this is a sum not a difference.
            height: swash_metrics.ascent + swash_metrics.descent + swash_metrics.leading,
            underline_position: if swash_metrics.underline_offset != 0.0 {
                swash_metrics.underline_offset
            } else {
                -swash_metrics.descent * 0.5
            },
            underline_thickness: stroke_size,
            strikeout_position: if swash_metrics.strikeout_offset != 0.0 {
                swash_metrics.strikeout_offset
            } else {
                swash_metrics.ascent * 0.4
            },
            strikeout_thickness: stroke_size,
            subscript_size: fallback_script_size,
            subscript_offset: (0.0, em * fallback::SUBSCRIPT_DROP),
            superscript_size: fallback_script_size,
            superscript_offset: (0.0, -(em * fallback::SUPERSCRIPT_RISE)),
            x_height: if swash_metrics.x_height != 0.0 {
                swash_metrics.x_height
            } else {
                em * fallback::X_HEIGHT
            },
            cap_height: if swash_metrics.cap_height != 0.0 {
                swash_metrics.cap_height
            } else {
                em * fallback::CAP_HEIGHT
            },
            // swash's leading is the hhea line gap (or the OS/2 typographic
            // line gap when the font opts into typographic metrics).
            line_gap: swash_metrics.leading,
            flags: FontFlags::new(is_regular, is_italic, is_bold, is_oblique, is_variable),
            weight,
            width,
        };

        Ok(Self {
            data: Box::new(data),
            face_index,
            units_per_em,
            metrics,
            glyphs: RefCell::default(),
            swash_scale_context: text_context.swash_scale_context(),
        })
    }

    #[cfg(not(any(feature = "textlayout", feature = "swash")))]
    pub fn new_with_data<T: AsRef<[u8]> + 'static>(
        _data: T,
        _face_index: u32,
        _text_context: &super::TextContextImpl,
    ) -> Result<Self, ErrorKind> {
        Err(ErrorKind::FontParseError)
    }

    #[allow(dead_code)]
    pub fn data(&self) -> &[u8] {
        (*self.data).as_ref()
    }

    #[allow(dead_code)]
    pub fn face_index(&self) -> u32 {
        self.face_index
    }

    #[cfg(feature = "textlayout")]
    pub(crate) fn face_ref(&self) -> FontFaceRef<'_> {
        FontFaceRef(ttf_parser::Face::parse(self.data.as_ref().as_ref(), self.face_index).unwrap())
    }

    #[cfg(not(feature = "textlayout"))]
    pub(crate) fn face_ref(&self) -> FontFaceRef<'_> {
        FontFaceRef(std::marker::PhantomData)
    }

    #[cfg(feature = "swash")]
    pub(crate) fn swash_font_ref(&self) -> Option<swash::FontRef<'_>> {
        swash::FontRef::from_index(self.data.as_ref().as_ref(), self.face_index as usize)
    }

    #[cfg(all(feature = "swash", not(feature = "textlayout")))]
    fn swash_scale_context(&self) -> &RefCell<swash::scale::ScaleContext> {
        &self.swash_scale_context
    }

    #[cfg(feature = "textlayout")]
    pub(crate) fn face_ref_with_variations(&self, variations: &FontVariations) -> FontFaceRef<'_> {
        let mut face = self.face_ref();
        for (tag, value) in variations.iter() {
            face.0.set_variation(ttf_parser::Tag(tag), value);
        }
        face
    }

    #[cfg(not(feature = "textlayout"))]
    #[allow(dead_code)]
    pub(crate) fn face_ref_with_variations(&self, _variations: &FontVariations) -> FontFaceRef<'_> {
        // Without ttf-parser, face is a phantom type. Variations are applied
        // through the swash scaler in the glyph() method instead.
        self.face_ref()
    }

    #[cfg(feature = "textlayout")]
    pub(crate) fn face_ref_with_normalized_coords(&self, coords: &[i16]) -> FontFaceRef<'_> {
        let mut face = self.face_ref();
        // Round-trip: convert normalized coords back to design-space values
        // so ttf-parser can re-normalize them (including avar remapping).
        // Coords are zipped with axes in order — the i-th coord maps to the i-th axis.
        for (axis, coord) in face.0.variation_axes().into_iter().zip(coords.iter()) {
            let value = *coord as f32 / 16384.0;
            let design_value = if value >= 0.0 {
                axis.def_value + value * (axis.max_value - axis.def_value)
            } else {
                axis.def_value + value * (axis.def_value - axis.min_value)
            };
            face.0.set_variation(axis.tag, design_value);
        }
        face
    }

    #[cfg(not(feature = "textlayout"))]
    pub(crate) fn face_ref_with_normalized_coords(&self, _coords: &[i16]) -> FontFaceRef<'_> {
        self.face_ref()
    }

    pub fn metrics(&self, size: f32) -> FontMetrics {
        let mut metrics = self.metrics;

        metrics.scale(self.scale(size));

        metrics
    }

    #[cfg(feature = "textlayout")]
    pub fn metrics_with_variations(&self, size: f32, variations: &FontVariations) -> FontMetrics {
        if variations.is_empty() {
            return self.metrics(size);
        }

        let face = self.face_ref_with_variations(variations);
        let mut metrics = FontMetrics {
            ascender: face.0.ascender() as f32,
            descender: face.0.descender() as f32,
            height: face.0.height() as f32,
            ..self.metrics
        };
        metrics.scale(self.scale(size));
        metrics
    }

    #[cfg(all(feature = "swash", not(feature = "textlayout")))]
    pub fn metrics_with_variations(&self, size: f32, variations: &FontVariations) -> FontMetrics {
        if variations.is_empty() {
            return self.metrics(size);
        }
        let Some(font_ref) = self.swash_font_ref() else {
            return self.metrics(size);
        };
        let settings: Vec<swash::Setting<f32>> = variations
            .iter()
            .map(|(tag, value)| swash::Setting {
                tag: swash::tag_from_bytes(&tag.to_be_bytes()),
                value,
            })
            .collect();
        let coords: Vec<swash::NormalizedCoord> = font_ref.variations().normalized_coords(settings).collect();
        let swash_metrics = font_ref.metrics(&coords);
        let mut metrics = FontMetrics {
            ascender: swash_metrics.ascent,
            descender: -swash_metrics.descent,
            height: swash_metrics.ascent + swash_metrics.descent + swash_metrics.leading,
            ..self.metrics
        };
        metrics.scale(self.scale(size));
        metrics
    }

    #[cfg(not(any(feature = "textlayout", feature = "swash")))]
    pub fn metrics_with_variations(&self, size: f32, _variations: &FontVariations) -> FontMetrics {
        self.metrics(size)
    }

    pub fn scale(&self, size: f32) -> f32 {
        size / self.units_per_em as f32
    }

    #[cfg(feature = "textlayout")]
    pub fn variation_axes(&self) -> Vec<VariationAxisInfo> {
        let face = self.face_ref();
        let mut axes = Vec::new();
        if let Some(table) = face.0.tables().fvar {
            for axis in table.axes {
                axes.push(VariationAxisInfo {
                    tag: axis.tag.to_bytes(),
                    min_value: axis.min_value,
                    def_value: axis.def_value,
                    max_value: axis.max_value,
                    name_id: axis.name_id,
                    hidden: axis.hidden,
                });
            }
        }
        axes
    }

    #[cfg(all(feature = "swash", not(feature = "textlayout")))]
    pub fn variation_axes(&self) -> Vec<VariationAxisInfo> {
        let Some(font_ref) = self.swash_font_ref() else {
            return Vec::new();
        };
        font_ref
            .variations()
            .map(|v| {
                let name_id = match v.name_id() {
                    swash::StringId::Other(id) => id,
                    _ => 0,
                };
                VariationAxisInfo {
                    tag: v.tag().to_be_bytes(),
                    min_value: v.min_value(),
                    def_value: v.default_value(),
                    max_value: v.max_value(),
                    name_id,
                    hidden: v.is_hidden(),
                }
            })
            .collect()
    }

    #[cfg(not(any(feature = "textlayout", feature = "swash")))]
    pub fn variation_axes(&self) -> Vec<VariationAxisInfo> {
        Vec::new()
    }

    #[cfg(feature = "textlayout")]
    pub(crate) fn normalize_variations(&self, variations: &FontVariations) -> Vec<i16> {
        let face = self.face_ref_with_variations(variations);
        face.0.variation_coordinates().iter().map(|c| c.get()).collect()
    }

    #[allow(dead_code)]
    fn hash_normalized_coords(coords: &[i16]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = fnv::FnvHasher::default();
        coords.hash(&mut hasher);
        hasher.finish()
    }

    #[cfg(feature = "textlayout")]
    pub(crate) fn glyph(
        &self,
        face: &FontFaceRef<'_>,
        codepoint: u16,
        normalized_coords: &[i16],
    ) -> Option<Ref<'_, Glyph>> {
        let cache_key: GlyphCacheKey = (codepoint, Self::hash_normalized_coords(normalized_coords));

        if let Entry::Vacant(entry) = self.glyphs.borrow_mut().entry(cache_key) {
            let mut path = Path::new();

            let id = GlyphId(codepoint);

            let maybe_glyph = if let Some(image) = face
                .0
                .glyph_raster_image(id, u16::MAX)
                .filter(|img| img.format == ttf_parser::RasterImageFormat::PNG)
            {
                let scale = if image.pixels_per_em != 0 {
                    self.units_per_em as f32 / image.pixels_per_em as f32
                } else {
                    1.0
                };
                Some(Glyph {
                    path: None,
                    metrics: GlyphMetrics {
                        width: image.width as f32 * scale,
                        height: image.height as f32 * scale,
                        bearing_x: image.x as f32 * scale,
                        bearing_y: (image.y as f32 + image.height as f32) * scale,
                    },
                })
            } else {
                face.0.outline_glyph(id, &mut path).map(|bbox| Glyph {
                    path: Some(path),
                    metrics: GlyphMetrics {
                        width: bbox.width() as f32,
                        height: bbox.height() as f32,
                        bearing_x: bbox.x_min as f32,
                        bearing_y: bbox.y_max as f32,
                    },
                })
            };

            if let Some(glyph) = maybe_glyph {
                entry.insert(glyph);
            }
        }

        Ref::filter_map(self.glyphs.borrow(), |glyphs| glyphs.get(&cache_key)).ok()
    }

    #[cfg(not(any(feature = "textlayout", feature = "swash")))]
    pub(crate) fn glyph(
        &self,
        _face: &FontFaceRef<'_>,
        _codepoint: u16,
        _normalized_coords: &[i16],
    ) -> Option<Ref<'_, Glyph>> {
        None
    }

    #[cfg(all(feature = "swash", not(feature = "textlayout")))]
    pub(crate) fn glyph(
        &self,
        _face: &FontFaceRef<'_>,
        codepoint: u16,
        normalized_coords: &[i16],
    ) -> Option<Ref<'_, Glyph>> {
        let cache_key: GlyphCacheKey = (codepoint, Self::hash_normalized_coords(normalized_coords));
        if let Entry::Vacant(entry) = self.glyphs.borrow_mut().entry(cache_key) {
            let font_ref = self.swash_font_ref()?;

            let mut scale_context = self.swash_scale_context().borrow_mut();
            let scaler_builder = scale_context
                .builder(font_ref)
                .size(self.units_per_em as f32)
                .hint(false)
                .normalized_coords(normalized_coords);
            let mut scaler = scaler_builder.build();

            let maybe_glyph = if let Some(outline) = scaler.scale_outline(codepoint) {
                use swash::zeno::{Command, PathData};
                let bounds = outline.bounds();
                let mut path = Path::new();
                for cmd in outline.path().commands() {
                    match cmd {
                        Command::MoveTo(p) => path.move_to(p.x, p.y),
                        Command::LineTo(p) => path.line_to(p.x, p.y),
                        Command::QuadTo(c, p) => path.quad_to(c.x, c.y, p.x, p.y),
                        Command::CurveTo(c1, c2, p) => path.bezier_to(c1.x, c1.y, c2.x, c2.y, p.x, p.y),
                        Command::Close => path.close(),
                    }
                }
                Some(Glyph {
                    path: Some(path),
                    metrics: GlyphMetrics {
                        width: bounds.width(),
                        height: bounds.height(),
                        bearing_x: bounds.min.x,
                        bearing_y: bounds.max.y,
                    },
                })
            } else {
                None
            };

            if let Some(glyph) = maybe_glyph {
                entry.insert(glyph);
            }
        }

        Ref::filter_map(self.glyphs.borrow(), |glyphs| glyphs.get(&cache_key)).ok()
    }

    #[cfg(feature = "textlayout")]
    pub(crate) fn glyph_rendering_representation(
        &self,
        face: &FontFaceRef<'_>,
        codepoint: u16,
        #[allow(unused_variables)] pixels_per_em: u16,
        normalized_coords: &[i16],
    ) -> Option<GlyphRendering<'_>> {
        #[cfg(feature = "image-loading")]
        if let Some(image) =
            face.0
                .glyph_raster_image(GlyphId(codepoint), pixels_per_em)
                .and_then(|raster_glyph_image| {
                    image::load_from_memory_with_format(raster_glyph_image.data, image::ImageFormat::Png).ok()
                })
        {
            return Some(GlyphRendering::RenderAsImage(image));
        }

        self.glyph(face, codepoint, normalized_coords).and_then(|glyph| {
            Ref::filter_map(glyph, |glyph| glyph.path.as_ref())
                .ok()
                .map(GlyphRendering::RenderAsPath)
        })
    }

    #[cfg(not(feature = "textlayout"))]
    pub(crate) fn glyph_rendering_representation(
        &self,
        _face: &FontFaceRef<'_>,
        codepoint: u16,
        #[allow(unused_variables)] _pixels_per_em: u16,
        normalized_coords: &[i16],
    ) -> Option<GlyphRendering<'_>> {
        self.glyph(_face, codepoint, normalized_coords).and_then(|glyph| {
            Ref::filter_map(glyph, |glyph| glyph.path.as_ref())
                .ok()
                .map(GlyphRendering::RenderAsPath)
        })
    }
}

// These tests exercise whichever font-parsing backend the enabled features
// select, so running them with `--features textlayout` and with
// `--no-default-features --features swash` checks that both backends report
// the same metrics availability.
#[cfg(all(test, any(feature = "textlayout", feature = "swash")))]
mod tests {
    use super::Font;

    fn parse_font(data: Vec<u8>) -> Font {
        Font::new_with_data(data, 0, &super::super::TextContextImpl::default()).expect("font should parse")
    }

    /// Builds a minimal TrueType font containing only the tables required for
    /// parsing (`head`, `hhea` and `maxp`), so every optional metric has to
    /// take its documented fallback. Units per em is 1024 and the
    /// ascender/descender are 800/-200 font units.
    fn minimal_font_without_optional_tables() -> Vec<u8> {
        fn push_u16(data: &mut Vec<u8>, value: u16) {
            data.extend_from_slice(&value.to_be_bytes());
        }
        fn push_i16(data: &mut Vec<u8>, value: i16) {
            data.extend_from_slice(&value.to_be_bytes());
        }
        fn push_u32(data: &mut Vec<u8>, value: u32) {
            data.extend_from_slice(&value.to_be_bytes());
        }

        let mut head = Vec::new();
        push_u16(&mut head, 1); // majorVersion
        push_u16(&mut head, 0); // minorVersion
        push_u32(&mut head, 0); // fontRevision
        push_u32(&mut head, 0); // checkSumAdjustment
        push_u32(&mut head, 0x5F0F_3CF5); // magicNumber
        push_u16(&mut head, 0); // flags
        push_u16(&mut head, 1024); // unitsPerEm
        push_u32(&mut head, 0); // created (upper half)
        push_u32(&mut head, 0); // created (lower half)
        push_u32(&mut head, 0); // modified (upper half)
        push_u32(&mut head, 0); // modified (lower half)
        push_i16(&mut head, 0); // xMin
        push_i16(&mut head, -200); // yMin
        push_i16(&mut head, 500); // xMax
        push_i16(&mut head, 800); // yMax
        push_u16(&mut head, 0); // macStyle
        push_u16(&mut head, 8); // lowestRecPPEM
        push_i16(&mut head, 2); // fontDirectionHint
        push_i16(&mut head, 0); // indexToLocFormat
        push_i16(&mut head, 0); // glyphDataFormat

        let mut hhea = Vec::new();
        push_u16(&mut hhea, 1); // majorVersion
        push_u16(&mut hhea, 0); // minorVersion
        push_i16(&mut hhea, 800); // ascender
        push_i16(&mut hhea, -200); // descender
        push_i16(&mut hhea, 0); // lineGap
        push_u16(&mut hhea, 500); // advanceWidthMax
        push_i16(&mut hhea, 0); // minLeftSideBearing
        push_i16(&mut hhea, 0); // minRightSideBearing
        push_i16(&mut hhea, 500); // xMaxExtent
        push_i16(&mut hhea, 1); // caretSlopeRise
        push_i16(&mut hhea, 0); // caretSlopeRun
        push_i16(&mut hhea, 0); // caretOffset
        for _ in 0..4 {
            push_i16(&mut hhea, 0); // reserved
        }
        push_i16(&mut hhea, 0); // metricDataFormat
        push_u16(&mut hhea, 0); // numberOfHMetrics

        let mut maxp = Vec::new();
        push_u32(&mut maxp, 0x0000_5000); // version 0.5
        push_u16(&mut maxp, 1); // numGlyphs

        // Table records must be sorted by tag.
        let tables: [(&[u8; 4], &Vec<u8>); 3] = [(b"head", &head), (b"hhea", &hhea), (b"maxp", &maxp)];

        let mut font = Vec::new();
        push_u32(&mut font, 0x0001_0000); // sfntVersion
        push_u16(&mut font, tables.len() as u16); // numTables
        push_u16(&mut font, 32); // searchRange
        push_u16(&mut font, 1); // entrySelector
        push_u16(&mut font, 16); // rangeShift

        let header_len = 12 + 16 * tables.len();
        let mut records = Vec::new();
        let mut body: Vec<u8> = Vec::new();
        for (tag, table) in tables {
            records.extend_from_slice(&tag[..]);
            push_u32(&mut records, 0); // checksum, not verified by the parsers
            push_u32(&mut records, (header_len + body.len()) as u32);
            push_u32(&mut records, table.len() as u32);
            body.extend_from_slice(table);
            while !body.len().is_multiple_of(4) {
                body.push(0); // tables start on 4-byte boundaries
            }
        }
        font.extend_from_slice(&records);
        font.extend_from_slice(&body);
        font
    }

    #[test]
    fn missing_optional_tables_fall_back_to_documented_ratios() {
        let font = parse_font(minimal_font_without_optional_tables());

        // Half the 1024 units per em, so raw font units scale by exactly 0.5.
        let em = 512.;
        let metrics = font.metrics(em);

        // Without an OS/2 table, every typesetting metric takes its documented
        // fallback, expressed as a conventional fraction of the em size.
        assert_eq!(metrics.x_height(), em * 0.5);
        assert_eq!(metrics.cap_height(), em * 0.7);
        assert_eq!(metrics.subscript_size(), (em * 0.65, em * 0.65));
        assert_eq!(metrics.subscript_offset(), (0.0, em * 0.14));
        assert_eq!(metrics.superscript_size(), (em * 0.65, em * 0.65));
        assert_eq!(metrics.superscript_offset(), (0.0, -(em * 0.48)));
        assert_eq!(metrics.line_gap(), 0.0);

        // The underline/strikeout fallbacks engage as well, derived from the
        // hhea ascender/descender of 800/-200 font units.
        assert_eq!(metrics.underline_position(), -50.0); // half the descender
        assert_eq!(metrics.strikeout_position(), 160.0); // 40% of the ascender
        assert!(metrics.underline_thickness() > 0.0);
        assert!(metrics.strikeout_thickness() > 0.0);
    }

    #[test]
    fn real_font_typographic_metrics_are_plausible() {
        let data = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/assets/RobotoFlex-VariableFont.ttf"
        ))
        .expect("Font not found");
        let font = parse_font(data);

        let em = 32.;
        let metrics = font.metrics(em);

        // The vertical extents are ordered: 0 < x-height < cap-height <= ascender.
        assert!(metrics.x_height() > 0.0);
        assert!(metrics.x_height() < metrics.cap_height());
        assert!(metrics.cap_height() <= metrics.ascender());

        // Sub/superscript glyphs are recommended at a readable fraction of the
        // em, dropped below the baseline and raised above it respectively
        // (canvas convention: +y points down).
        for (size, offset) in [
            (metrics.subscript_size(), metrics.subscript_offset()),
            (metrics.superscript_size(), metrics.superscript_offset()),
        ] {
            assert!(size.0 > 0.0 && size.0 < em);
            assert!(size.1 > 0.0 && size.1 < em);
            assert!(offset.1 != 0.0);
        }
        assert!(metrics.subscript_offset().1 > 0.0);
        assert!(metrics.superscript_offset().1 < 0.0);

        // The hhea line gap is commonly zero, but never negative for this font.
        assert!(metrics.line_gap() >= 0.0);
    }
}
