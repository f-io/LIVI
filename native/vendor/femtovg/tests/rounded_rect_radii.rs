//! Conformance tests for rounded rectangle corner radii.
//!
//! When the radii along a side are larger than that side can hold, both CSS
//! Backgrounds and Borders Level 3 ("Overlapping curves") and the Canvas
//! `roundRect()` algorithm reduce every radius by one common factor
//! `f = min(side length / sum of that side's two radii)`. Scaling them together
//! is what keeps a corner circular. Reducing each axis on its own instead
//! (`rx = min(r, w/2)`, `ry = min(r, h/2)`) fits the box but flattens the corner
//! into an ellipse, which is the shape of this bug in several engines.
//!
//! Expected geometry here was cross-checked against Chrome's `roundRect()`.

use femtovg::{Path, Verb};

/// The two radii of one corner, recovered from the emitted path.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Corner {
    rx: f32,
    ry: f32,
}

#[derive(Debug, Clone, Copy)]
struct Corners {
    top_left: Corner,
    top_right: Corner,
    bottom_right: Corner,
    bottom_left: Corner,
}

/// Reads the corner radii back out of a rounded rectangle sub-path.
///
/// The shape is emitted as `MoveTo`, then four `LineTo`/`BezierTo` pairs walking
/// the sides counter-clockwise from the left edge, so each radius is the distance
/// from a corner of the bounding box to the point where the straight edge ends.
/// Only the on-curve endpoints are read, never the Bézier control points, so the
/// values are exact rather than approximations of the arc.
fn corners_of(path: &Path, x: f32, y: f32, w: f32, h: f32) -> Corners {
    let mut points = Vec::new();
    for verb in path.verbs() {
        match verb {
            Verb::MoveTo(px, py) => points.push((px, py)),
            Verb::LineTo(px, py) => points.push((px, py)),
            Verb::BezierTo(_, _, _, _, px, py) => points.push((px, py)),
            _ => {}
        }
    }
    assert_eq!(points.len(), 9, "expected a move, four lines and four curves");

    Corners {
        // MoveTo starts on the left edge at the end of the top left corner.
        top_left: Corner {
            rx: points[7].0 - x,
            ry: points[0].1 - y,
        },
        bottom_left: Corner {
            rx: points[2].0 - x,
            ry: (y + h) - points[1].1,
        },
        bottom_right: Corner {
            rx: (x + w) - points[3].0,
            ry: (y + h) - points[4].1,
        },
        top_right: Corner {
            rx: (x + w) - points[6].0,
            ry: points[5].1 - y,
        },
    }
}

#[track_caller]
fn assert_close(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() < 0.01,
        "{what}: expected {expected}, got {actual}"
    );
}

#[track_caller]
fn assert_circular(corner: Corner, radius: f32, what: &str) {
    assert_close(corner.rx, radius, &format!("{what} rx"));
    assert_close(corner.ry, radius, &format!("{what} ry"));
}

/// A single radius larger than half the height must stay circular, giving a
/// stadium. This is the shape reported against Slint (slint-ui/slint#6506): a
/// 300x75 box with `border-radius: self.height`. Clamping the axes separately
/// gives `rx = 75`, `ry = 37.5`, an ellipse, and a visibly squashed end.
#[test]
fn a_radius_larger_than_the_box_stays_circular() {
    let mut path = Path::new();
    path.rounded_rect(0.0, 0.0, 300.0, 75.0, 75.0);
    let c = corners_of(&path, 0.0, 0.0, 300.0, 75.0);

    // f = min(300/150, 300/150, 75/150, 75/150) = 0.5, so every radius halves.
    for (corner, name) in [
        (c.top_left, "top left"),
        (c.top_right, "top right"),
        (c.bottom_right, "bottom right"),
        (c.bottom_left, "bottom left"),
    ] {
        assert_circular(corner, 37.5, name);
    }
}

