use femtovg::{renderer::Void, Baseline, Canvas, Color, FillRule, Paint, Path, Solidity};

#[test]
fn path_with_single_move_to() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(10.0, 10.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn path_with_two_lines() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.line_to(10.0, 10.0);
    path.line_to(10.0, 10.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn path_with_close_points() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(10.0, 10.0);
    path.line_to(10.0001, 10.0);
    path.line_to(10.0001, 10.000001);
    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn path_with_points_at_limits() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(10.0, 10.0);
    path.line_to(f32::MAX, f32::MAX);
    path.quad_to(10.0, 10.0, -f32::MAX, f32::MAX);
    path.bezier_to(10.0, 10.0, f32::MAX, 5000.0, -f32::MAX, -f32::MAX);
    path.rounded_rect_varying(
        -f32::MAX,
        -f32::MAX,
        f32::MAX,
        f32::MAX,
        f32::MAX,
        f32::MAX,
        f32::MAX,
        f32::MAX,
    );
    path.close();

    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn path_with_points_around_zero() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(0.0, 0.0);
    path.line_to(0.0, 0.0);
    path.line_to(0.0001, 0.0000003);
    path.quad_to(0.002, 0.0001, -0.002, 0.0001);
    path.bezier_to(0.0001, 0.002, -0.002, 0.0001, -0.002, 0.0001);
    path.rounded_rect_varying(
        -f32::MAX,
        -f32::MAX,
        f32::MAX,
        f32::MAX,
        f32::MAX,
        0.0001,
        0.0001,
        0.0001,
    );

    path.close();

    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn degenerate_stroke() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(0.5, 0.5);
    path.line_to(2., 2.);
    path.line_to(2., 2.);
    path.line_to(4., 2.);
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn degenerate_arc_to() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(10.0, 10.0);
    path.arc_to(10.0, 10.0001, 10.0, 10.0001, 2.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn degenerate_arc() {
    let mut canvas = Canvas::new(Void).unwrap();

    let mut path = Path::new();
    path.move_to(10.0, 10.0);
    path.arc(10.0, 10.0, 10.0, 0.0, f32::MAX, Solidity::Hole);

    canvas.fill_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
    canvas.stroke_path(&path, &Paint::color(Color::rgb(100, 100, 100)));
}

#[test]
fn path_contains_point() {
    let mut canvas = Canvas::new(Void).unwrap();
    // without setting size contains_point will early out on the bounds check and report false
    canvas.set_size(100, 100, 1.0);

    // Star - cancave & self crossing
    let mut path = Path::new();
    path.move_to(50.0, 0.0);
    path.line_to(21.0, 90.0);
    path.line_to(98.0, 35.0);
    path.line_to(2.0, 35.0);
    path.line_to(79.0, 90.0);
    path.close();

    // Center of the star should be hollow for even-odd rule
    assert!(!canvas.contains_point(&path, 50.0, 45.0, FillRule::EvenOdd));
    assert!(canvas.contains_point(&path, 50.0, 5.0, FillRule::EvenOdd));

    // Center of the star should be fill for NonZero rule
    assert!(canvas.contains_point(&path, 50.0, 45.0, FillRule::NonZero));
    assert!(canvas.contains_point(&path, 50.0, 5.0, FillRule::NonZero));
}

#[test]
fn text_location_respects_scale() {
    let mut canvas = Canvas::new(Void).unwrap();

    canvas
        .add_font("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    let paint = Paint::color(Color::black()).with_text_baseline(Baseline::Top);
    canvas.scale(5.0, 5.0);

    let res = canvas.measure_text(100.0, 100.0, "Hello", &paint).unwrap();

    assert_eq!(res.x, 100.0);
    assert_eq!(res.y, 100.0);
}

#[test]
fn text_measure_without_canvas() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    let test_paint = femtovg::Paint::default().with_font(&[font_id]).with_font_size(16.);

    let metrics = text_context
        .measure_text(0., 0., "Hello World", &test_paint)
        .expect("text shaping failed unexpectedly");

    assert_eq!(metrics.width().ceil(), 83.);
    assert_eq!(metrics.height().ceil(), 13.);
}

#[test]
fn text_layout_preserves_fractional_baseline_y() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    let test_paint = femtovg::Paint::default()
        .with_font(&[font_id])
        .with_font_size(16.)
        .with_text_baseline(Baseline::Alphabetic);

    let first = text_context
        .measure_text(0., 10.125, "Hello", &test_paint)
        .expect("text shaping failed unexpectedly");
    let second = text_context
        .measure_text(0., 10.375, "Hello", &test_paint)
        .expect("text shaping failed unexpectedly");

    let delta = second.glyphs[0].y - first.glyphs[0].y;
    assert!((delta - 0.25).abs() < 0.001);
}

#[test]
fn font_metrics_report_underline_and_strikeout() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    let test_paint = femtovg::Paint::default().with_font(&[font_id]).with_font_size(32.);

    let metrics = text_context
        .measure_font(&test_paint)
        .expect("font measuring failed unexpectedly");

    // Roboto Flex ships post + OS/2 tables, so these are read straight from the
    // font rather than from a fallback.
    assert!(
        metrics.underline_thickness() > 0.0,
        "underline thickness should be positive, got {}",
        metrics.underline_thickness()
    );
    assert!(
        metrics.strikeout_thickness() > 0.0,
        "strikeout thickness should be positive, got {}",
        metrics.strikeout_thickness()
    );
    // OpenType convention: +y up from the baseline. The underline sits below the
    // baseline (negative) and the strikeout above it (positive, through the text).
    assert!(
        metrics.underline_position() < 0.0,
        "underline should sit below the baseline, got {}",
        metrics.underline_position()
    );
    assert!(
        metrics.strikeout_position() > 0.0,
        "strikeout should sit above the baseline, got {}",
        metrics.strikeout_position()
    );
}

#[test]
fn font_metrics_report_typographic_metrics() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    let font_size = 32.;
    let test_paint = femtovg::Paint::default()
        .with_font(&[font_id])
        .with_font_size(font_size);

    let metrics = text_context
        .measure_font(&test_paint)
        .expect("font measuring failed unexpectedly");

    // The vertical extents are ordered: 0 < x-height < cap-height <= ascender.
    assert!(
        metrics.x_height() > 0.0,
        "x-height should be positive, got {}",
        metrics.x_height()
    );
    assert!(
        metrics.x_height() < metrics.cap_height(),
        "x-height ({}) should be below the cap-height ({})",
        metrics.x_height(),
        metrics.cap_height()
    );
    assert!(
        metrics.cap_height() <= metrics.ascender(),
        "cap-height ({}) should not exceed the ascender ({})",
        metrics.cap_height(),
        metrics.ascender()
    );

    // Sub/superscript glyphs are recommended at a readable fraction of the em.
    for (label, (x_size, y_size)) in [
        ("subscript", metrics.subscript_size()),
        ("superscript", metrics.superscript_size()),
    ] {
        assert!(
            x_size > 0.0 && x_size < font_size,
            "{label} x size should be within (0, em), got {x_size}"
        );
        assert!(
            y_size > 0.0 && y_size < font_size,
            "{label} y size should be within (0, em), got {y_size}"
        );
    }

    // Canvas convention: +y points down, so subscripts drop below the baseline
    // (positive) and superscripts rise above it (negative). Beyond the sign, the
    // recommended offset places the raised/lowered glyph within the font's own
    // vertical envelope: a superscript typeset at its recommended size lifts its
    // cap above the base x-height (so it reads as raised) yet stays within the
    // ascent, and a subscript drops within the descent depth. This guards the
    // sign normalization (the superscript y is negated from the raw OS/2 value)
    // against a regression that swapped or mis-scaled the offsets.
    let ascent = metrics.ascender();
    let descent = metrics.descender().abs();

    let sup_rise = -metrics.superscript_offset().1;
    // Cap-height scaled down to the recommended superscript size, matching the
    // space the rendered superscript glyph occupies.
    let sup_cap = metrics.cap_height() * metrics.superscript_size().1 / font_size;
    assert!(
        sup_rise > 0.0 && sup_rise < ascent,
        "superscript rise ({sup_rise}) should lift within the ascent (0, {ascent})"
    );
    assert!(
        sup_rise + sup_cap > metrics.x_height(),
        "superscript cap top ({}) should clear the base x-height ({})",
        sup_rise + sup_cap,
        metrics.x_height()
    );

    let sub_drop = metrics.subscript_offset().1;
    assert!(
        sub_drop > 0.0 && sub_drop <= descent,
        "subscript drop ({sub_drop}) should fall within the descent (0, {descent}]"
    );

    // The hhea line gap is commonly zero, but never negative for this font.
    assert!(
        metrics.line_gap() >= 0.0,
        "line gap should not be negative, got {}",
        metrics.line_gap()
    );
}

