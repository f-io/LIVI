//! Headless GPU render tests for Canvas 2D drop shadows.
//!
//! These exercise the actual pixel behavior of the shadow pipeline on the wgpu
//! backend: an opaque shadow color must paint shadow-colored pixels offset from
//! the shape, a transparent shadow color must paint nothing, the shadow color's
//! alpha must be respected, and blur must spread coverage past the shape edge.
//!
//! All tests gracefully skip (return early) when no GPU adapter is available so
//! they don't fail on backend-less CI.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, Paint, Path, RenderTarget};

const W: u32 = 120;
const H: u32 = 120;

/// Lazily create a headless wgpu device/queue. Returns `None` when no adapter is
/// available (e.g. backend-less CI), so the caller can skip the test.
fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::default();

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
        ..Default::default()
    }))
    .ok()?;

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("femtovg shadow test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
    }))
    .ok()?;

    Some((device, queue))
}

/// Render `draw` into an offscreen RGBA8 texture and read the pixels back.
/// Returns a row-major buffer of `[r, g, b, a]` per pixel.
fn render_to_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    draw: impl FnOnce(&mut Canvas<WGPURenderer>),
) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shadow test target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    let renderer = WGPURenderer::new(device.clone(), queue.clone());
    let mut canvas = Canvas::new(renderer).expect("failed to create canvas");
    canvas.set_size(W, H, 1.0);
    canvas.clear_rect(0, 0, W, H, Color::rgba(0, 0, 0, 0));

    draw(&mut canvas);

    let commands = canvas.flush_to_output(&target);
    queue.submit(commands);

    // Copy the target texture into a readback buffer. bytes_per_row must be a
    // multiple of 256.
    let unpadded_bytes_per_row = W * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shadow test readback"),
        size: (padded_bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();

    let mapped = slice.get_mapped_range().expect("mapped readback range");
    let mut pixels = vec![0u8; (unpadded_bytes_per_row * H) as usize];
    for row in 0..H as usize {
        let src = row * padded_bytes_per_row as usize;
        let dst = row * unpadded_bytes_per_row as usize;
        pixels[dst..dst + unpadded_bytes_per_row as usize]
            .copy_from_slice(&mapped[src..src + unpadded_bytes_per_row as usize]);
    }
    drop(mapped);
    readback.unmap();

    pixels
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

/// A filled red rect with an opaque green shadow offset down-right must produce
/// green pixels at the offset location, and the shadow must vanish when the
/// shadow color is transparent.
#[test]
fn opaque_shadow_paints_offset_pixels() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };

    // Rect at (20,20) size 30x30. Shadow offset by (+30, +30), no blur.
    let mut rect = Path::new();
    rect.rect(20.0, 20.0, 30.0, 30.0);

    let with_shadow = render_to_pixels(&device, &queue, |canvas| {
        canvas.set_shadow_color(Color::rgb(0, 255, 0));
        canvas.set_shadow_offset(30.0, 30.0);
        canvas.fill_path(&rect, &Paint::color(Color::rgb(255, 0, 0)));
    });

    // Sample inside the shadow region but outside the shape: the rect occupies
    // x,y in [20,50); the shadow is shifted to [50,80). Point (65,65) is shadow
    // only.
    let shadow_px = pixel(&with_shadow, 65, 65);
    assert!(
        shadow_px[1] > 180 && shadow_px[0] < 80 && shadow_px[3] > 180,
        "expected opaque green shadow at (65,65), got {shadow_px:?}"
    );

    // The shape itself is still red.
    let shape_px = pixel(&with_shadow, 35, 35);
    assert!(
        shape_px[0] > 180 && shape_px[1] < 80,
        "expected red shape at (35,35), got {shape_px:?}"
    );

    // With a transparent shadow color, the same offset paints nothing there.
    let no_shadow = render_to_pixels(&device, &queue, |canvas| {
        canvas.set_shadow_color(Color::rgba(0, 255, 0, 0));
        canvas.set_shadow_offset(30.0, 30.0);
        canvas.fill_path(&rect, &Paint::color(Color::rgb(255, 0, 0)));
    });
    let empty_px = pixel(&no_shadow, 65, 65);
    assert!(
        empty_px[3] < 16,
        "transparent shadow color must leave (65,65) empty, got {empty_px:?}"
    );
}

