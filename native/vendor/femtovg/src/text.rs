use std::{borrow::Borrow, cell::RefCell, ffi::OsStr, fs, hash::Hash, path::Path as FilePath, rc::Rc};

use fnv::FnvHashMap;
#[cfg(feature = "textlayout")]
use rustybuzz::ttf_parser;
use slotmap::{DefaultKey, SlotMap};

use crate::{
    geometry::Transform2D,
    paint::{FontVariations, PaintFlavor, StrokeSettings},
    Canvas, Color, ErrorKind, FillRule, ImageFlags, ImageId, ImageInfo, Paint, PixelFormat, PositionedGlyph,
    RenderTarget, Renderer,
};

mod atlas;
pub use atlas::Atlas;

mod font;
use font::{Font, GlyphRendering};
pub use font::{FontMetrics, VariationAxisInfo};

#[cfg(all(test, any(feature = "textlayout", feature = "swash")))]
mod glyph_atlas_tests;

#[cfg(feature = "textlayout")]
mod textlayout;
#[cfg(feature = "textlayout")]
pub use textlayout::*;

#[cfg(all(test, any(feature = "textlayout", feature = "swash")))]
pub(crate) mod test_fonts;

// This padding is an empty border around the glyph’s pixels but inside the
// sampled area (texture coordinates) for the quad in render_atlas().
pub(crate) const GLYPH_PADDING: u32 = 1;
// We add an additional margin of 1 texel outside of the sampled area.
// The atlas is NEAREST-sampled (since 62e5be6), so no filter tap blends
// across the boundary; the margin is a guard band for GPUs whose
// interpolated texture coordinates drift at quad edges and select a
// neighbouring texel instead (issues #22, #310). Atlas textures start
// zero-initialized on both backends, so margin texels are blank.
const GLYPH_MARGIN: u32 = 1;

pub(crate) const TEXTURE_SIZE: usize = 512;
// The longest glyph side that fits in a new atlas texture. Atlas::add_rect() refuses
// a cell as tall as the texture, hence the - 1.
pub(crate) const MAX_GLYPH_SIDE: u32 = TEXTURE_SIZE as u32 - 1 - 2 * (GLYPH_PADDING + GLYPH_MARGIN);
#[cfg(feature = "textlayout")]
const DEFAULT_LRU_CACHE_CAPACITY: usize = 1000;

/// A font handle.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FontId(DefaultKey);

/// Represents the vertical alignment of a text baseline.
///
/// The default value is `Alphabetic`.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Baseline {
    /// The text baseline is the top of the em square.
    Top,
    /// The text baseline is the middle of the em square.
    Middle,
    /// The text baseline is the normal alphabetic baseline.
    #[default]
    Alphabetic,
    /// The text baseline is the bottom of the bounding box.
    Bottom,
}

/// Represents the horizontal alignment of text.
///
/// The default value is `Left`.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Align {
    /// The text is left-aligned.
    #[default]
    Left,
    /// The text is centered.
    Center,
    /// The text is right-aligned.
    Right,
}

/// Represents the rendering mode for a path.
///
/// The default value is `Fill`.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Default)]
pub enum RenderMode {
    /// The path is filled.
    #[default]
    Fill,
    /// The path is stroked.
    Stroke,
}

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub struct RenderedGlyphId {
    glyph_index: u16,
    font_id: FontId,
    size: u32,
    line_width: u32,
    render_mode: RenderMode,
    subpixel_location: i8,
    variation_hash: u64,
}