/// The same rule stated as the property that actually matters: for a single
/// scalar radius the two axes of every corner stay equal, whatever the box.
#[test]
fn a_scalar_radius_never_produces_an_ellipse() {
    for (w, h, r) in [
        (300.0, 75.0, 75.0),
        (75.0, 300.0, 75.0),
        (100.0, 10.0, 400.0),
        (10.0, 100.0, 400.0),
        (50.0, 50.0, 25.0),
        (120.0, 40.0, 20.0),
    ] {
        let mut path = Path::new();
        path.rounded_rect(0.0, 0.0, w, h, r);
        let c = corners_of(&path, 0.0, 0.0, w, h);
        for (corner, name) in [
            (c.top_left, "top left"),
            (c.top_right, "top right"),
            (c.bottom_right, "bottom right"),
            (c.bottom_left, "bottom left"),
        ] {
            assert!(
                (corner.rx - corner.ry).abs() < 0.01,
                "{w}x{h} r={r}: {name} corner is elliptical, rx={} ry={}",
                corner.rx,
                corner.ry
            );
            // It must also still fit inside the box.
            assert!(corner.rx <= w / 2.0 + 0.01 || r > w, "{name} rx overflows");
        }
    }
}

/// Radii that already fit are left exactly as given.
#[test]
fn radii_that_fit_are_not_reduced() {
    let mut path = Path::new();
    path.rounded_rect(0.0, 0.0, 100.0, 100.0, 20.0);
    let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);
    assert_circular(c.top_left, 20.0, "top left");
    assert_circular(c.bottom_right, 20.0, "bottom right");

    // Exactly filling the side is still a fit: 50 + 50 == 100.
    let mut path = Path::new();
    path.rounded_rect(0.0, 0.0, 100.0, 100.0, 50.0);
    let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);
    assert_circular(c.top_left, 50.0, "top left at the limit");
    assert_circular(c.bottom_right, 50.0, "bottom right at the limit");
}

/// A corner may use the whole side when the corner next to it uses none of it.
/// The sum along each side is what is bounded, not the individual radius, so a
/// 100x100 box takes a top left radius of 100. An implementation that clamps to
/// half the dimension wrongly cuts this to 50.
#[test]
fn one_large_corner_beside_a_square_one_keeps_its_radius() {
    let mut path = Path::new();
    path.rounded_rect_varying(0.0, 0.0, 100.0, 100.0, 100.0, 0.0, 0.0, 0.0);
    let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);

    assert_circular(c.top_left, 100.0, "top left");
    assert_circular(c.top_right, 0.0, "top right");
    assert_circular(c.bottom_right, 0.0, "bottom right");
    assert_circular(c.bottom_left, 0.0, "bottom left");
}

/// Only one side overflowing still scales every corner, so their proportions to
/// one another are preserved. Cross-checked against Chrome.
#[test]
fn overflow_on_one_side_scales_every_corner() {
    let mut path = Path::new();
    // Top side needs 80 + 80 = 160 out of 100, so f = 0.625.
    path.rounded_rect_varying(0.0, 0.0, 100.0, 100.0, 80.0, 80.0, 20.0, 20.0);
    let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);

    assert_circular(c.top_left, 50.0, "top left");
    assert_circular(c.top_right, 50.0, "top right");
    assert_circular(c.bottom_right, 12.5, "bottom right");
    assert_circular(c.bottom_left, 12.5, "bottom left");
}

/// The factor is taken from the worst side, not the first one that overflows.
#[test]
fn the_scale_comes_from_the_most_overflowing_side() {
    let mut path = Path::new();
    // Left side needs 60 + 60 = 120 out of 40 (f = 1/3); the top needs
    // 60 + 10 = 70 out of 200 and does not overflow.
    path.rounded_rect_varying(0.0, 0.0, 200.0, 40.0, 60.0, 10.0, 10.0, 60.0);
    let c = corners_of(&path, 0.0, 0.0, 200.0, 40.0);

    assert_circular(c.top_left, 20.0, "top left");
    assert_circular(c.bottom_left, 20.0, "bottom left");
    assert_circular(c.top_right, 10.0 / 3.0, "top right");
    assert_circular(c.bottom_right, 10.0 / 3.0, "bottom right");
}

/// A radius that is negative or not finite does not describe a corner. It must
/// leave that corner square instead of reaching the geometry, where a negative
/// value bulges the corner outwards and a NaN spreads through every coordinate
/// and silently drops the shape in the tessellator.
#[test]
fn unusable_radii_leave_the_corner_square() {
    for bad in [-10.0f32, f32::NAN, f32::NEG_INFINITY] {
        let mut path = Path::new();
        path.rounded_rect_varying(0.0, 0.0, 100.0, 100.0, bad, 20.0, 20.0, 20.0);
        let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);

        assert_circular(c.top_left, 0.0, &format!("top left for {bad}"));
        assert_circular(c.top_right, 20.0, &format!("top right for {bad}"));

        for verb in path.verbs() {
            if let Verb::BezierTo(a, b, cc, d, e, f) = verb {
                for v in [a, b, cc, d, e, f] {
                    assert!(v.is_finite(), "{bad} produced a non-finite coordinate");
                }
            }
        }
    }
}

