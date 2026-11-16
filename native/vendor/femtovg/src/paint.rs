// TODO: prefix paint creation functions with make_ or new_
// so that they are easier to find when autocompleting

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::{
    geometry::{Position, Transform2D},
    Align, Baseline, Color, ColorSpace, FillRule, FontId, ImageId, LineCap, LineJoin,
};

const MAX_FONT_VARIATIONS: usize = 4;

/// Compact inline storage for font variation axis settings.
///
/// Stores up to 4 variation axis overrides (e.g. weight, italic, slant, width)
/// without heap allocation. Each entry is a (tag, value) pair where the tag is
/// the 4-byte OpenType axis tag stored as a `u32`.
#[derive(Clone, Debug)]
pub(crate) struct FontVariations {
    entries: [(u32, f32); MAX_FONT_VARIATIONS],
    len: u8,
}

impl Default for FontVariations {
    fn default() -> Self {
        Self {
            entries: [(0, 0.0); MAX_FONT_VARIATIONS],
            len: 0,
        }
    }
}

impl Hash for FontVariations {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Sort entries by tag before hashing to ensure order-independence.
        let len = self.len as usize;
        let mut sorted = self.entries;
        sorted[..len].sort_unstable_by_key(|&(tag, _)| tag);
        for &(tag, val) in &sorted[..len] {
            tag.hash(state);
            val.to_bits().hash(state);
        }
    }
}

impl FontVariations {
    /// Converts a 4-byte tag (e.g. `b"wght"`) to its `u32` representation.
    pub(crate) fn tag_to_u32(tag: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*tag)
    }

    /// Sets a variation axis value. If the tag already exists, updates it.
    /// Panics if the collection is full and the tag is new.
    pub(crate) fn set(&mut self, tag: u32, value: f32) {
        for i in 0..self.len as usize {
            if self.entries[i].0 == tag {
                self.entries[i].1 = value;
                return;
            }
        }
        assert!(
            (self.len as usize) < MAX_FONT_VARIATIONS,
            "FontVariations: too many axes (max {})",
            MAX_FONT_VARIATIONS
        );
        self.entries[self.len as usize] = (tag, value);
        self.len += 1;
    }

    /// Gets the value for a variation axis, or `None` if not set.
    pub(crate) fn get(&self, tag: u32) -> Option<f32> {
        for i in 0..self.len as usize {
            if self.entries[i].0 == tag {
                return Some(self.entries[i].1);
            }
        }
        None
    }

    /// Removes a variation axis. Returns whether the tag was present.
    pub(crate) fn remove(&mut self, tag: u32) -> bool {
        for i in 0..self.len as usize {
            if self.entries[i].0 == tag {
                // Swap-remove
                let last = self.len as usize - 1;
                self.entries[i] = self.entries[last];
                self.entries[last] = (0, 0.0);
                self.len -= 1;
                return true;
            }
        }
        false
    }

    /// Returns true if no variation axes are set.
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Iterates over all set (tag, value) pairs.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (u32, f32)> + '_ {
        self.entries[..self.len as usize].iter().copied()
    }

    /// Computes a hash of the variation settings for use in cache keys.
    #[allow(dead_code)]
    pub(crate) fn hash(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        Hash::hash(self, &mut hasher);
        hasher.finish()
    }

    /// Removes all variation axes.
    pub(crate) fn clear(&mut self) {
        self.len = 0;
    }
}

#[derive(Copy, Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct GradientStop(pub f32, pub Color);

// `total_cmp` orders -0.0 before 0.0; they draw the same, so must share a ramp.
fn key_cmp(a: f32, b: f32) -> Ordering {
    (a + 0.0).total_cmp(&(b + 0.0))
}

fn color_total_cmp(a: &Color, b: &Color) -> Ordering {
    key_cmp(a.r, b.r)
        .then_with(|| key_cmp(a.g, b.g))
        .then_with(|| key_cmp(a.b, b.b))
        .then_with(|| key_cmp(a.a, b.a))
}

fn is_finite_color(color: &Color) -> bool {
    [color.r, color.g, color.b, color.a].iter().all(|c| c.is_finite())
}

// MultiStopGradient is the key of GradientStore's BTreeMap, so the ordering
// must be total: with `<` a NaN compared `Equal` to anything and aliased
// another gradient's cached ramp. `PartialEq` follows `Ord` to keep `Eq` sound.
impl PartialEq for GradientStop {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for GradientStop {}
impl Ord for GradientStop {
    fn cmp(&self, other: &Self) -> Ordering {
        key_cmp(self.0, other.0).then_with(|| color_total_cmp(&self.1, &other.1))
    }
}

impl PartialOrd for GradientStop {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// How a gradient interpolates between its stops.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Interpolation {
    /// The color space the stop colors are interpolated in.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) space: ColorSpace,
    /// Whether the stop colors are interpolated premultiplied by their alpha.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) premultiplied: bool,
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MultiStopGradient {
    shared_stops: Rc<[GradientStop]>,
    tint: f32,
    #[cfg_attr(feature = "serde", serde(default))]
    interpolation: Interpolation,
}

impl MultiStopGradient {
    pub(crate) fn get(&self, index: usize) -> GradientStop {
        let mut stop = self
            .shared_stops
            .get(index)
            .copied()
            .unwrap_or_else(|| GradientStop(2.0, Color::black()));

        stop.1.a *= self.tint;
        stop
    }

    pub(crate) fn len(&self) -> usize {
        self.shared_stops.len()
    }

    pub(crate) fn pairs(&self) -> impl Iterator<Item = [GradientStop; 2]> + '_ {
        self.shared_stops.as_ref().windows(2).map(move |pair| {
            let mut stops = [pair[0], pair[1]];
            stops[0].1.a *= self.tint;
            stops[1].1.a *= self.tint;
            stops
        })
    }

    pub(crate) fn space(&self) -> ColorSpace {
        self.interpolation.space
    }

    #[cfg(test)]
    pub(crate) fn set_interpolation_space(&mut self, space: ColorSpace) {
        self.interpolation.space = space;
    }

