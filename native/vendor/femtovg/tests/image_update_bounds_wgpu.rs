//! `Renderer::update_image` must reject a copy that would fall outside the
//! destination image, or that carries a different pixel format, by returning the
//! documented error. The trait's own default implementation, the `Void` renderer
//! and the OpenGL backend all do this; when a backend instead forwards an
//! out-of-range copy to the graphics API, validation fails inside the driver and
//! takes the process (or, on wasm, the whole application) down with it.
#![cfg(feature = "wgpu")]

use femtovg::{renderer::WGPURenderer, Canvas, ErrorKind, ImageFlags};
use imgref::Img;
use rgb::{alt::Gray, RGBA8};

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
        label: Some("femtovg image update bounds test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
    }))
    .ok()?;
    Some((device, queue))
}

fn canvas() -> Option<Canvas<WGPURenderer>> {
    let (device, queue) = headless_device()?;
    let mut canvas = Canvas::new(WGPURenderer::new(device, queue)).expect("canvas");
    canvas.set_size(64, 64, 1.0);
    Some(canvas)
}

fn rgba(w: usize, h: usize) -> Vec<RGBA8> {
    vec![RGBA8::new(255, 0, 0, 255); w * h]
}

/// A copy whose origin pushes it past the right/bottom edge must be refused
/// rather than handed to the graphics API.
#[test]
fn update_image_past_the_edge_is_rejected() {
    let Some(mut canvas) = canvas() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let dst = rgba(32, 32);
    let id = canvas
        .create_image(Img::new(dst.as_slice(), 32, 32), ImageFlags::empty())
        .expect("create");

    let src = rgba(32, 32);
    // 32x32 written at (8, 8) reaches 40x40 inside a 32x32 image.
    let result = canvas.update_image(id, Img::new(src.as_slice(), 32, 32), 8, 8);
    assert!(
        matches!(result, Err(ErrorKind::ImageUpdateOutOfBounds)),
        "expected ImageUpdateOutOfBounds, got {result:?}"
    );
}

/// The same rule with the overflow only on one axis, and with a source that is
/// larger than the destination outright.
#[test]
fn update_image_larger_than_destination_is_rejected() {
    let Some(mut canvas) = canvas() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let dst = rgba(16, 16);
    let id = canvas
        .create_image(Img::new(dst.as_slice(), 16, 16), ImageFlags::empty())
        .expect("create");

    let wide = rgba(24, 16);
    let result = canvas.update_image(id, Img::new(wide.as_slice(), 24, 16), 0, 0);
    assert!(
        matches!(result, Err(ErrorKind::ImageUpdateOutOfBounds)),
        "expected ImageUpdateOutOfBounds for an oversized source, got {result:?}"
    );

    let tall = rgba(16, 24);
    let result = canvas.update_image(id, Img::new(tall.as_slice(), 16, 24), 0, 0);
    assert!(
        matches!(result, Err(ErrorKind::ImageUpdateOutOfBounds)),
        "expected ImageUpdateOutOfBounds on the y axis, got {result:?}"
    );
}

/// Updating with a source whose format differs from the texture's would compute
/// a bytes-per-row from the wrong pixel size; the OpenGL backend reports this
/// instead of writing, and so must every other backend.
#[test]
fn update_image_with_a_different_format_is_rejected() {
    let Some(mut canvas) = canvas() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let dst = rgba(16, 16);
    let id = canvas
        .create_image(Img::new(dst.as_slice(), 16, 16), ImageFlags::empty())
        .expect("create");

    let gray = vec![Gray(128u8); 16 * 16];
    let result = canvas.update_image(id, Img::new(gray.as_slice(), 16, 16), 0, 0);
    assert!(
        matches!(result, Err(ErrorKind::ImageUpdateWithDifferentFormat)),
        "expected ImageUpdateWithDifferentFormat, got {result:?}"
    );
}

/// The guard must not turn away a copy that does fit, including one placed hard
/// against the far corner.
#[test]
fn update_image_within_bounds_still_succeeds() {
    let Some(mut canvas) = canvas() else {
        eprintln!("skipping: no wgpu adapter available");
        return;
    };
    let dst = rgba(32, 32);
    let id = canvas
        .create_image(Img::new(dst.as_slice(), 32, 32), ImageFlags::empty())
        .expect("create");

    let patch = rgba(8, 8);
    canvas
        .update_image(id, Img::new(patch.as_slice(), 8, 8), 0, 0)
        .expect("origin update must succeed");
    // Exactly flush with the far corner: 24 + 8 == 32.
    canvas
        .update_image(id, Img::new(patch.as_slice(), 8, 8), 24, 24)
        .expect("flush-with-the-edge update must succeed");
    // A full-size update at the origin.
    let full = rgba(32, 32);
    canvas
        .update_image(id, Img::new(full.as_slice(), 32, 32), 0, 0)
        .expect("full-size update must succeed");
}