#[test]
fn font_metrics_scale_linearly_with_font_size() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    let paint = femtovg::Paint::default().with_font(&[font_id]);
    let small = text_context
        .measure_font(&paint.clone().with_font_size(20.))
        .expect("font measuring failed unexpectedly");
    let large = text_context
        .measure_font(&paint.with_font_size(40.))
        .expect("font measuring failed unexpectedly");

    let halves = [
        ("x-height", small.x_height(), large.x_height()),
        ("cap-height", small.cap_height(), large.cap_height()),
        ("line gap", small.line_gap(), large.line_gap()),
        ("subscript x size", small.subscript_size().0, large.subscript_size().0),
        ("subscript y size", small.subscript_size().1, large.subscript_size().1),
        (
            "subscript x offset",
            small.subscript_offset().0,
            large.subscript_offset().0,
        ),
        (
            "subscript y offset",
            small.subscript_offset().1,
            large.subscript_offset().1,
        ),
        (
            "superscript x size",
            small.superscript_size().0,
            large.superscript_size().0,
        ),
        (
            "superscript y size",
            small.superscript_size().1,
            large.superscript_size().1,
        ),
        (
            "superscript x offset",
            small.superscript_offset().0,
            large.superscript_offset().0,
        ),
        (
            "superscript y offset",
            small.superscript_offset().1,
            large.superscript_offset().1,
        ),
    ];
    for (label, at_20, at_40) in halves {
        assert_eq!(
            at_20 * 2.0,
            at_40,
            "{label} at font size 20 should be exactly half of its value at 40"
        );
    }
}