impl RenderedGlyphId {
    fn new(
        glyph_index: u16,
        font_id: FontId,
        font_size: f32,
        line_width: f32,
        mode: RenderMode,
        subpixel_location: i8,
        normalized_coords: &[i16],
    ) -> Self {
        use std::hash::Hasher;
        let mut hasher = fnv::FnvHasher::default();
        normalized_coords.hash(&mut hasher);
        Self {
            glyph_index,
            font_id,
            size: (font_size * 10.0).trunc() as u32,
            line_width: (line_width * 10.0).trunc() as u32,
            render_mode: mode,
            subpixel_location,
            variation_hash: hasher.finish(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct RenderedGlyph {
    texture_index: usize,
    width: u32,
    height: u32,
    bearing_x: f32,
    bearing_y: f32,
    atlas_x: u32,
    atlas_y: u32,
    color_glyph: bool,
    // The width and height of one atlas texel when the glyph is drawn, in pixels at the font
    // size. Both are 1, except for a bitmap glyph that is drawn from a smaller copy.
    texel_size: [f32; 2],
}

#[derive(Debug)]
pub struct FontTexture {
    pub atlas: Atlas,
    pub(crate) image_id: ImageId,
}

/// `TextContext` provides functionality for text processing in femtovg.
///
/// You can add fonts using the [`Self::add_font_file()`], [`Self::add_font_mem()`] and
/// [`Self::add_font_dir()`] functions. For each registered font a [`FontId`] is
/// returned.
///
/// The [`FontId`] can be supplied to [`crate::Paint`] along with additional parameters
/// such as the font size.
///
/// The paint is needed when using `TextContext`'s measurement functions such as
/// [`Self::measure_text()`].
///
/// Note that the measurements are done entirely with the supplied sizes in the paint
/// parameter. If you need measurements that take a [`crate::Canvas`]'s transform or dpi into
/// account (see [`crate::Canvas::set_size()`]), you need to use the measurement functions
/// on the canvas.
#[derive(Clone, Default, Debug)]
pub struct TextContext(pub(crate) Rc<RefCell<TextContextImpl>>);

impl TextContext {
    /// Registers all .ttf files from a directory with this text context. If successful, the
    /// font ids of all registered fonts are returned.
    pub fn add_font_dir<T: AsRef<FilePath>>(&self, path: T) -> Result<Vec<FontId>, ErrorKind> {
        self.0.borrow_mut().add_font_dir(path)
    }

    /// Registers the .ttf file from the specified path with this text context. If successful,
    /// the font id is returned.
    pub fn add_font_file<T: AsRef<FilePath>>(&self, path: T) -> Result<FontId, ErrorKind> {
        self.0.borrow_mut().add_font_file(path)
    }

    /// Registers the in-memory representation of a TrueType font pointed to by the data
    /// parameter with this text context. If successful, the font id is returned.
    pub fn add_font_mem(&self, data: &[u8]) -> Result<FontId, ErrorKind> {
        self.0.borrow_mut().add_font_mem(data)
    }

    /// Registers the in-memory representation of a TrueType font pointed to by the shared data
    /// parameter with this text context. If successful, the font id is returned. The `face_index`
    /// specifies the face index if the font data is a true type font collection. For plain true
    /// type fonts, use 0 as index.
    pub fn add_shared_font_with_index<T: AsRef<[u8]> + 'static>(
        &self,
        data: T,
        face_index: u32,
    ) -> Result<FontId, ErrorKind> {
        self.0.borrow_mut().add_shared_font_with_index(data, face_index)
    }

    /// Returns font metrics for a particular Paint.
    pub fn measure_font(&self, paint: &Paint) -> Result<FontMetrics, ErrorKind> {
        self.0
            .borrow_mut()
            .measure_font(paint.text.font_size, &paint.text.font_ids, &paint.text.font_variations)
    }

    /// Returns the variation axes available for the specified font.
    ///
    /// For variable fonts, this returns information about each axis (e.g. weight, width).
    /// For static fonts, this returns an empty vector.
    ///
    /// Axes are returned in the order they appear in the font's OpenType
    /// `fvar` table. This is the same order that
    /// [`Canvas::fill_glyph_run`](crate::Canvas::fill_glyph_run) and
    /// [`Canvas::stroke_glyph_run`](crate::Canvas::stroke_glyph_run) expect
    /// for their normalized coordinate (`i16` F2DOT14) slices: the i-th
    /// coordinate corresponds to the i-th axis.
    pub fn font_variation_axes(&self, font_id: FontId) -> Result<Vec<VariationAxisInfo>, ErrorKind> {
        let ctx = RefCell::borrow(&self.0);
        let font = ctx.font(font_id).ok_or(ErrorKind::NoFontFound)?;
        Ok(font.variation_axes())
    }
}

pub struct TextContextImpl {
    fonts: SlotMap<DefaultKey, Font>,
    #[cfg(feature = "textlayout")]
    shaping_run_cache: textlayout::ShapingRunCache<fnv::FnvBuildHasher>,
    #[cfg(feature = "textlayout")]
    shaped_words_cache: textlayout::ShapedWordsCache<fnv::FnvBuildHasher>,
    #[cfg(feature = "swash")]
    swash_scale_context: Rc<RefCell<swash::scale::ScaleContext>>,
}

impl std::fmt::Debug for TextContextImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextContextImpl").field("fonts", &self.fonts).finish()
    }
}

impl Default for TextContextImpl {
    fn default() -> Self {
        #[cfg(feature = "textlayout")]
        let fnv_run = fnv::FnvBuildHasher::default();
        #[cfg(feature = "textlayout")]
        let fnv_words = fnv::FnvBuildHasher::default();

        Self {
            fonts: SlotMap::default(),
            #[cfg(feature = "textlayout")]
            shaping_run_cache: lru::LruCache::with_hasher(
                std::num::NonZeroUsize::new(DEFAULT_LRU_CACHE_CAPACITY).unwrap(),
                fnv_run,
            ),
            #[cfg(feature = "textlayout")]
            shaped_words_cache: lru::LruCache::with_hasher(
                std::num::NonZeroUsize::new(DEFAULT_LRU_CACHE_CAPACITY).unwrap(),
                fnv_words,
            ),
            #[cfg(feature = "swash")]
            swash_scale_context: Rc::new(RefCell::new(swash::scale::ScaleContext::new())),
        }
    }
}

impl TextContextImpl {
    #[cfg(feature = "swash")]
    pub(crate) fn swash_scale_context(&self) -> Rc<RefCell<swash::scale::ScaleContext>> {
        self.swash_scale_context.clone()
    }