    pub(crate) fn premultiplied(&self) -> bool {
        self.interpolation.premultiplied
    }
}

impl PartialEq for MultiStopGradient {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for MultiStopGradient {}

impl PartialOrd for MultiStopGradient {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MultiStopGradient {
    fn cmp(&self, other: &Self) -> Ordering {
        self.shared_stops
            .cmp(&other.shared_stops)
            .then_with(|| key_cmp(self.tint, other.tint))
            .then_with(|| self.interpolation.cmp(&other.interpolation))
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum GradientColors {
    TwoStop {
        start_color: Color,
        end_color: Color,
        /// Its space is always sRGB: other spaces turn it into a multi-stop gradient.
        #[cfg_attr(feature = "serde", serde(default))]
        interpolation: Interpolation,
    },
    MultiStop {
        stops: MultiStopGradient,
    },
}
impl GradientColors {
    /// A two-stop gradient with the default interpolation.
    pub(crate) fn two_stop(start_color: Color, end_color: Color) -> Self {
        Self::TwoStop {
            start_color,
            end_color,
            interpolation: Interpolation::default(),
        }
    }

    fn interpolation_mut(&mut self) -> &mut Interpolation {
        match self {
            Self::TwoStop { interpolation, .. } => interpolation,
            Self::MultiStop { stops } => &mut stops.interpolation,
        }
    }

    fn mul_alpha(&mut self, a: f32) {
        match self {
            Self::TwoStop {
                start_color, end_color, ..
            } => {
                start_color.a *= a;
                end_color.a *= a;
            }
            Self::MultiStop { stops, .. } => {
                stops.tint *= a;
            }
        }
    }
    pub(crate) fn from_stops<Stops>(stops: Stops) -> Self
    where
        Stops: IntoIterator<Item = (f32, Color)>,
    {
        // Canvas 2D `addColorStop` throws on a non-finite offset; drop such
        // stops as `set_line_dash` drops a non-finite dash.
        let mut stops = stops
            .into_iter()
            .filter(|(offset, color)| offset.is_finite() && is_finite_color(color));
        let Some(first_stop) = stops.next() else {
            // No stops, we use black.
            return Self::two_stop(Color::black(), Color::black());
        };
        let Some(second_stop) = stops.next() else {
            // One stop devolves to a solid color fill (but using the gradient shader variation).
            return Self::two_stop(first_stop.1, first_stop.1);
        };

        let maybe_third_stop = stops.next();

        if maybe_third_stop.is_none() && first_stop.0 <= 0.0 && second_stop.0 >= 1.0 {
            // Two stops takes the classic gradient path, so long as the stop positions are at
            // the extents (if the stop positions are inset then we'll fill to them).
            return Self::two_stop(first_stop.1, second_stop.1);
        }

        // Actual multistop gradient. We copy out the stops and then use a stop with a
        // position > 1.0 as a sentinel. GradientStore ignores stop positions > 1.0
        // when synthesizing the gradient texture.
        let out_stops = [first_stop, second_stop]
            .into_iter()
            .chain(maybe_third_stop)
            .chain(stops)
            .map(|(stop, color)| GradientStop(stop, color))
            .collect();
        Self::MultiStop {
            stops: MultiStopGradient {
                shared_stops: out_stops,
                tint: 1.0,
                interpolation: Interpolation::default(),
            },
        }
    }

    /// Sets the color space the stop colors are interpolated in.
    pub(crate) fn set_interpolation_space(&mut self, space: ColorSpace) {
        self.interpolation_mut().space = space;
        match self {
            // The two-stop shader path only interpolates in sRGB; other spaces go through the LUT.
            Self::TwoStop {
                start_color,
                end_color,
                interpolation,
            } if space != ColorSpace::Srgb => {
                *self = Self::MultiStop {
                    stops: MultiStopGradient {
                        shared_stops: [GradientStop(0.0, *start_color), GradientStop(1.0, *end_color)].into(),
                        tint: 1.0,
                        interpolation: *interpolation,
                    },
                };
            }
            // Only promotion makes a two-stop `MultiStop` spanning the extents
            // (`from_stops` gives those `TwoStop`), so this undoes it.
            Self::MultiStop { stops }
                if space == ColorSpace::Srgb
                    && stops.len() == 2
                    && stops.shared_stops[0].0 <= 0.0
                    && stops.shared_stops[1].0 >= 1.0 =>
            {
                // `get` folds the tint into alpha, as `mul_alpha` does for `TwoStop`.
                *self = Self::TwoStop {
                    start_color: stops.get(0).1,
                    end_color: stops.get(1).1,
                    interpolation: stops.interpolation,
                };
            }
            _ => {}
        }
    }

    /// Sets whether the stop colors are interpolated premultiplied by their alpha.
    pub(crate) fn set_premultiplied_interpolation(&mut self, premultiplied: bool) {
        self.interpolation_mut().premultiplied = premultiplied;
    }
}

/// A colour-space conversion an image paint applies to each texel it
/// samples: the sRGB transfer curve on the unpremultiplied colour, alpha
/// untouched, rounded to 8 bits - what
/// [`ImageFilter::LinearRgbToSrgb`](crate::ImageFilter::LinearRgbToSrgb) and
/// its inverse store as a pass. A layer whose chain is one conversion
/// composites through it instead (see `LayerEffects::composite_transfer`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Transfer {
    ToSrgb,
    ToLinearRgb,
}

impl Transfer {
    /// The value the shaders read for it: 1 to sRGB, 2 to linearRGB (0, no
    /// transfer, is the uniform's default).
    pub(crate) fn uniform(self) -> f32 {
        match self {
            Self::ToSrgb => 1.0,
            Self::ToLinearRgb => 2.0,
        }
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PaintFlavor {
    Color(Color),
    #[cfg_attr(feature = "serde", serde(skip))]
    Image {
        id: ImageId,
        center: Position,
        width: f32,
        height: f32,
        angle: f32,
        tint: Color,
        transfer: Option<Transfer>,
    },
    LinearGradient {
        start: Position,
        end: Position,
        colors: GradientColors,
        /// Maps the gradient's own coordinate system into user space, the
        /// role SVG's `gradientTransform` plays. Applied before the canvas
        /// transform, so a gradient can be sheared or given different scales
        /// on its two axes without the shape following it.
        #[cfg_attr(feature = "serde", serde(default))]
        transform: Transform2D,
    },
    BoxGradient {
        pos: Position,
        width: f32,
        height: f32,
        radius: f32,
        feather: f32,
        colors: GradientColors,
        /// Maps the gradient's own coordinate system into user space, the
        /// role SVG's `gradientTransform` plays. Applied before the canvas
        /// transform, so a gradient can be sheared or given different scales
        /// on its two axes without the shape following it.
        #[cfg_attr(feature = "serde", serde(default))]
        transform: Transform2D,
    },
    RadialGradient {
        center: Position,
        /// (x, y) radii of the inner boundary. Equal components describe a circle.
        #[cfg_attr(feature = "serde", serde(deserialize_with = "deserialize_radius_pair"))]
        in_radius: (f32, f32),
        /// (x, y) radii of the outer boundary. Equal components describe a circle.
        #[cfg_attr(feature = "serde", serde(deserialize_with = "deserialize_radius_pair"))]
        out_radius: (f32, f32),
        colors: GradientColors,
        /// Maps the gradient's own coordinate system into user space, the
        /// role SVG's `gradientTransform` plays. Applied before the canvas
        /// transform, so a gradient can be sheared or given different scales
        /// on its two axes without the shape following it.
        #[cfg_attr(feature = "serde", serde(default))]
        transform: Transform2D,
    },
    ConicGradient {
        center: Position,
        // Paints serialized before `start_angle` existed represented conic
        // gradients as `{ center, colors }`. Defaulting the missing field to
        // 0.0 keeps those payloads deserializable, matching the original
        // (no-rotation) behavior.
        #[cfg_attr(feature = "serde", serde(default))]
        start_angle: f32,
        colors: GradientColors,
        /// Maps the gradient's own coordinate system into user space, the
        /// role SVG's `gradientTransform` plays. Applied before the canvas
        /// transform, so a gradient can be sheared or given different scales
        /// on its two axes without the shape following it.
        #[cfg_attr(feature = "serde", serde(default))]
        transform: Transform2D,
    },
    /// Two-point (independently centered) radial gradient, i.e. the general
    /// Canvas `createRadialGradient(x0, y0, r0, x1, y1, r1)` form where the start
    /// and end circles need not share a center. Concentric radial gradients use
    /// the cheaper [`RadialGradient`](Self::RadialGradient) box-gradient path.
    TwoPointRadialGradient {
        start_center: Position,
        start_radius: f32,
        end_center: Position,
        end_radius: f32,
        colors: GradientColors,
        /// Maps the gradient's own coordinate system into user space, the
        /// role SVG's `gradientTransform` plays. Applied before the canvas
        /// transform, so a gradient can be sheared or given different scales
        /// on its two axes without the shape following it.
        #[cfg_attr(feature = "serde", serde(default))]
        transform: Transform2D,
    },
}

// Shared by `gradient_colors` and `gradient_colors_mut`; match ergonomics picks `&` or `&mut`.
macro_rules! gradient_colors_arms {
    ($self:expr) => {
        match $self {
            PaintFlavor::LinearGradient { colors, .. } => Some(colors),
            PaintFlavor::BoxGradient { colors, .. } => Some(colors),
            PaintFlavor::RadialGradient { colors, .. } => Some(colors),
            PaintFlavor::ConicGradient { colors, .. } => Some(colors),
            PaintFlavor::TwoPointRadialGradient { colors, .. } => Some(colors),
            _ => None,
        }
    };
}

// Convenience method to fetch the GradientColors out of a PaintFlavor
impl PaintFlavor {
    pub(crate) fn mul_alpha(&mut self, a: f32) {
        match self {
            Self::Color(color) => {
                color.a *= a;
            }
            Self::Image { tint, .. } => {
                tint.a *= a;
            }
            Self::LinearGradient { colors, .. } => {
                colors.mul_alpha(a);
            }
            Self::BoxGradient { colors, .. } => {
                colors.mul_alpha(a);
            }
            Self::RadialGradient { colors, .. } => {
                colors.mul_alpha(a);
            }
            Self::ConicGradient { colors, .. } => {
                colors.mul_alpha(a);
            }
            Self::TwoPointRadialGradient { colors, .. } => {
                colors.mul_alpha(a);
            }
        }
    }

    pub(crate) fn gradient_colors(&self) -> Option<&GradientColors> {
        gradient_colors_arms!(self)
    }

    pub(crate) fn gradient_colors_mut(&mut self) -> Option<&mut GradientColors> {
        gradient_colors_arms!(self)
    }

    /// Returns true if this paint is an untransformed image paint without anti-aliasing at the edges in case of a fill
    pub(crate) fn is_straight_tinted_image(&self, shape_anti_alias: bool) -> bool {
        matches!(self, &Self::Image { angle, .. } if angle == 0.0 && !shape_anti_alias)
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum GlyphTexture {
    #[default]
    None,
    AlphaMask(ImageId),
    ColorTexture(ImageId),
}

impl GlyphTexture {
    pub(crate) fn image_id(&self) -> Option<ImageId> {
        match self {
            Self::None => None,
            Self::AlphaMask(image_id) | Self::ColorTexture(image_id) => Some(*image_id),
        }
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct StrokeSettings {
    pub(crate) stencil_strokes: bool,
    pub(crate) miter_limit: f32,
    pub(crate) line_width: f32,
    pub(crate) line_cap_start: LineCap,
    pub(crate) line_cap_end: LineCap,
    pub(crate) line_join: LineJoin,
    pub(crate) line_dash: Vec<f32>,
    pub(crate) line_dash_offset: f32,
}

impl StrokeSettings {
    /// How far a stroke `line_width` wide reaches from its path: half the
    /// width, further at a miter's tip or a square cap's corner.
    pub(crate) fn reach(&self, line_width: f32) -> f32 {
        let join = match self.line_join {
            LineJoin::Miter => self.miter_limit.max(1.0),
            LineJoin::Round | LineJoin::Bevel => 1.0,
        };
        let square = self.line_cap_start == LineCap::Square || self.line_cap_end == LineCap::Square;
        let cap = if square { std::f32::consts::SQRT_2 } else { 1.0 };
        line_width * 0.5 * join.max(cap)
    }
}

impl Default for StrokeSettings {
    fn default() -> Self {
        Self {
            stencil_strokes: true,
            miter_limit: 10.0,
            line_width: 1.0,
            line_cap_start: LineCap::default(),
            line_cap_end: LineCap::default(),
            line_join: LineJoin::default(),
            line_dash: Vec::new(),
            line_dash_offset: 0.0,
        }
    }
}

/// Set of text decoration lines (underline, strikethrough, overline) to draw
/// along a text run.
///
/// This is an extension toward SVG/CSS `text-decoration` parity; the HTML
/// Canvas 2D API has no equivalent. Lines combine additively, so any subset can
/// be enabled at once. The decoration is drawn in the text paint's color.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TextDecoration {
    /// Draw an underline below the baseline (from the font's `post` metrics).
    pub underline: bool,
    /// Draw a line through the middle of the text (from the font's OS/2 metrics).
    pub strikethrough: bool,
    /// Draw a line above the text, near the ascent.
    pub overline: bool,
}

impl TextDecoration {
    /// Returns `true` if no decoration line is enabled.
    #[inline]
    pub fn is_none(&self) -> bool {
        !self.underline && !self.strikethrough && !self.overline
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TextSettings {
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) font_ids: [Option<FontId>; 8],
    pub(crate) font_size: f32,
    pub(crate) letter_spacing: f32,
    pub(crate) text_baseline: Baseline,
    pub(crate) text_align: Align,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) font_variations: FontVariations,
    // Older serialized paints predate this field; default it so they keep
    // deserializing. `TextDecoration::default()` is all-false, i.e. no decoration.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) text_decoration: TextDecoration,
}

impl Default for TextSettings {
    fn default() -> Self {
        Self {
            font_ids: Default::default(),
            font_size: 16.0,
            letter_spacing: 0.0,
            text_baseline: Baseline::default(),
            text_align: Align::default(),
            font_variations: FontVariations::default(),
            text_decoration: TextDecoration::default(),
        }
    }
}

/// Struct controlling how graphical shapes are rendered.
///
/// The Paint struct is a relatively lightweight object which contains all the information needed to
/// display something on the canvas. Unlike the HTML canvas where the current drawing style is stored
/// in an internal stack this paint struct is simply passed to the relevant drawing methods on the canvas.
///
/// Clients code can have as many paints as they desire for different use cases and styles. This makes
/// the internal stack in the [Canvas](struct.Canvas.html) struct much lighter since it only needs to
/// contain the transform stack and current scissor rectangle.
///
/// # Example
/// ```
/// use femtovg::{Paint, Path, Color, Canvas, renderer::Void};
///
/// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
///
/// let fill_paint = Paint::color(Color::hex("454545"));
/// let stroke_paint = Paint::color(Color::hex("bababa")).with_line_width(4.0);
///
/// let mut path = Path::new();
/// path.rounded_rect(10.0, 10.0, 100.0, 100.0, 20.0);
/// canvas.fill_path(&path, &fill_paint);
/// canvas.stroke_path(&path, &stroke_paint);
/// ```
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Paint {
    pub(crate) flavor: PaintFlavor,
    pub(crate) shape_anti_alias: bool,
    pub(crate) stroke: StrokeSettings,
    pub(crate) text: TextSettings,
    pub(crate) fill_rule: FillRule,
}

impl Default for Paint {
    fn default() -> Self {
        Self {
            flavor: PaintFlavor::Color(Color::white()),
            shape_anti_alias: true,
            stroke: StrokeSettings::default(),
            text: TextSettings::default(),
            fill_rule: FillRule::default(),
        }
    }
}

impl Paint {
    // Standard OpenType usWeightClass values for use with `set_font_weight`.

    /// Thin (Hairline) font weight.
    pub const FONT_WEIGHT_THIN: f32 = 100.0;
    /// Extra Light (Ultra Light) font weight.
    pub const FONT_WEIGHT_EXTRA_LIGHT: f32 = 200.0;
    /// Light font weight.
    pub const FONT_WEIGHT_LIGHT: f32 = 300.0;
    /// Regular (Normal) font weight.
    pub const FONT_WEIGHT_REGULAR: f32 = 400.0;
    /// Medium font weight.
    pub const FONT_WEIGHT_MEDIUM: f32 = 500.0;
    /// Semi Bold (Demi Bold) font weight.
    pub const FONT_WEIGHT_SEMI_BOLD: f32 = 600.0;
    /// Bold font weight.
    pub const FONT_WEIGHT_BOLD: f32 = 700.0;
    /// Extra Bold (Ultra Bold) font weight.
    pub const FONT_WEIGHT_EXTRA_BOLD: f32 = 800.0;
    /// Black (Heavy) font weight.
    pub const FONT_WEIGHT_BLACK: f32 = 900.0;

    /// Creates a new solid color paint
    pub fn color(color: Color) -> Self {
        Self::with_flavor(PaintFlavor::Color(color))
    }

    fn with_flavor(flavor: PaintFlavor) -> Self {
        Self {
            flavor,
            ..Default::default()
        }
    }

    /// Creates a new image pattern paint.
    ///
    /// * `id` - is handle to the image to render
    /// * `cx` `cy` - Specify the top-left location of the image pattern
    /// * `width` `height` - The size of one image
    /// * `angle` - Rotation around the top-left corner
    /// * `alpha` - Transparency applied on the image
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let image_id = canvas.load_image_file("examples/assets/rust-logo.png", ImageFlags::GENERATE_MIPMAPS).expect("Cannot create image");
    /// let fill_paint = Paint::image(image_id, 10.0, 10.0, 85.0, 85.0, 0.0, 1.0);
    ///
    /// let mut path = Path::new();
    /// path.rect(10.0, 10.0, 85.0, 85.0);
    /// canvas.fill_path(&path, &fill_paint);
    /// ```
    pub fn image(id: ImageId, cx: f32, cy: f32, width: f32, height: f32, angle: f32, alpha: f32) -> Self {
        Self::with_flavor(PaintFlavor::Image {
            id,
            center: Position { x: cx, y: cy },
            width,
            height,
            angle,
            tint: Color::rgbaf(1.0, 1.0, 1.0, alpha),
            transfer: None,
        })
    }

    /// Like `image`, but allows for adding a tint, or a color which will transform each pixel's
    /// color via channel-wise multiplication.
    pub fn image_tint(id: ImageId, cx: f32, cy: f32, width: f32, height: f32, angle: f32, tint: Color) -> Self {
        Self::with_flavor(PaintFlavor::Image {
            id,
            center: Position { x: cx, y: cy },
            width,
            height,
            angle,
            tint,
            transfer: None,
        })
    }

    /// This image paint converting each texel by `transfer` as it samples it.
    pub(crate) fn with_transfer(mut self, transfer: Option<Transfer>) -> Self {
        if let PaintFlavor::Image { transfer: t, .. } = &mut self.flavor {
            *t = transfer;
        }
        self
    }

    /// Creates and returns a linear gradient paint.
    ///
    /// The gradient is transformed by the current transform when it is passed to `fill_path()` or `stroke_path()`.
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::linear_gradient(0.0, 0.0, 0.0, 100.0, Color::rgba(255, 255, 255, 16), Color::rgba(0, 0, 0, 16));
    /// let mut path = Path::new();
    /// path.rounded_rect(0.0, 0.0, 100.0, 100.0, 5.0);
    /// canvas.fill_path(&path, &bg);
    /// ```
    pub fn linear_gradient(
        start_x: f32,
        start_y: f32,
        end_x: f32,
        end_y: f32,
        start_color: Color,
        end_color: Color,
    ) -> Self {
        Self::with_flavor(PaintFlavor::LinearGradient {
            start: Position { x: start_x, y: start_y },
            end: Position { x: end_x, y: end_y },
            colors: GradientColors::two_stop(start_color, end_color),
            transform: Transform2D::identity(),
        })
    }
    /// Creates and returns a linear gradient paint with two or more stops.
    /// Stops with a non-finite offset or color component are ignored.
    ///
    /// The gradient is transformed by the current transform when it is passed to `fill_path()` or `stroke_path()`.
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::linear_gradient_stops(
    ///    0.0, 0.0,
    ///    0.0, 100.0,
    ///    [
    ///         (0.0, Color::rgba(255, 255, 255, 16)),
    ///         (0.5, Color::rgba(0, 0, 0, 16)),
    ///         (1.0, Color::rgba(255, 0, 0, 16))
    ///    ]);
    /// let mut path = Path::new();
    /// path.rounded_rect(0.0, 0.0, 100.0, 100.0, 5.0);
    /// canvas.fill_path(&path, &bg);
    /// ```
    pub fn linear_gradient_stops(
        start_x: f32,
        start_y: f32,
        end_x: f32,
        end_y: f32,
        stops: impl IntoIterator<Item = (f32, Color)>,
    ) -> Self {
        Self::with_flavor(PaintFlavor::LinearGradient {
            start: Position { x: start_x, y: start_y },
            end: Position { x: end_x, y: end_y },
            colors: GradientColors::from_stops(stops),
            transform: Transform2D::identity(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    /// Creates and returns a box gradient.
    ///
    /// Box gradient is a feathered rounded rectangle, it is useful for rendering
    /// drop shadows or highlights for boxes. Parameters (x,y) define the top-left corner of the rectangle,
    /// (w,h) define the size of the rectangle, r defines the corner radius, and f feather. Feather defines how blurry
    /// the border of the rectangle is. Parameter `inner_color` specifies the inner color and `outer_color` the outer color of the gradient.
    /// The gradient is transformed by the current transform when it is passed to `fill_path()` or `stroke_path()`.
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::box_gradient(
    ///    0.0,
    ///    0.0,
    ///    100.0,
    ///    100.0,
    ///    10.0,
    ///    10.0,
    ///    Color::rgba(0, 0, 0, 128),
    ///    Color::rgba(0, 0, 0, 0),
    /// );
    ///
    /// let mut path = Path::new();
    /// path.rounded_rect(0.0, 0.0, 100.0, 100.0, 5.0);
    /// canvas.fill_path(&path, &bg);
    /// ```
    pub fn box_gradient(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        feather: f32,
        inner_color: Color,
        outer_color: Color,
    ) -> Self {
        Self::with_flavor(PaintFlavor::BoxGradient {
            pos: Position { x, y },
            width,
            height,
            radius,
            feather,
            colors: GradientColors::two_stop(inner_color, outer_color),
            transform: Transform2D::identity(),
        })
    }

    /// Creates and returns a radial gradient.
    ///
    /// Parameters (`cx`,`cy`) specify the center, `in_radius` and `out_radius` specify
    /// the inner and outer radius of the gradient, `inner_color` specifies the start color and `outer_color` the end color.
    /// The gradient is transformed by the current transform when it is passed to `fill_paint()` or `stroke_paint()`.
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::radial_gradient(
    ///    50.0,
    ///    50.0,
    ///    18.0,
    ///    24.0,
    ///    Color::rgba(0, 0, 0, 128),
    ///    Color::rgba(0, 0, 0, 0),
    /// );
    ///
    /// let mut path = Path::new();
    /// path.circle(50.0, 50.0, 20.0);
    /// canvas.fill_path(&path, &bg);
    /// ```
    pub fn radial_gradient(
        cx: f32,
        cy: f32,
        in_radius: f32,
        out_radius: f32,
        inner_color: Color,
        outer_color: Color,
    ) -> Self {
        Self::with_flavor(PaintFlavor::RadialGradient {
            center: Position { x: cx, y: cy },
            in_radius: (in_radius, in_radius),
            out_radius: (out_radius, out_radius),
            colors: GradientColors::two_stop(inner_color, outer_color),
            transform: Transform2D::identity(),
        })
    }

    /// Creates and returns an elliptical gradient/
    ///
    /// Parameters (`cx`,`cy`) specify the center, `in_rx`/`in_ry` and `out_rx`/`out_ry` specify
    /// the inner and outer ellipse radii along the x and y axes, `inner_color` specifies the start
    /// color and `outer_color` the end color. Passing equal x/y radii is equivalent to
    /// [`Paint::radial_gradient`].
    /// The gradient is transformed by the current transform when it is passed to `fill_paint()` or `stroke_paint()`.
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::elliptical_gradient(
    ///    50.0,
    ///    50.0,
    ///    18.0,
    ///    9.0,
    ///    24.0,
    ///    12.0,
    ///    Color::rgba(0, 0, 0, 128),
    ///    Color::rgba(0, 0, 0, 0),
    /// );
    ///
    /// let mut path = Path::new();
    /// path.ellipse(50.0, 50.0, 30.0, 15.0);
    /// canvas.fill_path(&path, &bg);
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn elliptical_gradient(
        cx: f32,
        cy: f32,
        in_rx: f32,
        in_ry: f32,
        out_rx: f32,
        out_ry: f32,
        inner_color: Color,
        outer_color: Color,
    ) -> Self {
        Self::with_flavor(PaintFlavor::RadialGradient {
            center: Position { x: cx, y: cy },
            in_radius: (in_rx, in_ry),
            out_radius: (out_rx, out_ry),
            colors: GradientColors::two_stop(inner_color, outer_color),
            transform: Transform2D::identity(),
        })
    }

    /// Creates and returns a multi-stop radial gradient.
    ///
    /// Parameters (`cx`,`cy`) specify the center, `in_radius` and `out_radius` specify the inner and outer radius of the gradient,
    /// colors specifies a list of color stops with offsets. The first offset should be 0.0 and the last offset should be 1.0.
    /// Stops with a non-finite offset or color component are ignored.
    ///
    /// The gradient is transformed by the current transform when it is passed to `fill_paint()` or `stroke_paint()`.
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::radial_gradient_stops(
    ///    50.0,
    ///    50.0,
    ///    18.0,
    ///    24.0,
    ///    [
    ///         (0.0, Color::rgba(0, 0, 0, 128)),
    ///         (0.5, Color::rgba(0, 0, 128, 128)),
    ///         (1.0, Color::rgba(0, 128, 0, 128))
    ///    ]
    /// );
    ///
    /// let mut path = Path::new();
    /// path.circle(50.0, 50.0, 20.0);
    /// canvas.fill_path(&path, &bg);
    /// ```
    pub fn radial_gradient_stops(
        cx: f32,
        cy: f32,
        in_radius: f32,
        out_radius: f32,
        stops: impl IntoIterator<Item = (f32, Color)>,
    ) -> Self {
        Self::with_flavor(PaintFlavor::RadialGradient {
            center: Position { x: cx, y: cy },
            in_radius: (in_radius, in_radius),
            out_radius: (out_radius, out_radius),
            colors: GradientColors::from_stops(stops),
            transform: Transform2D::identity(),
        })
    }

    /// Creates and returns a multi-stop elliptical gradient.
    ///
    /// Parameters (`cx`,`cy`) specify the center, `in_rx`/`in_ry` and `out_rx`/`out_ry` specify
    /// the inner and outer ellipse radii along the x and y axes, colors specifies a list of color
    /// stops with offsets. The first offset should be 0.0 and the last offset should be 1.0.
    /// Passing equal x/y radii is equivalent to [`Paint::radial_gradient_stops`].
    /// Stops with a non-finite offset or color component are ignored.
    ///
    /// The gradient is transformed by the current transform when it is passed to `fill_paint()` or `stroke_paint()`.
    ///
    /// # Example
    /// ```
    /// use femtovg::{Paint, Path, Color, Canvas, ImageFlags, renderer::Void};
    ///
    /// let mut canvas = Canvas::new(Void).expect("Cannot create canvas");
    ///
    /// let bg = Paint::elliptical_gradient_stops(
    ///    50.0,
    ///    50.0,
    ///    18.0,
    ///    9.0,
    ///    24.0,
    ///    12.0,
    ///    [
    ///         (0.0, Color::rgba(0, 0, 0, 128)),
    ///         (0.5, Color::rgba(0, 0, 128, 128)),
    ///         (1.0, Color::rgba(0, 128, 0, 128))
    ///    ]
    /// );
    ///
    /// let mut path = Path::new();
    /// path.ellipse(50.0, 50.0, 30.0, 15.0);
    /// canvas.fill_path(&path, &bg);
    /// ```
    pub fn elliptical_gradient_stops(
        cx: f32,
        cy: f32,
        in_rx: f32,
        in_ry: f32,
        out_rx: f32,
        out_ry: f32,
        stops: impl IntoIterator<Item = (f32, Color)>,
    ) -> Self {
        Self::with_flavor(PaintFlavor::RadialGradient {
            center: Position { x: cx, y: cy },
            in_radius: (in_rx, in_ry),
            out_radius: (out_rx, out_ry),
            colors: GradientColors::from_stops(stops),
            transform: Transform2D::identity(),
        })
    }

    /// Creates and returns a two-color two-point radial gradient.
    ///
    /// This is the general Canvas 2D `createRadialGradient(x0, y0, r0, x1, y1, r1)`
    /// form: colour `0.0` fills the start circle (`x0`, `y0`, `r0`) and colour
    /// `1.0` fills the end circle (`x1`, `y1`, `r1`), which may be independently
    /// centered. Where the two circles are concentric prefer
    /// [`radial_gradient`](Self::radial_gradient), which uses a cheaper shader.
    ///
    /// The gradient is transformed by the current transform when it is passed to
    /// `fill_paint()` or `stroke_paint()`.
    #[allow(clippy::too_many_arguments)]
    pub fn two_point_radial_gradient(
        start_x: f32,
        start_y: f32,
        start_radius: f32,
        end_x: f32,
        end_y: f32,
        end_radius: f32,
        inner_color: Color,
        outer_color: Color,
    ) -> Self {
        Self::with_flavor(PaintFlavor::TwoPointRadialGradient {
            start_center: Position { x: start_x, y: start_y },
            start_radius,
            end_center: Position { x: end_x, y: end_y },
            end_radius,
            colors: GradientColors::two_stop(inner_color, outer_color),
            transform: Transform2D::identity(),
        })
    }

    /// Creates and returns a multi-stop two-point radial gradient.
    ///
    /// Like [`two_point_radial_gradient`](Self::two_point_radial_gradient), but
    /// takes a list of colour stops with offsets. The first offset should be
    /// `0.0` and the last `1.0`. This is the general Canvas 2D
    /// `createRadialGradient(x0, y0, r0, x1, y1, r1)` form with `addColorStop`.
    /// Stops with a non-finite offset or color component are ignored.
    ///
    /// The gradient is transformed by the current transform when it is passed to
    /// `fill_paint()` or `stroke_paint()`.
    #[allow(clippy::too_many_arguments)]
    pub fn two_point_radial_gradient_stops(
        start_x: f32,
        start_y: f32,
        start_radius: f32,
        end_x: f32,
        end_y: f32,
        end_radius: f32,
        stops: impl IntoIterator<Item = (f32, Color)>,
    ) -> Self {
        Self::with_flavor(PaintFlavor::TwoPointRadialGradient {
            start_center: Position { x: start_x, y: start_y },
            start_radius,
            end_center: Position { x: end_x, y: end_y },
            end_radius,
            colors: GradientColors::from_stops(stops),
            transform: Transform2D::identity(),
        })
    }

    /// Creates and returns a two-color conic gradient.
    ///
    /// Parameters (`cx`,`cy`) specify the center. The gradient begins at the
    /// positive x axis (the 3 o'clock direction) and proceeds clockwise, matching
    /// the Canvas 2D `createConicGradient(0, cx, cy)` semantics.
    pub fn conic_gradient(cx: f32, cy: f32, start_color: Color, end_color: Color) -> Self {
        Self::conic_gradient_with_angle(cx, cy, 0.0, start_color, end_color)
    }

    /// Creates and returns a two-color conic gradient with a start angle.
    ///
    /// Parameters (`cx`,`cy`) specify the center. `start_angle` is in radians,
    /// measured clockwise from the positive x axis, matching the Canvas 2D
    /// `createConicGradient(start_angle, cx, cy)` semantics.
    ///
    /// Following the Canvas convention that non-finite values must not poison
    /// rendering state, a non-finite `start_angle` (NaN or an infinity) is
    /// ignored and treated as `0.0`.
    pub fn conic_gradient_with_angle(cx: f32, cy: f32, start_angle: f32, start_color: Color, end_color: Color) -> Self {
        Self::with_flavor(PaintFlavor::ConicGradient {
            center: Position { x: cx, y: cy },
            start_angle: Self::finite_start_angle_or_zero(start_angle),
            colors: GradientColors::two_stop(start_color, end_color),
            transform: Transform2D::identity(),
        })
    }

    /// Creates and returns a multi-stop conic gradient.
    ///
    /// Parameters (`cx`,`cy`) specify the center. The gradient begins at the
    /// positive x axis (the 3 o'clock direction) and proceeds clockwise, matching
    /// the Canvas 2D `createConicGradient(0, cx, cy)` semantics.
    /// Stops with a non-finite offset or color component are ignored.
    pub fn conic_gradient_stops(cx: f32, cy: f32, stops: impl IntoIterator<Item = (f32, Color)>) -> Self {
        Self::conic_gradient_stops_with_angle(cx, cy, 0.0, stops)
    }

    /// Creates and returns a multi-stop conic gradient with a start angle.
    ///
    /// Parameters (`cx`,`cy`) specify the center. `start_angle` is in radians,
    /// measured clockwise from the positive x axis, matching the Canvas 2D
    /// `createConicGradient(start_angle, cx, cy)` semantics.
    ///
    /// Following the Canvas convention that non-finite values must not poison
    /// rendering state, a non-finite `start_angle` (NaN or an infinity) is
    /// ignored and treated as `0.0`. Stops with a non-finite offset or color component are ignored.
    pub fn conic_gradient_stops_with_angle(
        cx: f32,
        cy: f32,
        start_angle: f32,
        stops: impl IntoIterator<Item = (f32, Color)>,
    ) -> Self {
        Self::with_flavor(PaintFlavor::ConicGradient {
            center: Position { x: cx, y: cy },
            start_angle: Self::finite_start_angle_or_zero(start_angle),
            colors: GradientColors::from_stops(stops),
            transform: Transform2D::identity(),
        })
    }

    /// A NaN or infinite conic start angle would propagate through the shader's
    /// `fract()` wrap and poison every covered pixel, so the constructors above
    /// drop such values in favor of the no-rotation default.
    fn finite_start_angle_or_zero(start_angle: f32) -> f32 {
        if start_angle.is_finite() {
            start_angle
        } else {
            0.0
        }
    }

    /// Sets the color of the paint.
    pub fn set_color(&mut self, color: Color) {
        self.flavor = PaintFlavor::Color(color);
    }

    /// Returns the paint with the color set to the specified value.
    #[inline]
    pub fn with_color(mut self, color: Color) -> Self {
        self.set_color(color);
        self
    }

    /// Returns the current anti-alias setting.
    #[inline]
    pub fn anti_alias(&self) -> bool {
        self.shape_anti_alias
    }

    /// Sets whether shapes drawn with this paint will be anti-aliased.
    #[inline]
    pub fn set_anti_alias(&mut self, value: bool) {
        self.shape_anti_alias = value;
    }

    /// Returns the paint with anti-alias set to the specified value.
    #[inline]
    pub fn with_anti_alias(mut self, value: bool) -> Self {
        self.set_anti_alias(value);
        self
    }

    /// Returns whether higher quality stencil strokes are used.
    #[inline]
    pub fn stencil_strokes(&self) -> bool {
        self.stroke.stencil_strokes
    }

    /// Sets whether to use higher quality stencil strokes.
    #[inline]
    pub fn set_stencil_strokes(&mut self, value: bool) {
        self.stroke.stencil_strokes = value;
    }

    /// Returns the paint with stencil strokes set to the specified value.
    #[inline]
    pub fn with_stencil_strokes(mut self, value: bool) -> Self {
        self.set_stencil_strokes(value);
        self
    }

    /// Returns the current line width.
    #[inline]
    pub fn line_width(&self) -> f32 {
        self.stroke.line_width
    }

    /// Sets the line width.
    #[inline]
    pub fn set_line_width(&mut self, width: f32) {
        self.stroke.line_width = width;
    }

    /// Returns the paint with line width set to the specified value.
    #[inline]
    pub fn with_line_width(mut self, width: f32) -> Self {
        self.set_line_width(width);
        self
    }

    /// Returns the current miter limit.
    #[inline]
    pub fn miter_limit(&self) -> f32 {
        self.stroke.miter_limit
    }

    /// Sets the limit at which a sharp corner is drawn beveled.
    ///
    /// If the miter at a corner exceeds this limit, `LineJoin` is replaced with `LineJoin::Bevel`.
    #[inline]
    pub fn set_miter_limit(&mut self, limit: f32) {
        self.stroke.miter_limit = limit;
    }

    /// Returns the paint with the miter limit set to the specified value.
    #[inline]
    pub fn with_miter_limit(mut self, limit: f32) -> Self {
        self.set_miter_limit(limit);
        self
    }

    /// Returns the current start line cap.
    #[inline]
    pub fn line_cap_start(&self) -> LineCap {
        self.stroke.line_cap_start
    }

    /// Returns the current end line cap.
    #[inline]
    pub fn line_cap_end(&self) -> LineCap {
        self.stroke.line_cap_end
    }

    /// Sets the line cap for both start and end of the line.
    pub fn set_line_cap(&mut self, cap: LineCap) {
        self.stroke.line_cap_start = cap;
        self.stroke.line_cap_end = cap;
    }

    /// Returns the paint with the line cap set to the specified value.
    #[inline]
    pub fn with_line_cap(mut self, cap: LineCap) -> Self {
        self.set_line_cap(cap);
        self
    }

    /// Sets the line cap for the start of the line.
    #[inline]
    pub fn set_line_cap_start(&mut self, cap: LineCap) {
        self.stroke.line_cap_start = cap;
    }

    /// Returns the paint with the start line cap set to the specified value.
    #[inline]
    pub fn with_line_cap_start(mut self, cap: LineCap) -> Self {
        self.set_line_cap_start(cap);
        self
    }

    /// Sets the line cap for the end of the line.
    #[inline]
    pub fn set_line_cap_end(&mut self, cap: LineCap) {
        self.stroke.line_cap_end = cap;
    }

    /// Returns the paint with the end line cap set to the specified value.
    #[inline]
    pub fn with_line_cap_end(mut self, cap: LineCap) -> Self {
        self.set_line_cap_end(cap);
        self
    }

    /// Returns the current line join.
    #[inline]
    pub fn line_join(&self) -> LineJoin {
        self.stroke.line_join
    }

    /// Sets the line join.
    #[inline]
    pub fn set_line_join(&mut self, join: LineJoin) {
        self.stroke.line_join = join;
    }

    /// Returns the paint with the line join set to the specified value.
    #[inline]
    pub fn with_line_join(mut self, join: LineJoin) -> Self {
        self.set_line_join(join);
        self
    }

    /// Returns the current dash pattern used for stroked paths.
    ///
    /// An empty pattern draws a solid stroke. Odd-length patterns are stored as
    /// the pattern repeated twice, matching SVG and Canvas 2D behavior.
    #[inline]
    pub fn line_dash(&self) -> &[f32] {
        &self.stroke.line_dash
    }

    /// Sets the dash pattern used for stroked paths.
    ///
    /// Passing an empty slice clears the dash pattern. Patterns containing
    /// non-finite or negative values are ignored, leaving the current pattern
    /// unchanged. If the supplied pattern has an odd number of entries, it is
    /// repeated once to form an even pattern as required by SVG and Canvas 2D.
    pub fn set_line_dash(&mut self, dash: &[f32]) {
        if dash.iter().any(|value| !value.is_finite() || *value < 0.0) {
            return;
        }

        self.stroke.line_dash.clear();
        if dash.is_empty() {
            return;
        }

        self.stroke.line_dash.extend_from_slice(dash);
        if dash.len() % 2 == 1 {
            self.stroke.line_dash.extend_from_slice(dash);
        }
    }

    /// Returns the paint with the dash pattern set to the specified value.
    #[inline]
    pub fn with_line_dash(mut self, dash: &[f32]) -> Self {
        self.set_line_dash(dash);
        self
    }

    /// Returns the current dash pattern offset used for stroked paths.
    #[inline]
    pub fn line_dash_offset(&self) -> f32 {
        self.stroke.line_dash_offset
    }

    /// Sets the dash pattern offset used for stroked paths.
    #[inline]
    pub fn set_line_dash_offset(&mut self, offset: f32) {
        if offset.is_finite() {
            self.stroke.line_dash_offset = offset;
        }
    }

    /// Returns the paint with the dash pattern offset set to the specified value.
    #[inline]
    pub fn with_line_dash_offset(mut self, offset: f32) -> Self {
        self.set_line_dash_offset(offset);
        self
    }

    /// Sets the font.
    pub fn set_font(&mut self, font_ids: &[FontId]) {
        self.text.font_ids = Default::default();

        for (i, id) in font_ids.iter().take(8).enumerate() {
            self.text.font_ids[i] = Some(*id);
        }
    }

    /// Returns the paint with the font set to the specified value.
    #[inline]
    pub fn with_font(mut self, font_ids: &[FontId]) -> Self {
        self.set_font(font_ids);
        self
    }

    /// Returns the current font size for text operations.
    #[inline]
    pub fn font_size(&self) -> f32 {
        self.text.font_size
    }

    /// Sets the font size for text operations.
    #[inline]
    pub fn set_font_size(&mut self, size: f32) {
        self.text.font_size = size;
    }

    /// Returns the paint with the font size set to the specified value.
    #[inline]
    pub fn with_font_size(mut self, size: f32) -> Self {
        self.set_font_size(size);
        self
    }

    /// Returns the current letter spacing for text operations.
    #[inline]
    pub fn letter_spacing(&self) -> f32 {
        self.text.letter_spacing
    }

    /// Sets the letter spacing for text operations.
    #[inline]
    pub fn set_letter_spacing(&mut self, spacing: f32) {
        self.text.letter_spacing = spacing;
    }

    /// Returns the paint with the letter spacing set to the specified value.
    #[inline]
    pub fn with_letter_spacing(mut self, spacing: f32) -> Self {
        self.set_letter_spacing(spacing);
        self
    }

    /// Returns the current text baseline for text operations.
    #[inline]
    pub fn text_baseline(&self) -> Baseline {
        self.text.text_baseline
    }

    /// Sets the text baseline for text operations.
    #[inline]
    pub fn set_text_baseline(&mut self, align: Baseline) {
        self.text.text_baseline = align;
    }

    /// Returns the paint with the text baseline set to the specified value.
    #[inline]
    pub fn with_text_baseline(mut self, align: Baseline) -> Self {
        self.set_text_baseline(align);
        self
    }

    /// Returns the current text alignment for text operations.
    #[inline]
    pub fn text_align(&self) -> Align {
        self.text.text_align
    }

    /// Sets the text alignment for text operations.
    #[inline]
    pub fn set_text_align(&mut self, align: Align) {
        self.text.text_align = align;
    }

    /// Returns the paint with the text alignment set to the specified value.
    #[inline]
    pub fn with_text_align(mut self, align: Align) -> Self {
        self.set_text_align(align);
        self
    }

    // --- Text decoration (underline / strikethrough / overline) ---

    /// Returns the current set of text decoration lines for text operations.
    #[inline]
    pub fn text_decoration(&self) -> TextDecoration {
        self.text.text_decoration
    }

    /// Sets the text decoration lines drawn along text runs.
    ///
    /// Decoration lines are drawn in the paint's color using the font's own
    /// underline/strikeout metrics (overline is derived from the ascent). This
    /// extends femtovg toward SVG/CSS `text-decoration`; Canvas 2D has no
    /// equivalent.
    #[inline]
    pub fn set_text_decoration(&mut self, decoration: TextDecoration) {
        self.text.text_decoration = decoration;
    }

    /// Returns the paint with the text decoration set to the specified value.
    #[inline]
    pub fn with_text_decoration(mut self, decoration: TextDecoration) -> Self {
        self.set_text_decoration(decoration);
        self
    }

    // --- Font weight (wght axis) ---

    /// Returns the current font weight override for variable fonts, in design space.
    ///
    /// Returns `None` if no weight override is set, meaning the font's default weight is used.
    #[inline]
    pub fn font_weight(&self) -> Option<f32> {
        self.text.font_variations.get(FontVariations::tag_to_u32(b"wght"))
    }

    /// Sets the font weight for variable fonts.
    ///
    /// The value is in design space (e.g. 100–900 for the `wght` axis), not normalized.
    /// Common values: 100 (Thin), 300 (Light), 400 (Regular), 700 (Bold), 900 (Black).
    /// The actual supported range depends on the font; query it with
    /// [`Canvas::font_variation_axes`](crate::Canvas::font_variation_axes).
    /// This only affects variable fonts; for static fonts it has no effect.
    #[inline]
    pub fn set_font_weight(&mut self, weight: f32) {
        self.text
            .font_variations
            .set(FontVariations::tag_to_u32(b"wght"), weight);
    }

    /// Returns the paint with the font weight set to the specified value.
    ///
    /// See [`set_font_weight`](Self::set_font_weight) for details on values and range.
    #[inline]
    pub fn with_font_weight(mut self, weight: f32) -> Self {
        self.set_font_weight(weight);
        self
    }

    /// Clears the font weight override, reverting to the font's default weight.
    #[inline]
    pub fn clear_font_weight(&mut self) {
        self.text.font_variations.remove(FontVariations::tag_to_u32(b"wght"));
    }

    // --- Font italic (ital axis) ---

    /// Returns the current font italic override for variable fonts.
    ///
    /// Returns `None` if no italic override is set.
    /// `true` means italic (ital=1), `false` means upright (ital=0).
    #[inline]
    pub fn font_italic(&self) -> Option<bool> {
        self.text
            .font_variations
            .get(FontVariations::tag_to_u32(b"ital"))
            .map(|v| v >= 0.5)
    }

    /// Sets the font italic for variable fonts.
    ///
    /// `true` sets `ital=1` (italic), `false` sets `ital=0` (upright).
    /// Also sets the `slnt` axis as a fallback (`-12` for italic, `0` for upright),
    /// so fonts with only a `slnt` axis (no `ital`) will still render slanted.
    /// An explicit `set_font_slant()` call after this will override the fallback value.
    #[inline]
    pub fn set_font_italic(&mut self, italic: bool) {
        self.text
            .font_variations
            .set(FontVariations::tag_to_u32(b"ital"), if italic { 1.0 } else { 0.0 });
        self.text
            .font_variations
            .set(FontVariations::tag_to_u32(b"slnt"), if italic { -12.0 } else { 0.0 });
    }

    /// Returns the paint with the font italic set to the specified value.
    #[inline]
    pub fn with_font_italic(mut self, italic: bool) -> Self {
        self.set_font_italic(italic);
        self
    }

    /// Clears the font italic override, removing both `ital` and `slnt` axes.
    #[inline]
    pub fn clear_font_italic(&mut self) {
        self.text.font_variations.remove(FontVariations::tag_to_u32(b"ital"));
        self.text.font_variations.remove(FontVariations::tag_to_u32(b"slnt"));
    }

    // --- Font slant (slnt axis) ---

    /// Returns the current font slant override for variable fonts, in design space (degrees).
    ///
    /// Returns `None` if no slant override is set.
    /// Negative values slant backward (typical italic direction).
    #[inline]
    pub fn font_slant(&self) -> Option<f32> {
        self.text.font_variations.get(FontVariations::tag_to_u32(b"slnt"))
    }

    /// Sets the font slant for variable fonts.
    ///
    /// The value is in design space (degrees for the `slnt` axis), not normalized.
    /// Negative values slant backward (typical italic direction).
    /// For example, `-12.0` is a common italic slant.
    /// The actual supported range depends on the font; query it with
    /// [`Canvas::font_variation_axes`](crate::Canvas::font_variation_axes).
    /// This only affects variable fonts with a `slnt` axis.
    #[inline]
    pub fn set_font_slant(&mut self, degrees: f32) {
        self.text
            .font_variations
            .set(FontVariations::tag_to_u32(b"slnt"), degrees);
    }

    /// Returns the paint with the font slant set to the specified value.
    ///
    /// See [`set_font_slant`](Self::set_font_slant) for details on values and range.
    #[inline]
    pub fn with_font_slant(mut self, degrees: f32) -> Self {
        self.set_font_slant(degrees);
        self
    }

    /// Clears the font slant override.
    #[inline]
    pub fn clear_font_slant(&mut self) {
        self.text.font_variations.remove(FontVariations::tag_to_u32(b"slnt"));
    }

    // --- Generic font variation API ---

    /// Returns the value of a font variation axis by its 4-byte tag, in design space.
    ///
    /// For example, `paint.font_variation(b"wdth")` returns the width axis value.
    /// Returns `None` if the axis has not been set.
    #[inline]
    pub fn font_variation(&self, tag: &[u8; 4]) -> Option<f32> {
        self.text.font_variations.get(FontVariations::tag_to_u32(tag))
    }

    /// Sets a font variation axis by its 4-byte tag.
    ///
    /// Values are in design space (the raw axis values defined in the font's
    /// `fvar` table), not normalized coordinates. For example,
    /// `paint.set_font_variation(b"wdth", 75.0)` sets the width axis to 75%
    /// condensed. The valid range depends on the font; query it with
    /// [`Canvas::font_variation_axes`](crate::Canvas::font_variation_axes).
    /// This only affects variable fonts with the specified axis.
    #[inline]
    pub fn set_font_variation(&mut self, tag: &[u8; 4], value: f32) {
        self.text.font_variations.set(FontVariations::tag_to_u32(tag), value);
    }

    /// Returns the paint with a font variation axis set.
    ///
    /// See [`set_font_variation`](Self::set_font_variation) for details on values and range.
    #[inline]
    pub fn with_font_variation(mut self, tag: &[u8; 4], value: f32) -> Self {
        self.set_font_variation(tag, value);
        self
    }

    /// Clears a single font variation axis by its 4-byte tag.
    #[inline]
    pub fn clear_font_variation(&mut self, tag: &[u8; 4]) {
        self.text.font_variations.remove(FontVariations::tag_to_u32(tag));
    }

    /// Clears all font variation overrides.
    #[inline]
    pub fn clear_font_variations(&mut self) {
        self.text.font_variations.clear();
    }

    /// Returns the current fill rule for filling paths.
    #[inline]
    pub fn fill_rule(&self) -> FillRule {
        self.fill_rule
    }

    /// Sets a transform mapping the gradient's own coordinate system into user
    /// space, the role SVG's `gradientTransform` plays.
    ///
    /// It applies to the gradient only, ahead of the canvas transform, so the
    /// shape being filled does not move with it. This is what expresses a
    /// gradient whose two axes are scaled differently — an elliptical radial
    /// gradient, say, which the circle and radius parameters cannot describe on
    /// their own. Paints that are not gradients ignore it.
    pub fn set_gradient_transform(&mut self, transform: Transform2D) {
        match &mut self.flavor {
            PaintFlavor::LinearGradient { transform: t, .. }
            | PaintFlavor::BoxGradient { transform: t, .. }
            | PaintFlavor::RadialGradient { transform: t, .. }
            | PaintFlavor::ConicGradient { transform: t, .. }
            | PaintFlavor::TwoPointRadialGradient { transform: t, .. } => *t = transform,
            PaintFlavor::Color(_) | PaintFlavor::Image { .. } => {}
        }
    }

    /// Builder form of [`Paint::set_gradient_transform`].
    #[must_use]
    pub fn with_gradient_transform(mut self, transform: Transform2D) -> Self {
        self.set_gradient_transform(transform);
        self
    }

    /// Sets the color space gradient stops are interpolated in.
    /// Ignored on non-gradient paints.
    pub fn set_interpolation_space(&mut self, space: ColorSpace) {
        if let Some(colors) = self.flavor.gradient_colors_mut() {
            colors.set_interpolation_space(space);
        }
    }

    /// Returns the paint with the color space set to the specified value.
    ///
    /// See [`set_interpolation_space`](Self::set_interpolation_space) for details.
    #[inline]
    #[must_use]
    pub fn with_interpolation_space(mut self, space: ColorSpace) -> Self {
        self.set_interpolation_space(space);
        self
    }

    /// Sets whether gradient stops are interpolated premultiplied by their alpha.
    /// Ignored on non-gradient paints.
    ///
    /// By default, stops are interpolated straight (unpremultiplied), as Canvas 2D and SVG
    /// gradients are, so a stop's color shows even where it fades out:
    /// `transparent` (transparent black) to white passes through gray.
    /// CSS gradients interpolate premultiplied, so a transparent stop adds no color:
    /// `transparent` to white stays white and only fades in.
    pub fn set_premultiplied_interpolation(&mut self, premultiplied: bool) {
        if let Some(colors) = self.flavor.gradient_colors_mut() {
            colors.set_premultiplied_interpolation(premultiplied);
        }
    }

    /// Returns the paint with premultiplied interpolation set to the specified value.
    ///
    /// See [`set_premultiplied_interpolation`](Self::set_premultiplied_interpolation) for details.
    #[inline]
    #[must_use]
    pub fn with_premultiplied_interpolation(mut self, premultiplied: bool) -> Self {
        self.set_premultiplied_interpolation(premultiplied);
        self
    }

    /// Sets the fill rule for filling paths.
    #[inline]
    pub fn set_fill_rule(&mut self, rule: FillRule) {
        self.fill_rule = rule;
    }

    /// Returns the paint with the fill rule set to the specified value.
    #[inline]
    pub fn with_fill_rule(mut self, rule: FillRule) -> Self {
        self.set_fill_rule(rule);
        self
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::{GradientColors, GradientStop, Interpolation, MultiStopGradient, Paint, PaintFlavor};
    #[cfg(feature = "serde")]
    use super::{Position, Transform2D};
    use crate::{Color, ColorSpace};

    #[test]
    fn with_interpolation_space_turns_two_stop_gradient_into_multi_stop() {
        let (start, end) = (Color::rgb(255, 0, 0), Color::rgb(0, 255, 0));
        let paint = Paint::linear_gradient(0.0, 0.0, 1.0, 1.0, start, end).with_interpolation_space(ColorSpace::Oklab);

        let PaintFlavor::LinearGradient { colors, .. } = paint.flavor else {
            panic!("expected LinearGradient");
        };
        let GradientColors::MultiStop { stops } = colors else {
            panic!("expected MultiStop");
        };
        assert_eq!(stops.space(), ColorSpace::Oklab);
        assert_eq!(stops.len(), 2);
        assert_eq!((stops.get(0).0, stops.get(0).1), (0.0, start));
        assert_eq!((stops.get(1).0, stops.get(1).1), (1.0, end));
    }

    #[test]
    fn with_interpolation_space_srgb_keeps_two_stop_gradient() {
        let paint = Paint::linear_gradient(0.0, 0.0, 1.0, 1.0, Color::rgb(255, 0, 0), Color::rgb(0, 255, 0))
            .with_interpolation_space(ColorSpace::Srgb);

        let PaintFlavor::LinearGradient { colors, .. } = paint.flavor else {
            panic!("expected LinearGradient");
        };
        assert!(matches!(colors, GradientColors::TwoStop { .. }));
    }

    #[test]
    fn with_interpolation_space_srgb_restores_promoted_two_stop_gradient() {
        let (start, end) = (Color::rgbf(1.0, 0.0, 0.0), Color::rgbf(0.0, 1.0, 0.0));
        let mut paint = Paint::linear_gradient(0.0, 0.0, 1.0, 1.0, start, end)
            .with_interpolation_space(ColorSpace::Oklab)
            .with_premultiplied_interpolation(true);
        paint.flavor.mul_alpha(0.5);
        paint.set_interpolation_space(ColorSpace::Srgb);

        let PaintFlavor::LinearGradient { colors, .. } = paint.flavor else {
            panic!("expected LinearGradient");
        };
        assert_eq!(
            colors,
            GradientColors::TwoStop {
                start_color: Color::rgbaf(1.0, 0.0, 0.0, 0.5),
                end_color: Color::rgbaf(0.0, 1.0, 0.0, 0.5),
                interpolation: Interpolation {
                    space: ColorSpace::Srgb,
                    premultiplied: true,
                },
            }
        );
    }

    #[test]
    fn with_interpolation_space_sets_multi_stop_gradient_interpolation_space() {
        let paint = Paint::linear_gradient_stops(
            0.0,
            0.0,
            1.0,
            1.0,
            [
                (0.0, Color::rgb(255, 0, 0)),
                (0.5, Color::rgb(0, 255, 0)),
                (1.0, Color::rgb(0, 0, 255)),
            ],
        )
        .with_interpolation_space(ColorSpace::Oklab);

        let PaintFlavor::LinearGradient { colors, .. } = paint.flavor else {
            panic!("expected LinearGradient");
        };
        let GradientColors::MultiStop { stops } = colors else {
            panic!("expected MultiStop");
        };
        assert_eq!(stops.space(), ColorSpace::Oklab);
    }

    #[test]
    fn with_premultiplied_interpolation_keeps_a_fading_two_stop_gradient_on_the_two_stop_path() {
        let (start, end) = (Color::rgba(0, 0, 0, 0), Color::white());
        let paint = Paint::linear_gradient(0.0, 0.0, 1.0, 1.0, start, end).with_premultiplied_interpolation(true);

        let PaintFlavor::LinearGradient { colors, .. } = paint.flavor else {
            panic!("expected LinearGradient");
        };
        assert_eq!(
            colors,
            GradientColors::TwoStop {
                start_color: start,
                end_color: end,
                interpolation: Interpolation {
                    space: ColorSpace::Srgb,
                    premultiplied: true,
                },
            }
        );
    }

    #[test]
    fn with_premultiplied_interpolation_survives_a_later_interpolation_space() {
        let paint = Paint::linear_gradient(0.0, 0.0, 1.0, 1.0, Color::rgba(0, 0, 0, 0), Color::white())
            .with_premultiplied_interpolation(true)
            .with_interpolation_space(ColorSpace::Oklab);

        let PaintFlavor::LinearGradient { colors, .. } = paint.flavor else {
            panic!("expected LinearGradient");
        };
        let GradientColors::MultiStop { stops } = colors else {
            panic!("expected MultiStop");
        };
        assert!(stops.premultiplied());
        assert_eq!(stops.space(), ColorSpace::Oklab);
    }

    #[test]
    fn with_interpolation_space_ignores_non_gradient_paints() {
        let original = Color::rgb(1, 2, 3);
        let paint = Paint::color(original).with_interpolation_space(ColorSpace::Oklab);
        let PaintFlavor::Color(color) = paint.flavor else {
            panic!("expected Color");
        };
        assert_eq!(color, original);
    }

    fn gradient_colors(paint: Paint) -> GradientColors {
        match paint.flavor {
            PaintFlavor::LinearGradient { colors, .. }
            | PaintFlavor::BoxGradient { colors, .. }
            | PaintFlavor::RadialGradient { colors, .. }
            | PaintFlavor::ConicGradient { colors, .. }
            | PaintFlavor::TwoPointRadialGradient { colors, .. } => colors,
            other => panic!("expected a gradient, got {other:?}"),
        }
    }

    fn every_stops_constructor(stops: &[(f32, Color)]) -> Vec<GradientColors> {
        let s = || stops.iter().copied();
        [
            Paint::linear_gradient_stops(0.0, 0.0, 1.0, 1.0, s()),
            Paint::radial_gradient_stops(0.0, 0.0, 1.0, 2.0, s()),
            Paint::elliptical_gradient_stops(0.0, 0.0, 1.0, 2.0, 3.0, 4.0, s()),
            Paint::two_point_radial_gradient_stops(0.0, 0.0, 1.0, 1.0, 1.0, 2.0, s()),
            Paint::conic_gradient_stops(0.0, 0.0, s()),
            Paint::conic_gradient_stops_with_angle(0.0, 0.0, 1.0, s()),
        ]
        .into_iter()
        .map(gradient_colors)
        .collect()
    }

    #[test]
    fn gradient_stops_drop_non_finite_offsets_and_colors() {
        let (red, blue, white) = (Color::rgb(255, 0, 0), Color::rgb(0, 0, 255), Color::white());
        let nan_color = Color::rgbaf(f32::NAN, 0.0, 0.0, 1.0);
        let inf_color = Color::rgbaf(0.0, 0.0, 0.0, f32::INFINITY);
        let stops = [
            (0.0, red),
            (f32::NAN, white),
            (f32::INFINITY, white),
            (f32::NEG_INFINITY, white),
            (0.3, nan_color),
            (0.4, inf_color),
            (0.5, blue),
            (1.0, white),
        ];

        for colors in every_stops_constructor(&stops) {
            let GradientColors::MultiStop { stops } = colors else {
                panic!("expected MultiStop");
            };
            let kept: Vec<_> = (0..stops.len()).map(|i| (stops.get(i).0, stops.get(i).1)).collect();
            assert_eq!(kept, [(0.0, red), (0.5, blue), (1.0, white)]);
        }
    }

    /// Dropping the NaN stop can leave an extent-spanning pair for the two-stop shader.
    #[test]
    fn gradient_with_only_a_nan_middle_stop_becomes_two_stop() {
        let (red, blue) = (Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
        for colors in every_stops_constructor(&[(0.0, red), (f32::NAN, Color::white()), (1.0, blue)]) {
            assert_eq!(colors, GradientColors::two_stop(red, blue));
        }
    }

    /// `GradientStore` keys on `Ord`; a NaN that slips past the constructors
    /// (e.g. through deserialization) must not alias another gradient's ramp.
    #[test]
    fn multi_stop_gradient_ordering_is_total_for_nan() {
        let gradient = |offset: f32, tint: f32| MultiStopGradient {
            shared_stops: [
                GradientStop(0.0, Color::black()),
                GradientStop(offset, Color::white()),
                GradientStop(1.0, Color::black()),
            ]
            .into(),
            tint,
            interpolation: Interpolation::default(),
        };

        let half = gradient(0.5, 1.0);
        let nan = gradient(f32::NAN, 1.0);
        assert_ne!(half.cmp(&nan), Ordering::Equal);
        assert_eq!(half.cmp(&nan), nan.cmp(&half).reverse());
        assert_eq!(nan.cmp(&nan.clone()), Ordering::Equal);
        assert_eq!(nan, nan.clone());
        assert_ne!(gradient(0.5, f32::NAN).cmp(&half), Ordering::Equal);

        let channel_nan = GradientStop(0.5, Color::rgbaf(f32::NAN, 1.0, 1.0, 1.0));
        assert_ne!(channel_nan.cmp(&GradientStop(0.5, Color::white())), Ordering::Equal);
    }

    /// -0.0 and 0.0 draw the same ramp, so `GradientStore` must not make two textures.
    #[test]
    fn multi_stop_gradient_ordering_equates_signed_zeros() {
        let gradient = |offset: f32, channel: f32, tint: f32| MultiStopGradient {
            shared_stops: [
                GradientStop(offset, Color::rgbaf(channel, 0.0, 0.0, 1.0)),
                GradientStop(0.5, Color::white()),
                GradientStop(1.0, Color::black()),
            ]
            .into(),
            tint,
            interpolation: Interpolation::default(),
        };

        assert_eq!(gradient(-0.0, 0.0, 1.0), gradient(0.0, 0.0, 1.0));
        assert_eq!(gradient(0.0, -0.0, 1.0), gradient(0.0, 0.0, 1.0));
        assert_eq!(gradient(0.0, 0.0, -0.0), gradient(0.0, 0.0, 0.0));
    }

    #[test]
    fn line_dash_empty_pattern_clears() {
        let mut paint = Paint::default().with_line_dash(&[1.0, 2.0]);

        paint.set_line_dash(&[]);

        assert!(paint.line_dash().is_empty());
    }

    #[test]
    fn line_dash_invalid_pattern_preserves_current_pattern() {
        let mut paint = Paint::default().with_line_dash(&[1.0, 2.0]);

        paint.set_line_dash(&[-1.0]);
        assert_eq!(paint.line_dash(), &[1.0, 2.0]);

        paint.set_line_dash(&[f32::NAN]);
        assert_eq!(paint.line_dash(), &[1.0, 2.0]);

        paint.set_line_dash(&[f32::INFINITY]);
        assert_eq!(paint.line_dash(), &[1.0, 2.0]);
    }

    #[test]
    fn line_dash_odd_pattern_repeats() {
        let paint = Paint::default().with_line_dash(&[1.0, 2.0, 3.0]);

        assert_eq!(paint.line_dash(), &[1.0, 2.0, 3.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn line_dash_all_zero_pattern_is_stored() {
        let paint = Paint::default().with_line_dash(&[0.0, 0.0]);

        assert_eq!(paint.line_dash(), &[0.0, 0.0]);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn conic_gradient_deserializes_without_start_angle() {
        let flavor = PaintFlavor::ConicGradient {
            center: Position { x: 1.0, y: 2.0 },
            start_angle: 1.5,
            colors: GradientColors::two_stop(Color::black(), Color::white()),
            transform: Transform2D::identity(),
        };

        // Learn the current serde representation, then strip `start_angle`
        // to mirror the "old format" payload produced before that field
        // existed (`ConicGradient { center, colors }`).
        let mut value = serde_json::to_value(&flavor).expect("serialize ConicGradient");
        let inner = value
            .get_mut("ConicGradient")
            .and_then(|v| v.as_object_mut())
            .expect("externally tagged ConicGradient object");
        assert!(inner.remove("start_angle").is_some(), "start_angle should be present");

        // Deserializing the field-less payload must succeed and default to 0.0.
        let restored: PaintFlavor = serde_json::from_value(value).expect("deserialize old format");
        match restored {
            PaintFlavor::ConicGradient {
                center, start_angle, ..
            } => {
                assert_eq!(start_angle, 0.0);
                assert_eq!(center.x, 1.0);
                assert_eq!(center.y, 2.0);
            }
            other => panic!("expected ConicGradient, got {other:?}"),
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn conic_gradient_round_trip_preserves_start_angle() {
        let flavor = PaintFlavor::ConicGradient {
            center: Position { x: 3.0, y: 4.0 },
            start_angle: 2.75,
            colors: GradientColors::two_stop(Color::black(), Color::white()),
            transform: Transform2D::identity(),
        };

        let json = serde_json::to_string(&flavor).expect("serialize ConicGradient");
        let restored: PaintFlavor = serde_json::from_str(&json).expect("deserialize ConicGradient");
        match restored {
            PaintFlavor::ConicGradient { start_angle, .. } => assert_eq!(start_angle, 2.75),
            other => panic!("expected ConicGradient, got {other:?}"),
        }
    }

    #[test]
    fn conic_gradient_non_finite_start_angle_is_treated_as_zero() {
        let start_angle_of = |paint: Paint| match paint.flavor {
            PaintFlavor::ConicGradient { start_angle, .. } => start_angle,
            other => panic!("expected ConicGradient, got {other:?}"),
        };

        for bad_angle in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let two_stop = Paint::conic_gradient_with_angle(10.0, 20.0, bad_angle, Color::black(), Color::white());
            assert_eq!(start_angle_of(two_stop), 0.0);

            let multi_stop = Paint::conic_gradient_stops_with_angle(
                10.0,
                20.0,
                bad_angle,
                [(0.0, Color::black()), (1.0, Color::white())],
            );
            assert_eq!(start_angle_of(multi_stop), 0.0);
        }

        // Finite angles must pass through untouched.
        let finite = Paint::conic_gradient_with_angle(10.0, 20.0, -1.25, Color::black(), Color::white());
        assert_eq!(start_angle_of(finite), -1.25);
    }
}

/// Accepts either the current `(x, y)` radius pair or the scalar a radial
/// gradient serialized before the radii became per-axis, expanding the scalar
/// to a circle, so older serialized paints keep deserializing.
#[cfg(feature = "serde")]
fn deserialize_radius_pair<'de, D>(deserializer: D) -> Result<(f32, f32), D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum ScalarOrPair {
        Scalar(f32),
        Pair(f32, f32),
    }
    Ok(match <ScalarOrPair as serde::Deserialize>::deserialize(deserializer)? {
        ScalarOrPair::Scalar(r) => (r, r),
        ScalarOrPair::Pair(x, y) => (x, y),
    })
}

#[cfg(all(test, feature = "serde"))]
mod serde_tests {
    use super::*;

    /// A gradient flavor serialized before `transform` existed omits that
    /// field. With `#[serde(default)]` it must fall back to the identity
    /// transform instead of failing to deserialize, so older serialized
    /// paints keep their exact prior rendering.
    #[test]
    fn gradient_flavor_deserializes_without_transform() {
        let json = r#"{
            "RadialGradient": {
                "center": { "x": 10.0, "y": 20.0 },
                "in_radius": [0.0, 0.0],
                "out_radius": [30.0, 30.0],
                "colors": {
                    "TwoStop": {
                        "start_color": { "r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0 },
                        "end_color": { "r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0 }
                    }
                }
            }
        }"#;
        let restored: PaintFlavor = serde_json::from_str(json).expect("pre-transform paint deserializes");
        let PaintFlavor::RadialGradient { transform, .. } = restored else {
            panic!("wrong flavor");
        };
        assert_eq!(
            transform,
            crate::Transform2D::identity(),
            "missing transform falls back to identity"
        );
    }

    /// A radial gradient serialized before the radii became per-axis pairs
    /// holds scalars; they must deserialize as circles rather than erroring.
    #[test]
    fn radial_gradient_deserializes_scalar_radii() {
        let json = r#"{
            "RadialGradient": {
                "center": { "x": 0.0, "y": 0.0 },
                "in_radius": 5.0,
                "out_radius": 30.0,
                "colors": {
                    "TwoStop": {
                        "start_color": { "r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0 },
                        "end_color": { "r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0 }
                    }
                }
            }
        }"#;
        let restored: PaintFlavor = serde_json::from_str(json).expect("scalar-radius paint deserializes");
        let PaintFlavor::RadialGradient {
            in_radius, out_radius, ..
        } = restored
        else {
            panic!("wrong flavor");
        };
        assert_eq!(in_radius, (5.0, 5.0));
        assert_eq!(out_radius, (30.0, 30.0));
    }

    /// A paint serialized before `text_decoration` existed omits that field. With
    /// `#[serde(default)]` the field must fall back to `TextDecoration::default()`
    /// (no decoration) instead of failing to deserialize. This guards back-compat
    /// with older serialized paints / configs.
    #[test]
    fn text_settings_deserializes_without_text_decoration() {
        // Representation of a pre-`text_decoration` TextSettings: every serialized
        // field except `text_decoration`. (`font_ids` / `font_variations` are
        // `serde(skip)`, so they never appear.)
        let json = r#"{
            "font_size": 16.0,
            "letter_spacing": 0.0,
            "text_baseline": "Alphabetic",
            "text_align": "Left"
        }"#;

        let settings: TextSettings = serde_json::from_str(json).expect("must deserialize without text_decoration");
        assert_eq!(
            settings.text_decoration,
            TextDecoration::default(),
            "missing text_decoration must default to TextDecoration::default()"
        );
        assert!(
            settings.text_decoration.is_none(),
            "the default decoration must enable no lines"
        );
    }

    /// Same guarantee through the full `Paint`: a paint whose `text` block omits
    /// `text_decoration` round-trips to the default (no) decoration.
    #[test]
    fn paint_deserializes_without_text_decoration() {
        let json = r#"{
            "flavor": { "Color": { "r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0 } },
            "shape_anti_alias": true,
            "stroke": {
                "stencil_strokes": true,
                "miter_limit": 10.0,
                "line_width": 1.0,
                "line_cap_start": "Butt",
                "line_cap_end": "Butt",
                "line_join": "Miter",
                "line_dash": [],
                "line_dash_offset": 0.0
            },
            "text": {
                "font_size": 16.0,
                "letter_spacing": 0.0,
                "text_baseline": "Alphabetic",
                "text_align": "Left"
            },
            "fill_rule": "NonZero"
        }"#;

        let paint: Paint = serde_json::from_str(json).expect("Paint must deserialize without text_decoration");
        assert_eq!(
            paint.text.text_decoration,
            TextDecoration::default(),
            "missing text_decoration must default to TextDecoration::default()"
        );
    }

    /// A present `text_decoration` still deserializes normally (the default does
    /// not mask explicit values).
    #[test]
    fn text_settings_deserializes_with_text_decoration() {
        let settings = TextSettings {
            text_decoration: TextDecoration {
                underline: true,
                strikethrough: false,
                overline: true,
            },
            ..TextSettings::default()
        };

        let json = serde_json::to_string(&settings).expect("serialize");
        let restored: TextSettings = serde_json::from_str(&json).expect("round-trip");
        assert_eq!(restored.text_decoration, settings.text_decoration);
    }
}
