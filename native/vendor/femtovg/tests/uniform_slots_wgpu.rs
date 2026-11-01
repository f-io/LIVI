//! Headless GPU test: the uniform buffer the wgpu backend sizes up front covers a whole frame.
//!
//! `render` reserves `UNIFORM_SLOTS_PER_COMMAND` slots per command before recording, because
//! growing the buffer mid-recording would invalidate the bind groups already recorded against it.
//! A frame mixing every command shape that records more than one set of params (concave fill,
//! stencil stroke) alongside the single-params ones would overflow a bound that is too tight: the
//! debug assertion in `render` fires, and past it wgpu rejects the oversized `write_buffer`.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, Color, FillRule, Paint, Path};

const W: u32 = 256;
const H: u32 = 256;
const SHAPES: usize = 300;

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
        label: Some("femtovg uniform slot test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
    }))
    .ok()?;
    Some((device, queue))
}

/// A bowtie: the two triangles overlap, so the fill goes through the stencil path.
fn bowtie(x: f32, y: f32) -> Path {
    let mut path = Path::new();
    path.move_to(x, y);
    path.line_to(x + 12.0, y + 12.0);
    path.line_to(x, y + 12.0);
    path.line_to(x + 12.0, y);
    path.close();
    path
}

#[test]
fn the_frame_uniform_buffer_covers_every_command_shape() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("uniform slots out"),
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
    let mut canvas = Canvas::new(WGPURenderer::new(device.clone(), queue.clone())).expect("canvas");
    canvas.set_size(W, H, 1.0);

    for i in 0..SHAPES {
        let x = (i % 20) as f32 * 12.0;
        let y = (i / 20) as f32 * 12.0;
        let paint = Paint::color(Color::rgb((i % 255) as u8, 90, 40));

        // Convex fill: one set of params.
        let mut rect = Path::new();
        rect.rect(x, y, 10.0, 10.0);
        canvas.fill_path(&rect, &paint);

        // Concave fill: stencil params, then fill params.
        canvas.fill_path(&bowtie(x, y), &paint.clone().with_fill_rule(FillRule::EvenOdd));

        // Stroke with anti-aliasing: two sets of params through the stencil stroke path.
        canvas.stroke_path(&bowtie(x, y), &paint.clone().with_line_width(2.0));
    }

    // A too-tight bound trips the debug assertion in `render`, or wgpu's validation past it.
    queue.submit(canvas.flush_to_output(&target));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("frame submitted");
}