/// A radius that is merely small is still a corner. nanovg, and femtovg after
/// it, dropped every radius under 0.1 and emitted a plain rectangle instead,
/// which is a threshold in path units that knows nothing about the transform or
/// about how the shape will be painted.
///
/// Both of those bite. Under a scale of 100 a radius of 0.05 is a five pixel
/// corner, not a square one. And stroking magnifies it: a stroke is the path
/// offset by half the line width, so a 0.01 corner stroked 50 wide has an outer
/// edge of radius 25.01, while the squared-off substitute has a miter spike out
/// to the corner point. That is slint-ui/slint#1988, where a rectangle whose
/// background is filled with the outer radius and whose border is stroked along
/// the inset path had the border poke out past the background at every corner.
#[test]
fn small_radii_still_round_the_corner() {
    for radius in [0.001f32, 0.01, 0.05, 0.099] {
        let mut path = Path::new();
        path.rounded_rect(0.0, 0.0, 100.0, 100.0, radius);

        // Stated separately from the radii below, because the tolerance that
        // reads a corner back off the path is wider than these radii are: a
        // squared-off shape would otherwise pass on the numbers alone.
        let curves = path.verbs().filter(|verb| matches!(verb, Verb::BezierTo(..))).count();
        assert_eq!(curves, 4, "radius {radius} lost its corners");

        let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);

        assert_circular(c.top_left, radius, &format!("top left for {radius}"));
        assert_circular(c.top_right, radius, &format!("top right for {radius}"));
        assert_circular(c.bottom_right, radius, &format!("bottom right for {radius}"));
        assert_circular(c.bottom_left, radius, &format!("bottom left for {radius}"));
    }
}

/// From WPT `2d.path.roundrect.negative`: with a negative width or height the
/// rectangle is drawn back towards its origin, and the top left radius rounds the
/// corner at (`x`, `y`) wherever that lands on screen. The reduction has to run
/// on the side lengths rather than the signed extents, or a side summing to zero
/// divides into a negative width and scales every radius by minus infinity.
///
/// Verified to match Chrome on the scene that test draws: the four quadrants each
/// keep their outermost corner rounded.
#[test]
fn wpt_negative_extents_round_the_origin_corner() {
    // Right to left: the origin (100, 0) is the top right corner on screen.
    let mut path = Path::new();
    path.rounded_rect_varying(100.0, 0.0, -50.0, 25.0, 10.0, 0.0, 0.0, 0.0);
    let mut points = Vec::new();
    for verb in path.verbs() {
        match verb {
            Verb::MoveTo(px, py) | Verb::LineTo(px, py) => points.push((px, py)),
            Verb::BezierTo(_, _, _, _, px, py) => points.push((px, py)),
            _ => {}
        }
    }
    // The curve leaving the origin corner runs between (x - 10, y) and (x, y + 10).
    assert_close(points[7].0, 90.0, "top left arc start x");
    assert_close(points[7].1, 0.0, "top left arc start y");
    assert_close(points[0].0, 100.0, "top left arc end x");
    assert_close(points[0].1, 10.0, "top left arc end y");

    // The other three corners stay square, so the shape keeps its full extent.
    let c = corners_of(&path, 100.0, 0.0, -50.0, 25.0);
    assert_close(c.top_right.rx, 0.0, "top right rx");
    assert_close(c.bottom_right.rx, 0.0, "bottom right rx");
    assert_close(c.bottom_left.rx, 0.0, "bottom left rx");
}

