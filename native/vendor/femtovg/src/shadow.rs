//! Drop shadows: the shadow state and the coverage pass that casts a shape's
//! or a layer's shadow.

use super::*;

impl<T> Canvas<T>
where
    T: Renderer,
{
    /// Sets the color of drop shadows drawn behind subsequent fills, strokes and text.
    ///
    /// This mirrors the Canvas 2D `shadowColor` attribute. The default is a fully
    /// transparent color (`rgba(0, 0, 0, 0)`), which disables shadows: when the
    /// shadow color is fully transparent no offscreen shadow pass is performed and
    /// drawing adds zero overhead. The shadow color's own alpha multiplies the
    /// shadow's coverage.
    ///
    /// # Performance
    ///
    /// Shadows are not cheap: every shadowed fill, stroke or text draw renders the
    /// shape's coverage into a transient offscreen image sized to its padded
    /// bounds, runs a two-pass Gaussian blur over it (when `shadowBlur` is
    /// non-zero), and composites the result — per draw, every frame. Prefer
    /// shadowing a few composed shapes over many small primitives, and if the
    /// same shadowed shape is drawn every frame, consider rendering it once into
    /// an [image](Self::create_image_empty) via
    /// [`set_render_target`](Self::set_render_target) and re-drawing that cached
    /// image instead. Setting a fully transparent shadow color restores the
    /// zero-overhead path.
    pub fn set_shadow_color(&mut self, color: Color) {
        self.state_mut().shadow_color = color;
    }

    /// Sets the blur radius applied to drop shadows.
    ///
    /// This mirrors the Canvas 2D `shadowBlur` attribute. Following the HTML
    /// drawing model, the shadow image is blurred with a Gaussian whose standard
    /// deviation is `shadowBlur / 2`, expressed in output (device) pixels. The
    /// default is `0` (no blur). Negative or non-finite values are ignored.
    ///
    /// One blur shader pass covers a standard deviation of 8 device pixels
    /// (`shadowBlur` 16; its kernel reach is bounded at +/-24 px, a GLES 2.0
    /// constraint on loop bounds). A larger blur runs as several passes that
    /// compose to the requested sigma, the way a filter chain's blur does
    /// (see [`filter_image_chain`](Self::filter_image_chain)), with the
    /// shadow's offscreen padded by the full reach, so `shadowBlur` 40 spreads
    /// like the browsers' sigma 20 rather than a sigma-8 one; the pass count
    /// grows with the square of the sigma, up to a sigma of 128 (`shadowBlur`
    /// 256), and each pass's kernel stops at 2.875 sigma, so the composed
    /// blur lands within 2 % of the requested sigma.
    pub fn set_shadow_blur(&mut self, blur: f32) {
        if blur.is_finite() && blur >= 0.0 {
            self.state_mut().shadow_blur = blur;
        }
    }

    /// Sets the drop shadow offset, in output (device) pixels.
    ///
    /// This mirrors the Canvas 2D `shadowOffsetX`/`shadowOffsetY` attributes.
    /// Positive `x` shifts the shadow right and positive `y` shifts it down. Per
    /// the spec the offset is *not* affected by the current transformation
    /// matrix: it keeps the same magnitude and direction relative to the shape
    /// regardless of scale or rotation. Non-finite values are ignored (the
    /// previous offset is preserved), matching the Canvas setter semantics used
    /// by `set_shadow_blur`. The default is `(0, 0)`.
    pub fn set_shadow_offset(&mut self, x: f32, y: f32) {
        if x.is_finite() && y.is_finite() {
            self.state_mut().shadow_offset = [x, y];
        }
    }

    /// Returns `true` when the current state would paint a visible drop shadow.
    ///
    /// Matching the Canvas spec, a shadow is only drawn when the shadow color is
    /// not fully transparent *and* at least one of the blur or offset components
    /// is non-zero. (An opaque shadow color with zero blur and zero offset would
    /// land exactly under the shape and contribute nothing, so the spec treats it
    /// as no shadow.) When this returns `false` the drawing entry points skip the
    /// whole offscreen shadow pass, so the common (no-shadow) case has zero added
    /// cost.
    pub(crate) fn shadow_enabled(&self) -> bool {
        let state = self.state();
        state.alpha > 0.0
            && state.shadow_color.a > 0.0
            && (state.shadow_blur != 0.0 || state.shadow_offset[0] != 0.0 || state.shadow_offset[1] != 0.0)
    }

    /// Returns `true` when the drop shadow for a shape with the given device-space
    /// `shape_bounds` could land on the render target once the (device-space)
    /// shadow offset and blur spread are taken into account.
    ///
    /// Drawing entry points must not cull a shadow on the shape's own bounds
    /// alone: a shape entirely off-screen can still cast a visible shadow when the
    /// offset and/or blur pull the shadow back onto the target. The blur spread is
    /// `ceil(3 * sigma)` (sigma = `shadowBlur / 2`), which covers >99.7% of the
    /// Gaussian — the same reach `render_shadow` uses to pad its offscreen image.
    pub(crate) fn shadow_could_be_visible(&self, shape_bounds: Bounds) -> bool {
        let state = self.state();
        let [ox, oy] = state.shadow_offset;
        let spread = (state.shadow_blur / 2.0 * 3.0).ceil();
        let minx = shape_bounds.minx + ox - spread;
        let miny = shape_bounds.miny + oy - spread;
        let maxx = shape_bounds.maxx + ox + spread;
        let maxy = shape_bounds.maxy + oy + spread;
        maxx >= 0.0 && minx <= self.width() as f32 && maxy >= 0.0 && miny <= self.height() as f32
    }

    /// Renders a drop shadow for a shape whose device-space bounding box is
    /// `shape_bounds`, using the supplied closure to draw the shape's coverage.
    ///
    /// Per the Canvas drawing model the shadow is built from the *alpha of the
    /// actually-rendered source*, not from a forced-opaque tint: a semi-transparent
    /// fill, or a gradient/image with transparent texels, must cast a
    /// correspondingly weaker shadow. To achieve this the closure draws the real
    /// source (its actual paint and per-pixel alpha) into a transient offscreen
    /// image, then opaque white is composited over it with
    /// `CompositeOperation::SourceIn`, which leaves the source's alpha in every
    /// channel. That image is then Gaussian-blurred (standard deviation
    /// `shadowBlur / 2`) through the chain planner's passes ([`blur_passes`]):
    /// a sigma above the shader's per-pass bound runs down the pyramid and
    /// back, and the result is composited into the current render target,
    /// translated by the device-space shadow offset, drawn *under* the actual
    /// shape and tinted by `shadowColor` as it lands, so each pixel gets
    /// `shadowColor.rgb` at alpha `blurred alpha * shadowColor.a`. The current
    /// scissor, global alpha and composite operation are honored when
    /// compositing.
    ///
    /// The colour goes on after the blur, rounded once into the target, in
    /// Canvas 2D's order (copy the alpha, blur it, then colour it), as
    /// Firefox applies it too. Coloured first, an 8-bit store
    /// holds the colour premultiplied by the shadow's alpha at every blur
    /// pass. That is a fraction of a step for a near-black colour, the more so
    /// in linearRGB, where `#0b0d10` at alpha 0.6 is 0.5 to 0.8 of one. Each
    /// pass rounds it to a whole step, and unpremultiplied the shadow comes out
    /// up to twice its colour.
    ///
    /// `draw_coverage` is expected to issue the shape's normal draw command(s) with
    /// its real paint; the canvas transform in effect during the call already maps
    /// the shape's device-space coordinates into the offscreen image.
    pub(crate) fn render_shadow(&mut self, shape_bounds: Bounds, draw_coverage: impl FnOnce(&mut Self)) {
        // Degenerate / off-screen bounds: nothing to cast a shadow from.
        if shape_bounds.maxx <= shape_bounds.minx || shape_bounds.maxy <= shape_bounds.miny {
            return;
        }
        // A draw whose commands are suppressed - past MAX_STATE_DEPTH, or in
        // a discarded pass-through layer - loses its composite too: no
        // coverage pass into a store nothing reads.
        if self.commands_suppressed() {
            return;
        }

        let state = *self.state();
        let shadow_color = state.shadow_color;

        // Standard deviation in device pixels (HTML drawing model: sigma = blur/2).
        let sigma = state.shadow_blur / 2.0;

        // The shadow offset is expressed in output (device) pixels and, per the
        // Canvas spec, is NOT affected by the current transformation matrix: it
        // keeps the same magnitude and direction relative to the shape under any
        // scale or rotation. Apply the raw components directly when positioning
        // the blurred shadow (matching WebKit: "canvas shadows must not be
        // affected by any transformation and keep the same offset relative to the
        // shape").
        let [txx, txy] = state.shadow_offset;

        // Pad the offscreen image for the blur kernel reach (~3 sigma covers
        // >99.7% of the Gaussian) plus a fringe pixel for antialiased edges.
        // The reach is the true sigma's: the blur below runs as as many
        // passes as it takes to compose to it.
        let pad = blur_reach([[sigma; 2]]).map_or(0.0, |reach| reach[0]) + FRINGE_PAD;
        // Bounded like a layer's: a shadow whose padded coverage would pass
        // the texture limit keeps its coverage and loses reach at the edge.
        let pad = bounded_pad(
            pad,
            (shape_bounds.maxx - shape_bounds.minx).max(shape_bounds.maxy - shape_bounds.miny),
            self.renderer.max_texture_size(),
            transient::SHADOW_GRANULARITY,
        );

        // Coverage is rendered at the shape's own location; the offset is applied
        // later when compositing, so the offscreen only needs to bound the shape.
        let plan = StoreSpan::padded(
            shape_bounds.minx,
            shape_bounds.miny,
            shape_bounds.maxx,
            shape_bounds.maxy,
            [pad; 2],
        )
        .store(transient::SHADOW_GRANULARITY);
        let limit = self.renderer.max_texture_size();
        if !plan.fits(limit) {
            return;
        }
        let (minx, miny) = plan.origin;
        let (width, height) = (plan.width, plan.height);

        let blur_plan = (sigma >= 0.01).then(|| blur_passes(sigma, sigma));
        let work = blur_plan
            .as_deref()
            .map_or(0, |passes| filter_work(passes, width, height));
        if !self.reserve_filter_work(work) {
            return;
        }

        // Offscreen render targets store premultiplied-alpha results, so flag the
        // images as PREMULTIPLIED. Otherwise the image-sampling shader would
        // re-premultiply on composite (multiplying rgb by alpha a second time),
        // darkening partially-transparent shadow texels — which the source-alpha
        // shadow now produces wherever the source is semi-transparent or
        // antialiased.
        //
        // Image render targets store their content vertically flipped in texture
        // space: both backends keep the GL FBO convention where canvas y = 0 lands
        // on the *last* texture row (the wgpu backend's texture-target vertex
        // stage reproduces it deliberately, and the glyph atlas pre-flips its
        // rasterization coordinates to compensate). FLIP_Y declares that
        // orientation so the composite below samples the coverage upright;
        // without it the shadow is mirrored about its rect's horizontal midline.
        // A blur pass keeps that orientation (its two draws flip twice); each
        // identity pass of a pyramid turns it over once, so the blurred image
        // declares FLIP_Y only when the plan leaves it stored that way.
        let image_flags = ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y;
        // Both come from the transient pool: past the budget the shadow is
        // skipped rather than allocated, like a layer degrading.
        let Ok(coverage_image) = self.acquire_transient_image(width, height, image_flags) else {
            self.refund_filter_work(work);
            return;
        };
        // The blur kernel divides by sigma, so a zero (or sub-pixel) blur skips
        // the filter pass entirely — and with it the second offscreen image.
        let (blurred_image, scratch) = match &blur_plan {
            Some(passes) => {
                let blurred_flags = if plan_stores_flipped(passes, true) {
                    image_flags
                } else {
                    ImageFlags::PREMULTIPLIED
                };
                match self.acquire_transient_image(width, height, blurred_flags) {
                    Ok(image) => match self.acquire_filter_scratches(width, height, passes, 1, 0) {
                        Ok(scratch) => (Some(image), Some(scratch)),
                        Err(_) => {
                            self.rollback_transient_image(image);
                            self.rollback_transient_image(coverage_image);
                            self.refund_filter_work(work);
                            return;
                        }
                    },
                    Err(_) => {
                        self.rollback_transient_image(coverage_image);
                        self.refund_filter_work(work);
                        return;
                    }
                }
            }
            None => (None, None),
        };

        // The coverage pass: the real source (its actual paint and per-pixel
        // alpha) drawn into the coverage image, in device space translated so
        // the padded bbox origin maps to (0, 0), then recolored opaque white
        // under SourceIn so every channel carries source.alpha - a transparent
        // source casts nothing, a half-alpha source a half-strength shadow.
        // Then the blur: the planner's passes from the coverage image into the
        // blurred one, through the scratches reserved for them; a sharp shadow
        // composites the coverage directly. The colour comes in at the
        // composite below.
        let mut coverage_transform = Transform2D::translation(-minx, -miny);
        coverage_transform.premultiply(&state.transform);
        let source_image = self.offscreen_pass(RenderTarget::Image(coverage_image), coverage_transform, |canvas| {
            canvas.clear_rect(0, 0, width as u32, height as u32, Color::rgbaf(0.0, 0.0, 0.0, 0.0));
            draw_coverage(canvas);
            canvas.state_mut().composite_operation = CompositeOperationState::new(CompositeOperation::SourceIn);
            canvas.fill_device_rect(
                0.0,
                0.0,
                width as f32,
                height as f32,
                &PaintFlavor::Color(Color::white()),
            );
            let (Some(blurred_image), Some(scratch)) = (blurred_image, scratch) else {
                return coverage_image;
            };
            let passes = blur_plan.as_deref().expect("a blurred image has a blur plan");
            canvas.run_filter_passes(blurred_image, passes, coverage_image, scratch, true, (0.0, 0.0));
            blurred_image
        });

        let dst_x = minx + txx;
        let dst_y = miny + txy;

        // The shadow image carries the blurred source alpha in every channel
        // (white, premultiplied); the tint, premultiplied by the paint, scales
        // it to shadowColor at shadowColor.a times the current global alpha,
        // computed per pixel and rounded once into the target.
        let tint = Color::rgbaf(
            shadow_color.r,
            shadow_color.g,
            shadow_color.b,
            shadow_color.a * state.alpha,
        );
        let mut shadow_paint = Paint::image_tint(source_image, dst_x, dst_y, width as f32, height as f32, 0.0, tint);
        shadow_paint.set_anti_alias(false);

        // Composite in plain device space (identity transform) at the offset
        // position, honoring the caller's scissor and composite operation.
        // A shadow casts no shadow of its own: mute the shadow state around
        // the blit.
        self.state_mut().shadow_color = Color::rgbaf(0.0, 0.0, 0.0, 0.0);
        self.fill_device_rect(dst_x, dst_y, width as f32, height as f32, &shadow_paint.flavor);
        self.state_mut().shadow_color = shadow_color;

        // The composite that reads them is recorded; the next shadow of this
        // size draws into the same images (the chain released its scratches).
        self.release_transient_image(coverage_image);
        if let Some(blurred_image) = blurred_image {
            self.release_transient_image(blurred_image);
        }
    }
}