    pub fn add_font_dir<T: AsRef<FilePath>>(&mut self, path: T) -> Result<Vec<FontId>, ErrorKind> {
        let path = path.as_ref();
        let mut fonts = Vec::new();

        if path.is_dir() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    fonts.extend(self.add_font_dir(&path)?);
                } else if Some("ttf") == path.extension().and_then(OsStr::to_str) {
                    fonts.push(self.add_font_file(path)?);
                } else if Some("ttc") == path.extension().and_then(OsStr::to_str) {
                    fonts.extend(self.add_font_file_collection(path)?);
                }
            }
        }

        Ok(fonts)
    }

    pub fn add_font_file<T: AsRef<FilePath>>(&mut self, path: T) -> Result<FontId, ErrorKind> {
        let data = std::fs::read(path)?;

        self.add_font_mem(&data)
    }

    pub fn add_font_file_collection<T: AsRef<FilePath>>(
        &mut self,
        path: T,
    ) -> Result<impl Iterator<Item = FontId> + '_, ErrorKind> {
        let data = std::fs::read(path)?;

        #[cfg(feature = "textlayout")]
        let count = ttf_parser::fonts_in_collection(&data).unwrap_or(1);
        #[cfg(all(feature = "swash", not(feature = "textlayout")))]
        let count = swash::FontDataRef::new(&data).map_or(1, |d| d.len() as u32);
        #[cfg(not(any(feature = "textlayout", feature = "swash")))]
        let count = 1u32;
        Ok((0..count).filter_map(move |index| self.add_font_mem_with_index(&data, index).ok()))
    }

    pub fn add_font_mem(&mut self, data: &[u8]) -> Result<FontId, ErrorKind> {
        self.add_font_mem_with_index(data, 0)
    }

    pub fn add_font_mem_with_index(&mut self, data: &[u8], face_index: u32) -> Result<FontId, ErrorKind> {
        let data_copy = data.to_owned();
        let font = Font::new_with_data(data_copy, face_index, self)?;
        self.clear_caches();
        Ok(FontId(self.fonts.insert(font)))
    }

    pub fn add_shared_font_with_index<T: AsRef<[u8]> + 'static>(
        &mut self,
        data: T,
        face_index: u32,
    ) -> Result<FontId, ErrorKind> {
        let font = Font::new_with_data(data, face_index, self)?;
        self.clear_caches();
        Ok(FontId(self.fonts.insert(font)))
    }

    pub fn font(&self, id: FontId) -> Option<&Font> {
        self.fonts.get(id.0)
    }

    #[cfg(feature = "textlayout")]
    pub fn find_font<F, T>(&mut self, font_ids: &[Option<FontId>; 8], mut callback: F) -> Result<T, ErrorKind>
    where
        F: FnMut((FontId, &mut Font)) -> (bool, T),
    {
        // Try each font in the paint
        for maybe_font_id in font_ids {
            if let &Some(font_id) = maybe_font_id {
                if let Some(font) = self.fonts.get_mut(font_id.0) {
                    let (has_missing, result) = callback((font_id, font));

                    if !has_missing {
                        return Ok(result);
                    }
                }
            } else {
                break;
            }
        }

        // Try each registered font
        // An optimisation here would be to skip fonts that were tried by the paint
        for (id, font) in &mut self.fonts {
            let (has_missing, result) = callback((FontId(id), font));

            if !has_missing {
                return Ok(result);
            }
        }

        // Just return the first font at this point and let it render .nodef glyphs
        if let Some((id, font)) = self.fonts.iter_mut().next() {
            return Ok(callback((FontId(id), font)).1);
        }

        Err(ErrorKind::NoFontFound)
    }

    fn clear_caches(&mut self) {
        #[cfg(feature = "textlayout")]
        self.shaping_run_cache.clear();
        #[cfg(feature = "textlayout")]
        self.shaped_words_cache.clear();
    }

    pub fn measure_font(
        &self,
        font_size: f32,
        font_ids: &[Option<FontId>; 8],
        variations: &FontVariations,
    ) -> Result<FontMetrics, ErrorKind> {
        if let Some(Some(id)) = font_ids.first() {
            if let Some(font) = self.font(*id) {
                return Ok(font.metrics_with_variations(font_size, variations));
            }
        }

        Err(ErrorKind::NoFontFound)
    }
}

// Renderer

/// Represents a command to draw an image with a set of quads.
#[derive(Clone, Debug)]
pub struct DrawCommand {
    /// The ID of the image to draw.
    pub image_id: ImageId,
    /// The quads defining the positions and texture coordinates for drawing the image.
    pub quads: Vec<Quad>,
}

/// Represents a quad with position and texture coordinates.
#[derive(Copy, Clone, Default, Debug)]
pub struct Quad {
    /// X-coordinate of the top-left corner of the quad.
    pub x0: f32,
    /// Y-coordinate of the top-left corner of the quad.
    pub y0: f32,
    /// U-coordinate (horizontal texture coordinate) of the top-left corner of the quad.
    pub s0: f32,
    /// V-coordinate (vertical texture coordinate) of the top-left corner of the quad.
    pub t0: f32,
    /// X-coordinate of the bottom-right corner of the quad.
    pub x1: f32,
    /// Y-coordinate of the bottom-right corner of the quad.
    pub y1: f32,
    /// U-coordinate (horizontal texture coordinate) of the bottom-right corner of the quad.
    pub s1: f32,
    /// V-coordinate (vertical texture coordinate) of the bottom-right corner of the quad.
    pub t1: f32,
}

/// Represents the drawing commands for glyphs, separated into alpha and color glyphs.
#[derive(Default, Debug)]
pub struct GlyphDrawCommands {
    /// Drawing commands for alpha (opacity) glyphs.
    pub alpha_glyphs: Vec<DrawCommand>,
    /// Drawing commands for color glyphs.
    pub color_glyphs: Vec<DrawCommand>,
}

// A path glyph that has an atlas cell reserved but has not been drawn into it yet.
struct PendingMask<'a> {
    path: std::cell::Ref<'a, crate::Path>,
    image_id: ImageId,
    // Arguments for clear_rect(): x, y, width, height.
    cell: (u32, u32, u32, u32),
    // Translation to the glyph's origin in the atlas texture.
    origin: Transform2D,
    scale: f32,
    // Stroke width in font units.
    line_width: f32,
}