/// The shadow color's alpha must modulate the painted shadow: a half-alpha shadow
/// produces a partially transparent shadow region.
#[test]
fn shadow_color_alpha_is_respected() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };

    let mut rect = Path::new();
    rect.rect(20.0, 20.0, 30.0, 30.0);

    let pixels = render_to_pixels(&device, &queue, |canvas| {
        canvas.set_shadow_color(Color::rgba(0, 0, 255, 128));
        canvas.set_shadow_offset(30.0, 30.0);
        canvas.fill_path(&rect, &Paint::color(Color::rgb(255, 0, 0)));
    });

    // Onto a transparent background, a half-alpha shadow yields ~50% alpha.
    let px = pixel(&pixels, 65, 65);
    assert!(
        px[3] > 90 && px[3] < 170,
        "expected ~half alpha shadow at (65,65), got {px:?}"
    );
    assert!(px[2] > 100, "shadow should carry blue channel, got {px:?}");
}

/// Blur must spread the shadow's coverage beyond the shape's hard edge: with a
/// zero offset and a blur, pixels just outside the shape rectangle pick up
/// shadow coverage that a sharp (unblurred) shadow would not reach.
#[test]
fn blur_spreads_coverage_beyond_edge() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };

    let mut rect = Path::new();
    rect.rect(40.0, 40.0, 40.0, 40.0); // occupies [40,80)

    // Sample a point a few pixels outside the right edge of the rect.
    let probe = (86u32, 60u32);

    let sharp = render_to_pixels(&device, &queue, |canvas| {
        canvas.set_shadow_color(Color::rgb(0, 0, 0));
        canvas.set_shadow_blur(0.0);
        canvas.fill_path(&rect, &Paint::color(Color::rgb(255, 0, 0)));
    });
    let sharp_px = pixel(&sharp, probe.0, probe.1);

    let blurred = render_to_pixels(&device, &queue, |canvas| {
        canvas.set_shadow_color(Color::rgb(0, 0, 0));
        canvas.set_shadow_blur(12.0);
        canvas.fill_path(&rect, &Paint::color(Color::rgb(255, 0, 0)));
    });
    let blurred_px = pixel(&blurred, probe.0, probe.1);

    // The sharp shadow lands exactly under the shape, so 6px outside the edge it
    // contributes nothing; the blurred shadow spreads coverage out there.
    assert!(
        sharp_px[3] < 16,
        "unblurred zero-offset shadow must not reach outside the shape, got {sharp_px:?}"
    );
    assert!(
        blurred_px[3] > sharp_px[3] + 16,
        "blur must spread coverage past the edge (sharp a={}, blurred a={})",
        sharp_px[3],
        blurred_px[3]
    );
}

/// Drop shadows must also render under offscreen image render targets, not just
/// the screen. This also confirms shadows compose correctly when the current
/// render target is an image.
#[test]
fn shadow_renders_into_image_target() {
    use femtovg::{ImageFlags, PixelFormat};

    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };

    // We render into an image target inside the draw closure, then blit it to the
    // screen so render_to_pixels can read it back.
    let mut rect = Path::new();
    rect.rect(20.0, 20.0, 30.0, 30.0);

    let pixels = render_to_pixels(&device, &queue, |canvas| {
        let img = canvas
            .create_image_empty(W as usize, H as usize, PixelFormat::Rgba8, ImageFlags::empty())
            .unwrap();
        canvas.set_render_target(RenderTarget::Image(img));
        canvas.clear_rect(0, 0, W, H, Color::rgba(0, 0, 0, 0));
        canvas.set_shadow_color(Color::rgb(0, 255, 0));
        canvas.set_shadow_offset(30.0, 30.0);
        canvas.fill_path(&rect, &Paint::color(Color::rgb(255, 0, 0)));

        canvas.set_render_target(RenderTarget::Screen);
        let mut full = Path::new();
        full.rect(0.0, 0.0, W as f32, H as f32);
        canvas.fill_path(&full, &Paint::image(img, 0.0, 0.0, W as f32, H as f32, 0.0, 1.0));
        // `img` is referenced by deferred commands until flush; the canvas frees it
        // on drop.
    });

    let shadow_px = pixel(&pixels, 65, 65);
    assert!(
        shadow_px[1] > 150 && shadow_px[3] > 150,
        "expected green shadow drawn via image target at (65,65), got {shadow_px:?}"
    );
}

