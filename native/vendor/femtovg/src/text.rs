use std::{borrow::Borrow, cell::RefCell, ffi::OsStr, fs, hash::Hash, path::Path as FilePath, rc::Rc};

use fnv::FnvHashMap;
#[cfg(feature = "textlayout")]
use rustybuzz::ttf_parser;
use slotmap::{DefaultKey, SlotMap};

use crate::{
    paint::{FontVariations, PaintFlavor, StrokeSettings},
    Canvas, Color, ErrorKind, FillRule, ImageFlags, ImageId, ImageInfo, Paint, PixelFormat, PositionedGlyph,
    RenderTarget, Renderer,
};

mod atlas;
pub use atlas::Atlas;

mod font;
use font::{Font, GlyphRendering};
pub use font::{FontMetrics, VariationAxisInfo};

#[cfg(feature = "textlayout")]
mod textlayout;
#[cfg(feature = "textlayout")]
pub use textlayout::*;

// This padding is an empty border around the glyph’s pixels but inside the
// sampled area (texture coordinates) for the quad in render_atlas().
const GLYPH_PADDING: u32 = 1;
// We add an additional margin of 1 texel outside of the sampled area.
// The atlas is NEAREST-sampled (since 62e5be6), so no filter tap blends
// across the boundary; the margin is a guard band for GPUs whose
// interpolated texture coordinates drift at quad edges and select a
// neighbouring texel instead (issues #22, #310). Atlas textures start
// zero-initialized on both backends, so margin texels are blank.
const GLYPH_MARGIN: u32 = 1;

const TEXTURE_SIZE: usize = 512;
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
    subpixel_location: u8,
    variation_hash: u64,
}

impl RenderedGlyphId {
    fn new(
        glyph_index: u16,
        font_id: FontId,
        font_size: f32,
        line_width: f32,
        mode: RenderMode,
        subpixel_location: u8,
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
    bearing_x: i32,
    bearing_y: i32,
    atlas_x: u32,
    atlas_y: u32,
    color_glyph: bool,
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
                    self.add_font_dir(&path)?;
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
        self.clear_caches();

        let data_copy = data.to_owned();
        let font = Font::new_with_data(data_copy, face_index, self)?;
        Ok(FontId(self.fonts.insert(font)))
    }

