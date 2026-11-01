use std::{
    hash::{Hash, Hasher},
    ops::Range,
};

use fnv::FnvHasher;
use lru::LruCache;

use crate::{
    paint::{FontVariations, TextSettings},
    Align, Baseline, ErrorKind, FontId, Paint,
};

use unicode_bidi::BidiInfo;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Copy, Clone, Debug)]
pub struct ShapedGlyph {
    pub x: f32,
    pub y: f32,
    pub c: char,
    pub byte_index: usize,
    pub font_id: FontId,
    pub glyph_id: u16,
    pub width: f32,
    pub height: f32,
    pub advance_x: f32,
    pub advance_y: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub bearing_y: f32,
}

#[derive(Clone, Debug, Default)]
pub(super) struct ShapedWord {
    glyphs: Vec<ShapedGlyph>,
    width: f32,
}

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub(super) struct ShapingId {
    size: u32,
    word_hash: u64,
    font_ids: [Option<FontId>; 8],
    variation_hash: u64,
    // The same word shapes differently right-to-left than left-to-right
    // (mirrored brackets, reversed cluster order), so the cached shaping is
    // only valid for the direction it was produced under. Direction-neutral
    // words (digits, punctuation) genuinely occur in both. `None` marks a
    // whole-run key, where the direction is derived from the text itself.
    rtl: Option<bool>,
    // Letter spacing is baked into the cached advances by `shape_word`, so
    // shapings at different spacings must not share an entry.
    letter_spacing_key: u32,
}

impl ShapingId {
    fn new(
        font_size: f32,
        font_ids: [Option<FontId>; 8],
        word: &str,
        max_width: Option<f32>,
        variations: &FontVariations,
        letter_spacing: f32,
        rtl: Option<bool>,
    ) -> Self {
        let mut hasher = FnvHasher::default();
        word.hash(&mut hasher);
        if let Some(max_width) = max_width {
            (max_width.trunc() as i32).hash(&mut hasher);
        }

        Self {
            size: (font_size * 10.0).trunc() as u32,
            word_hash: hasher.finish(),
            font_ids,
            variation_hash: variations.hash(),
            rtl,
            letter_spacing_key: (letter_spacing * 10.0).trunc() as u32,
        }
    }
}

pub(super) type ShapedWordsCache<H> = LruCache<ShapingId, Result<ShapedWord, ErrorKind>, H>;
pub(super) type ShapingRunCache<H> = LruCache<ShapingId, TextMetrics, H>;

impl super::TextContext {
    /// Returns information on how the provided text will be drawn with the specified paint.
    pub fn measure_text<S: AsRef<str>>(
        &self,
        x: f32,
        y: f32,
        text: S,
        paint: &Paint,
    ) -> Result<TextMetrics, ErrorKind> {
        self.0.borrow_mut().measure_text(x, y, text, &paint.text)
    }

    /// Returns the maximum index-th byte of text that will fit inside `max_width`.
    ///
    /// The retuned index will always lie at the start and/or end of a UTF-8 code point sequence or at the start or end of the text
    pub fn break_text<S: AsRef<str>>(&self, max_width: f32, text: S, paint: &Paint) -> Result<usize, ErrorKind> {
        self.0.borrow_mut().break_text(max_width, text, &paint.text)
    }

    /// Returnes a list of ranges representing each line of text that will fit inside `max_width`
    pub fn break_text_vec<S: AsRef<str>>(
        &self,
        max_width: f32,
        text: S,
        paint: &Paint,
    ) -> Result<Vec<Range<usize>>, ErrorKind> {
        self.0.borrow_mut().break_text_vec(max_width, text, &paint.text)
    }

    /// Adjusts the capacity of the shaping run cache. This is a cache for measurements of whole
    /// strings.
    pub fn resize_shaping_run_cache(&self, capacity: std::num::NonZeroUsize) {
        self.0.borrow_mut().resize_shaping_run_cache(capacity)
    }

    /// Adjusts the capacity of the shaped words cache. This is a cache for measurements of
    /// individual words. Words are separated by
    /// [UAX#29 word boundaries](http://www.unicode.org/reports/tr29/#Word_Boundaries).
    pub fn resize_shaped_words_cache(&self, capacity: std::num::NonZeroUsize) {
        self.0.borrow_mut().resize_shaped_words_cache(capacity)
    }
}

impl super::TextContextImpl {
    pub fn resize_shaping_run_cache(&mut self, capacity: std::num::NonZeroUsize) {
        self.shaping_run_cache.resize(capacity);
    }

