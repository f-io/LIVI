//! Headless GPU regression test: image blits must honor rounded scissor clips.
//!
//! An axis-aligned, untinted, non-antialiased image fill of a rectangle takes a
//! fast path that copies the texture without per-pixel clipping
//! (`ShaderType::TextureCopyUnclipped`), based on `Scissor::as_rect`. A rounded
//! scissor's corner radius is applied only by the fragment shader's scissor
//! mask, which that fast path bypasses, so rounded clips must fall back to the
//! normal path or the image paints straight through the rounded corners.
//!
//! The test gracefully skips (prints and returns) when no GPU adapter is
//! available, so it doesn't fail on backend-less CI.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, ImageFlags, Paint, Path, PixelFormat, RenderTarget};

const W: u32 = 200;
const H: u32 = 200;

/// Lazily create a headless wgpu device/queue. Returns `None` when no adapter
/// is available, so the caller can skip the test.
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
        label: Some("femtovg blit scissor test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
    }))
    .ok()?;

    Some((device, queue))
}

/// Clear the canvas white, set a rounded scissor of 100x100 at (50,50) with a
/// 40px corner radius, and fill `fill_rect` with an untinted, non-antialiased
/// image paint (a solid red 100x100 image) — the exact shape of draw that is
/// eligible for the unclipped image blit fast path. Returns the rendered
/// pixels as a row-major `[r, g, b, a]` buffer.
fn render_scissored_blit(device: &wgpu::Device, queue: &wgpu::Queue, fill_rect: (f32, f32, f32, f32)) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("blit scissor test target"),
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

    // Build a solid red 100x100 source image.
    let image = canvas
        .create_image_empty(100, 100, PixelFormat::Rgba8, ImageFlags::empty())
        .expect("failed to create image");
    canvas.set_render_target(RenderTarget::Image(image));
    canvas.clear_rect(0, 0, 100, 100, Color::rgb(255, 0, 0));
    canvas.set_render_target(RenderTarget::Screen);

    canvas.clear_rect(0, 0, W, H, Color::white());

    canvas.rounded_scissor(50.0, 50.0, 100.0, 100.0, 40.0);

    let (x, y, w, h) = fill_rect;
    let mut path = Path::new();
    path.rect(x, y, w, h);
    let paint = Paint::image(image, x, y, w, h, 0.0, 1.0).with_anti_alias(false);
    canvas.fill_path(&path, &paint);

    let commands = canvas.flush_to_output(&target);
    queue.submit(commands);

    // Copy the target texture into a readback buffer. bytes_per_row must be a
    // multiple of 256.
    let unpadded_bytes_per_row = W * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("blit scissor test readback"),
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

/// Red image pixel: high red, no green/blue. The background is white, so a
/// correctly clipped pixel has high green/blue instead.
fn is_red(px: [u8; 4]) -> bool {
    px[0] > 200 && px[1] < 128 && px[2] < 128
}

/// White background pixel, i.e. the clip removed the image there.
fn is_background(px: [u8; 4]) -> bool {
    px[0] > 200 && px[1] > 200 && px[2] > 200
}

/// Fill rect fully contained in the scissor rect: exercises the blit fast
/// path's full-containment branch. The pixel at (54,54) lies inside the fill
/// rect but outside the 40px rounded corner (distance ~50 from the corner
/// center at (90,90)), so it must show the white background, while the clip
/// must not eat into the interior or the straight edges.
#[test]
fn contained_image_blit_is_clipped_by_rounded_scissor() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };

    let pixels = render_scissored_blit(&device, &queue, (50.0, 50.0, 100.0, 100.0));

    let corner = pixel(&pixels, 54, 54);
    let center = pixel(&pixels, 100, 100);
    let edge = pixel(&pixels, 100, 52);
    println!("contained blit: corner(54,54)={corner:?} center(100,100)={center:?} edge(100,52)={edge:?}");

    assert!(
        !is_red(corner) && is_background(corner),
        "pixel (54,54) is outside the rounded corner and must be clipped to the \
         white background, got {corner:?}"
    );
    assert!(
        is_red(center),
        "pixel (100,100) is well inside the clip and must show the red image, got {center:?}"
    );
    assert!(
        is_red(edge),
        "pixel (100,52) sits on the straight top edge inside the clip and must \
         not be over-clipped, got {edge:?}"
    );
}

/// Fill rect extending past the scissor rect: exercises the blit fast path's
/// partial-overlap branch (the blit is reduced to the intersection with the
/// scissor rect, still without per-pixel clipping). The rounded corner must
/// clip that intersection too.
#[test]
fn partially_scissored_image_blit_is_clipped_by_rounded_scissor() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };

    let pixels = render_scissored_blit(&device, &queue, (40.0, 40.0, 120.0, 120.0));

    let corner = pixel(&pixels, 54, 54);
    let center = pixel(&pixels, 100, 100);
    let outside = pixel(&pixels, 45, 100);
    println!("overflowing blit: corner(54,54)={corner:?} center(100,100)={center:?} outside(45,100)={outside:?}");

    assert!(
        !is_red(corner) && is_background(corner),
        "pixel (54,54) is outside the rounded corner and must be clipped to the \
         white background, got {corner:?}"
    );
    assert!(
        is_red(center),
        "pixel (100,100) is well inside the clip and must show the red image, got {center:?}"
    );
    assert!(
        is_background(outside),
        "pixel (45,100) is covered by the fill rect but left of the scissor \
         rect entirely and must stay background, got {outside:?}"
    );
}
