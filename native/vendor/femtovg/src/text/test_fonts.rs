//! Fonts for tests, assembled from their tables.

/// The version tag of a font with TrueType outlines.
const TRUETYPE: [u8; 4] = [0, 1, 0, 0];

/// Assembles a font from its tables.
///
/// A font file is a 12-byte header (version, table count, and three fields
/// derived from the count that help a binary search of the records), one
/// 16-byte record per table (tag, checksum, offset, length), then the table
/// data. The function sorts the records by tag and pads each table to a
/// four-byte boundary, as the format requires. The parsers used here do not
/// verify checksums, so they are left zero.
fn font_from_tables(sfnt_version: [u8; 4], mut tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    tables.sort_by_key(|(tag, _)| *tag);

    let table_count = tables.len() as u16;
    let entry_selector = table_count.ilog2() as u16;
    let search_range: u16 = 16 << entry_selector;
    let range_shift = table_count * 16 - search_range;
    let mut font = sfnt_version.to_vec();
    for field in [table_count, search_range, entry_selector, range_shift] {
        font.extend_from_slice(&field.to_be_bytes());
    }

    let data_start = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, table) in &tables {
        let offset = (data_start + body.len()) as u32;
        font.extend_from_slice(tag);
        font.extend_from_slice(&[0; 4]); // checksum
        font.extend_from_slice(&offset.to_be_bytes());
        font.extend_from_slice(&(table.len() as u32).to_be_bytes());
        body.extend_from_slice(table);
        body.resize(body.len().next_multiple_of(4), 0);
    }
    font.extend_from_slice(&body);
    font
}

fn push_u16(data: &mut Vec<u8>, value: u16) {
    data.extend_from_slice(&value.to_be_bytes());
}

fn push_i16(data: &mut Vec<u8>, value: i16) {
    data.extend_from_slice(&value.to_be_bytes());
}

fn push_u32(data: &mut Vec<u8>, value: u32) {
    data.extend_from_slice(&value.to_be_bytes());
}

/// The tables required for parsing (`head`, `hhea` and `maxp`) for a font
/// with one glyph.
fn required_tables() -> Vec<([u8; 4], Vec<u8>)> {
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

    vec![(*b"head", head), (*b"hhea", hhea), (*b"maxp", maxp)]
}

/// Builds a minimal TrueType font containing only the tables required for
/// parsing (`head`, `hhea` and `maxp`), so every optional metric has to
/// take its documented fallback. Units per em is 1024 and the
/// ascender/descender are 800/-200 font units.
pub(crate) fn minimal_font_without_optional_tables() -> Vec<u8> {
    font_from_tables(TRUETYPE, required_tables())
}

/// Builds a font whose only glyph is a PNG bitmap in an `sbix` strike.
pub(crate) fn png_glyph_font() -> Vec<u8> {
    font_with_png_glyph(&[(24, &png(24, 24))], (0, 0), Vec::new())
}

/// Builds a font whose only glyph is a 36 by 24 PNG bitmap in an `sbix`
/// strike, with its bottom left corner 3 pixels right of and 5 pixels below
/// the glyph origin.
#[cfg(any(feature = "swash", all(feature = "textlayout", feature = "image-loading")))]
pub(crate) fn offset_png_glyph_font() -> Vec<u8> {
    font_with_png_glyph(&[(24, &png(36, 24))], (3, -5), Vec::new())
}

/// Builds a font whose only glyph has a PNG in two `sbix` strikes that cover
/// different fractions of the em: 600 pixels square at 512 ppem, and 1024
/// pixels square at 1024 ppem.
#[cfg(feature = "swash")]
pub(crate) fn two_strike_png_glyph_font() -> Vec<u8> {
    font_with_png_glyph(&[(512, &png(600, 600)), (1024, &png(1024, 1024))], (0, 0), Vec::new())
}

/// Encodes a `width` by `height` PNG of one color.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut png = std::io::Cursor::new(Vec::new());
    ::image::RgbaImage::from_pixel(width, height, ::image::Rgba([255, 0, 64, 255]))
        .write_to(&mut png, ::image::ImageFormat::Png)
        .unwrap();
    png.into_inner()
}