impl PendingMask<'_> {
    // Clears the glyph's atlas cell and draws the path into it with 8x supersampling.
    // Must be called inside an offscreen_pass() with the atlas texture as the render target,
    // because it changes the current state.
    fn draw<T: Renderer>(&self, canvas: &mut Canvas<T>, mode: RenderMode) {
        // clear_rect() records the current composite operation, which the previous mask left
        // additive. Reset the state so the clear is queued with the default one.
        canvas.enter_offscreen_state(self.origin);
        let (x, y, width, height) = self.cell;
        canvas.clear_rect(x, y, width, height, Color::black());
        canvas.global_composite_blend_func(crate::BlendFactor::SrcAlpha, crate::BlendFactor::One);

        let factor = 1.0 / 8.0;

        let mask_color = Color::rgbf(factor, factor, factor);

        // 4x
        // let points = [
        //     (-3.0/8.0, 1.0/8.0),
        //     (1.0/8.0, 3.0/8.0),
        //     (3.0/8.0, -1.0/8.0),
        //     (-1.0/8.0, -3.0/8.0),
        // ];

        // 8x
        let points = [
            (-7.0 / 16.0, -1.0 / 16.0),
            (-1.0 / 16.0, -5.0 / 16.0),
            (3.0 / 16.0, -7.0 / 16.0),
            (5.0 / 16.0, -3.0 / 16.0),
            (7.0 / 16.0, 1.0 / 16.0),
            (1.0 / 16.0, 5.0 / 16.0),
            (-3.0 / 16.0, 7.0 / 16.0),
            (-5.0 / 16.0, 3.0 / 16.0),
        ];

        for point in &points {
            canvas.state_mut().transform = self.origin;
            canvas.translate(point.0, point.1);
            canvas.scale(self.scale, self.scale);

            if mode == RenderMode::Stroke {
                canvas.stroke_path_internal(
                    &self.path,
                    None,
                    &PaintFlavor::Color(mask_color),
                    false,
                    &StrokeSettings {
                        line_width: self.line_width,
                        ..Default::default()
                    },
                );
            } else {
                canvas.fill_path_internal(
                    &self.path,
                    None,
                    &PaintFlavor::Color(mask_color),
                    false,
                    FillRule::NonZero,
                );
            }
        }
    }
}

pub struct GlyphAtlas {
    pub rendered_glyphs: RefCell<FnvHashMap<RenderedGlyphId, Option<RenderedGlyph>>>,
    pub glyph_textures: RefCell<Vec<FontTexture>>,
    #[cfg(feature = "swash")]
    swash_scale_context: Rc<RefCell<swash::scale::ScaleContext>>,
}

impl GlyphAtlas {
    pub(crate) fn new(_text_context: &Rc<RefCell<TextContextImpl>>) -> Self {
        Self {
            rendered_glyphs: RefCell::default(),
            glyph_textures: RefCell::default(),
            #[cfg(feature = "swash")]
            swash_scale_context: (**_text_context).borrow().swash_scale_context(),
        }
    }
}

impl std::fmt::Debug for GlyphAtlas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlyphAtlas")
            .field("rendered_glyphs", &self.rendered_glyphs)
            .field("glyph_textures", &self.glyph_textures)
            .finish()
    }
}

impl GlyphAtlas {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_atlas<T: Renderer>(
        &self,
        canvas: &mut Canvas<T>,
        font_id: FontId,
        font: &Font,
        font_face: &font::FontFaceRef<'_>,
        mut glyphs: impl Iterator<Item = PositionedGlyph>,
        font_size: f32,
        line_width: f32,
        mode: RenderMode,
        normalized_coords: &[i16],
    ) -> Result<GlyphDrawCommands, ErrorKind> {
        let mut alpha_cmd_map = FnvHashMap::default();
        let mut color_cmd_map = FnvHashMap::default();

        let line_width_offset = if mode == RenderMode::Stroke {
            (line_width / 2.0).ceil()
        } else {
            0.0
        };

        let mut add_glyph = |canvas: &mut Canvas<T>, glyph: PositionedGlyph| -> Result<(), ErrorKind> {
            // The fraction of x in tenths of a pixel, -10 to 10, with the sign of x. It is the
            // offset of the glyph from `glyph.x.trunc()`, where the quad is placed below.
            // `quantize` rounds to the nearest step only for a non-negative value, so it is given
            // the absolute value.
            let fract = glyph.x.fract();
            let subpixel_location = (crate::geometry::quantize(fract.abs(), 0.1) * 10.0).copysign(fract) as i8;

            let id = RenderedGlyphId::new(
                glyph.glyph_id,
                font_id,
                font_size,
                line_width,
                mode,
                subpixel_location,
                normalized_coords,
            );

            let mut rendered_glyphs = self.rendered_glyphs.borrow_mut();
            let glyph_cache_entry = rendered_glyphs.entry(id);
            let glyph_cache_entry = match glyph_cache_entry {
                std::collections::hash_map::Entry::Occupied(occupied_entry) => occupied_entry,
                std::collections::hash_map::Entry::Vacant(_) => {
                    let placed = self.place_glyph(
                        canvas,
                        font_size,
                        line_width,
                        mode,
                        font,
                        font_face,
                        glyph.glyph_id,
                        f32::from(subpixel_location) / 10.0,
                        normalized_coords,
                    )?;
                    if let Some((_, Some(mask))) = &placed {
                        // The masks of one run can be in different atlas textures.
                        canvas.set_render_target(RenderTarget::Image(mask.image_id));
                        mask.draw(canvas, mode);
                    }
                    glyph_cache_entry.insert_entry(placed.map(|(rendered, _)| rendered))
                }
            };

            let Some(rendered) = glyph_cache_entry.get() else {
                return Ok(());
            };

            if let Some(texture) = self.glyph_textures.borrow().get(rendered.texture_index) {
                let image_id = texture.image_id;
                let size = texture.atlas.size();
                let itw = 1.0 / size.0 as f32;
                let ith = 1.0 / size.1 as f32;

                let cmd_map = if rendered.color_glyph {
                    &mut color_cmd_map
                } else {
                    &mut alpha_cmd_map
                };

                let cmd = cmd_map.entry(rendered.texture_index).or_insert_with(|| DrawCommand {
                    image_id,
                    quads: Vec::new(),
                });

                let mut q = Quad::default();

                let line_width_offset = if rendered.color_glyph { 0. } else { line_width_offset };

                let [texel_width, texel_height] = rendered.texel_size;
                q.x0 = glyph.x.trunc() + rendered.bearing_x - (line_width_offset + GLYPH_PADDING as f32) * texel_width;
                q.y0 = glyph.y.round() - rendered.bearing_y - (line_width_offset + GLYPH_PADDING as f32) * texel_height;
                q.x1 = q.x0 + rendered.width as f32 * texel_width;
                q.y1 = q.y0 + rendered.height as f32 * texel_height;

                q.s0 = rendered.atlas_x as f32 * itw;
                q.t0 = rendered.atlas_y as f32 * ith;
                q.s1 = (rendered.atlas_x + rendered.width) as f32 * itw;
                q.t1 = (rendered.atlas_y + rendered.height) as f32 * ith;

                cmd.quads.push(q);
            }

            Ok(())
        };

        // Glyph masks are drawn inside one offscreen_pass(). The pass restores the caller's state
        // and render target when it ends, also when a glyph fails. It is opened on the caller's
        // own render target, so no target switch is queued unless a mask is drawn.
        let target = canvas.current_render_target;
        canvas.offscreen_pass(target, Transform2D::identity(), |canvas| {
            glyphs.try_for_each(|glyph| add_glyph(canvas, glyph))
        })?;

        Ok(GlyphDrawCommands {
            alpha_glyphs: alpha_cmd_map.drain().map(|(_, cmd)| cmd).collect(),
            color_glyphs: color_cmd_map.drain().map(|(_, cmd)| cmd).collect(),
        })
    }