    pub fn resize_shaped_words_cache(&mut self, capacity: std::num::NonZeroUsize) {
        self.shaped_words_cache.resize(capacity);
    }

    pub fn measure_text<S: AsRef<str>>(
        &mut self,
        x: f32,
        y: f32,
        text: S,
        text_settings: &TextSettings,
    ) -> Result<TextMetrics, ErrorKind> {
        shape(x, y, self, text_settings, text.as_ref(), None)
    }

    pub fn break_text<S: AsRef<str>>(
        &mut self,
        max_width: f32,
        text: S,
        text_settings: &TextSettings,
    ) -> Result<usize, ErrorKind> {
        let layout = shape(0.0, 0.0, self, text_settings, text.as_ref(), Some(max_width))?;

        Ok(layout.final_byte_index)
    }

    pub fn break_text_vec<S: AsRef<str>>(
        &mut self,
        max_width: f32,
        text: S,
        text_settings: &TextSettings,
    ) -> Result<Vec<Range<usize>>, ErrorKind> {
        let text = text.as_ref();

        let mut res = Vec::new();
        let mut start = 0;

        while start < text.len() {
            let Ok(index) = self.break_text(max_width, &text[start..], text_settings) else {
                break;
            };

            if index == 0 {
                break;
            }

            let index = start + index;
            res.push(start..index);
            start += &text[start..index].len();
        }

        Ok(res)
    }
}

/// Represents the result of a text shaping run.
#[derive(Clone, Default, Debug)]
pub struct TextMetrics {
    /// X-coordinate of the starting position for the shaped text.
    pub x: f32,
    /// Y-coordinate of the starting position for the shaped text.
    pub y: f32,
    width: f32,
    height: f32,
    baseline: f32,
    /// Vector of shaped glyphs resulting from the text shaping run.
    pub glyphs: Vec<ShapedGlyph>,
    pub(crate) final_byte_index: usize,
}

impl TextMetrics {
    pub(crate) fn scale(&mut self, scale: f32) {
        self.x *= scale;
        self.y *= scale;
        self.width *= scale;
        self.height *= scale;
        self.baseline *= scale;

        for glyph in &mut self.glyphs {
            glyph.x *= scale;
            glyph.y *= scale;
            glyph.width *= scale;
            glyph.height *= scale;
            glyph.bearing_y *= scale;
        }
    }

    /// width of the glyphs as drawn
    pub fn width(&self) -> f32 {
        self.width
    }

    /// Y-coordinate of the run's alphabetic baseline, in the same space as the
    /// glyph positions: the requested `y` adjusted for the paint's
    /// [`Baseline`](crate::Baseline) setting. Glyphs and text decorations are
    /// positioned relative to this line.
    pub fn baseline(&self) -> f32 {
        self.baseline
    }

    /// height of the glyphs as drawn
    pub fn height(&self) -> f32 {
        self.height
    }
}

// Shaper

pub fn shape(
    x: f32,
    y: f32,
    context: &mut super::TextContextImpl,
    text_settings: &TextSettings,
    text: &str,
    max_width: Option<f32>,
) -> Result<TextMetrics, ErrorKind> {
    let variations = &text_settings.font_variations;
    let id = ShapingId::new(
        text_settings.font_size,
        text_settings.font_ids,
        text,
        max_width,
        variations,
        text_settings.letter_spacing,
        // The run cache keys the whole string; per-run direction is derived
        // from the text itself below.
        None,
    );

    if !context.shaping_run_cache.contains(&id) {
        let metrics = shape_run(
            context,
            text_settings.font_size,
            text_settings.font_ids,
            text_settings.letter_spacing,
            text,
            max_width,
            variations,
        );
        context.shaping_run_cache.put(id, metrics);
    }

    if let Some(mut metrics) = context.shaping_run_cache.get(&id).cloned() {
        layout(x, y, context, &mut metrics, text_settings)?;

        return Ok(metrics);
    }

    Err(ErrorKind::UnknownError)
}

