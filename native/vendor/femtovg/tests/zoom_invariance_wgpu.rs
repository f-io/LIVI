//! Device-space metrics must track the canvas scale: a user-space quantity times
//! the zoom, with one law at every zoom. The text pipeline picks between three
//! rasterization regimes (path fallback, atlas, scale-baked atlas) based on the
//! transform, the font size, and the paint flavor, and a quantity that crosses
//! into bake space without its scale factor -- or with it applied twice -- shows
//! up as a metric that changes law at a regime boundary rather than scaling.
//! Glyph positions had this once, stroke widths had it twice; these tests hold
//! the seams for whatever comes next.
#![cfg(all(feature = "wgpu", feature = "textlayout"))]

use femtovg::{renderer::WGPURenderer, Canvas, Color, Paint, Path};

const W: u32 = 1000;
const H: u32 = 400;
const FONT: &[u8] = include_bytes!("../examples/assets/RobotoFlex-VariableFont.ttf");

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
        label: Some("femtovg zoom invariance test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
    }))
    .ok()?;
    Some((device, queue))
}

fn render(device: &wgpu::Device, queue: &wgpu::Queue, draw: impl FnOnce(&mut Canvas<WGPURenderer>)) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
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
    let mut canvas = Canvas::new(renderer).expect("canvas");
    canvas.set_size(W, H, 1.0);
    canvas.clear_rect(0, 0, W, H, Color::white());
    draw(&mut canvas);
    let commands = canvas.flush_to_output(&target);
    queue.submit(commands);
    read_back(device, queue, &target)
}

fn read_back(device: &wgpu::Device, queue: &wgpu::Queue, target: &wgpu::Texture) -> Vec<u8> {
    let unpadded = W * 4;
    let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let rb = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &rb,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(enc.finish()));
    let sl = rb.slice(..);
    sl.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let m = sl.get_mapped_range().expect("readback");
    let mut px = vec![0u8; (unpadded * H) as usize];
    for row in 0..H as usize {
        let s = row * padded as usize;
        let d = row * unpadded as usize;
        px[d..d + unpadded as usize].copy_from_slice(&m[s..s + unpadded as usize]);
    }
    drop(m);
    rb.unmap();
    px
}

/// Width of the first ink band along a row, as summed coverage, so antialiased
/// edges contribute their fractions instead of rounding at a threshold.
fn first_band(pixels: &[u8], y: u32, x0: u32, x1: u32) -> Option<f32> {
    let cov = |x: u32| 1.0 - pixels[((y * W + x) * 4) as usize] as f32 / 255.0;
    let mut x = x0;
    while x < x1 && cov(x) < 0.2 {
        x += 1;
    }
    if x >= x1 {
        return None;
    }
    let mut sum = 0.0;
    let mut clear = 0;
    while x < x1 && clear < 2 {
        let c = cov(x);
        sum += c;
        clear = if c < 0.05 { clear + 1 } else { 0 };
        x += 1;
    }
    Some(sum)
}

fn text_paint(font: femtovg::FontId, gradient: bool) -> Paint {
    let base = if gradient {
        // A near-black gradient: same look, but forces the path regime, which
        // is one of the seams under test.
        Paint::linear_gradient(0.0, 0.0, 400.0, 0.0, Color::black(), Color::rgb(25, 25, 25))
    } else {
        Paint::color(Color::black())
    };
    base.with_font(&[font]).with_font_size(80.0).with_line_width(4.0)
}

/// The one law: a stroked glyph's band must match a stroked path's band under
/// the same paint and transform, at every zoom and in every regime. The path
/// stroke is the user-space reference the rest of the canvas already obeys.
#[test]
fn stroke_text_width_matches_stroke_path_at_every_zoom() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    // The ladder crosses every regime seam at font size 80: 1.0 is the plain
    // atlas, 1.1 bakes into the scaled atlas (88px, under the 92px cap), 1.5
    // and up cross the cap into the path fallback, and the gradient flavor
    // forces the path fallback at every zoom.
    for gradient in [false, true] {
        for s in [1.0f32, 1.1, 1.5, 2.0, 2.5] {
            let pixels = render(&device, &queue, |c| {
                let font = c.add_font_mem(FONT).unwrap();
                c.save();
                c.scale(s, s);
                let paint = text_paint(font, gradient);
                c.stroke_text(20.0, 100.0, "H", &paint).unwrap();
                let mut p = Path::new();
                p.move_to(220.0, 20.0);
                p.line_to(220.0, 110.0);
                c.stroke_path(&p, &paint);
                c.restore();
            });
            let row = (75.0 * s) as u32;
            let text_band = first_band(&pixels, row, 0, (160.0 * s) as u32).unwrap_or(0.0);
            let path_band = first_band(&pixels, row, (160.0 * s) as u32, W - 1).unwrap_or(0.0);
            assert!(
                (text_band - path_band).abs() <= 1.2,
                "s={s} gradient={gradient}: stroke_text band {text_band:.2}px vs stroke_path band {path_band:.2}px"
            );
        }
    }
}

/// The same metric stated against the absolute law, catching the case where
/// text and path drift together.
#[test]
fn stroke_text_width_is_linear_in_the_zoom() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    for s in [1.0f32, 1.1, 1.5, 2.0, 2.5] {
        let pixels = render(&device, &queue, |c| {
            let font = c.add_font_mem(FONT).unwrap();
            c.save();
            c.scale(s, s);
            c.stroke_text(20.0, 100.0, "H", &text_paint(font, false)).unwrap();
            c.restore();
        });
        let row = (75.0 * s) as u32;
        let band = first_band(&pixels, row, 0, (160.0 * s) as u32).unwrap_or(0.0);
        let expected = 4.0 * s;
        assert!(
            (band - expected).abs() <= 1.2,
            "s={s}: band {band}px, expected {expected}px"
        );
    }
}

/// Zooming in and back out on one canvas must reproduce the original frame:
/// nothing rendered at the intermediate zoom may leak through cached state
/// (glyph atlas entries, path caches, gradient stores).
#[test]
fn zoom_round_trip_reproduces_the_original_frame() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let frame = |device: &wgpu::Device, queue: &wgpu::Queue, scales: &[f32]| -> Vec<u8> {
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
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
        let mut canvas = Canvas::new(renderer).expect("canvas");
        canvas.set_size(W, H, 1.0);
        let font = canvas.add_font_mem(FONT).unwrap();
        let mut last = Vec::new();
        for &s in scales {
            canvas.clear_rect(0, 0, W, H, Color::white());
            canvas.save();
            canvas.scale(s, s);
            let paint = text_paint(font, false);
            canvas.fill_text(20.0, 60.0, "Hello zoom", &paint).unwrap();
            canvas.stroke_text(20.0, 120.0, "Hello zoom", &paint).unwrap();
            canvas.restore();
            let commands = canvas.flush_to_output(&target);
            queue.submit(commands);
            last = read_back(device, queue, &target);
        }
        last
    };
    let direct = frame(&device, &queue, &[1.0]);
    let round_trip = frame(&device, &queue, &[1.0, 2.5, 0.5, 1.0]);
    let mismatched = direct
        .chunks_exact(4)
        .zip(round_trip.chunks_exact(4))
        .filter(|(a, b)| a[0].abs_diff(b[0]) > 2 || a[1].abs_diff(b[1]) > 2 || a[2].abs_diff(b[2]) > 2)
        .count();
    assert!(
        mismatched == 0,
        "{mismatched} pixels differ between a direct render and a zoom round trip"
    );
}