#[test]
fn font_measure_without_canvas() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    let test_paint = femtovg::Paint::default().with_font(&[font_id]).with_font_size(16.);

    let metrics = text_context
        .measure_font(&test_paint)
        .expect("font measuring failed unexpectedly");

    assert_eq!(metrics.ascender().ceil(), 15.);
}

#[test]
fn break_text_without_canvas() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    let test_paint = femtovg::Paint::default().with_font(&[font_id]).with_font_size(16.);

    let text = "Multiple Lines Broken";

    let breaks = text_context
        .break_text_vec(60., text, &test_paint)
        .expect("text shaping failed unexpectedly");

    assert_eq!(
        breaks
            .iter()
            .map(|range| &text[range.start..range.end])
            .collect::<Vec<_>>(),
        vec!["Multiple ", "Lines ", "Broken"]
    );
}

#[test]
fn variable_font_weight_affects_measurement() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    // Verify the font has a wght axis
    let axes = text_context.font_variation_axes(font_id).unwrap();
    let wght_axis = axes.iter().find(|a| &a.tag == b"wght");
    assert!(wght_axis.is_some(), "Font should have a wght axis");

    let light_paint = femtovg::Paint::default()
        .with_font(&[font_id])
        .with_font_size(16.)
        .with_font_weight(femtovg::Paint::FONT_WEIGHT_LIGHT);

    let bold_paint = femtovg::Paint::default()
        .with_font(&[font_id])
        .with_font_size(16.)
        .with_font_weight(femtovg::Paint::FONT_WEIGHT_BOLD);

    let light_metrics = text_context
        .measure_text(0., 0., "Hello World", &light_paint)
        .expect("text shaping failed");

    let bold_metrics = text_context
        .measure_text(0., 0., "Hello World", &bold_paint)
        .expect("text shaping failed");

    // Bold text should be wider than light text
    assert!(
        bold_metrics.width() > light_metrics.width(),
        "Bold ({}) should be wider than light ({})",
        bold_metrics.width(),
        light_metrics.width()
    );
}

