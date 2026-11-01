//! Headless GPU regression tests: scaled-atlas text must stay locked to the same
//! on-screen position as vector geometry under a zoom transform.
//!
//! For a uniform-scale + translation canvas transform, solid-color text is drawn
//! from a glyph atlas rasterized at a scale quantized to 1/16 steps (so small
//! zoom changes don't churn the atlas). The quantized scale must only choose the
//! bitmap resolution — positioning must use the true scale. If glyph positions
//! are premultiplied by the quantized scale instead, a glyph at world position p
//! drifts from its correct screen position by `p * (quantized - true)`, which
//! grows with distance from the origin and snaps at each 1/16 boundary while
//! vector shapes stay put.
//!
//! Each test places the zoom pivot on the drawn feature so its true screen
//! position is constant across every zoom level, then sweeps the zoom across
//! 1/16 boundaries and asserts nothing jumps. They cover both axes and text
//! co-located with a vector shape. Each skips (prints and returns) when no GPU
//! adapter is available.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, Paint, Path};

const W: u32 = 640;
const H: u32 = 400;
const FONT: &[u8] = include_bytes!("../examples/assets/RobotoFlex-VariableFont.ttf");
// Zoom levels straddling the 1/16 boundaries at 1.4375 and 1.5. Positioning by
// the quantized scale jumps by ~pivot_coord/16 at each boundary; a correct
// renderer only shifts by the glyph's own growth (a fraction of a pixel per step).
const ZS: [f32; 7] = [1.40, 1.42, 1.44, 1.46, 1.48, 1.50, 1.52];
// The quantized-position bug produces ~20-31 px jumps; a correct renderer shifts
// by under a pixel per step. 6 px cleanly separates the two while tolerating
// antialiasing and centroid noise.
const MAX_STEP: f32 = 6.0;

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
        label: Some("femtovg text scale position test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
    }))
    .ok()?;
    Some((device, queue))
}

/// Render a scene under a zoom of `z` around `pivot`. Returns row-major
/// `[r,g,b,a]` pixels.
fn render_scene(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    z: f32,
    pivot: (f32, f32),
    draw: impl FnOnce(&mut Canvas<WGPURenderer>),
) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("text scale position target"),
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
    canvas.clear_rect(0, 0, W, H, Color::white());
    canvas.reset_transform();
    canvas.translate(pivot.0, pivot.1);
    canvas.scale(z, z);
    canvas.translate(-pivot.0, -pivot.1);
    draw(&mut canvas);

    let commands = canvas.flush_to_output(&target);
    queue.submit(commands);

    let unpadded_bytes_per_row = W * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("text scale position readback"),
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

fn text_paint(font: femtovg::FontId) -> Paint {
    Paint::color(Color::rgb(0, 0, 0))
        .with_font(&[font])
        .with_font_size(40.0)
}

/// Centroid `(x, y)` of dark (ink) pixels within a row band `[y0, y1)`, or
/// `None` if there are none.
fn dark_centroid(pixels: &[u8], y0: usize, y1: usize) -> Option<(f32, f32)> {
    let (mut sx, mut sy, mut n) = (0.0f64, 0.0f64, 0u64);
    for y in y0..y1 {
        for x in 0..W as usize {
            if pixels[(y * W as usize + x) * 4] < 100 {
                sx += x as f64;
                sy += y as f64;
                n += 1;
            }
        }
    }
    (n > 0).then(|| ((sx / n as f64) as f32, (sy / n as f64) as f32))
}

fn max_adjacent_step(values: &[f32]) -> f32 {
    values.windows(2).map(|p| (p[1] - p[0]).abs()).fold(0.0, f32::max)
}

/// Horizontal: a glyph far from the origin, with the pivot on its column, must
/// keep a constant screen x as the zoom sweeps.
#[test]
fn scaled_atlas_text_does_not_drift_horizontally() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };
    let world_x = 500.0f32;
    let pivot = (world_x, H as f32 / 2.0);
    let xs: Vec<f32> = ZS
        .iter()
        .map(|&z| {
            let px = render_scene(&device, &queue, z, pivot, |c| {
                let f = c.add_font_mem(FONT).unwrap();
                c.fill_text(world_x, pivot.1, "1", &text_paint(f)).unwrap();
            });
            dark_centroid(&px, 0, H as usize)
                .unwrap_or_else(|| panic!("no ink at z={z}"))
                .0
        })
        .collect();
    let step = max_adjacent_step(&xs);
    assert!(
        step < MAX_STEP,
        "glyph drifted horizontally between adjacent zoom steps (max {step:.1} px); xs: {xs:?}"
    );
}

/// Vertical: the same defect on the y axis. A glyph low on the canvas, with the
/// pivot on its row, must keep a constant screen y as the zoom sweeps.
#[test]
fn scaled_atlas_text_does_not_drift_vertically() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };
    let world_y = 330.0f32;
    let pivot = (W as f32 / 2.0, world_y);
    let ys: Vec<f32> = ZS
        .iter()
        .map(|&z| {
            let px = render_scene(&device, &queue, z, pivot, |c| {
                let f = c.add_font_mem(FONT).unwrap();
                c.fill_text(pivot.0, world_y, "1", &text_paint(f)).unwrap();
            });
            dark_centroid(&px, 0, H as usize)
                .unwrap_or_else(|| panic!("no ink at z={z}"))
                .1
        })
        .collect();
    let step = max_adjacent_step(&ys);
    assert!(
        step < MAX_STEP,
        "glyph drifted vertically between adjacent zoom steps (max {step:.1} px); ys: {ys:?}"
    );
}

/// Co-location: text must stay glued to vector geometry drawn at the same world
/// point (the actual reported symptom — a label sliding out of its shape). A
/// reference bar and a glyph share a world column on the pivot; the glyph's
/// offset from the bar must not change as the zoom sweeps.
#[test]
fn scaled_atlas_text_tracks_vector_geometry() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };
    let world_x = 500.0f32;
    let pivot = (world_x, H as f32 / 2.0);
    let offsets: Vec<f32> = ZS
        .iter()
        .map(|&z| {
            let px = render_scene(&device, &queue, z, pivot, |c| {
                let f = c.add_font_mem(FONT).unwrap();
                // Reference vector bar in the top band...
                let mut bar = Path::new();
                bar.rect(world_x, 60.0, 2.0, 40.0);
                c.fill_path(&bar, &Paint::color(Color::rgb(0, 0, 0)));
                // ...and the glyph in the bottom band, same world column.
                c.fill_text(world_x, 300.0, "1", &text_paint(f)).unwrap();
            });
            let bar = dark_centroid(&px, 0, H as usize / 2).unwrap_or_else(|| panic!("no bar at z={z}"));
            let glyph = dark_centroid(&px, H as usize / 2, H as usize).unwrap_or_else(|| panic!("no glyph at z={z}"));
            glyph.0 - bar.0
        })
        .collect();
    let step = max_adjacent_step(&offsets);
    assert!(
        step < MAX_STEP,
        "text separated from vector geometry between adjacent zoom steps (max {step:.1} px); \
         glyph-minus-bar offsets: {offsets:?}"
    );
}
