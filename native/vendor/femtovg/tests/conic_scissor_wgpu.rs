//! Headless GPU regression test: conic gradient fills must honor scissor clips
//! on the wgpu backend.
//!
//! The WGSL conic gradient cases returned their color directly, before the
//! scissor mask and stroke-antialiasing multiply that every other fill type
//! falls through to. A conic-filled shape therefore ignored the scissor (and its
//! rounded corners) entirely. This renders a conic gradient over a rounded
//! scissor and asserts the corner outside the radius is clipped away, mirroring
//! `blit_scissor_wgpu.rs` for the image-blit fast path.
//!
//! Skips (prints and returns) when no GPU adapter is available.
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
        label: Some("femtovg conic scissor test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
    }))
    .ok()?;
    Some((device, queue))
}

/// Clear white, set a 100x100 rounded scissor at (50,50) with a 40px radius, and
/// fill that rect with a conic gradient centred in it. Returns row-major
/// `[r,g,b,a]` pixels.
fn render_scissored_conic(device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("conic scissor target"),
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

    canvas.rounded_scissor(50.0, 50.0, 100.0, 100.0, 40.0);
    let stops = [
        (0.0, Color::rgb(230, 40, 40)),
        (0.5, Color::rgb(40, 60, 230)),
        (1.0, Color::rgb(230, 40, 40)),
    ];
    let mut path = Path::new();
    path.rect(50.0, 50.0, 100.0, 100.0);
    canvas.fill_path(&path, &Paint::conic_gradient_stops_with_angle(100.0, 100.0, 0.0, stops));

    let commands = canvas.flush_to_output(&target);
    queue.submit(commands);

    let unpadded_bytes_per_row = W * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("conic scissor readback"),
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

/// White background pixel — the clip removed the gradient there.
fn is_background(px: [u8; 4]) -> bool {
    px[0] > 200 && px[1] > 200 && px[2] > 200
}

#[test]
fn conic_gradient_is_clipped_by_rounded_scissor() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let pixels = render_scissored_conic(&device, &queue);

    // (54,54) is inside the 100x100 rect but outside the 40px rounded corner
    // (centre (90,90), distance ~50), so it must be clipped to the background.
    let corner = pixel(&pixels, 54, 54);
    // (100,100) is the gradient centre — well inside the clip.
    let center = pixel(&pixels, 100, 100);
    // (100,52) is on the straight top edge inside the clip — must not over-clip.
    let edge = pixel(&pixels, 100, 52);
    println!("conic scissor: corner(54,54)={corner:?} center(100,100)={center:?} edge(100,52)={edge:?}");

    assert!(
        is_background(corner),
        "pixel (54,54) is outside the rounded corner; the conic fill must be \
         clipped to the white background there, got {corner:?}"
    );
    assert!(
        !is_background(center),
        "pixel (100,100) is inside the clip and must show the conic gradient, got {center:?}"
    );
    assert!(
        !is_background(edge),
        "pixel (100,52) is on the straight edge inside the clip and must not be \
         over-clipped, got {edge:?}"
    );
}