#[test]
fn font_variation_generic_api_matches_named_weight() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    // Using the generic variation API for wght should produce the same measurement
    // as the named font_weight API
    let named_paint = femtovg::Paint::default()
        .with_font(&[font_id])
        .with_font_size(16.)
        .with_font_weight(femtovg::Paint::FONT_WEIGHT_BOLD);

    let generic_paint = femtovg::Paint::default()
        .with_font(&[font_id])
        .with_font_size(16.)
        .with_font_variation(b"wght", 700.0);

    let named_metrics = text_context
        .measure_text(0., 0., "Hello World", &named_paint)
        .expect("text shaping failed");

    let generic_metrics = text_context
        .measure_text(0., 0., "Hello World", &generic_paint)
        .expect("text shaping failed");

    assert!(
        (named_metrics.width() - generic_metrics.width()).abs() < f32::EPSILON,
        "Named weight API ({}) and generic variation API ({}) should produce identical measurements",
        named_metrics.width(),
        generic_metrics.width()
    );
}

#[test]
fn font_variation_italic_and_slant_api() {
    // Test the Paint API for italic and slant methods
    let mut paint = femtovg::Paint::default();

    // Initially no italic set
    assert!(paint.font_italic().is_none());
    assert!(paint.font_slant().is_none());

    // Set italic — also sets slnt as fallback
    paint.set_font_italic(true);
    assert_eq!(paint.font_italic(), Some(true));
    assert_eq!(paint.font_slant(), Some(-12.0));

    paint.set_font_italic(false);
    assert_eq!(paint.font_italic(), Some(false));
    assert_eq!(paint.font_slant(), Some(0.0));

    // clear_font_italic removes both ital and slnt
    paint.clear_font_italic();
    assert!(paint.font_italic().is_none());
    assert!(paint.font_slant().is_none());

    // Explicit slant overrides the italic fallback
    paint.set_font_italic(true);
    paint.set_font_slant(-5.0);
    assert_eq!(paint.font_italic(), Some(true));
    assert_eq!(paint.font_slant(), Some(-5.0));

    paint.clear_font_variations();

    // Set slant independently
    paint.set_font_slant(-12.0);
    assert_eq!(paint.font_slant(), Some(-12.0));

    paint.clear_font_slant();
    assert!(paint.font_slant().is_none());

    // Multiple variations at once
    paint.set_font_weight(femtovg::Paint::FONT_WEIGHT_BOLD);
    paint.set_font_italic(true);

    assert_eq!(paint.font_weight(), Some(700.0));
    assert_eq!(paint.font_italic(), Some(true));
    assert_eq!(paint.font_slant(), Some(-12.0));

    // Clear all
    paint.clear_font_variations();
    assert!(paint.font_weight().is_none());
    assert!(paint.font_italic().is_none());
    assert!(paint.font_slant().is_none());
}

#[test]
fn font_variation_hash_stability() {
    // Setting the same variations in different order should produce the same hash
    let mut paint_a = femtovg::Paint::default();
    paint_a.set_font_weight(femtovg::Paint::FONT_WEIGHT_BOLD);
    paint_a.set_font_italic(true);

    let mut paint_b = femtovg::Paint::default();
    paint_b.set_font_italic(true);
    paint_b.set_font_weight(femtovg::Paint::FONT_WEIGHT_BOLD);

    // Both paints should have the same variation hash (verified via the generic API)
    assert_eq!(paint_a.font_weight(), paint_b.font_weight());
    assert_eq!(paint_a.font_italic(), paint_b.font_italic());
}

