//! Compositor-drawn decorations: titlebar, title text and round controls,
//! rasterized with tiny-skia + ab_glyph into memory buffers for the renderer.

use ab_glyph::{Font, FontVec, ScaleFont};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::utils::Transform;
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform as SkTransform};

use crate::state::{BTN_W, TITLEBAR_H};

pub struct DecoSet {
    pub titlebar: MemoryRenderBuffer,
    pub titlebar_w: i32,
    pub title: MemoryRenderBuffer,
    pub btn_min: MemoryRenderBuffer,
    pub btn_fs: MemoryRenderBuffer,
    pub btn_close: MemoryRenderBuffer,
}

pub enum BtnSym {
    Min,
    Fs,
    Close,
}

#[allow(clippy::all)]
fn to_buffer(pixmap: &Pixmap) -> MemoryRenderBuffer {
    // tiny-skia is RGBA premultiplied, the renderer wants ARGB8888
    // little-endian (BGRA byte order), so swap R and B.
    let mut data = pixmap.data().to_vec();
    for px in data.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    MemoryRenderBuffer::from_slice(
        &data,
        Fourcc::Argb8888,
        (pixmap.width() as i32, pixmap.height() as i32),
        1,
        Transform::Normal,
        None,
    )
}

pub fn draw_titlebar(w: i32) -> MemoryRenderBuffer {
    let mut pm = Pixmap::new(w.max(1) as u32, TITLEBAR_H as u32).unwrap();
    pm.fill(Color::from_rgba(0.13, 0.13, 0.16, 1.0).unwrap());
    to_buffer(&pm)
}

pub fn draw_button(sym: BtnSym) -> MemoryRenderBuffer {
    let (w, h) = (BTN_W as f32, TITLEBAR_H as f32);
    let mut pm = Pixmap::new(w as u32, h as u32).unwrap();
    let (cx, cy) = (w / 2.0, h / 2.0);
    let rad = h * 0.34;

    let mut fill = Paint::default();
    fill.set_color(Color::from_rgba(1.0, 1.0, 1.0, 0.10).unwrap());
    fill.anti_alias = true;
    let circle = PathBuilder::from_circle(cx, cy, rad).unwrap();
    pm.fill_path(&circle, &fill, tiny_skia::FillRule::Winding, SkTransform::identity(), None);

    let mut stroke_paint = Paint::default();
    stroke_paint.set_color(Color::from_rgba(1.0, 1.0, 1.0, 0.80).unwrap());
    stroke_paint.anti_alias = true;
    let stroke = Stroke {
        width: 1.5,
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..Default::default()
    };
    let g = rad * 0.33;
    let mut pb = PathBuilder::new();
    match sym {
        BtnSym::Close => {
            pb.move_to(cx - g, cy - g);
            pb.line_to(cx + g, cy + g);
            pb.move_to(cx + g, cy - g);
            pb.line_to(cx - g, cy + g);
        }
        BtnSym::Min => {
            pb.move_to(cx - g, cy);
            pb.line_to(cx + g, cy);
        }
        BtnSym::Fs => {
            let e = g * 0.8;
            pb.move_to(cx - g + e, cy - g);
            pb.line_to(cx - g, cy - g);
            pb.line_to(cx - g, cy - g + e);
            pb.move_to(cx + g - e, cy + g);
            pb.line_to(cx + g, cy + g);
            pb.line_to(cx + g, cy + g - e);
        }
    }
    if let Some(path) = pb.finish() {
        pm.stroke_path(&path, &stroke_paint, &stroke, SkTransform::identity(), None);
    }
    to_buffer(&pm)
}

fn load_font() -> Option<FontVec> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let query = fontdb::Query {
        families: &[fontdb::Family::SansSerif],
        ..Default::default()
    };
    let id = db.query(&query)?;
    let (source, index) = db.face_source(id)?;
    let data = match source {
        fontdb::Source::Binary(data) => data.as_ref().as_ref().to_vec(),
        fontdb::Source::File(path) => std::fs::read(path).ok()?,
        fontdb::Source::SharedFile(_, data) => data.as_ref().as_ref().to_vec(),
    };
    FontVec::try_from_vec_and_index(data, index).ok()
}

pub fn draw_title(text: &str) -> (MemoryRenderBuffer, i32) {
    let h = TITLEBAR_H as f32;
    let size = h * 0.55;
    let Some(font) = load_font() else {
        let pm = Pixmap::new(1, TITLEBAR_H as u32).unwrap();
        return (to_buffer(&pm), 1);
    };
    let scaled = font.as_scaled(size);
    let mut width = 4.0f32;
    for c in text.chars() {
        width += scaled.h_advance(scaled.scaled_glyph(c).id);
    }
    let w = width.ceil().max(1.0) as u32;
    let mut pm = Pixmap::new(w, TITLEBAR_H as u32).unwrap();
    let baseline = (h + scaled.ascent() + scaled.descent()) / 2.0 - scaled.descent();
    let mut x = 2.0f32;
    for c in text.chars() {
        let glyph = scaled.scaled_glyph(c);
        let advance = scaled.h_advance(glyph.id);
        let glyph = ab_glyph::Glyph {
            position: ab_glyph::point(x, baseline),
            ..glyph
        };
        if let Some(outline) = scaled.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            let data = pm.data_mut();
            let stride = w as usize * 4;
            outline.draw(|gx, gy, cov| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px < 0 || py < 0 || px >= w as i32 || py >= TITLEBAR_H {
                    return;
                }
                let a = (cov * 0.85 * 255.0) as u8;
                let off = py as usize * stride + px as usize * 4;
                // white text, premultiplied
                data[off] = a.max(data[off]);
                data[off + 1] = a.max(data[off + 1]);
                data[off + 2] = a.max(data[off + 2]);
                data[off + 3] = a.max(data[off + 3]);
            });
        }
        x += advance;
    }
    (to_buffer(&pm), w as i32)
}

pub fn build(role: &str, width: i32) -> DecoSet {
    let (title, _title_w) = draw_title(crate::state::role_title(role));
    DecoSet {
        titlebar: draw_titlebar(width),
        titlebar_w: width,
        title,
        btn_min: draw_button(BtnSym::Min),
        btn_fs: draw_button(BtnSym::Fs),
        btn_close: draw_button(BtnSym::Close),
    }
}