fn shape_run(
    context: &mut super::TextContextImpl,
    font_size: f32,
    font_ids: [Option<FontId>; 8],
    letter_spacing: f32,
    text: &str,
    max_width: Option<f32>,
    variations: &FontVariations,
) -> TextMetrics {
    let mut result = TextMetrics {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
        baseline: 0.0,
        glyphs: Vec::with_capacity(text.len()),
        final_byte_index: 0,
    };

    // The paragraph base direction follows the first strong character
    // (UAX #9 rules P2/P3) rather than being pinned left-to-right: an Arabic
    // or Hebrew sentence is an RTL paragraph, so its neutral punctuation
    // sits at its visual left end and embedded LTR words order correctly.
    let bidi_info = BidiInfo::new(text, None);

    // this controls whether we should break within words
    let mut first_word_in_paragraph = true;

    let Some(paragraph) = bidi_info.paragraphs.first() else {
        return result;
    };

    let line = paragraph.range.clone();

    let (levels, runs) = bidi_info.visual_runs(paragraph, line);

    for run in runs {
        let sub_text = &text[run.clone()];

        if sub_text.is_empty() {
            continue;
        }

        let hb_direction = if levels[run.start].is_rtl() {
            rustybuzz::Direction::RightToLeft
        } else {
            rustybuzz::Direction::LeftToRight
        };

        let mut words = Vec::new();
        let mut word_break_reached = false;
        let mut byte_index = run.start;

        let is_rtl_run = hb_direction == rustybuzz::Direction::RightToLeft;

        for mut word_txt in sub_text.split_word_bounds() {
            let id = ShapingId::new(
                font_size,
                font_ids,
                word_txt,
                max_width,
                variations,
                letter_spacing,
                Some(is_rtl_run),
            );

            if !context.shaped_words_cache.contains(&id) {
                let word = shape_word(
                    word_txt,
                    hb_direction,
                    context,
                    font_size,
                    &font_ids,
                    letter_spacing,
                    variations,
                );
                context.shaped_words_cache.put(id, word);
            }

            if let Some(Ok(word)) = context.shaped_words_cache.get(&id) {
                let mut word = word.clone();

                if let Some(max_width) = max_width {
                    if result.width + word.width >= max_width {
                        word_break_reached = true;
                        if first_word_in_paragraph {
                            // search for the largest prefix of the word that can fit
                            let mut bytes_included = 0;
                            let mut subword_width = 0.0;
                            let target_width = max_width - result.width;
                            for glyph in word.glyphs {
                                bytes_included = glyph.byte_index;
                                let glyph_width = glyph.advance_x + letter_spacing;

                                // nuance: we want to include the first glyph even if it breaks
                                // the bounds. this is to allow pathologically small bounds to
                                // at least complete rendering
                                if subword_width + glyph_width >= target_width && bytes_included != 0 {
                                    break;
                                }

                                subword_width += glyph_width;
                            }

                            if bytes_included == 0 {
                                // just in case - never mind!
                                break;
                            }

                            let subword_txt = &word_txt[..bytes_included];
                            let id = ShapingId::new(
                                font_size,
                                font_ids,
                                subword_txt,
                                Some(max_width),
                                variations,
                                letter_spacing,
                                Some(is_rtl_run),
                            );
                            if !context.shaped_words_cache.contains(&id) {
                                let subword = shape_word(
                                    subword_txt,
                                    hb_direction,
                                    context,
                                    font_size,
                                    &font_ids,
                                    letter_spacing,
                                    variations,
                                );
                                context.shaped_words_cache.put(id, subword);
                            }

                            if let Some(Ok(subword)) = context.shaped_words_cache.get(&id) {
                                // replace the outer variables so we can continue normally
                                word = subword.clone();
                                word_txt = subword_txt;
                            } else {
                                break;
                            }
                        } else if word.glyphs.iter().all(|g| g.c.is_whitespace()) {
                            // the last word we've broken in the middle of is whitespace.
                            // include this word for now, but we will discard its metrics in a moment.
                        } else {
                            // we are not breaking up words - discard this word
                            break;
                        }
                    }
                }

                // if we have broken in the middle of whitespace, do not include this word in metrics
                if !word_break_reached || !word.glyphs.iter().all(|g| g.c.is_whitespace()) {
                    result.width += word.width;
                }

                for glyph in &mut word.glyphs {
                    glyph.byte_index += byte_index;
                    debug_assert!(text.get(glyph.byte_index..).is_some());
                }
                words.push(word);
                first_word_in_paragraph = false;
            }

            byte_index += word_txt.len();

            if word_break_reached {
                break;
            }
        }

        if levels[run.start].is_rtl() {
            words.reverse();
        }

        for word in words {
            result.glyphs.extend(word.glyphs.clone());
        }

        result.final_byte_index = byte_index;

        if word_break_reached {
            break;
        }
    }

    result
}