    // Reserves a cell in the atlas for the glyph and returns its RenderedGlyph.
    // Image glyphs are uploaded here. A path glyph is not drawn here: its PendingMask is
    // returned with the RenderedGlyph and the caller draws it. This function does not change
    // the canvas state or render target.
    // Returns Ok(None) if there exists no path or image for the glyph in the font (missing glyph).
    #[allow(clippy::too_many_arguments)]
    fn place_glyph<'a, T: Renderer>(
        &self,
        canvas: &mut Canvas<T>,
        font_size: f32,
        line_width: f32,
        mode: RenderMode,
        font: &'a Font,
        font_face: &font::FontFaceRef<'_>,
        glyph_id: u16,
        _subpixel_x: f32,
        normalized_coords: &[i16],
    ) -> Result<Option<(RenderedGlyph, Option<PendingMask<'a>>)>, ErrorKind> {
        let (bitmap_glyph, maybe_glyph_metrics) = font
            .glyph(font_face, glyph_id, normalized_coords)
            .map_or((false, None), |g| (g.path.is_none(), Some(g.metrics.clone())));

        // If a bitmap glyph doesn't fit the atlas at font_size, rasterize a smaller copy of it,
        // which render_atlas() scales up by its texel_size. Aim one texel below MAX_GLYPH_SIDE
        // because f32 rounding can land just above the aim and the cell size below rounds up.
        let longest_bitmap_side = maybe_glyph_metrics
            .as_ref()
            .filter(|_| bitmap_glyph)
            .map_or(0.0, |metrics| metrics.width.max(metrics.height) * font.scale(font_size));
        let raster_size = if longest_bitmap_side > MAX_GLYPH_SIDE as f32 {
            font_size * (MAX_GLYPH_SIDE - 1) as f32 / longest_bitmap_side
        } else {
            font_size
        };

        #[cfg(feature = "swash")]
        if mode == RenderMode::Fill || bitmap_glyph {
            if let Some(result) = self.render_glyph_swash(
                canvas,
                font,
                font_size,
                raster_size,
                mode,
                glyph_id,
                _subpixel_x,
                normalized_coords,
            )? {
                return Ok(Some((result, None)));
            }
        }

        let padding = GLYPH_PADDING + GLYPH_MARGIN;

        let (glyph_representation, glyph_metrics, scale) = {
            let scale = font.scale(raster_size);
            // A smaller copy is made from the largest strike, which the cell below is sized from.
            let pixels_per_em = if raster_size == font_size {
                raster_size as u16
            } else {
                u16::MAX
            };

            if let (Some(glyph_representation), Some(glyph_metrics)) = (
                font.glyph_rendering_representation(font_face, glyph_id, pixels_per_em, normalized_coords),
                maybe_glyph_metrics,
            ) {
                (glyph_representation, glyph_metrics, scale)
            } else {
                return Ok(None);
            }
        };

        #[cfg(feature = "image-loading")]
        let color_glyph = matches!(glyph_representation, GlyphRendering::RenderAsImage(..));
        #[cfg(not(feature = "image-loading"))]
        let color_glyph = false;

        let line_width = if color_glyph || mode != RenderMode::Stroke {
            0.0
        } else {
            line_width
        };

        let line_width_offset = (line_width / 2.0).ceil();

        let glyph_width = (glyph_metrics.width * scale).ceil() as u32;
        let glyph_height = (glyph_metrics.height * scale).ceil() as u32;
        let width = glyph_width + (line_width_offset * 2.0) as u32 + padding * 2;
        let height = glyph_height + (line_width_offset * 2.0) as u32 + padding * 2;

        let (dst_index, dst_image_id, (dst_x, dst_y)) =
            self.find_texture_or_alloc(canvas, width as usize, height as usize)?;

        let rendered_bearing_x = (glyph_metrics.bearing_x * scale).round();
        let rendered_bearing_y = (glyph_metrics.bearing_y * scale).round();
        let x = dst_x as f32 - rendered_bearing_x + line_width_offset + padding as f32;
        let y = TEXTURE_SIZE as f32 - dst_y as f32 - rendered_bearing_y - line_width_offset - padding as f32;

        // A smaller copy is drawn where the bitmap is at font_size, and its glyph_width by
        // glyph_height texels are scaled to the bitmap's size there.
        let (bearing_x, bearing_y, texel_size) = if raster_size == font_size {
            (rendered_bearing_x, rendered_bearing_y, [1.0, 1.0])
        } else {
            let font_scale = font.scale(font_size);
            (
                glyph_metrics.bearing_x * font_scale,
                glyph_metrics.bearing_y * font_scale,
                [
                    glyph_metrics.width * font_scale / glyph_width as f32,
                    glyph_metrics.height * font_scale / glyph_height as f32,
                ],
            )
        };

        let rendered_glyph = RenderedGlyph {
            width: width - 2 * GLYPH_MARGIN,
            height: height - 2 * GLYPH_MARGIN,
            bearing_x,
            bearing_y,
            atlas_x: dst_x as u32 + GLYPH_MARGIN,
            atlas_y: dst_y as u32 + GLYPH_MARGIN,
            texture_index: dst_index,
            color_glyph,
            texel_size,
        };

        let pending_mask = match glyph_representation {
            GlyphRendering::RenderAsPath(path) => Some(PendingMask {
                path,
                image_id: dst_image_id,
                cell: (dst_x as u32, TEXTURE_SIZE as u32 - dst_y as u32 - height, width, height),
                origin: Transform2D::translation(x, y),
                scale,
                line_width: line_width / scale,
            }),
            #[cfg(feature = "image-loading")]
            GlyphRendering::RenderAsImage(image_buffer) => {
                // The image fills the cell inside its padding, which stays blank.
                let target_x = (rendered_glyph.atlas_x + GLYPH_PADDING) as usize;
                let target_y = (rendered_glyph.atlas_y + GLYPH_PADDING) as usize;
                let target_width = rendered_glyph.width - 2 * GLYPH_PADDING;
                let target_height = rendered_glyph.height - 2 * GLYPH_PADDING;

                // A smaller copy is magnified when it is drawn, so it is resized with a filter,
                // as swash resizes it with a Mitchell filter.
                let filter = if raster_size == font_size {
                    image::imageops::FilterType::Nearest
                } else {
                    image::imageops::FilterType::Triangle
                };
                let image_buffer = image_buffer.resize_exact(target_width, target_height, filter);
                if let Ok(image) = crate::image::ImageSource::try_from(&image_buffer) {
                    canvas.update_image(dst_image_id, image, target_x, target_y)?;
                }
                None
            }
        };

        Ok(Some((rendered_glyph, pending_mask)))
    }

    #[cfg(feature = "swash")]
    fn render_glyph_swash<T: Renderer>(
        &self,
        canvas: &mut Canvas<T>,
        font: &Font,
        font_size: f32,
        raster_size: f32,
        mode: RenderMode,
        glyph_id: u16,
        subpixel_x: f32,
        normalized_coords: &[i16],
    ) -> Result<Option<RenderedGlyph>, ErrorKind> {
        use swash::scale::{Render, Source, StrikeWith};
        use swash::zeno::Format;

        let font_ref = match font.swash_font_ref() {
            Some(f) => f,
            None => return Ok(None),
        };

        // A smaller copy is made from the largest strike, which raster_size is estimated from. The
        // strike nearest raster_size can cover more of the em, and then its copy would not fit.
        let strike = if raster_size == font_size {
            StrikeWith::BestFit
        } else {
            StrikeWith::LargestSize
        };

        let image = {
            let mut scale_context = self.swash_scale_context.borrow_mut();
            let scaler_builder = scale_context
                .builder(font_ref)
                .size(raster_size)
                .hint(true)
                .normalized_coords(normalized_coords);
            let mut scaler = scaler_builder.build();

            Render::new(&[Source::ColorOutline(0), Source::ColorBitmap(strike), Source::Outline])
                .format(Format::Alpha)
                .offset(swash::zeno::Vector::new(subpixel_x, 0.0))
                .render(&mut scaler, glyph_id)
        };

        let image = match image {
            // Guard against glyphs with no visual (spaces)
            Some(img) if img.placement.width > 0 && img.placement.height > 0 => img,
            _ => return Ok(None),
        };

        // swash cannot stroke, so a stroke takes only a color image from it. The atlas would draw
        // a mask as a shifted fill (e.g., the outline of a glyph whose bitmap does not decode).
        if mode == RenderMode::Stroke && image.content != swash::scale::image::Content::Color {
            return Ok(None);
        }

        let is_color = image.content == swash::scale::image::Content::Color;

        // Only a color bitmap is drawn from a smaller copy. Anything else that swash draws for a
        // bitmap glyph (e.g., the outline of a glyph whose bitmap does not decode) is rasterized
        // at font_size.
        if raster_size != font_size && !is_color {
            return self.render_glyph_swash(
                canvas,
                font,
                font_size,
                font_size,
                mode,
                glyph_id,
                subpixel_x,
                normalized_coords,
            );
        }

        let (bearing_x, bearing_y, texel_size) = if raster_size == font_size {
            (image.placement.left as f32, image.placement.top as f32, [1.0, 1.0])
        } else {
            // swash rounds the copy's placement and size down to whole texels, and drawing the copy
            // scaled up would scale that error up too. So the copy is placed where the largest
            // strike puts the bitmap at font_size. Without a size, swash gives that strike's
            // bitmap as it is, at whole pixels of the strike.
            let Some(ppem) = font_ref
                .color_strikes()
                .find_by_largest_ppem(glyph_id)
                .map(|strike| strike.ppem())
            else {
                return Ok(None);
            };
            let strike_bitmap = self
                .swash_scale_context
                .borrow_mut()
                .builder(font_ref)
                .build()
                .scale_color_bitmap(glyph_id, StrikeWith::LargestSize);
            let Some(strike_bitmap) = strike_bitmap else {
                return Ok(None);
            };
            let scale = font_size / f32::from(ppem);
            let placement = strike_bitmap.placement;
            (
                placement.left as f32 * scale,
                placement.top as f32 * scale,
                [
                    placement.width as f32 * scale / image.placement.width as f32,
                    placement.height as f32 * scale / image.placement.height as f32,
                ],
            )
        };

        let padding = GLYPH_PADDING + GLYPH_MARGIN;
        let glyph_width = image.placement.width;
        let glyph_height = image.placement.height;
        let alloc_width = glyph_width + 2 * padding;
        let alloc_height = glyph_height + 2 * padding;

        // Return None for a glyph too large for the atlas. Without image-loading, place_glyph
        // then skips it instead of failing.
        let (dst_index, dst_image_id, (dst_x, dst_y)) =
            match self.find_texture_or_alloc(canvas, alloc_width as usize, alloc_height as usize) {
                Err(ErrorKind::FontSizeTooLargeForAtlas) => return Ok(None),
                result => result?,
            };

        // Create a padded image with zeroed borders so the sampled rect's edge
        // texels are blank (the texture itself starts zero-initialized).
        let padded_width = (glyph_width + 2 * GLYPH_PADDING) as usize;
        let padded_height = (glyph_height + 2 * GLYPH_PADDING) as usize;
        let mut pixels = vec![rgb::RGBA8::new(0, 0, 0, 0); padded_width * padded_height];

        if is_color {
            for y in 0..glyph_height as usize {
                for x in 0..glyph_width as usize {
                    let src_idx = (y * glyph_width as usize + x) * 4;
                    let dst_idx = (y + GLYPH_PADDING as usize) * padded_width + x + GLYPH_PADDING as usize;
                    pixels[dst_idx] = rgb::RGBA8::new(
                        image.data[src_idx],
                        image.data[src_idx + 1],
                        image.data[src_idx + 2],
                        image.data[src_idx + 3],
                    );
                }
            }
        } else {
            for y in 0..glyph_height as usize {
                for x in 0..glyph_width as usize {
                    let src_idx = y * glyph_width as usize + x;
                    let dst_idx = (y + GLYPH_PADDING as usize) * padded_width + x + GLYPH_PADDING as usize;
                    pixels[dst_idx] = rgb::RGBA8::new(image.data[src_idx], 0, 0, 0);
                }
            }
        }

        let target_x = dst_x + GLYPH_MARGIN as usize;
        let target_y = dst_y + GLYPH_MARGIN as usize;

        let img = imgref::Img::new(&pixels[..], padded_width, padded_height);
        canvas.update_image(dst_image_id, crate::image::ImageSource::from(img), target_x, target_y)?;

        Ok(Some(RenderedGlyph {
            texture_index: dst_index,
            width: alloc_width - 2 * GLYPH_MARGIN,
            height: alloc_height - 2 * GLYPH_MARGIN,
            bearing_x,
            bearing_y,
            atlas_x: dst_x as u32 + GLYPH_MARGIN,
            atlas_y: dst_y as u32 + GLYPH_MARGIN,
            color_glyph: is_color,
            texel_size,
        }))
    }

    // Returns (texture index, image id, glyph padding box)
    fn find_texture_or_alloc<T: Renderer>(
        &self,
        canvas: &mut Canvas<T>,
        width: usize,
        height: usize,
    ) -> Result<(usize, ImageId, (usize, usize)), ErrorKind> {
        // Find a free location in one of the atlases
        let mut texture_search_result = {
            let mut glyph_textures = self.glyph_textures.borrow_mut();
            let mut textures = glyph_textures.iter_mut().enumerate();
            textures.find_map(|(index, texture)| {
                texture
                    .atlas
                    .add_rect(width, height)
                    .map(|loc| (index, texture.image_id, loc))
            })
        };

        if texture_search_result.is_none() {
            // All atlases are exausted and a new one must be created
            let mut atlas = Atlas::new(TEXTURE_SIZE, TEXTURE_SIZE);

            let loc = atlas
                .add_rect(width, height)
                .ok_or(ErrorKind::FontSizeTooLargeForAtlas)?;

            // Using PixelFormat::Gray8 works perfectly and takes less VRAM.
            // We keep Rgba8 for now because it might be useful for sub-pixel
            // anti-aliasing (ClearType®), and the atlas debug display is much
            // clearer with different colors. Also, Rgba8 is required for color
            // fonts (typically used for emojis).
            let info = ImageInfo::new(ImageFlags::NEAREST, atlas.size().0, atlas.size().1, PixelFormat::Rgba8);
            let image_id = canvas.images.alloc(&mut canvas.renderer, info)?;

            #[cfg(feature = "debug_inspector")]
            if cfg!(debug_assertions) {
                // Fill the texture with red pixels only in debug builds.
                // This has to be an upload, not clear_rect(): clear_rect() runs at flush,
                // after glyphs have already been uploaded, and would overwrite them.
                let (width, height) = atlas.size();
                let fill = vec![rgb::RGBA8::new(255, 0, 0, 0); width * height];
                let fill = imgref::Img::new(&fill[..], width, height);
                if let Err(error) = canvas.update_image(image_id, crate::image::ImageSource::from(fill), 0, 0) {
                    // The texture was never added to glyph_textures, so delete its image here.
                    canvas.delete_image(image_id);
                    return Err(error);
                }
            }

            self.glyph_textures.borrow_mut().push(FontTexture { atlas, image_id });

            let index = self.glyph_textures.borrow().len() - 1;
            texture_search_result = Some((index, image_id, loc));
        }

        texture_search_result.ok_or(ErrorKind::UnknownError)
    }

    pub(crate) fn clear<T: Renderer>(&self, canvas: &mut Canvas<T>) {
        let image_ids = std::mem::take(&mut *self.glyph_textures.borrow_mut())
            .into_iter()
            .map(|font_texture| font_texture.image_id);
        image_ids.for_each(|id| canvas.delete_image(id));

        self.rendered_glyphs.borrow_mut().clear();
    }
}