/// The Canvas 2D shadow attributes must start at their spec-mandated defaults:
/// a fully transparent shadow color, zero blur and zero offset.
#[test]
fn shadow_attribute_defaults_match_spec() {
    let canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    let state = canvas.state();

    assert_eq!(state.shadow_color, Color::rgbaf(0.0, 0.0, 0.0, 0.0));
    assert_eq!(state.shadow_blur, 0.0);
    assert_eq!(state.shadow_offset, [0.0, 0.0]);
    // Transparent shadow color disables shadows entirely.
    assert!(!canvas.shadow_enabled());

    // Per the enable rule, even an opaque shadow color stays disabled while blur
    // and offset are both zero (the shadow would land exactly under the shape).
    let mut canvas = canvas;
    canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
    assert!(
        !canvas.shadow_enabled(),
        "opaque color alone (zero blur, zero offset) must not enable a shadow"
    );
    canvas.set_shadow_offset(1.0, 0.0);
    assert!(
        canvas.shadow_enabled(),
        "a non-zero offset with an opaque color must enable the shadow"
    );
}

/// `set_shadow_blur` ignores negative and non-finite values, matching the Canvas
/// spec ("on setting, if the value is negative, infinite, or NaN, it must be
/// ignored").
#[test]
fn shadow_blur_rejects_invalid_values() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();

    canvas.set_shadow_blur(4.0);
    assert_eq!(canvas.state().shadow_blur, 4.0);

    canvas.set_shadow_blur(-1.0);
    assert_eq!(canvas.state().shadow_blur, 4.0, "negative blur must be ignored");

    canvas.set_shadow_blur(f32::NAN);
    assert_eq!(canvas.state().shadow_blur, 4.0, "NaN blur must be ignored");

    canvas.set_shadow_blur(f32::INFINITY);
    assert_eq!(canvas.state().shadow_blur, 4.0, "infinite blur must be ignored");
}

