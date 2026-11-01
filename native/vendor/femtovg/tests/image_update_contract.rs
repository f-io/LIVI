//! `update_image` is specified to report a copy it cannot perform, rather than
//! forwarding it to the graphics API. These run on the `Void` renderer so they
//! hold on every platform and in CI without a GPU; the backend-specific
//! regression lives in `image_update_bounds_wgpu.rs`.

use femtovg::{renderer::Void, Canvas, ErrorKind, ImageFlags};
use imgref::Img;
use rgb::{alt::Gray, RGBA8};

fn canvas() -> Canvas<Void> {
    let mut canvas = Canvas::new(Void).expect("canvas");
    canvas.set_size(64, 64, 1.0);
    canvas
}

fn rgba(w: usize, h: usize) -> Vec<RGBA8> {
    vec![RGBA8::new(255, 0, 0, 255); w * h]
}

#[test]
fn a_copy_reaching_past_the_destination_is_reported() {
    let mut canvas = canvas();
    let dst = rgba(32, 32);
    let id = canvas
        .create_image(Img::new(dst.as_slice(), 32, 32), ImageFlags::empty())
        .expect("create");

    let src = rgba(32, 32);
    for (x, y) in [(8, 0), (0, 8), (8, 8), (32, 0), (0, 32)] {
        let result = canvas.update_image(id, Img::new(src.as_slice(), 32, 32), x, y);
        assert!(
            matches!(result, Err(ErrorKind::ImageUpdateOutOfBounds)),
            "({x}, {y}) should be out of bounds, got {result:?}"
        );
    }
}

#[test]
fn a_source_larger_than_the_destination_is_reported() {
    let mut canvas = canvas();
    let dst = rgba(16, 16);
    let id = canvas
        .create_image(Img::new(dst.as_slice(), 16, 16), ImageFlags::empty())
        .expect("create");

    for (w, h) in [(24, 16), (16, 24), (24, 24)] {
        let src = rgba(w, h);
        let result = canvas.update_image(id, Img::new(src.as_slice(), w, h), 0, 0);
        assert!(
            matches!(result, Err(ErrorKind::ImageUpdateOutOfBounds)),
            "a {w}x{h} source into a 16x16 image should be out of bounds, got {result:?}"
        );
    }
}

/// An origin near the top of the address space must report the error rather than
/// wrapping around while the bound is computed, which in a release build would
/// let an enormous origin look like a valid one.
#[test]
fn an_origin_that_would_overflow_is_reported() {
    let mut canvas = canvas();
    let dst = rgba(16, 16);
    let id = canvas
        .create_image(Img::new(dst.as_slice(), 16, 16), ImageFlags::empty())
        .expect("create");

    let src = rgba(8, 8);
    let result = canvas.update_image(id, Img::new(src.as_slice(), 8, 8), usize::MAX, 0);
    assert!(
        matches!(result, Err(ErrorKind::ImageUpdateOutOfBounds)),
        "an overflowing x origin should be out of bounds, got {result:?}"
    );
    let result = canvas.update_image(id, Img::new(src.as_slice(), 8, 8), 0, usize::MAX);
    assert!(
        matches!(result, Err(ErrorKind::ImageUpdateOutOfBounds)),
        "an overflowing y origin should be out of bounds, got {result:?}"
    );
}

#[test]
fn a_source_in_another_format_is_reported() {
    let mut canvas = canvas();
    let dst = rgba(16, 16);
    let id = canvas
        .create_image(Img::new(dst.as_slice(), 16, 16), ImageFlags::empty())
        .expect("create");

    let gray = vec![Gray(128u8); 16 * 16];
    let result = canvas.update_image(id, Img::new(gray.as_slice(), 16, 16), 0, 0);
    assert!(
        matches!(result, Err(ErrorKind::ImageUpdateWithDifferentFormat)),
        "a Gray8 source into an Rgba8 image should be refused, got {result:?}"
    );
}

/// The guard must leave every copy that does fit alone, including one flush with
/// the far corner and one covering the image exactly.
#[test]
fn copies_that_fit_are_left_alone() {
    let mut canvas = canvas();
    let dst = rgba(32, 32);
    let id = canvas
        .create_image(Img::new(dst.as_slice(), 32, 32), ImageFlags::empty())
        .expect("create");

    let patch = rgba(8, 8);
    for (x, y) in [(0, 0), (24, 0), (0, 24), (24, 24), (12, 12)] {
        canvas
            .update_image(id, Img::new(patch.as_slice(), 8, 8), x, y)
            .unwrap_or_else(|e| panic!("({x}, {y}) fits and should succeed, got {e:?}"));
    }

    let full = rgba(32, 32);
    canvas
        .update_image(id, Img::new(full.as_slice(), 32, 32), 0, 0)
        .expect("a full size update at the origin should succeed");
}