/// Converts design-space font variation settings (tag/value pairs from Paint)
/// into normalized coordinates for the first font in the font list.
/// Used by the high-level `fill_text`/`stroke_text` path, which stores
/// variations on Paint but needs normalized coords for `draw_glyph_run`.
#[cfg(feature = "textlayout")]
pub(crate) fn normalize_variations(
    text_context: &TextContextImpl,
    font_ids: &[Option<FontId>; 8],
    variations: &FontVariations,
) -> Vec<i16> {
    if variations.is_empty() {
        return Vec::new();
    }
    if let Some(Some(id)) = font_ids.first() {
        if let Some(font) = text_context.font(*id) {
            return font.normalize_variations(variations);
        }
    }
    Vec::new()
}

#[allow(clippy::too_many_arguments)]
pub fn render_direct<T: Renderer>(
    canvas: &mut Canvas<T>,
    font: &Font,
    glyphs: impl Iterator<Item = PositionedGlyph>,
    paint_flavor: &PaintFlavor,
    anti_alias: bool,
    stroke: &StrokeSettings,
    font_size: f32,
    mode: RenderMode,
    normalized_coords: &[i16],
) -> Result<(), ErrorKind> {
    let face = font.face_ref_with_normalized_coords(normalized_coords);

    for glyph in glyphs {
        let (glyph_rendering, scale) = {
            let scale = font.scale(font_size);

            let Some(glyph_rendering) =
                font.glyph_rendering_representation(&face, glyph.glyph_id, font_size as u16, normalized_coords)
            else {
                continue;
            };

            (glyph_rendering, scale)
        };

        let line_width = match mode {
            RenderMode::Fill => stroke.line_width,
            RenderMode::Stroke => stroke.line_width / scale,
        };

        // Maps the outline from font units (y up) to the glyph's position in the
        // run. It is passed with the path and not set on the canvas because the
        // canvas transform also maps the paint's coordinates, and a gradient or
        // image paint has to be mapped the same way for every glyph of the run.
        let outline_transform = Transform2D::new(scale, 0.0, 0.0, -scale, glyph.x, glyph.y);

        match glyph_rendering {
            GlyphRendering::RenderAsPath(path) => {
                if mode == RenderMode::Stroke {
                    canvas.stroke_path_internal(
                        path.borrow(),
                        Some(&outline_transform),
                        paint_flavor,
                        anti_alias,
                        &StrokeSettings {
                            line_width,
                            ..stroke.clone()
                        },
                    );
                } else {
                    canvas.fill_path_internal(
                        path.borrow(),
                        Some(&outline_transform),
                        paint_flavor,
                        anti_alias,
                        FillRule::NonZero,
                    );
                }
            }
            #[cfg(feature = "image-loading")]
            GlyphRendering::RenderAsImage(_) => unreachable!(),
        }
    }

    Ok(())
}