/// `set_shadow_offset` ignores non-finite values, preserving the previous offset.
/// This matches the Canvas setter semantics already used by `set_shadow_blur` and
/// keeps NaN/inf out of the offscreen geometry.
#[test]
fn shadow_offset_rejects_non_finite_values() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();

    canvas.set_shadow_offset(10.0, -5.0);
    assert_eq!(canvas.state().shadow_offset, [10.0, -5.0]);

    canvas.set_shadow_offset(f32::NAN, 7.0);
    assert_eq!(
        canvas.state().shadow_offset,
        [10.0, -5.0],
        "NaN x must be ignored, previous offset preserved"
    );

    canvas.set_shadow_offset(3.0, f32::INFINITY);
    assert_eq!(
        canvas.state().shadow_offset,
        [10.0, -5.0],
        "infinite y must be ignored, previous offset preserved"
    );

    canvas.set_shadow_offset(f32::NEG_INFINITY, f32::NAN);
    assert_eq!(
        canvas.state().shadow_offset,
        [10.0, -5.0],
        "non-finite components must be ignored, previous offset preserved"
    );

    // A subsequent finite update still applies.
    canvas.set_shadow_offset(2.0, 4.0);
    assert_eq!(canvas.state().shadow_offset, [2.0, 4.0]);
}