/// Radii far larger than anything drawable still round the corners as far as the
/// box allows, rather than collapsing the shape back to a plain rectangle.
///
/// This is where reducing by a factor has a trap that clamping does not: the two
/// radii on a side have to be added together first, and two enormous ones
/// overflow single precision to infinity, which makes the factor zero and takes
/// every corner with it. Skia adds them at double precision for this reason,
/// noting in `SkRRect::scaleRadii` that single precision "can completely miss the
/// fact that a scale is required" (crbug.com/463920), and Gecko does the same.
/// Flutter, which scales in single precision, has the visible form of this:
/// `BorderRadius.circular(double.infinity)` produces a square box
/// (flutter/flutter#124525) while a merely huge radius produces a circle.
#[test]
fn enormous_radii_round_as_far_as_they_fit() {
    for radius in [1.0e30f32, f32::MAX, f32::INFINITY] {
        let mut path = Path::new();
        path.rounded_rect(0.0, 0.0, 100.0, 100.0, radius);
        let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);
        for (corner, name) in [
            (c.top_left, "top left"),
            (c.top_right, "top right"),
            (c.bottom_right, "bottom right"),
            (c.bottom_left, "bottom left"),
        ] {
            assert_circular(corner, 50.0, &format!("radius {radius} {name}"));
        }
    }
}

/// Rectangles given with a negative width or height are drawn from the opposite
/// corner; the radii follow the sides rather than escaping the box.
#[test]
fn reversed_rectangles_keep_their_radii_inside() {
    let mut path = Path::new();
    path.rounded_rect(100.0, 100.0, -300.0, -75.0, 75.0);
    let c = corners_of(&path, 100.0, 100.0, -300.0, -75.0);

    // The magnitudes match the positive case; the signs follow the direction.
    for (corner, name) in [
        (c.top_left, "top left"),
        (c.top_right, "top right"),
        (c.bottom_right, "bottom right"),
        (c.bottom_left, "bottom left"),
    ] {
        assert_close(corner.rx.abs(), 37.5, &format!("{name} rx magnitude"));
        assert_close(corner.ry.abs(), 37.5, &format!("{name} ry magnitude"));
    }
}

/// Vectors ported from WPT `css/css-backgrounds/border-radius-sum-of-radii-001.htm`,
/// whose border box is 100x100. Each case there overlays the reduced shape on a
/// reference drawn with the already-reduced radii and passes when nothing shows
/// through, so the reference values are the used radii the rule must produce.
///
/// Case 4 is the one worth reading twice: the bottom and right sides both fit on
/// their own, yet their radii still shrink from 30 to 25, because the factor
/// comes from the overflowing top side and applies to every corner. An
/// implementation that reduces only what overflows leaves them at 30.
#[test]
fn wpt_css_border_radius_sum_of_radii() {
    // [top left, top right, bottom right, bottom left], as the CSS shorthand orders them.
    let cases: [([f32; 4], [f32; 4]); 8] = [
        ([60.0, 150.0, 30.0, 30.0], [28.5714, 71.4286, 14.2857, 14.2857]),
        ([0.0, 150.0, 0.0, 30.0], [0.0, 100.0, 0.0, 20.0]),
        ([150.0, 0.0, 30.0, 0.0], [100.0, 0.0, 20.0, 0.0]),
        ([50.0, 70.0, 30.0, 30.0], [41.6667, 58.3333, 25.0, 25.0]),
        ([150.0, 150.0, 30.0, 30.0], [50.0, 50.0, 10.0, 10.0]),
        ([30.0, 50.0, 70.0, 30.0], [25.0, 41.6667, 58.3333, 25.0]),
        ([30.0, 30.0, 50.0, 70.0], [25.0, 25.0, 41.6667, 58.3333]),
        ([70.0, 30.0, 30.0, 50.0], [58.3333, 25.0, 25.0, 41.6667]),
    ];

    for (input, expected) in cases {
        let mut path = Path::new();
        path.rounded_rect_varying(0.0, 0.0, 100.0, 100.0, input[0], input[1], input[2], input[3]);
        let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);
        let label = format!("radii {input:?}");
        assert_circular(c.top_left, expected[0], &format!("{label} top left"));
        assert_circular(c.top_right, expected[1], &format!("{label} top right"));
        assert_circular(c.bottom_right, expected[2], &format!("{label} bottom right"));
        assert_circular(c.bottom_left, expected[3], &format!("{label} bottom left"));
    }
}

