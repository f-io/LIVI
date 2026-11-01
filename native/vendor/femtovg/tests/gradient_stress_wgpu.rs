//! Headless GPU coverage for gradient scenarios the unit tests missed, all
//! cross-checked to match Chrome: the multi-stop LUT conic path (start angle +
//! dither), and a semi-transparent gradient (the premultiplied-alpha /
//! dither-fringe case). Skips when no GPU adapter is available.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, Paint, Path};

const W: u32 = 200;
const H: u32 = 200;

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
        label: Some("femtovg gradient stress test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
    }))
    .ok()?;
    Some((device, queue))
}

fn render_scene(device: &wgpu::Device, queue: &wgpu::Queue, draw: impl FnOnce(&mut Canvas<WGPURenderer>)) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("grad stress out"),
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

    let unpadded = W * 4;
    let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    enc.copy_texture_to_buffer(
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
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = slice.get_mapped_range().expect("readback");
    let mut pixels = vec![0u8; (unpadded * H) as usize];
    for row in 0..H as usize {
        let src = row * padded as usize;
        let dst = row * unpadded as usize;
        pixels[dst..dst + unpadded as usize].copy_from_slice(&mapped[src..src + unpadded as usize]);
    }
    drop(mapped);
    readback.unmap();
    pixels
}

fn px(pixels: &[u8], x: usize, y: usize) -> [u8; 3] {
    let i = (y * W as usize + x) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

/// A multi-stop conic gradient uses the LUT (`renderImageGradientConic`) path,
/// distinct from the two-colour `mix` path the other conic tests cover. Sampling
/// a ring around the centre must show real angular colour variation (the start
/// angle rotates it; the dither doesn't wash it out).
#[test]
fn multi_stop_conic_renders_distinct_hues() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let (cx, cy) = (W as f32 / 2.0, H as f32 / 2.0);
    let wheel = [
        (0.0, Color::rgb(230, 60, 60)),
        (0.25, Color::rgb(230, 200, 50)),
        (0.5, Color::rgb(60, 200, 110)),
        (0.75, Color::rgb(60, 110, 230)),
        (1.0, Color::rgb(230, 60, 60)),
    ];
    let pixels = render_scene(&device, &queue, |c| {
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &Paint::conic_gradient_stops_with_angle(cx, cy, 0.0, wheel));
    });

    // Sample 8 points on a ring of radius 60 around the centre.
    let mut samples = vec![];
    for k in 0..8 {
        let a = k as f32 / 8.0 * std::f32::consts::TAU;
        let x = (cx + 60.0 * a.cos()).round() as usize;
        let y = (cy + 60.0 * a.sin()).round() as usize;
        samples.push(px(&pixels, x, y));
    }
    // Distinct hues (coarsely, by rounding each channel to 32s) — a solid fill or
    // a broken conic would collapse to one.
    let distinct: std::collections::HashSet<_> = samples.iter().map(|c| [c[0] / 32, c[1] / 32, c[2] / 32]).collect();
    println!("ring samples: {samples:?} distinct={}", distinct.len());
    assert!(
        distinct.len() >= 4,
        "multi-stop conic must vary around the ring, got {} distinct: {samples:?}",
        distinct.len()
    );
    // Angular: the point at 0deg (+x) differs from 180deg (-x).
    assert_ne!(samples[0], samples[4], "conic must differ across the diameter");
}

/// A gradient from opaque red to fully transparent red, over a white background.
/// Correct premultiplied interpolation fades to the background with no dark band,
/// and the dither must not tint the transparent end (a premultiply/fringe bug
/// would leave the far end reddish or grey instead of white).
#[test]
fn semi_transparent_gradient_fades_to_background_without_fringe() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render_scene(&device, &queue, |c| {
        let paint = Paint::linear_gradient(
            0.0,
            0.0,
            W as f32,
            0.0,
            Color::rgba(230, 30, 30, 255),
            Color::rgba(230, 30, 30, 0),
        );
        let mut p = Path::new();
        p.rect(0.0, 0.0, W as f32, H as f32);
        c.fill_path(&p, &paint);
    });
    let y = H as usize / 2;
    let near = px(&pixels, 3, y); // opaque end
    let far = px(&pixels, W as usize - 4, y); // transparent end
    println!("near(opaque)={near:?} far(transparent)={far:?}");

    // Opaque end is red.
    assert!(
        near[0] > 180 && near[1] < 90 && near[2] < 90,
        "opaque end must be red, got {near:?}"
    );
    // Transparent end shows the white background — not tinted red/grey by the
    // dither or a premultiply error.
    assert!(
        far[0] > 244 && far[1] > 244 && far[2] > 244,
        "transparent end must fade to the white background (no fringe), got {far:?}"
    );
    // No dark band: green never dips below the opaque end's green along the row.
    let min_green = (0..W as usize).map(|x| px(&pixels, x, y)[1]).min().unwrap();
    assert!(
        min_green + 3 >= near[1],
        "no pixel may be darker (lower green) than the opaque end — a dark band would mean bad premultiply; min green {min_green} vs opaque {}",
        near[1]
    );
}