/// Per the Canvas spec a shadow is painted only when the shadow color is
/// non-transparent AND at least one of blur, offsetX or offsetY is non-zero. An
/// opaque shadow color with zero blur and zero offset must therefore emit no
/// offscreen shadow pass; flipping on a non-zero offset *or* a non-zero blur must
/// re-enable it.
#[test]
fn shadow_enable_rule_requires_blur_or_offset() {
    use renderer::CommandType;

    let run = |configure: &dyn Fn(&mut Canvas<RecordingRenderer>)| -> bool {
        let renderer = RecordingRenderer::default();
        let recorded = renderer.last_commands.clone();
        let mut canvas = Canvas::new(renderer).unwrap();
        canvas.set_size(100, 100, 1.0);
        configure(&mut canvas);

        let mut path = Path::new();
        path.rect(10.0, 10.0, 30.0, 30.0);
        canvas.fill_path(&path, &Paint::color(Color::rgb(255, 0, 0)));
        canvas.flush_to_output(());

        let commands = recorded.borrow();
        commands
            .iter()
            .any(|c| matches!(c.cmd_type, CommandType::SetRenderTarget(RenderTarget::Image(_))))
    };

    // Opaque color, zero blur, zero offset: no shadow.
    assert!(
        !run(&|canvas| {
            canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
            canvas.set_shadow_blur(0.0);
            canvas.set_shadow_offset(0.0, 0.0);
        }),
        "opaque color with zero blur and zero offset must not emit a shadow pass"
    );

    // A non-zero offsetX re-enables the shadow.
    assert!(
        run(&|canvas| {
            canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
            canvas.set_shadow_offset(5.0, 0.0);
        }),
        "a non-zero offset must re-enable the shadow"
    );

    // A non-zero offsetY re-enables the shadow.
    assert!(
        run(&|canvas| {
            canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
            canvas.set_shadow_offset(0.0, 5.0);
        }),
        "a non-zero offsetY must re-enable the shadow"
    );

    // A non-zero blur re-enables the shadow.
    assert!(
        run(&|canvas| {
            canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
            canvas.set_shadow_blur(4.0);
        }),
        "a non-zero blur must re-enable the shadow"
    );

    // Non-zero blur/offset but transparent color stays disabled.
    assert!(
        !run(&|canvas| {
            canvas.set_shadow_color(Color::rgba(0, 0, 0, 0));
            canvas.set_shadow_blur(4.0);
            canvas.set_shadow_offset(5.0, 5.0);
        }),
        "transparent shadow color must keep the shadow disabled"
    );
}