    pub fn add_shared_font_with_index<T: AsRef<[u8]> + 'static>(
        &mut self,
        data: T,
        face_index: u32,
    ) -> Result<FontId, ErrorKind> {
        self.clear_caches();

        let font = Font::new_with_data(data, face_index, self)?;
        Ok(FontId(self.fonts.insert(font)))
    }

    pub fn font(&self, id: FontId) -> Option<&Font> {
        self.fonts.get(id.0)
    }

    pub fn font_mut(&mut self, id: FontId) -> Option<&mut Font> {
        self.fonts.get_mut(id.0)
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
        glyphs: impl Iterator<Item = PositionedGlyph>,
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

        let initial_render_target = canvas.current_render_target;

        for glyph in glyphs {
            let subpixel_location = crate::geometry::quantize(glyph.x.fract(), 0.1) * 10.0;

            let id = RenderedGlyphId::new(
                glyph.glyph_id,
                font_id,
                font_size,
                line_width,
                mode,
                subpixel_location as u8,
                normalized_coords,
            );

            let mut rendered_glyphs = self.rendered_glyphs.borrow_mut();
            let glyph_cache_entry = rendered_glyphs.entry(id);
            let glyph_cache_entry = match glyph_cache_entry {
                std::collections::hash_map::Entry::Occupied(occupied_entry) => occupied_entry,
                std::collections::hash_map::Entry::Vacant(_) => {
                    let result = self.render_glyph(
                        canvas,
                        font_size,
                        line_width,
                        mode,
                        font,
                        font_face,
                        glyph.glyph_id,
                        subpixel_location / 10.0,
                        normalized_coords,
                    )?;
                    glyph_cache_entry.insert_entry(result)
                }
            };

            let Some(rendered) = glyph_cache_entry.get() else {
                continue;
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

                q.x0 = glyph.x.trunc() + rendered.bearing_x as f32 - line_width_offset - GLYPH_PADDING as f32;
                q.y0 = glyph.y.round() - rendered.bearing_y as f32 - line_width_offset - GLYPH_PADDING as f32;
                q.x1 = q.x0 + rendered.width as f32;
                q.y1 = q.y0 + rendered.height as f32;

                q.s0 = rendered.atlas_x as f32 * itw;
                q.t0 = rendered.atlas_y as f32 * ith;
                q.s1 = (rendered.atlas_x + rendered.width) as f32 * itw;
                q.t1 = (rendered.atlas_y + rendered.height) as f32 * ith;

                cmd.quads.push(q);
            }
        }

        canvas.set_render_target(initial_render_target);

        Ok(GlyphDrawCommands {
            alpha_glyphs: alpha_cmd_map.drain().map(|(_, cmd)| cmd).collect(),
            color_glyphs: color_cmd_map.drain().map(|(_, cmd)| cmd).collect(),
        })
    }

    // Renders the glyph into the atlas and returns the RenderedGlyph struct for it.
    // Returns Ok(None) if there exists no path or image for the glyph in the font (missing glyph).
    #[allow(clippy::too_many_arguments)]
    fn render_glyph<T: Renderer>(
        &self,
        canvas: &mut Canvas<T>,
        font_size: f32,
        line_width: f32,
        mode: RenderMode,
        font: &Font,
        font_face: &font::FontFaceRef<'_>,
        glyph_id: u16,
        _subpixel_x: f32,
        normalized_coords: &[i16],
    ) -> Result<Option<RenderedGlyph>, ErrorKind> {
        #[cfg(feature = "swash")]
        if mode == RenderMode::Fill {
            if let Some(result) =
                self.render_glyph_swash(canvas, font, font_size, glyph_id, _subpixel_x, normalized_coords)?
            {
                return Ok(Some(result));
            }
        }

        let padding = GLYPH_PADDING + GLYPH_MARGIN;

        let (mut glyph_representation, glyph_metrics, scale) = {
            let scale = font.scale(font_size);
            let maybe_glyph_metrics = font
                .glyph(font_face, glyph_id, normalized_coords)
                .map(|g| g.metrics.clone());

            if let (Some(glyph_representation), Some(glyph_metrics)) = (
                font.glyph_rendering_representation(font_face, glyph_id, font_size as u16, normalized_coords),
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

        let width = (glyph_metrics.width * scale).ceil() as u32 + (line_width_offset * 2.0) as u32 + padding * 2;
        let height = (glyph_metrics.height * scale).ceil() as u32 + (line_width_offset * 2.0) as u32 + padding * 2;

        let (dst_index, dst_image_id, (dst_x, dst_y)) =
            self.find_texture_or_alloc(canvas, width as usize, height as usize)?;

        // render glyph to image
        canvas.save();
        canvas.reset();

        let rendered_bearing_x = (glyph_metrics.bearing_x * scale).round();
        let rendered_bearing_y = (glyph_metrics.bearing_y * scale).round();
        let x = dst_x as f32 - rendered_bearing_x + line_width_offset + padding as f32;
        let y = TEXTURE_SIZE as f32 - dst_y as f32 - rendered_bearing_y - line_width_offset - padding as f32;

        let rendered_glyph = RenderedGlyph {
            width: width - 2 * GLYPH_MARGIN,
            height: height - 2 * GLYPH_MARGIN,
            bearing_x: rendered_bearing_x as i32,
            bearing_y: rendered_bearing_y as i32,
            atlas_x: dst_x as u32 + GLYPH_MARGIN,
            atlas_y: dst_y as u32 + GLYPH_MARGIN,
            texture_index: dst_index,
            color_glyph,
        };

        match glyph_representation {
            GlyphRendering::RenderAsPath(ref mut path) => {
                canvas.translate(x, y);

                canvas.set_render_target(RenderTarget::Image(dst_image_id));
                canvas.clear_rect(
                    dst_x as u32,
                    TEXTURE_SIZE as u32 - dst_y as u32 - height,
                    width,
                    height,
                    Color::black(),
                );
                let factor = 1.0 / 8.0;

                let mask_color = Color::rgbf(factor, factor, factor);

                let mut line_width = line_width;

                if mode == RenderMode::Stroke {
                    line_width /= scale;
                }

                canvas.global_composite_blend_func(crate::BlendFactor::SrcAlpha, crate::BlendFactor::One);

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
                    canvas.save();
                    canvas.translate(point.0, point.1);

                    canvas.scale(scale, scale);

                    if mode == RenderMode::Stroke {
                        canvas.stroke_path_internal(
                            path,
                            &PaintFlavor::Color(mask_color),
                            false,
                            &StrokeSettings {
                                line_width,
                                ..Default::default()
                            },
                        );
                    } else {
                        canvas.fill_path_internal(path, &PaintFlavor::Color(mask_color), false, FillRule::NonZero);
                    }

                    canvas.restore();
                }
            }
            #[cfg(feature = "image-loading")]
            GlyphRendering::RenderAsImage(image_buffer) => {
                let target_x = rendered_glyph.atlas_x as usize;
                let target_y = rendered_glyph.atlas_y as usize;
                let target_width = rendered_glyph.width;
                let target_height = rendered_glyph.height;

                let image_buffer =
                    image_buffer.resize(target_width, target_height, image::imageops::FilterType::Nearest);
                if let Ok(image) = crate::image::ImageSource::try_from(&image_buffer) {
                    canvas.update_image(dst_image_id, image, target_x, target_y).unwrap();
                }
            }
        }

        canvas.restore();

        Ok(Some(rendered_glyph))
    }

    #[cfg(feature = "swash")]
    fn render_glyph_swash<T: Renderer>(
        &self,
        canvas: &mut Canvas<T>,
        font: &Font,
        font_size: f32,
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

        let image = {
            let mut scale_context = self.swash_scale_context.borrow_mut();
            let scaler_builder = scale_context
                .builder(font_ref)
                .size(font_size)
                .hint(true)
                .normalized_coords(normalized_coords);
            let mut scaler = scaler_builder.build();

            Render::new(&[
                Source::ColorOutline(0),
                Source::ColorBitmap(StrikeWith::BestFit),
                Source::Outline,
            ])
            .format(Format::Alpha)
            .offset(swash::zeno::Vector::new(subpixel_x, 0.0))
            .render(&mut scaler, glyph_id)
        };

        let image = match image {
            // Guard against glyphs with no visual (spaces)
            Some(img) if img.placement.width > 0 && img.placement.height > 0 => img,
            _ => return Ok(None),
        };

        let padding = GLYPH_PADDING + GLYPH_MARGIN;
        let glyph_width = image.placement.width;
        let glyph_height = image.placement.height;
        let alloc_width = glyph_width + 2 * padding;
        let alloc_height = glyph_height + 2 * padding;

        let (dst_index, dst_image_id, (dst_x, dst_y)) =
            self.find_texture_or_alloc(canvas, alloc_width as usize, alloc_height as usize)?;

        let is_color = image.content == swash::scale::image::Content::Color;

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
            bearing_x: image.placement.left,
            bearing_y: image.placement.top,
            atlas_x: dst_x as u32 + GLYPH_MARGIN,
            atlas_y: dst_y as u32 + GLYPH_MARGIN,
            color_glyph: is_color,
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
                if let Ok(size) = canvas.image_size(image_id) {
                    // With image-loading we then subsequently support color fonts, where
                    // the color glyphs are uploaded directly. Since that's immediately and
                    // the clear_rect() is run much later, it would overwrite any uploaded
                    // glyphs. So then when for the debug-inspector, use an image to clear.
                    #[cfg(feature = "image-loading")]
                    {
                        use rgb::FromSlice;
                        let clear_image = image::RgbaImage::from_pixel(
                            size.0 as u32,
                            size.1 as u32,
                            image::Rgba::<u8>([255, 0, 0, 0]),
                        );
                        canvas
                            .update_image(
                                image_id,
                                crate::image::ImageSource::from(imgref::Img::new(
                                    clear_image.as_rgba(),
                                    clear_image.width() as usize,
                                    clear_image.height() as usize,
                                )),
                                0,
                                0,
                            )
                            .unwrap();
                    }
                    #[cfg(not(feature = "image-loading"))]
                    {
                        canvas.save();
                        canvas.reset();
                        canvas.set_render_target(RenderTarget::Image(image_id));
                        canvas.clear_rect(
                            0,
                            0,
                            size.0 as u32,
                            size.1 as u32,
                            Color::rgb(255, 0, 0), // Shown as white if using Gray8.,
                        );
                        canvas.restore();
                    }
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

        canvas.save();

        let line_width = match mode {
            RenderMode::Fill => stroke.line_width,
            RenderMode::Stroke => stroke.line_width / scale,
        };

        canvas.translate(glyph.x, glyph.y);
        canvas.scale(scale, -scale);

        match glyph_rendering {
            GlyphRendering::RenderAsPath(path) => {
                if mode == RenderMode::Stroke {
                    canvas.stroke_path_internal(
                        path.borrow(),
                        paint_flavor,
                        anti_alias,
                        &StrokeSettings {
                            line_width,
                            ..stroke.clone()
                        },
                    );
                } else {
                    canvas.fill_path_internal(path.borrow(), paint_flavor, anti_alias, FillRule::NonZero);
                }
            }
            #[cfg(feature = "image-loading")]
            GlyphRendering::RenderAsImage(_) => unreachable!(),
        }

        canvas.restore();
    }

    Ok(())
}