/// From Gecko's `layout/reftests/border-radius/corner-3.html`, whose border box is
/// 30 wide and 130 tall with the two right corners rounded by 30. Each horizontal
/// side holds one radius of 30 in 30 units of width, so nothing is reduced and
/// the right side bulges across the full width. Clamping to half the box would
/// halve both to 15.
#[test]
fn gecko_corner_3_keeps_full_width_radii() {
    let mut path = Path::new();
    path.rounded_rect_varying(0.0, 0.0, 30.0, 130.0, 0.0, 30.0, 30.0, 0.0);
    let c = corners_of(&path, 0.0, 0.0, 30.0, 130.0);

    assert_circular(c.top_left, 0.0, "top left");
    assert_circular(c.top_right, 30.0, "top right");
    assert_circular(c.bottom_right, 30.0, "bottom right");
    assert_circular(c.bottom_left, 0.0, "bottom left");
}

/// From Gecko's `layout/reftests/border-radius/border-reduce-height.html`, added
/// with its implementation of this rule: a 60x20 box with radii 5, 20, 5, 20
/// reduces by 0.8 to 4, 16, 4, 16.
///
/// The factor comes from the short vertical sides, yet it also reduces the
/// horizontal extent of every corner, which the 60 units of width had ample room
/// for. That is the part a per-side or per-axis correction cannot reproduce.
#[test]
fn gecko_border_reduce_height_scales_the_wide_axis_too() {
    let mut path = Path::new();
    path.rounded_rect_varying(0.0, 0.0, 60.0, 20.0, 5.0, 20.0, 5.0, 20.0);
    let c = corners_of(&path, 0.0, 0.0, 60.0, 20.0);

    assert_circular(c.top_left, 4.0, "top left");
    assert_circular(c.top_right, 16.0, "top right");
    assert_circular(c.bottom_right, 4.0, "bottom right");
    assert_circular(c.bottom_left, 16.0, "bottom left");
}

/// From WPT `css/css-backgrounds/border-radius-sum-of-radii-002.htm`: radii that
/// exactly fill a side give a factor of one, and the rule only applies below one,
/// so nothing is reduced. Reducing here would shrink shapes that are already
/// correct.
#[test]
fn wpt_a_factor_of_exactly_one_reduces_nothing() {
    let mut path = Path::new();
    path.rounded_rect_varying(0.0, 0.0, 100.0, 100.0, 50.0, 50.0, 30.0, 30.0);
    let c = corners_of(&path, 0.0, 0.0, 100.0, 100.0);

    assert_circular(c.top_left, 50.0, "top left");
    assert_circular(c.top_right, 50.0, "top right");
    assert_circular(c.bottom_right, 30.0, "bottom right");
    assert_circular(c.bottom_left, 30.0, "bottom left");
}

/// The geometry behind WPT `2d.path.roundrect.radius.intersecting.1` and `.2`,
/// which fill a 100x50 canvas with radii of 40 and then 1000. Both reduce to a
/// circular 25, giving a stadium.
///
/// Those two tests only sample pixels well inside the shape, so per-axis
/// clamping — 40x25 and 50x25 ellipses — passes them both. Asserting the radii
/// directly is what separates the two behaviours, so this covers the intent of
/// the WPT cases rather than reproducing assertions that cannot fail.
#[test]
fn wpt_canvas_intersecting_radii_reduce_to_a_circle() {
    for radius in [40.0f32, 1000.0] {
        let mut path = Path::new();
        path.rounded_rect(0.0, 0.0, 100.0, 50.0, radius);
        let c = corners_of(&path, 0.0, 0.0, 100.0, 50.0);
        for (corner, name) in [
            (c.top_left, "top left"),
            (c.top_right, "top right"),
            (c.bottom_right, "bottom right"),
            (c.bottom_left, "bottom left"),
        ] {
            assert_circular(corner, 25.0, &format!("radius {radius} {name}"));
        }
    }
}

/// A box with no room at all collapses the radii rather than emitting geometry
/// outside it or dividing by zero.
#[test]
fn a_degenerate_box_produces_finite_geometry() {
    for (w, h) in [(0.0f32, 100.0f32), (100.0, 0.0), (0.0, 0.0)] {
        let mut path = Path::new();
        path.rounded_rect(0.0, 0.0, w, h, 40.0);
        for verb in path.verbs() {
            let coords = match verb {
                Verb::MoveTo(a, b) | Verb::LineTo(a, b) => vec![a, b],
                Verb::BezierTo(a, b, c, d, e, f) => vec![a, b, c, d, e, f],
                _ => vec![],
            };
            for v in coords {
                assert!(v.is_finite(), "{w}x{h} produced a non-finite coordinate");
            }
        }
    }
}