/// Shadow attributes are part of the drawing state and must be stacked by
/// save()/restore() like every other state member.
#[test]
fn shadow_state_is_saved_and_restored() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();

    canvas.set_shadow_color(Color::rgba(10, 20, 30, 40));
    canvas.set_shadow_blur(5.0);
    canvas.set_shadow_offset(3.0, 7.0);

    canvas.save();
    canvas.set_shadow_color(Color::rgba(99, 99, 99, 99));
    canvas.set_shadow_blur(11.0);
    canvas.set_shadow_offset(-1.0, -2.0);
    assert_eq!(canvas.state().shadow_blur, 11.0);
    canvas.restore();

    assert_eq!(canvas.state().shadow_color, Color::rgba(10, 20, 30, 40));
    assert_eq!(canvas.state().shadow_blur, 5.0);
    assert_eq!(canvas.state().shadow_offset, [3.0, 7.0]);
}

/// With a transparent shadow color (the default), filling a path must NOT emit
/// any offscreen shadow work: no SetRenderTarget and no RenderFilteredImage
/// commands, just the plain fill. This guards the "zero added overhead" rule.
#[test]
fn transparent_shadow_emits_no_offscreen_work() {
    use renderer::CommandType;

    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);

    // Shadow color left at its transparent default.
    canvas.set_shadow_blur(10.0);
    canvas.set_shadow_offset(5.0, 5.0);

    let mut path = Path::new();
    path.rect(10.0, 10.0, 30.0, 30.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(255, 0, 0)));
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c.cmd_type, CommandType::RenderFilteredImage { .. })),
        "transparent shadow must not run the blur filter"
    );
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c.cmd_type, CommandType::SetRenderTarget(RenderTarget::Image(_)))),
        "transparent shadow must not allocate an offscreen render target"
    );
}