/// Filled text must also cast a drop shadow. Render a glyph-heavy string with a
/// large offset and assert that shadow-colored pixels appear in the offset band
/// (where the glyphs themselves are not), and that nothing appears there once the
/// shadow color is made transparent.
#[test]
#[cfg(feature = "textlayout")]
fn text_casts_drop_shadow() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };

    let font = include_bytes!("../examples/assets/amiri-regular.ttf");

    // Count opaque, blue-tinted shadow pixels in a band offset from the text.
    let count_blue = |with_shadow: bool| -> usize {
        let pixels = render_to_pixels(&device, &queue, |canvas| {
            let id = canvas.add_font_mem(font).expect("load font");
            let mut paint = Paint::color(Color::rgb(255, 0, 0));
            paint.set_font(&[id]);
            paint.set_font_size(40.0);
            canvas.set_shadow_color(if with_shadow {
                Color::rgb(0, 0, 255)
            } else {
                Color::rgba(0, 0, 255, 0)
            });
            canvas.set_shadow_offset(20.0, 20.0);
            let _ = canvas.fill_text(10.0, 50.0, "Ag", &paint);
        });

        let mut blue = 0;
        for y in 0..H {
            for x in 0..W {
                let p = pixel(&pixels, x, y);
                // Blue-dominant, reasonably opaque => a shadow pixel.
                if p[2] > 120 && p[0] < 80 && p[3] > 120 {
                    blue += 1;
                }
            }
        }
        blue
    };

    let with = count_blue(true);
    let without = count_blue(false);

    assert!(with > 30, "text shadow should paint blue pixels, got {with}");
    assert_eq!(
        without, 0,
        "transparent shadow color must paint no blue pixels, got {without}"
    );
}

/// The shadow offset must NOT be scaled by the current transform: per the Canvas
/// spec (and WebKit's rule that "canvas shadows must not be affected by any
/// transformation") `shadowOffsetX/Y` are in output pixels. Under `scale(2, 2)`
/// with offset `(10, 0)` the shadow must land ~10 device px from the shape, not
/// 20.
#[test]
fn shadow_offset_is_not_scaled_by_transform() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };

    // User rect (5,5) size 10x10. Under scale(2,2) it occupies device x,y in
    // [10,30). Opaque green shadow, offset (10,0) in *user* units passed to the
    // setter, no blur.
    let pixels = render_to_pixels(&device, &queue, |canvas| {
        canvas.scale(2.0, 2.0);
        canvas.set_shadow_color(Color::rgb(0, 255, 0));
        canvas.set_shadow_offset(10.0, 0.0);
        let mut rect = Path::new();
        rect.rect(5.0, 5.0, 10.0, 10.0);
        canvas.fill_path(&rect, &Paint::color(Color::rgb(255, 0, 0)));
    });

    // Raw (correct) offset of 10 device px puts the shadow at device x in [20,40).
    // x=37,y=20 is inside that band but outside the shape ([10,30)).
    let inside = pixel(&pixels, 37, 20);
    assert!(
        inside[1] > 150 && inside[0] < 90 && inside[3] > 150,
        "expected green shadow ~10px from the shape at (37,20), got {inside:?}"
    );

    // x=45 lies past the raw-offset shadow (40) but *inside* a wrongly CTM-scaled
    // shadow (offset 20 => band [30,50)). It must be empty.
    let beyond = pixel(&pixels, 45, 20);
    assert!(
        beyond[3] < 32,
        "shadow must not extend to 20px (a CTM-scaled offset); (45,20) should be empty, got {beyond:?}"
    );
}