fn shape_word(
    word: &str,
    hb_direction: rustybuzz::Direction,
    context: &mut super::TextContextImpl,
    font_size: f32,
    font_ids: &[Option<FontId>; 8],
    letter_spacing: f32,
    variations: &FontVariations,
) -> Result<ShapedWord, ErrorKind> {
    // find_font will call the closure with each font matching the provided style
    // until a font capable of shaping the word is found
    context.find_font(font_ids, |(font_id, font)| {
        let font_face = font.face_ref_with_variations(variations);
        let normalized_coords = font.normalize_variations(variations);
        let face = rustybuzz::Face::from_face(font_face.0.clone());
        // Call harfbuzz
        let output = {
            let mut buffer = rustybuzz::UnicodeBuffer::new();
            buffer.push_str(word);
            buffer.set_direction(hb_direction);

            rustybuzz::shape(&face, &[], buffer)
        };

        let positions = output.glyph_positions();
        let infos = output.glyph_infos();

        let mut shaped_word = ShapedWord {
            glyphs: Vec::with_capacity(positions.len()),
            width: 0.0,
        };

        let mut has_missing = false;

        for (position, (info, c)) in positions.iter().zip(infos.iter().zip(word.chars())) {
            if info.glyph_id == 0 {
                has_missing = true;
            }

            let scale = font.scale(font_size);

            let mut g = ShapedGlyph {
                x: 0.0,
                y: 0.0,
                c,
                byte_index: info.cluster as usize,
                font_id,
                glyph_id: info
                    .glyph_id
                    .try_into()
                    .expect("rustybuzz guarantees the output glyph id is u16"),
                width: 0.0,
                height: 0.0,
                advance_x: position.x_advance as f32 * scale,
                advance_y: position.y_advance as f32 * scale,
                offset_x: position.x_offset as f32 * scale,
                offset_y: position.y_offset as f32 * scale,
                bearing_y: 0.0,
            };

            if let Some(glyph) = font.glyph(&font_face, g.glyph_id, &normalized_coords) {
                g.width = glyph.metrics.width * scale;
                g.height = glyph.metrics.height * scale;
                g.bearing_y = glyph.metrics.bearing_y * scale;
            }

            shaped_word.width += g.advance_x + letter_spacing;
            shaped_word.glyphs.push(g);
        }

        (has_missing, shaped_word)
    })
}

// Calculates the x,y coordinates for each glyph based on their advances. Calculates total width and height of the shaped text run
fn layout(
    x: f32,
    y: f32,
    context: &mut super::TextContextImpl,
    res: &mut TextMetrics,
    text_settings: &TextSettings,
) -> Result<(), ErrorKind> {
    let mut cursor_x = x;
    let mut cursor_y = y;

    // Horizontal alignment
    match text_settings.text_align {
        Align::Center => cursor_x -= res.width / 2.0,
        Align::Right => cursor_x -= res.width,
        Align::Left => (),
    }

    res.x = cursor_x;

    let mut min_y = cursor_y;
    let mut max_y = cursor_y;

    let mut ascender: f32 = 0.;
    let mut descender: f32 = 0.;

    for glyph in &mut res.glyphs {
        let font = context.font_mut(glyph.font_id).ok_or(ErrorKind::NoFontFound)?;
        let metrics = font.metrics(text_settings.font_size);
        ascender = ascender.max(metrics.ascender());
        descender = descender.min(metrics.descender());
    }

    let primary_metrics = context.find_font(&text_settings.font_ids, |(_, font)| {
        (false, font.metrics(text_settings.font_size))
    })?;
    if ascender.abs() < f32::EPSILON {
        ascender = primary_metrics.ascender();
    }
    if descender.abs() < f32::EPSILON {
        descender = primary_metrics.descender();
    }

    // Baseline alignment
    let alignment_offset_y = match text_settings.text_baseline {
        Baseline::Top => ascender,
        Baseline::Middle => ascender.midpoint(descender),
        Baseline::Alphabetic => 0.0,
        Baseline::Bottom => descender,
    };

    // Record where the alphabetic baseline lands so consumers (glyph drawing,
    // text decorations) need not re-derive it from glyph positions, which
    // would be skewed by per-glyph y-offsets such as combining marks.
    res.baseline = cursor_y + alignment_offset_y;

    for glyph in &mut res.glyphs {
        glyph.x = cursor_x + glyph.offset_x;
        glyph.y = cursor_y + alignment_offset_y + glyph.offset_y;

        // `glyph.y` is the baseline, so the ink box starts `bearing_y` above it.
        let ink_top = glyph.y - glyph.bearing_y;
        min_y = min_y.min(ink_top);
        max_y = max_y.max(ink_top + glyph.height);

        cursor_x += glyph.advance_x + text_settings.letter_spacing;
        cursor_y += glyph.advance_y;
    }

    res.y = min_y;
    res.height = max_y - min_y;

    Ok(())
}