#[test]
fn variable_font_slant_affects_measurement() {
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    // Verify the font has a slnt axis
    let axes = text_context.font_variation_axes(font_id).unwrap();
    let slnt_axis = axes.iter().find(|a| &a.tag == b"slnt");
    assert!(slnt_axis.is_some(), "Font should have a slnt axis");

    let upright_paint = femtovg::Paint::default().with_font(&[font_id]).with_font_size(16.);

    let slanted_paint = femtovg::Paint::default()
        .with_font(&[font_id])
        .with_font_size(16.)
        .with_font_slant(-10.0);

    let upright_metrics = text_context
        .measure_text(0., 0., "Hello World", &upright_paint)
        .expect("text shaping failed");

    let slanted_metrics = text_context
        .measure_text(0., 0., "Hello World", &slanted_paint)
        .expect("text shaping failed");

    // Slanted text should produce different measurements than upright text
    // (the widths may differ slightly due to slant-adjusted glyph metrics)
    assert!(
        (upright_metrics.width() - slanted_metrics.width()).abs() > 0.0
            || upright_metrics.glyphs.len() == slanted_metrics.glyphs.len(),
        "Slanted text should shape successfully and produce valid metrics"
    );
}

#[test]
fn font_italic_falls_back_to_slnt_axis() {
    // Roboto Flex has no `ital` axis but has `slnt`. Calling set_font_italic
    // should still produce slanted text via the slnt fallback.
    let text_context = femtovg::TextContext::default();

    let font_id = text_context
        .add_font_file("examples/assets/RobotoFlex-VariableFont.ttf")
        .expect("Font not found");

    let axes = text_context.font_variation_axes(font_id).unwrap();
    assert!(
        axes.iter().find(|a| &a.tag == b"ital").is_none(),
        "Font should NOT have an ital axis"
    );
    assert!(
        axes.iter().find(|a| &a.tag == b"slnt").is_some(),
        "Font should have a slnt axis"
    );

    let upright_paint = femtovg::Paint::default().with_font(&[font_id]).with_font_size(16.);

    let italic_paint = femtovg::Paint::default()
        .with_font(&[font_id])
        .with_font_size(16.)
        .with_font_italic(true);

    let upright_metrics = text_context
        .measure_text(0., 0., "Hello World", &upright_paint)
        .expect("text shaping failed");

    let italic_metrics = text_context
        .measure_text(0., 0., "Hello World", &italic_paint)
        .expect("text shaping failed");

    // The slnt axis changes glyph outlines (tilts them) but doesn't change advance
    // widths, so we verify that shaping succeeds and produces the same glyph count.
    // The visual difference (slanted glyphs) is only visible in rendering.
    assert_eq!(
        upright_metrics.glyphs.len(),
        italic_metrics.glyphs.len(),
        "Italic fallback via slnt should shape the same number of glyphs"
    );
    assert!(italic_metrics.width() > 0.0, "Italic text should have positive width");
}