/// A shape whose own bounds are off-screen must still cast a shadow when the
/// offset (and/or blur) pulls the shadow back onto the render target. The
/// off-screen cull must therefore not be based on the shape's own bounds alone.
#[test]
fn offscreen_shape_with_offset_still_casts_shadow() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };

    // Rect placed fully to the LEFT of the canvas (x in [-40,-10)), so its own
    // bounds never touch the target. A +50 px shadow offset moves the shadow to
    // x in [10,40), squarely on-screen.
    let mut rect = Path::new();
    rect.rect(-40.0, 40.0, 30.0, 30.0);

    let pixels = render_to_pixels(&device, &queue, |canvas| {
        canvas.set_shadow_color(Color::rgb(0, 255, 0));
        canvas.set_shadow_offset(50.0, 0.0);
        canvas.fill_path(&rect, &Paint::color(Color::rgb(255, 0, 0)));
    });

    // Sample inside the shifted shadow band.
    let shadow_px = pixel(&pixels, 25, 55);
    assert!(
        shadow_px[1] > 150 && shadow_px[0] < 90 && shadow_px[3] > 150,
        "off-screen shape's offset shadow must paint on-screen at (25,55), got {shadow_px:?}"
    );

    // The shape itself stays off-screen: nothing red is painted.
    let shape_band = pixel(&pixels, 5, 55);
    assert!(
        shape_band[0] < 90,
        "the shape must remain off-screen (no red on the target), got {shape_band:?}"
    );
}

/// The shadow must be built from the *alpha of the actually-rendered source*. A
/// 50%-alpha fill must cast a visibly weaker shadow than a 100%-alpha fill, and a
/// fully transparent source must cast no shadow at all.
#[test]
fn shadow_strength_follows_source_alpha() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };

    let mut rect = Path::new();
    rect.rect(20.0, 20.0, 30.0, 30.0);

    // Opaque black shadow, offset clear of the shape, no blur. Vary only the
    // *source* fill alpha.
    let shadow_alpha_for_source = |src_alpha: u8| -> u8 {
        let pixels = render_to_pixels(&device, &queue, |canvas| {
            canvas.set_shadow_color(Color::rgb(0, 0, 0));
            canvas.set_shadow_offset(30.0, 30.0);
            canvas.fill_path(&rect, &Paint::color(Color::rgba(255, 0, 0, src_alpha)));
        });
        // (65,65) is in the shadow band ([50,80)) but outside the shape ([20,50)).
        pixel(&pixels, 65, 65)[3]
    };

    let opaque_shadow = shadow_alpha_for_source(255);
    let half_shadow = shadow_alpha_for_source(128);
    let transparent_shadow = shadow_alpha_for_source(0);

    assert!(
        opaque_shadow > 180,
        "a fully opaque source must cast a strong shadow, got alpha {opaque_shadow}"
    );
    // The half-alpha source's shadow must be clearly weaker (~half) and not just
    // marginally so.
    assert!(
        half_shadow + 40 < opaque_shadow,
        "a 50%-alpha source must cast a visibly weaker shadow (half={half_shadow}, opaque={opaque_shadow})"
    );
    assert!(
        half_shadow > 40,
        "a 50%-alpha source must still cast some shadow, got alpha {half_shadow}"
    );
    assert!(
        transparent_shadow < 16,
        "a fully transparent source must cast no shadow, got alpha {transparent_shadow}"
    );
}