#[cfg(all(test, feature = "textlayout"))]
#[test]
fn rejected_fonts_preserve_shaping_caches() {
    let data = include_bytes!("../examples/assets/RobotoFlex-VariableFont.ttf");
    for shared in [false, true] {
        let context = TextContext::default();
        let font = context.add_font_mem(data).unwrap();
        let paint = Paint::color(crate::Color::black()).with_font(&[font]);
        context.measure_text(0.0, 0.0, "cached text", &paint).unwrap();

        let mut context = context.0.borrow_mut();
        let cached = (context.shaping_run_cache.len(), context.shaped_words_cache.len());
        assert!(cached.0 > 0 && cached.1 > 0);
        for (data, index) in [(b"invalid font".as_slice(), 0), (data.as_slice(), 1)] {
            let result = if shared {
                context.add_shared_font_with_index(data, index)
            } else {
                context.add_font_mem_with_index(data, index)
            };
            assert!(matches!(result, Err(ErrorKind::FontParseError)));
            assert_eq!(
                (context.shaping_run_cache.len(), context.shaped_words_cache.len()),
                cached
            );
        }
        if shared {
            context.add_shared_font_with_index(data.as_slice(), 0).unwrap();
        } else {
            context.add_font_mem_with_index(data, 0).unwrap();
        }
        assert!(context.shaping_run_cache.is_empty() && context.shaped_words_cache.is_empty());
    }
}