/// Builds a font whose only glyph has a square outline and a PNG in an `sbix`
/// strike. The PNG ends after its header, so parsers can read its size but
/// cannot decode it.
#[cfg(all(feature = "textlayout", feature = "swash"))]
pub(crate) fn outline_glyph_with_broken_png_font() -> Vec<u8> {
    let png = png(24, 24);

    // One contour through four on-curve points, a 500-unit square. Each
    // flag byte is 1 (on curve), so each coordinate is a 16-bit delta.
    let mut glyf = Vec::new();
    for value in [1, 0, 0, 500, 500] {
        push_i16(&mut glyf, value); // numberOfContours, xMin, yMin, xMax, yMax
    }
    push_u16(&mut glyf, 3); // endPtsOfContours[0]
    push_u16(&mut glyf, 0); // instructionLength
    glyf.extend_from_slice(&[1; 4]); // flags
    for delta in [0, 0, 500, 0, 0, 500, 0, -500] {
        push_i16(&mut glyf, delta); // x deltas, then y deltas
    }

    let mut loca = Vec::new();
    push_u16(&mut loca, 0); // offsets[0], in units of two bytes
    push_u16(&mut loca, glyf.len() as u16 / 2); // offsets[1]

    // swash reads `glyf` outlines only when the font has an `hmtx` table.
    // `hhea` gives no glyph full metrics, so `hmtx` holds only the left side bearing.
    let mut hmtx = Vec::new();
    push_i16(&mut hmtx, 0); // leftSideBearings[0]

    // The 8-byte signature and the 25-byte IHDR chunk.
    let header = &png[..33];
    font_with_png_glyph(
        &[(24, header)],
        (0, 0),
        vec![(*b"glyf", glyf), (*b"hmtx", hmtx), (*b"loca", loca)],
    )
}

/// Builds a font whose only glyph has one PNG in each of the `sbix` strikes in
/// `strikes`, given as (ppem, PNG data), with the required tables and `extra_tables`.
/// `origin` is the PNG's bottom left corner relative to the glyph origin, in pixels, y up.
fn font_with_png_glyph(strikes: &[(u16, &[u8])], origin: (i16, i16), extra_tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    let mut sbix = Vec::new();
    push_u16(&mut sbix, 1); // version
    push_u16(&mut sbix, 1); // flags
    push_u32(&mut sbix, strikes.len() as u32); // numStrikes
                                               // strikeOffsets: the strikes follow these offsets, one after another.
    let mut strike_offset = 8 + 4 * strikes.len() as u32;
    for (_, png) in strikes {
        push_u32(&mut sbix, strike_offset);
        strike_offset += 12 + 8 + png.len() as u32;
    }

    for (ppem, png) in strikes {
        // The strike. glyphDataOffsets[0] and [1] are the start and end of the
        // glyph's data, measured from the start of the strike. The strike's own
        // fields take 12 bytes, and a glyph has 8 bytes before its PNG data.
        push_u16(&mut sbix, *ppem); // ppem
        push_u16(&mut sbix, 72); // ppi
        push_u32(&mut sbix, 12); // glyphDataOffsets[0]
        push_u32(&mut sbix, 12 + 8 + png.len() as u32); // glyphDataOffsets[1]
        push_i16(&mut sbix, origin.0); // originOffsetX
        push_i16(&mut sbix, origin.1); // originOffsetY
        sbix.extend_from_slice(b"png "); // graphicType
        sbix.extend_from_slice(png);
    }

    let mut tables = [required_tables(), extra_tables].concat();
    tables.push((*b"sbix", sbix));
    font_from_tables(TRUETYPE, tables)
}

/// Rebuilds a sfnt/TrueType font byte buffer with the named 4-byte tables
/// removed, so the fallback metric paths can be exercised on real assets.
#[cfg(feature = "textlayout")]
pub(crate) fn font_without_tables(data: &[u8], drop_tags: &[&[u8; 4]]) -> Vec<u8> {
    let read_u16 = |buf: &[u8], at: usize| u16::from_be_bytes([buf[at], buf[at + 1]]);
    let read_u32 =
        |buf: &[u8], at: usize| u32::from_be_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]]) as usize;

    let num_tables = read_u16(data, 4) as usize;

    let mut kept = Vec::new();
    for i in 0..num_tables {
        let rec = 12 + i * 16;
        let tag = [data[rec], data[rec + 1], data[rec + 2], data[rec + 3]];
        if drop_tags.iter().any(|d| **d == tag) {
            continue;
        }
        let offset = read_u32(data, rec + 8);
        let length = read_u32(data, rec + 12);
        kept.push((tag, data[offset..offset + length].to_vec()));
    }
    font_from_tables([data[0], data[1], data[2], data[3]], kept)
}