/// Spec-derived text-shadow property (WHATWG canvas drawing model): the shadow
/// is built once from the alpha channel of the *entire* painting operation's
/// output, so its alpha can never exceed the shadow color's alpha — even where
/// glyphs overlap. An implementation that shadows glyphs individually compounds
/// alpha in overlap regions (`1 - (1-a)^2`, ~0.75 for a=0.5), which this test
/// rejects. Checked for both rasterization strategies (atlas and outline), since
/// the drawing model is defined on the painting operation, not the rasterizer.
#[cfg(feature = "textlayout")]
#[test]
fn text_shadow_alpha_never_exceeds_shadow_color_alpha() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };

    // Overlapping glyphs via strongly negative letter spacing; the shadow is
    // offset to the side so shadow-only pixels can be isolated by comparing
    // against a shadowless render of the same text.
    let mut check = |font_size: f32, label: &str| {
        let draw_text = |canvas: &mut Canvas<femtovg::renderer::WGPURenderer>, with_shadow: bool| {
            let font = canvas
                .add_font("examples/assets/RobotoFlex-VariableFont.ttf")
                .expect("Font not found");
            let paint = Paint::color(Color::white())
                .with_font(&[font])
                .with_font_size(font_size)
                .with_letter_spacing(-0.25 * font_size);
            if with_shadow {
                canvas.set_shadow_color(Color::rgba(0, 0, 0, 128));
                canvas.set_shadow_offset(20.0, 20.0);
            }
            canvas.fill_text(10.0, 50.0, "lll", &paint).unwrap();
        };

        let plain = render_to_pixels(&device, &queue, |canvas| draw_text(canvas, false));
        let shadowed = render_to_pixels(&device, &queue, |canvas| draw_text(canvas, true));

        // Max shadow alpha over pixels the text itself does not touch.
        let mut max_shadow = 0u8;
        for y in 0..H {
            for x in 0..W {
                if pixel(&plain, x, y)[3] == 0 {
                    max_shadow = max_shadow.max(pixel(&shadowed, x, y)[3]);
                }
            }
        }

        assert!(
            max_shadow >= 100,
            "{label}: expected a visible shadow (~128 alpha), got max {max_shadow}"
        );
        // Union-alpha bound: 128 (= shadowColor.a) plus a little AA headroom.
        // Per-glyph shadow compounding in the overlap region would reach ~191.
        assert!(
            max_shadow <= 140,
            "{label}: shadow alpha {max_shadow} exceeds the shadow color's alpha; \
             overlapping glyphs must not compound (shadow must derive from the \
             painting operation's union alpha)"
        );
    };

    check(40.0, "atlas rasterization");
    check(100.0, "outline rasterization");
}

/// Regression test for the Gaussian blur falloff *width* (kernel reach).
///
/// The Canvas/SVG drawing model maps `shadowBlur` to a Gaussian with standard
/// deviation `sigma = shadowBlur / 2`, whose edge response falls off over a
/// 10%-90% width of about `2.563 * sigma`. The blur shader previously sampled
/// only `+/-1.5*sigma` taps and renormalized by that partial sum, which shrank
/// the effective sigma to ~0.79x and produced only a ~12px 10-90 width at
/// `shadowBlur` 12 (sigma 6) -- i.e. shadows were ~21% too tight versus the spec
/// and versus Chrome/Firefox. With the corrected `+/-3*sigma` reach the width is
/// ~17px (effective sigma ~6.6, matching Chrome Canary). This test measures that
/// width directly so a regression back to the truncated kernel fails here rather
/// than only showing up in a screenshot diff.
#[test]
fn shadow_blur_falloff_width_matches_spec_sigma() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };

    // Black shadow, offset far right so its right edge falls on the empty
    // background (clear of the shape). Shape at x[10,40] -> shadow core x[70,100].
    let pixels = render_to_pixels(&device, &queue, |canvas| {
        canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
        canvas.set_shadow_blur(12.0); // spec sigma = 6
        canvas.set_shadow_offset(60.0, 0.0);
        let mut p = Path::new();
        p.rect(10.0, 30.0, 30.0, 60.0);
        canvas.fill_path(&p, &Paint::color(Color::rgb(255, 0, 0)));
    });

    // Shadow alpha along the vertical-center row, across the shadow's right edge.
    let row = 60;
    let alpha = |x: u32| pixel(&pixels, x, row)[3] as f32;
    let mut peak = 0.0f32;
    let mut peak_x = 70u32;
    for x in 70..=105 {
        let a = alpha(x);
        if a > peak {
            peak = a;
            peak_x = x;
        }
    }
    assert!(peak > 200.0, "shadow core alpha too low ({peak}); test geometry is off");

    // Sub-pixel x where the falloff (scanning right from the core peak) crosses a
    // threshold, via linear interpolation between adjacent samples.
    let cross = |thresh: f32| -> f32 {
        for x in peak_x..(W - 1) {
            let (a0, a1) = (alpha(x), alpha(x + 1));
            if a0 >= thresh && a1 < thresh {
                return x as f32 + (a0 - thresh) / (a0 - a1);
            }
        }
        (W - 1) as f32
    };
    let width = cross(0.1 * peak) - cross(0.9 * peak);
    let sigma_eff = width / 2.563;

    // Corrected +/-3sigma kernel: ~17px (sigma_eff ~6.6). Truncated +/-1.5sigma
    // kernel: ~12px (sigma_eff ~4.7). The lower bound rejects the truncated
    // kernel; the upper bound guards against an over-wide regression.
    assert!(
        (14.0..=21.0).contains(&width),
        "blur 10-90 falloff width {width:.1}px (effective sigma {sigma_eff:.2}) is out of the \
         expected ~17px band: a value near 12px means the kernel reach regressed to ~1.5*sigma"
    );
}

