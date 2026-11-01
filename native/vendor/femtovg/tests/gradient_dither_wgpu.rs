//! Headless GPU test: gradients are dithered to break up 8-bit banding
//! (femtovg/femtovg#239). A gradient between two very close colours would, without
//! dithering, quantize to flat vertical bands — every pixel in a column identical.
//! With the screen-space ordered dither, the sub-LSB offset varies per pixel, so a
//! column inside a band carries two adjacent quantized values. The test asserts
//! most columns are dithered (they would all be flat without it).
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, Paint, Path};

const W: u32 = 220;
const H: u32 = 64;

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
        label: Some("femtovg gradient dither test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
    }))
    .ok()?;
    Some((device, queue))
}

#[test]
fn gradients_are_dithered_to_break_banding() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("dither out"),
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

    // Two very close grays across the width: only ~8 distinct 8-bit values, so
    // without dithering the fill is a handful of flat vertical bands.
    let paint = Paint::linear_gradient(
        0.0,
        0.0,
        W as f32,
        0.0,
        Color::rgb(120, 120, 120),
        Color::rgb(128, 128, 128),
    );
    let mut p = Path::new();
    p.rect(0.0, 0.0, W as f32, H as f32);
    canvas.fill_path(&p, &paint);

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
    let red = |x: usize, y: usize| mapped[y * padded as usize + x * 4];

    // A column is "dithered" if its red channel takes more than one value down the
    // column. Without dithering every column is flat (one value).
    let mut dithered_columns = 0;
    for x in 0..W as usize {
        let first = red(x, 0);
        if (1..H as usize).any(|y| red(x, y) != first) {
            dithered_columns += 1;
        }
    }
    println!("dithered columns: {dithered_columns}/{W}");
    assert!(
        dithered_columns > W as usize / 2,
        "expected most columns of a close-colour gradient to be dithered (vary down the \
         column); got {dithered_columns}/{W}. Without dithering they would all be flat bands."
    );
}