/// With an opaque shadow color and a non-zero blur, filling a path must emit the
/// offscreen shadow pass: render the coverage into an image target and run the
/// Gaussian blur filter before the final fill.
#[test]
fn opaque_shadow_emits_offscreen_blur_pass() {
    use renderer::CommandType;

    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);

    canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
    canvas.set_shadow_blur(6.0);
    canvas.set_shadow_offset(4.0, 4.0);

    let mut path = Path::new();
    path.rect(10.0, 10.0, 30.0, 30.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(255, 0, 0)));
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    let filtered = commands.iter().find_map(|c| match c.cmd_type {
        CommandType::RenderFilteredImage { filter, .. } => Some(filter),
        _ => None,
    });

    match filtered {
        Some(ImageFilter::GaussianBlur { sigma_x, sigma_y }) => {
            // HTML drawing model: sigma == shadowBlur / 2, on both axes.
            assert_eq!(sigma_x, sigma_y);
            assert!(
                (sigma_x - 3.0).abs() < 1e-4,
                "expected sigma 3.0 for blur 6.0, got {sigma_x}"
            );
        }
        Some(other) => panic!("opaque shadow must run the Gaussian blur filter, got {other:?}"),
        None => panic!("opaque shadow must run the Gaussian blur filter"),
    }

    assert!(
        commands
            .iter()
            .any(|c| matches!(c.cmd_type, CommandType::SetRenderTarget(RenderTarget::Image(_)))),
        "opaque shadow must render coverage into an offscreen image target"
    );
}

#[test]
fn an_unrepresentably_large_shadow_is_skipped_without_overflow() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    canvas.set_shadow_color(Color::black());
    canvas.set_shadow_blur(1.0);
    let extent = f32::MAX / 2.0;
    canvas.render_shadow(
        Bounds {
            minx: -extent,
            miny: 0.0,
            maxx: extent,
            maxy: 1.0,
        },
        |_| panic!("an oversized shadow must be rejected before drawing"),
    );
    assert!(canvas.transients.images.is_empty());
}