/// Text decoration + drop shadow interaction. A decorated text run is a single
/// painting operation, so its decoration lines are part of the same shadow as
/// the glyphs: the run-level shadow pass renders glyphs *and* decorations into
/// its coverage, and the decoration lines must NOT cast a second shadow of their
/// own. This verifies both halves — that the underline contributes to the shadow
/// (it is not missing) and that its shadow alpha never exceeds the shadow color's
/// alpha (it is not doubled by a per-decoration shadow pass).
#[cfg(feature = "textlayout")]
#[test]
fn decoration_shares_the_single_text_shadow() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };

    // (shadow, decoration lines u/s/o) -> rendered pixels. A far vertical offset
    // keeps the shadow band clear of the ink so shadow-only pixels are isolable.
    let render = |shadow: bool, lines: (bool, bool, bool)| -> Vec<u8> {
        render_to_pixels(&device, &queue, |canvas| {
            let font = canvas
                .add_font("examples/assets/RobotoFlex-VariableFont.ttf")
                .expect("Font not found");
            let paint = Paint::color(Color::white())
                .with_font(&[font])
                .with_font_size(40.0)
                .with_text_decoration(femtovg::TextDecoration {
                    underline: lines.0,
                    strikethrough: lines.1,
                    overline: lines.2,
                });
            if shadow {
                canvas.set_shadow_color(Color::rgba(0, 0, 0, 128));
                canvas.set_shadow_offset(0.0, 40.0); // straight down, well clear of the ink
            }
            canvas.fill_text(6.0, 50.0, "www", &paint).unwrap();
        })
    };

    let plain_shadow_cover = {
        let ink = render(false, (false, false, false));
        let sh = render(true, (false, false, false));
        (0..H * W)
            .filter(|i| {
                let (x, y) = (i % W, i / W);
                pixel(&ink, x, y)[3] == 0 && pixel(&sh, x, y)[3] > 32
            })
            .count()
    };

    // Each decoration kind, on its own, must be covered by one shared shadow.
    for (name, lines) in [
        ("underline", (true, false, false)),
        ("strikethrough", (false, true, false)),
        ("overline", (false, false, true)),
    ] {
        let ink = render(false, lines); // decorated ink, no shadow (mask)
        let shadowed = render(true, lines);

        let mut max_shadow = 0u8;
        let mut cover = 0usize;
        for y in 0..H {
            for x in 0..W {
                if pixel(&ink, x, y)[3] == 0 {
                    let a = pixel(&shadowed, x, y)[3];
                    max_shadow = max_shadow.max(a);
                    if a > 32 {
                        cover += 1;
                    }
                }
            }
        }

        // Not doubled: a per-decoration shadow would compound to ~1-(1-0.5)^2 ~= 191.
        assert!(
            max_shadow <= 140,
            "{name}: decorated text shadow alpha {max_shadow} exceeds the shadow color's \
             alpha; the line must not cast its own shadow on top of the run shadow"
        );
        // Not missing: the decoration adds a shadow band the plain run does not have.
        assert!(
            cover > plain_shadow_cover + 40,
            "{name}: the decoration line must cast a shadow (coverage {cover} \
             vs plain {plain_shadow_cover})"
        );
    }
}