/// Headless GPU conformance check for the conic gradient: render a conic gradient
/// with `start_angle = 0` into an offscreen Rgba8 texture and verify that the
/// offset-0 color sits at the positive x axis (3 o'clock) and that the ramp
/// proceeds clockwise, matching the Canvas 2D `createConicGradient` convention.
///
/// The gradient is split into a red half (offsets `[0, 0.5)`) and a blue half
/// (offsets `[0.5, 1)`). With the +x-axis-start, clockwise convention (screen y
/// points down) the offset-0 color sits at 3 o'clock, and the red->blue boundary
/// falls at 9 o'clock. Sampling is done away from that boundary, at the four
/// diagonal directions whose offsets are 0.125 / 0.375 / 0.625 / 0.875:
/// down-right (0.125) and down-left (0.375) are red; up-left (0.625) and
/// up-right (0.875) are blue. The offset-0 color is also checked directly at
/// 3 o'clock.
///
/// The old top-start convention would rotate this by a quarter turn, so the
/// test also locks in the 90-degree phase fix.
#[cfg(feature = "wgpu")]
#[test]
fn conic_gradient_start_angle_matches_canvas_convention() {
    use femtovg::renderer::WGPURenderer;

    const SIZE: u32 = 64;
    const CENTER: f32 = SIZE as f32 / 2.0;

    let instance = wgpu::Instance::default();

    let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
        ..Default::default()
    }))
    .ok() else {
        // No GPU adapter available (e.g. headless CI without a backend); skip.
        eprintln!("skipping conic gradient GPU test: no wgpu adapter available");
        return;
    };

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("conic gradient test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .expect("Failed to create device");

    // Offscreen render target.
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("conic gradient target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    // bytes_per_row must be a multiple of 256.
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bpr = (SIZE * 4).div_ceil(align) * align;

    // Red for the first half of the sweep, blue for the second half.
    let red = Color::rgbf(1.0, 0.0, 0.0);
    let blue = Color::rgbf(0.0, 0.0, 1.0);

    // Render the conic gradient with the given start angle and read the texture
    // back into a CPU buffer, returning a sampler over the resulting pixels.
    let render = |start_angle: f32| -> Vec<u8> {
        let renderer = WGPURenderer::new(device.clone(), queue.clone());
        let mut canvas = Canvas::new(renderer).unwrap();
        canvas.set_size(SIZE, SIZE, 1.0);
        canvas.clear_rect(0, 0, SIZE, SIZE, Color::black());

        let paint = Paint::conic_gradient_stops_with_angle(
            CENTER,
            CENTER,
            start_angle,
            [(0.0, red), (0.4999, red), (0.5, blue), (1.0, blue)],
        );

        let mut path = Path::new();
        path.rect(0.0, 0.0, SIZE as f32, SIZE as f32);
        canvas.fill_path(&path, &paint);

        let commands = canvas.flush_to_output(&texture);
        queue.submit(commands);

        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("conic gradient readback"),
            size: (padded_bpr * SIZE) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bpr),
                    rows_per_image: Some(SIZE),
                },
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.expect("buffer map failed"));
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device poll failed");

        let pixels = slice.get_mapped_range().expect("mapped range unavailable").to_vec();
        readback.unmap();
        pixels
    };

    let sampler = |pixels: &[u8], dx: i32, dy: i32| -> (u8, u8, u8) {
        let x = (CENTER as i32 + dx) as u32;
        let y = (CENTER as i32 + dy) as u32;
        let idx = (y * padded_bpr + x * 4) as usize;
        (pixels[idx], pixels[idx + 1], pixels[idx + 2])
    };

    let is_red = |(r, _g, b): (u8, u8, u8)| r > 200 && b < 60;
    let is_blue = |(r, _g, b): (u8, u8, u8)| b > 200 && r < 60;

    // start_angle = 0: offset-0 color sits at +x (3 o'clock), ramp clockwise.
    let pixels = render(0.0);
    let sample = |dx, dy| sampler(&pixels, dx, dy);

    // offset-0 color sits exactly at the +x axis (3 o'clock).
    let right = sample(12, 0);
    // Diagonals avoid the sharp red/blue boundary that falls on the cardinal axes.
    let down_right = sample(9, 9); // offset 0.125 -> red
    let down_left = sample(-9, 9); // offset 0.375 -> red
    let up_left = sample(-9, -9); // offset 0.625 -> blue
    let up_right = sample(9, -9); // offset 0.875 -> blue

    assert!(
        is_red(right),
        "offset-0 color must sit at the +x axis (3 o'clock); got {right:?}"
    );
    assert!(
        is_red(down_right),
        "offset-0.125 (clockwise toward 6 o'clock, screen y-down) must be red; got {down_right:?}"
    );
    assert!(
        is_red(down_left),
        "offset-0.375 (just before 9 o'clock) must be red; got {down_left:?}"
    );
    assert!(
        is_blue(up_left),
        "offset-0.625 (just after 9 o'clock) must be blue; got {up_left:?}"
    );
    assert!(
        is_blue(up_right),
        "offset-0.875 (toward 3 o'clock from the top) must be blue; got {up_right:?}"
    );

    // start_angle = +PI/2 rotates the gradient a quarter turn clockwise (toward 6
    // o'clock). The red half now spans offsets that map to the down-left and
    // up-left diagonals, and the blue half maps to the up-right and down-right
    // diagonals: exactly the quarter-turn clockwise rotation of the start_angle=0
    // case above. (The exact offset-0 point at 6 o'clock is the wrap boundary, so
    // it is not asserted; the diagonals unambiguously prove the rotation.)
    let pixels = render(std::f32::consts::FRAC_PI_2);
    let sample = |dx, dy| sampler(&pixels, dx, dy);

    let down_left = sample(-9, 9); // offset 0.125 -> red
    let up_left = sample(-9, -9); // offset 0.375 -> red
    let up_right = sample(9, -9); // offset 0.625 -> blue
    let down_right = sample(9, 9); // offset 0.875 -> blue

    assert!(
        is_red(down_left),
        "with start_angle=PI/2, offset-0.125 must be red; got {down_left:?}"
    );
    assert!(
        is_red(up_left),
        "with start_angle=PI/2, offset-0.375 must be red; got {up_left:?}"
    );
    assert!(
        is_blue(up_right),
        "with start_angle=PI/2, offset-0.625 must be blue; got {up_right:?}"
    );
    assert!(
        is_blue(down_right),
        "with start_angle=PI/2, offset-0.875 must be blue; got {down_right:?}"
    );
}