/// Shadow coverage rounds to 8 px, not the layers' 64: a 20 px shadowed
/// shape under a 2 px blur takes a 40 x 40 store, not 64 x 64.
#[test]
fn shadow_stores_round_to_eight_pixels() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(200, 200, 1.0);
    canvas.set_shadow_color(Color::rgba(0, 0, 0, 128));
    canvas.set_shadow_blur(4.0); // sigma 2: pad 8 each side
    let mut path = Path::new();
    path.rect(40.0, 40.0, 20.0, 20.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(200, 0, 0)));
    let info = canvas.images.info(canvas.transients.images[0]).unwrap();
    assert_eq!((info.width(), info.height()), (40, 40));
}

/// Shadows draw through the pool too: the coverage and blurred images of one
/// shadow serve the next shadow of the same size.
#[test]
fn shadow_passes_reuse_their_images() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(256, 256, 1.0);
    canvas.set_shadow_color(Color::rgba(0, 0, 0, 128));
    canvas.set_shadow_blur(4.0);
    let mut path = Path::new();
    path.rect(40.0, 40.0, 100.0, 60.0);
    for _ in 0..5 {
        canvas.fill_path(&path, &Paint::color(Color::rgb(200, 0, 0)));
    }
    assert_eq!(
        canvas.transients.images.len(),
        3,
        "five same-sized shadows allocate coverage, blur and horizontal scratch images"
    );
    assert_eq!(canvas.transients.free.len(), 3);
    canvas.flush_to_output(());
    assert_eq!(canvas.transients.images.len(), 0);
}

/// Past the depth limit the shape's own draw is dropped, and so is the
/// composite its shadow would feed: the coverage pass is skipped outright,
/// target switches and blur passes included, rather than recorded into a
/// store nothing reads.
#[test]
fn a_shadowed_draw_past_the_depth_limit_records_nothing() {
    use renderer::CommandType;
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
    canvas.set_shadow_blur(6.0);
    canvas.set_shadow_offset(4.0, 4.0);
    while canvas.state_stack.len() < MAX_STATE_DEPTH {
        canvas.save();
    }
    canvas.save();
    assert!(canvas.saturated());
    let mut rect = Path::new();
    rect.rect(10.0, 10.0, 30.0, 30.0);
    canvas.fill_path(&rect, &Paint::color(Color::rgb(255, 0, 0)));
    canvas.flush_to_output(());
    let commands = recorded.borrow();
    assert!(
        !commands.iter().any(|c| matches!(
            c.cmd_type,
            CommandType::SetRenderTarget(RenderTarget::Image(_)) | CommandType::RenderFilteredImage { .. }
        )),
        "a suppressed shadow acquires no store and runs no pass"
    );
}

/// At exactly the limit nothing is suppressed, and the shadow's own save()
/// used to be what saturated the stack: its coverage was dropped while its
/// composite was not, painting a stale store. The pass runs outside the
/// stack now, so the stream is the one the same draw records at the root.
#[test]
fn a_shadowed_draw_at_the_depth_limit_casts_as_at_the_root() {
    let stream = |depth: usize| {
        let renderer = RecordingRenderer::default();
        let recorded = renderer.last_commands.clone();
        let mut canvas = Canvas::new(renderer).unwrap();
        canvas.set_size(64, 64, 1.0);
        canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
        canvas.set_shadow_blur(6.0);
        canvas.set_shadow_offset(4.0, 4.0);
        while canvas.state_stack.len() < depth {
            canvas.save();
        }
        assert!(!canvas.saturated());
        let mut rect = Path::new();
        rect.rect(10.0, 10.0, 30.0, 30.0);
        canvas.fill_path(&rect, &Paint::color(Color::rgb(255, 0, 0)));
        canvas.flush_to_output(());
        let kinds: Vec<_> = recorded
            .borrow()
            .iter()
            .map(|c| std::mem::discriminant(&c.cmd_type))
            .collect();
        kinds
    };
    let at_limit = stream(MAX_STATE_DEPTH);
    assert!(
        at_limit.len() > 2,
        "the coverage, its blur and the composite are all recorded"
    );
    assert_eq!(at_limit, stream(1));
}