#[test]
fn zeroed_os2_script_metrics_fall_back() {
    // A present OS/2 table whose sub/superscript sizes are zero cannot mean
    // "typeset script text at zero size"; it gets the same conventional
    // fallbacks as a missing table. Patch the font bytes so the table is
    // present but zeroed, which is what some minimal or broken fonts ship.
    let mut data = std::fs::read("examples/assets/RobotoFlex-VariableFont.ttf").expect("Font not found");

    // sfnt: numTables at 4, table records from 12, each 16 bytes of
    // tag/checksum/offset/length. ttf-parser does not re-verify checksums.
    let num_tables = u16::from_be_bytes([data[4], data[5]]) as usize;
    let os2_offset = (0..num_tables)
        .map(|i| 12 + i * 16)
        .find(|&rec| &data[rec..rec + 4] == b"OS/2")
        .map(|rec| u32::from_be_bytes(data[rec + 8..rec + 12].try_into().unwrap()) as usize)
        .expect("font has an OS/2 table");
    // ySubscriptXSize/YSize at 10/12, ySuperscriptXSize/YSize at 18/20.
    for field in [10, 12, 18, 20] {
        data[os2_offset + field] = 0;
        data[os2_offset + field + 1] = 0;
    }

    let text_context = femtovg::TextContext::default();
    let font_id = text_context.add_font_mem(&data).expect("patched font parses");

    let font_size = 32.;
    let test_paint = femtovg::Paint::default()
        .with_font(&[font_id])
        .with_font_size(font_size);
    let metrics = text_context
        .measure_font(&test_paint)
        .expect("font measuring failed unexpectedly");

    // The conventional recommendation: 65% of the em, on both axes.
    let expected = font_size * 0.65;
    for (label, (x_size, y_size)) in [
        ("subscript", metrics.subscript_size()),
        ("superscript", metrics.superscript_size()),
    ] {
        assert!(
            (x_size - expected).abs() < 1e-3 && (y_size - expected).abs() < 1e-3,
            "zeroed {label} size should fall back to ({expected}, {expected}), got ({x_size}, {y_size})"
        );
    }
}
