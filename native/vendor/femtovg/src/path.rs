use std::{
    cell::{RefCell, RefMut},
    f32::consts::PI,
    slice,
};

use crate::geometry::{Position, Transform2D, Vector};
#[cfg(feature = "textlayout")]
use rustybuzz::ttf_parser;

mod cache;
pub use cache::{Convexity, PathCache};

// Length proportional to radius of a cubic bezier handle for 90deg arcs.
const KAPPA90: f32 = 0.552_284_8; // 0.552_284_749_3;

/// Specifies whether a shape is solid or a hole when adding it to a path.
///
/// The default value is `Solid`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Default)]
pub enum Solidity {
    /// The shape is solid (filled).
    #[default]
    Solid = 1,
    /// The shape is a hole (not filled).
    Hole = 2,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PackedVerb {
    MoveTo,
    LineTo,
    BezierTo,
    Solid,
    Hole,
    Close,
}

/// A verb describes how to interpret one or more points to continue the countour
/// of a [`Path`].
#[derive(Copy, Clone, Debug)]
pub enum Verb {
    /// Terminates the current sub-path and defines the new current point by the
    /// given x/y f32 coordinates.
    MoveTo(f32, f32),
    /// Describes that the contour of the path should continue as a line from the
    /// current point to the given x/y f32 coordinates.
    LineTo(f32, f32),
    /// Describes that the contour of the path should continue as a cubie bezier segment from the
    /// current point via two control points (as f32 pairs) to the point in the last f32 pair.
    BezierTo(f32, f32, f32, f32, f32, f32),
    /// Sets the current sub-path winding to be solid.
    Solid,
    /// Sets the current sub-path winding to be hole.
    Hole,
    /// Closes the current sub-path.
    Close,
}

impl Verb {
    fn num_coordinates(&self) -> usize {
        match *self {
            Self::MoveTo(..) => 1,
            Self::LineTo(..) => 1,
            Self::BezierTo(..) => 3,
            Self::Solid => 0,
            Self::Hole => 0,
            Self::Close => 0,
        }
    }

    fn from_packed(packed: &PackedVerb, coords: &[Position]) -> Self {
        match *packed {
            PackedVerb::MoveTo => Self::MoveTo(coords[0].x, coords[0].y),
            PackedVerb::LineTo => Self::LineTo(coords[0].x, coords[0].y),
            PackedVerb::BezierTo => Self::BezierTo(
                coords[0].x,
                coords[0].y,
                coords[1].x,
                coords[1].y,
                coords[2].x,
                coords[2].y,
            ),
            PackedVerb::Solid => Self::Solid,
            PackedVerb::Hole => Self::Hole,
            PackedVerb::Close => Self::Close,
        }
    }
}

/// A collection of verbs (`move_to()`, `line_to()`, `bezier_to()`, etc.)
/// describing one or more contours.
///
/// A `Path` is `Send` but intentionally not `Sync`: the interior tessellation
/// cache uses a [`RefCell`], so sharing a `&Path` between threads is rejected
/// at compile time. Move (or clone) a `Path` into each thread instead.
///
/// ```compile_fail
/// let path = femtovg::Path::new();
/// std::thread::scope(|scope| {
///     // ERROR: `&Path` is not `Send` because `Path` is not `Sync`.
///     scope.spawn(|| path.is_empty());
/// });
/// ```
#[derive(Default, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Path {
    verbs: Vec<PackedVerb>,
    coords: Vec<Position>,
    last_pos: Position,
    dist_tol: f32,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) cache: RefCell<Option<(u64, PathCache)>>,
}

impl Path {
    /// Creates a new empty path with a distance tolerance of 0.01.
    pub fn new() -> Self {
        Self {
            dist_tol: 0.01,
            ..Default::default()
        }
    }

    /// Returns the memory size in bytes used by the path.
    pub fn size(&self) -> usize {
        std::mem::size_of::<PackedVerb>() * self.verbs.len() + std::mem::size_of::<f32>() * self.coords.len()
    }

    /// Checks if the path is empty (contains no verbs).
    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty()
    }

    /// Sets the distance tolerance used for path operations.
    pub fn set_distance_tolerance(&mut self, value: f32) {
        self.dist_tol = value;
    }

    /// Returns an iterator over the path's verbs.
    pub fn verbs(&self) -> PathIter<'_> {
        PathIter {
            verbs: self.verbs.iter(),
            coords: &self.coords,
        }
    }

    pub(crate) fn cache<'a>(&'a self, transform: &Transform2D, tess_tol: f32, dist_tol: f32) -> RefMut<'a, PathCache> {
        // The path cache saves a flattened and transformed version of the path. If client code calls
        // (fill|stroke)_path repeatedly with the same Path under the same transform circumstances then it will be
        // retrieved from cache. I'm not sure if transform.cache_key() is actually good enough for this
        // and if it will produce the correct cache keys under different float edge cases.

        let key = transform.cache_key();

        // this shouldn't need a bool once non lexic lifetimes are stable
        let needs_rebuild = if let Some((transform_cache_key, _cache)) = &*self.cache.borrow() {
            key != *transform_cache_key
        } else {
            true
        };

        if needs_rebuild {
            let path_cache = PathCache::new(self.verbs(), transform, tess_tol, dist_tol);
            *self.cache.borrow_mut() = Some((key, path_cache));
        }

        RefMut::map(self.cache.borrow_mut(), |cache| &mut cache.as_mut().unwrap().1)
    }

    /// Returns a path containing only the visible segments of this path when
    /// stroked with the given dash pattern and offset.
    ///
    /// Empty patterns, patterns whose entries sum to zero, and patterns with
    /// non-finite or negative entries return this path unchanged. Odd-length
    /// patterns are repeated once, matching SVG and Canvas 2D behavior.
    pub fn dashed(&self, dash: &[f32], offset: f32) -> Self {
        self.dashed_with_tolerance(dash, offset, self.dist_tol)
    }

    pub(crate) fn dashed_with_tolerance(&self, dash: &[f32], offset: f32, tess_tol: f32) -> Self {
        let dash = normalize_dash_pattern(dash);
        if dash.is_empty() {
            return self.clone();
        }

        let contours = self.flattened_contours(tess_tol, self.dist_tol);
        let mut dashed = Self::new();
        dashed.dist_tol = self.dist_tol;

        for contour in contours {
            if contour.points.len() < 2 {
                continue;
            }

            let mut cursor = DashCursor::new(&dash, offset);
            for segment in contour.points.windows(2) {
                dash_line_segment(&mut dashed, segment[0], segment[1], &mut cursor, self.dist_tol);
            }

            if contour.closed {
                dash_line_segment(
                    &mut dashed,
                    *contour.points.last().unwrap(),
                    contour.points[0],
                    &mut cursor,
                    self.dist_tol,
                );
            }
        }

        dashed
    }

    fn flattened_contours(&self, tess_tol: f32, dist_tol: f32) -> Vec<FlattenedContour> {
        let mut contours = Vec::new();
        let mut current = FlattenedContour::default();
        let mut current_pos = Position::default();
        let mut first_pos = None;

        let finish_current = |contours: &mut Vec<FlattenedContour>, current: &mut FlattenedContour| {
            if current.points.len() >= 2 {
                contours.push(std::mem::take(current));
            } else {
                current.points.clear();
                current.closed = false;
            }
        };

        for verb in self.verbs() {
            match verb {
                Verb::MoveTo(x, y) => {
                    finish_current(&mut contours, &mut current);
                    current_pos = Position { x, y };
                    first_pos = Some(current_pos);
                    current.points.push(current_pos);
                }
                Verb::LineTo(x, y) => {
                    current_pos = Position { x, y };
                    push_flattened_point(&mut current.points, current_pos, dist_tol);
                }
                Verb::BezierTo(c1x, c1y, c2x, c2y, x, y) => {
                    let c1 = Position { x: c1x, y: c1y };
                    let c2 = Position { x: c2x, y: c2y };
                    let end = Position { x, y };
                    flatten_bezier(&mut current.points, current_pos, c1, c2, end, 0, tess_tol, dist_tol);
                    current_pos = end;
                }
                Verb::Close => {
                    if let Some(first) = first_pos {
                        current.closed = true;
                        current_pos = first;
                    }
                }
                Verb::Solid | Verb::Hole => {}
            }
        }

        finish_current(&mut contours, &mut current);
        contours
    }

    // Path funcs

    /// Starts a new sub-path with the specified point as the first point.
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.append(&[PackedVerb::MoveTo], &[Position { x, y }]);
    }

    /// Adds a line segment from the last point in the path to the specified point.
    pub fn line_to(&mut self, x: f32, y: f32) {
        self.append(&[PackedVerb::LineTo], &[Position { x, y }]);
    }

    /// Adds a cubic bezier segment from the last point in the path via two control points to the specified point.
    pub fn bezier_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.append(
            &[PackedVerb::BezierTo],
            &[
                Position { x: c1x, y: c1y },
                Position { x: c2x, y: c2y },
                Position { x, y },
            ],
        );
    }

    /// Adds a quadratic bezier segment from the last point in the path via a control point to the specified point.
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let pos0 = self.last_pos;
        let cpos = Position { x: cx, y: cy };
        let pos = Position { x, y };
        let pos1 = pos0 + (cpos - pos0) * (2.0 / 3.0);
        let pos2 = pos + (cpos - pos) * (2.0 / 3.0);

        self.append(&[PackedVerb::BezierTo], &[pos1, pos2, pos]);
    }

    /// Closes the current sub-path with a line segment.
    pub fn close(&mut self) {
        self.append(&[PackedVerb::Close], &[]);
    }

    /// Sets the current sub-path winding, see [`Solidity`].
    pub fn solidity(&mut self, solidity: Solidity) {
        match solidity {
            Solidity::Solid => self.append(&[PackedVerb::Solid], &[]),
            Solidity::Hole => self.append(&[PackedVerb::Hole], &[]),
        }
    }

    /// Creates new circle arc shaped sub-path. The arc center is at `cx`,`cy`, the arc radius is `r`,
    /// and the arc is drawn from angle `a0` to `a1`, and swept in direction `dir` (Winding)
    /// Angles are specified in radians.
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, dir: Solidity) {
        let cpos = Position { x: cx, y: cy };

        let mut da = a1 - a0;

        if dir == Solidity::Hole {
            if da.abs() >= PI * 2.0 {
                da = PI * 2.0;
            } else {
                while da < 0.0 {
                    da += PI * 2.0
                }
            }
        } else if da.abs() >= PI * 2.0 {
            da = -PI * 2.0;
        } else {
            while da > 0.0 {
                da -= PI * 2.0
            }
        }

        // Split arc into max 90 degree segments.
        let ndivs = ((da.abs() / (PI * 0.5) + 0.5) as i32).clamp(1, 5);
        let hda = (da / ndivs as f32) / 2.0;
        // 4/3 * tan(hda/2), the same value as 4/3 * (1 - cos hda) / sin hda by
        // the half-angle identity, but defined where that form is not: a sweep
        // of zero leaves both parts of the quotient at zero and yields NaN,
        // which then spreads through every control point of the shape. The
        // tangent form also keeps its accuracy for very short segments, where
        // 1 - cos hda loses all of its significant bits.
        let mut kappa = (4.0 / 3.0 * (hda / 2.0).tan()).abs();

        let mut commands = Vec::with_capacity(ndivs as usize);
        let mut coords = Vec::with_capacity(ndivs as usize);

        if dir == Solidity::Solid {
            kappa = -kappa;
        }

        let (mut ppos, mut ptanpos) = (Position { x: 0.0, y: 0.0 }, Vector::zero());

        for i in 0..=ndivs {
            let a = a0 + da * (i as f32 / ndivs as f32);
            let dpos = Vector::from_angle(a);
            let pos = cpos + dpos * r;
            let tanpos = -dpos.orthogonal() * r * kappa;

            if i == 0 {
                let first_move = if self.verbs.is_empty() {
                    PackedVerb::MoveTo
                } else {
                    PackedVerb::LineTo
                };

                commands.push(first_move);
                coords.extend_from_slice(&[pos]);
            } else {
                commands.push(PackedVerb::BezierTo);
                let pos1 = ppos + ptanpos;
                let pos2 = pos - tanpos;
                coords.extend_from_slice(&[pos1, pos2, pos]);
            }

            ppos = pos;
            ptanpos = tanpos;
        }

        self.append(&commands, &coords);
    }

    /// Adds an arc segment at the corner defined by the last path point and two specified points.
    pub fn arc_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, radius: f32) {
        if self.verbs.is_empty() {
            return;
        }

        let pos0 = self.last_pos;
        let pos1 = Position { x: x1, y: y1 };
        let pos2 = Position { x: x2, y: y2 };

        // Handle degenerate cases.
        if Position::equals(pos0, pos1, self.dist_tol)
            || Position::equals(pos1, pos2, self.dist_tol)
            || Position::segment_distance(pos1, pos0, pos2) < self.dist_tol * self.dist_tol
            || radius < self.dist_tol
        {
            // The corner cannot be rounded, so the line to it is the whole
            // segment. Falling through would append a second, meaningless arc
            // built from vectors that are about to be normalized to nothing.
            return self.line_to(pos1.x, pos1.y);
        }

        let mut dpos0 = pos0 - pos1;
        let mut dpos1 = pos2 - pos1;

        dpos0.normalize();
        dpos1.normalize();

        let a = dpos0.dot(dpos1).acos();
        let d = radius / (a / 2.0).tan();

        if d > 10000.0 {
            return self.line_to(pos1.x, pos1.y);
        }

        let (cpos, a0, a1, dir);

        if dpos0.cross(dpos1) > 0.0 {
            cpos = pos1 + dpos0 * d + dpos0.orthogonal() * radius;
            a0 = dpos0.angle();
            a1 = (-dpos1).angle();
            dir = Solidity::Hole;
        } else {
            cpos = pos1 + dpos0 * d - dpos0.orthogonal() * radius;
            a0 = (-dpos0).angle();
            a1 = dpos1.angle();
            dir = Solidity::Solid;
        }

        self.arc(cpos.x, cpos.y, radius, a0 + PI / 2.0, a1 + PI / 2.0, dir);
    }

    /// Adds an SVG elliptical arc (the path data `A`/`a` command) from the
    /// current point to (`x`, `y`), emitting cubic bezier segments.
    ///
    /// `rx`/`ry` are the ellipse radii, `x_axis_rotation` is the rotation of the
    /// ellipse's x-axis relative to the current coordinate system **in radians**
    /// (the SVG attribute is in degrees), `large_arc` selects the larger of the
    /// two possible arc sweeps, and `sweep` selects the positive-angle
    /// (clockwise in SVG's y-down system) direction.
    ///
    /// If the path is empty the arc starts from the origin, matching how the
    /// other arc builders treat an empty current point. Following the W3C SVG
    /// implementation notes (section F.6): identical start/end points omit the
    /// arc, a zero radius degrades to a straight line, negative radii use their
    /// absolute value, and radii too small to span the endpoints are scaled up.
    // The seven parameters mirror the SVG path arc command `A rx ry
    // x-axis-rotation large-arc-flag sweep-flag x y` one-to-one, so the count is
    // fixed by the spec rather than a design choice.
    #[allow(clippy::too_many_arguments)]
    pub fn svg_arc_to(&mut self, rx: f32, ry: f32, x_axis_rotation: f32, large_arc: bool, sweep: bool, x: f32, y: f32) {
        // The HTML Canvas path primitives (`ellipse()`, `arc()`, `arcTo()`)
        // return early without changing the path "if any of the arguments are
        // infinite or NaN"; apply the same rule here so a single bad value
        // cannot poison every later coordinate derived from the current point.
        if !rx.is_finite() || !ry.is_finite() || !x_axis_rotation.is_finite() || !x.is_finite() || !y.is_finite() {
            return;
        }

        let start = if self.verbs.is_empty() {
            let origin = Position { x: 0.0, y: 0.0 };
            self.move_to(origin.x, origin.y);
            origin
        } else {
            self.last_pos
        };
        let end = Position { x, y };

        // F.6.2: identical endpoints omit the arc entirely.
        if start.x == end.x && start.y == end.y {
            return;
        }

        // F.6.6 step 1 / F.6.2: a zero radius degrades to a straight line.
        // F.6.2: negative radii drop their sign.
        //
        // The endpoint-to-center conversion below runs in f64: the squared
        // terms of F.6.6's lambda and the F.6.5.2 quotient overflow f32's
        // exponent range long before the resulting arc geometry itself leaves
        // f32 range (e.g. lambda for rx = 1e-30 radii that merely need to be
        // scaled up, or (1e30)^2 midpoint terms for large translations).
        // Reference SVG arc implementations (resvg via kurbo, Batik) perform
        // this conversion in f64 for the same reason.
        let mut rx = f64::from(rx).abs();
        let mut ry = f64::from(ry).abs();
        if rx == 0.0 || ry == 0.0 {
            self.line_to(end.x, end.y);
            return;
        }

        let (sin_phi, cos_phi) = f64::from(x_axis_rotation).sin_cos();

        // F.6.5.1: midpoint translation followed by rotation by -phi.
        let dx2 = (f64::from(start.x) - f64::from(end.x)) * 0.5;
        let dy2 = (f64::from(start.y) - f64::from(end.y)) * 0.5;
        let x1p = cos_phi * dx2 + sin_phi * dy2;
        let y1p = -sin_phi * dx2 + cos_phi * dy2;

        // F.6.6 step 3: scale the radii up if they cannot span the endpoints.
        let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
        if lambda > 1.0 {
            let s = lambda.sqrt();
            rx *= s;
            ry *= s;
        }

        // F.6.5.2: center in the rotated/translated frame.
        let rx2 = rx * rx;
        let ry2 = ry * ry;
        let x1p2 = x1p * x1p;
        let y1p2 = y1p * y1p;
        let numerator = (rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2).max(0.0);
        let denominator = rx2 * y1p2 + ry2 * x1p2;
        let mut coef = (numerator / denominator).sqrt();
        // The sign is positive when the flags differ, negative when they match.
        if large_arc == sweep {
            coef = -coef;
        }
        let cxp = coef * (rx * y1p / ry);
        let cyp = coef * -(ry * x1p / rx);

        // F.6.5.3: transform the center back to the original frame.
        let cx = cos_phi * cxp - sin_phi * cyp + (f64::from(start.x) + f64::from(end.x)) * 0.5;
        let cy = sin_phi * cxp + cos_phi * cyp + (f64::from(start.y) + f64::from(end.y)) * 0.5;

        // F.6.5.5 / F.6.5.6: starting angle and swept angle via the angle helper.
        let ux = (x1p - cxp) / rx;
        let uy = (y1p - cyp) / ry;
        let vx = (-x1p - cxp) / rx;
        let vy = (-y1p - cyp) / ry;

        let theta1 = svg_arc_angle(1.0, 0.0, ux, uy);
        let mut delta = svg_arc_angle(ux, uy, vx, vy);

        // Enforce the sweep flag direction (F.6.5.6 modulo rule).
        if !sweep && delta > 0.0 {
            delta -= std::f64::consts::PI * 2.0;
        } else if sweep && delta < 0.0 {
            delta += std::f64::consts::PI * 2.0;
        }

        // Split into segments of at most 90 degrees and emit cubic beziers.
        // The sweep never exceeds a full turn (|delta| <= 2*pi), so at most four
        // 90-degree segments are ever emitted; the extra headroom only guards
        // against floating-point spill past the exact quarter-turn boundaries.
        const MAX_SEGMENTS: usize = 8;
        let ndivs = ((delta.abs() / (std::f64::consts::PI * 0.5)).ceil() as usize).clamp(1, MAX_SEGMENTS);
        let seg = delta / ndivs as f64;
        // Per-segment handle length: the same kappa `arc()` and `ellipse()` use,
        // 4/3 * tan(seg/4), which puts the curve through the midpoint of its own
        // segment. `arc()` spells it 4/3 * (1 - cos(seg/2)) / sin(seg/2), the
        // same value by the half-angle identity; the tangent form is used here
        // because it carries the sign of `seg`, so a negative sweep gets a handle
        // pointing back along the derivative without a separate branch. At a
        // quarter turn it is the KAPPA90 the other primitives are built from.
        let alpha = 4.0 / 3.0 * (seg * 0.25).tan();

        // The segment chain is staged on the stack: emission must stay
        // transactional (the finite-output guard below inspects the whole chain
        // before anything is appended to the path), and the bounded segment
        // count makes that possible without a per-call heap allocation.
        let mut coords = [Position { x: 0.0, y: 0.0 }; MAX_SEGMENTS * 3];

        // Point and derivative of the rotated ellipse at parametric angle t.
        let point = |t: f64| -> (f64, f64) {
            let (sin_t, cos_t) = t.sin_cos();
            (
                cx + rx * cos_phi * cos_t - ry * sin_phi * sin_t,
                cy + rx * sin_phi * cos_t + ry * cos_phi * sin_t,
            )
        };
        let derivative = |t: f64| -> (f64, f64) {
            let (sin_t, cos_t) = t.sin_cos();
            (
                -rx * cos_phi * sin_t - ry * sin_phi * cos_t,
                -rx * sin_phi * sin_t + ry * cos_phi * cos_t,
            )
        };
        let to_position = |p: (f64, f64)| Position {
            x: p.0 as f32,
            y: p.1 as f32,
        };

        for i in 0..ndivs {
            let t1 = theta1 + seg * i as f64;
            let t2 = t1 + seg;
            let p1 = point(t1);
            let p2 = point(t2);
            let d1 = derivative(t1);
            let d2 = derivative(t2);
            let c1 = (p1.0 + d1.0 * alpha, p1.1 + d1.1 * alpha);
            let c2 = (p2.0 - d2.0 * alpha, p2.1 - d2.1 * alpha);

            coords[i * 3] = to_position(c1);
            coords[i * 3 + 1] = to_position(c2);
            coords[i * 3 + 2] = to_position(p2);
        }
        let coords = &mut coords[..ndivs * 3];

        // Guarantee the path endpoint lands exactly on the requested point.
        coords[ndivs * 3 - 1] = end;

        // Extreme (but finite) inputs can describe arc geometry that exceeds
        // f32 range even though both endpoints are representable (e.g. F.6.6
        // scaling of wildly mismatched radii yields a control point beyond
        // f32::MAX). Degrade to the chord rather than emit non-finite vertices
        // that would poison downstream tessellation; this preserves the
        // invariant that the path always continues to the requested endpoint
        // with finite geometry.
        if coords
            .iter()
            .any(|position| !position.x.is_finite() || !position.y.is_finite())
        {
            self.line_to(end.x, end.y);
            return;
        }

        self.append(&[PackedVerb::BezierTo; MAX_SEGMENTS][..ndivs], coords);
    }

    /// Creates a new rectangle shaped sub-path.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.append(
            &[
                PackedVerb::MoveTo,
                PackedVerb::LineTo,
                PackedVerb::LineTo,
                PackedVerb::LineTo,
                PackedVerb::Close,
            ],
            &{
                let hoffset = Vector::x(w);
                let voffset = Vector::y(h);

                let tl = Position { x, y };
                let tr = tl + hoffset;
                let br = tr + voffset;
                let bl = tl + voffset;

                [tl, bl, br, tr]
            },
        );
    }

    /// Creates a new rounded rectangle shaped sub-path.
    pub fn rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        self.rounded_rect_varying(x, y, w, h, r, r, r, r);
    }

    /// Creates a new rounded rectangle shaped sub-path with varying radii for each corner.
    ///
    /// Radii that are too large for the rectangle are reduced: when the two radii
    /// along a side add up to more than that side's length, every radius is
    /// multiplied by the same factor until they all fit. Scaling them together
    /// keeps each corner the shape it was asked for, so a radius larger than half
    /// the height still yields a fully rounded end rather than a flattened one.
    /// This is the "overlapping curves" rule shared by CSS backgrounds and
    /// borders and by the Canvas `roundRect()` algorithm.
    ///
    /// A negative radius, or one that is NaN, does not describe a corner and is
    /// treated as zero, leaving that corner square. An infinite radius asks for
    /// as much rounding as there is room for and is reduced to fit, like any
    /// other radius that is too large. Only a radius of zero squares a corner
    /// off: however small the rest are, they are kept, since path units say
    /// nothing about how large the corner ends up once the transform and the
    /// pen have had their say.
    pub fn rounded_rect_varying(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        rad_top_left: f32,
        rad_top_right: f32,
        rad_bottom_right: f32,
        rad_bottom_left: f32,
    ) {
        // An infinite radius stands in for the largest one there is, so that the
        // reduction below brings it down to what fits rather than the shape
        // losing its corners entirely. Anything not positive, NaN included, is
        // not a corner at all.
        let usable = |r: f32| {
            if r.is_nan() || r <= 0.0 {
                0.0
            } else if r.is_infinite() {
                f32::MAX
            } else {
                r
            }
        };

        let mut tl = usable(rad_top_left);
        let mut tr = usable(rad_top_right);
        let mut br = usable(rad_bottom_right);
        let mut bl = usable(rad_bottom_left);

        // Only a shape with no corners at all is a plain rectangle. nanovg
        // squared off anything under 0.1 here, which is a threshold in path
        // units and so blind to both the transform and the line width: a 0.05
        // radius is five pixels under a scale of 100, and stroking a 0.01
        // corner 50 wide should give an outer edge of radius 25.01 rather than
        // a mitre spike (slint-ui/slint#1988).
        if tl == 0.0 && tr == 0.0 && br == 0.0 && bl == 0.0 {
            self.rect(x, y, w, h);
        } else {
            let (width, height) = (f64::from(w.abs()), f64::from(h.abs()));
            let (tl64, tr64) = (f64::from(tl), f64::from(tr));
            let (br64, bl64) = (f64::from(br), f64::from(bl));

            // One factor for every radius, taken from whichever side overflows
            // the most. Clamping the axes separately instead would turn a corner
            // into an ellipse. A side whose radii are both zero cannot overflow,
            // so it is left out of the comparison.
            //
            // The sums are taken at double precision, as Skia and Gecko also do.
            // Two large radii overflow to infinity when added in single
            // precision, which makes the factor zero and squares off every corner
            // of a shape that should merely have been reduced.
            let scale = [
                (width, tl64 + tr64),
                (width, bl64 + br64),
                (height, tl64 + bl64),
                (height, tr64 + br64),
            ]
            .into_iter()
            .filter(|&(_, sum)| sum > 0.0)
            .map(|(side, sum)| side / sum)
            .fold(1.0f64, f64::min);

            if scale < 1.0 {
                tl = (tl64 * scale) as f32;
                tr = (tr64 * scale) as f32;
                br = (br64 * scale) as f32;
                bl = (bl64 * scale) as f32;
            }

            let (sign_x, sign_y) = (w.signum(), h.signum());

            let rx_bl = bl * sign_x;
            let ry_bl = bl * sign_y;

            let rx_br = br * sign_x;
            let ry_br = br * sign_y;

            let rx_tr = tr * sign_x;
            let ry_tr = tr * sign_y;

            let rx_tl = tl * sign_x;
            let ry_tl = tl * sign_y;

            self.append(
                &[
                    PackedVerb::MoveTo,
                    PackedVerb::LineTo,
                    PackedVerb::BezierTo,
                    PackedVerb::LineTo,
                    PackedVerb::BezierTo,
                    PackedVerb::LineTo,
                    PackedVerb::BezierTo,
                    PackedVerb::LineTo,
                    PackedVerb::BezierTo,
                    PackedVerb::Close,
                ],
                &[
                    Position { x, y: y + ry_tl },
                    Position { x, y: y + h - ry_bl },
                    //
                    Position {
                        x,
                        y: y + h - ry_bl * (1.0 - KAPPA90),
                    },
                    Position {
                        x: x + rx_bl * (1.0 - KAPPA90),
                        y: y + h,
                    },
                    Position { x: x + rx_bl, y: y + h },
                    //
                    Position {
                        x: x + w - rx_br,
                        y: y + h,
                    },
                    //
                    Position {
                        x: x + w - rx_br * (1.0 - KAPPA90),
                        y: y + h,
                    },
                    Position {
                        x: x + w,
                        y: y + h - ry_br * (1.0 - KAPPA90),
                    },
                    Position {
                        x: x + w,
                        y: y + h - ry_br,
                    },
                    //
                    Position { x: x + w, y: y + ry_tr },
                    //
                    Position {
                        x: x + w,
                        y: y + ry_tr * (1.0 - KAPPA90),
                    },
                    Position {
                        x: x + w - rx_tr * (1.0 - KAPPA90),
                        y,
                    },
                    Position { x: x + w - rx_tr, y },
                    //
                    Position { x: x + rx_tl, y },
                    //
                    Position {
                        x: x + rx_tl * (1.0 - KAPPA90),
                        y,
                    },
                    Position {
                        x,
                        y: y + ry_tl * (1.0 - KAPPA90),
                    },
                    Position { x, y: y + ry_tl },
                ],
            );
        }
    }

    /// Creates a new ellipse shaped sub-path.
    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32) {
        self.append(
            &[
                PackedVerb::MoveTo,
                PackedVerb::BezierTo,
                PackedVerb::BezierTo,
                PackedVerb::BezierTo,
                PackedVerb::BezierTo,
                PackedVerb::Close,
            ],
            &{
                let cpos = Position { x: cx, y: cy };
                let hoffset = Vector::x(rx);
                let voffset = Vector::y(ry);
                [
                    cpos - hoffset,
                    cpos - hoffset + voffset * KAPPA90,
                    cpos - hoffset * KAPPA90 + voffset,
                    cpos + voffset,
                    cpos + hoffset * KAPPA90 + voffset,
                    cpos + hoffset + voffset * KAPPA90,
                    cpos + hoffset,
                    cpos + hoffset - voffset * KAPPA90,
                    cpos + hoffset * KAPPA90 - voffset,
                    cpos - voffset,
                    cpos - hoffset * KAPPA90 - voffset,
                    cpos - hoffset - voffset * KAPPA90,
                    cpos - hoffset,
                ]
            },
        );
    }

    /// Creates a new circle shaped sub-path.
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32) {
        self.ellipse(cx, cy, r, r);
    }

    /// Appends a slice of verbs and coordinates to the path.
    fn append(&mut self, verbs: &[PackedVerb], coords: &[Position]) {
        if !coords.is_empty() {
            self.last_pos = coords[coords.len() - 1];
        }

        self.verbs.extend_from_slice(verbs);
        self.coords.extend_from_slice(coords);
    }
}

#[derive(Default)]
struct FlattenedContour {
    points: Vec<Position>,
    closed: bool,
}

struct DashCursor<'a> {
    dash: &'a [f32],
    index: usize,
    remaining: f32,
    drawing: bool,
}

impl<'a> DashCursor<'a> {
    fn new(dash: &'a [f32], offset: f32) -> Self {
        let total = dash.iter().sum::<f32>();
        let mut normalized_offset = if offset.is_finite() { offset % total } else { 0.0 };
        if normalized_offset < 0.0 {
            normalized_offset += total;
        }

        let mut index = 0;
        for (dash_index, interval) in dash.iter().copied().enumerate() {
            if normalized_offset > interval || (normalized_offset == interval && interval > 0.0) {
                normalized_offset -= interval;
                index = (dash_index + 1) % dash.len();
            } else {
                index = dash_index;
                break;
            }
        }

        let mut cursor = Self {
            dash,
            index,
            remaining: (dash[index] - normalized_offset).max(0.0),
            drawing: index % 2 == 0,
        };
        cursor.skip_empty_entries();
        cursor
    }

    fn advance(&mut self) {
        self.index = (self.index + 1) % self.dash.len();
        self.remaining = self.dash[self.index];
        self.drawing = self.index % 2 == 0;
        self.skip_empty_entries();
    }

    fn skip_empty_entries(&mut self) {
        for _ in 0..self.dash.len() {
            if self.remaining > f32::EPSILON {
                break;
            }
            self.index = (self.index + 1) % self.dash.len();
            self.remaining = self.dash[self.index];
            self.drawing = self.index % 2 == 0;
        }
    }
}

/// Signed angle between vectors `(ux, uy)` and `(vx, vy)` as defined by the W3C
/// SVG implementation notes F.6.5.4: `±arccos((u·v)/(|u||v|))` where the sign is
/// `+` when `ux*vy − uy*vx ≥ 0` and `−` otherwise.
///
/// The cross product is treated as a strict two-way sign so that a zero cross
/// product takes the positive branch. For collinear-opposite vectors the cross
/// product is zero and F.6.5.4 requires `+π` (not 0). `signum()` is unsuitable
/// here: it never returns 0 but returns `-1.0` for a `-0.0` input, and the cross
/// product of e.g. `(-1, 0)` and `(1, 0)` evaluates to `-0.0`, which would give
/// `-π` and violate the spec for that semicircle. The `< 0.0 → -1 else +1` rule
/// treats both `+0.0` and `-0.0` as positive, yielding `+π`; the F.6.5.6
/// sweep-modulo rule downstream then flips `+π` to `−π` when the sweep flag is
/// unset, so both semicircle directions render correctly.
///
/// Operates in f64 like the rest of the endpoint-to-center conversion.
fn svg_arc_angle(ux: f64, uy: f64, vx: f64, vy: f64) -> f64 {
    let dot = ux * vx + uy * vy;
    let len = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
    let mut cos = if len == 0.0 { 0.0 } else { dot / len };
    cos = cos.clamp(-1.0, 1.0);
    let sign = if (ux * vy - uy * vx) < 0.0 { -1.0 } else { 1.0 };
    sign * cos.acos()
}

fn normalize_dash_pattern(dash: &[f32]) -> Vec<f32> {
    if dash.is_empty() || dash.iter().any(|value| !value.is_finite() || *value < 0.0) {
        return Vec::new();
    }

    let sum = dash.iter().sum::<f32>();
    if sum <= f32::EPSILON {
        return Vec::new();
    }

    let mut normalized = dash.to_vec();
    if normalized.len() % 2 == 1 {
        normalized.extend_from_slice(dash);
    }

    normalized
}

fn push_flattened_point(points: &mut Vec<Position>, point: Position, dist_tol: f32) {
    if points
        .last()
        .is_some_and(|last| Position::equals(*last, point, dist_tol))
    {
        return;
    }

    points.push(point);
}

#[allow(clippy::too_many_arguments)]
fn flatten_bezier(
    points: &mut Vec<Position>,
    p0: Position,
    p1: Position,
    p2: Position,
    p3: Position,
    level: usize,
    tess_tol: f32,
    dist_tol: f32,
) {
    if level > 10 {
        push_flattened_point(points, p3, dist_tol);
        return;
    }

    let p01 = Position {
        x: (p0.x + p1.x) * 0.5,
        y: (p0.y + p1.y) * 0.5,
    };
    let p12 = Position {
        x: (p1.x + p2.x) * 0.5,
        y: (p1.y + p2.y) * 0.5,
    };
    let p23 = Position {
        x: (p2.x + p3.x) * 0.5,
        y: (p2.y + p3.y) * 0.5,
    };
    let p012 = Position {
        x: (p01.x + p12.x) * 0.5,
        y: (p01.y + p12.y) * 0.5,
    };
    let p123 = Position {
        x: (p12.x + p23.x) * 0.5,
        y: (p12.y + p23.y) * 0.5,
    };
    let p0123 = Position {
        x: (p012.x + p123.x) * 0.5,
        y: (p012.y + p123.y) * 0.5,
    };

    let dx = p3.x - p0.x;
    let dy = p3.y - p0.y;
    let d1 = ((p1.x - p3.x) * dy - (p1.y - p3.y) * dx).abs();
    let d2 = ((p2.x - p3.x) * dy - (p2.y - p3.y) * dx).abs();

    if (d1 + d2) * (d1 + d2) < tess_tol * (dx * dx + dy * dy) {
        push_flattened_point(points, p3, dist_tol);
        return;
    }

    flatten_bezier(points, p0, p01, p012, p0123, level + 1, tess_tol, dist_tol);
    flatten_bezier(points, p0123, p123, p23, p3, level + 1, tess_tol, dist_tol);
}

fn dash_line_segment(path: &mut Path, start: Position, end: Position, cursor: &mut DashCursor<'_>, dist_tol: f32) {
    let delta = end - start;
    let length = delta.mag2().sqrt();
    if length <= dist_tol {
        return;
    }

    let direction = delta * (1.0 / length);
    let mut travelled = 0.0;
    while travelled < length {
        if cursor.remaining <= f32::EPSILON {
            cursor.advance();
            continue;
        }

        let step = cursor.remaining.min(length - travelled);
        if cursor.drawing && step > dist_tol {
            let dash_start = start + direction * travelled;
            let dash_end = start + direction * (travelled + step);
            path.move_to(dash_start.x, dash_start.y);
            path.line_to(dash_end.x, dash_end.y);
        }

        travelled += step;
        cursor.remaining -= step;
    }
}

/// An iterator over the verbs and coordinates of a path.
#[derive(Debug)]
pub struct PathIter<'a> {
    verbs: slice::Iter<'a, PackedVerb>,
    coords: &'a [Position],
}

impl Iterator for PathIter<'_> {
    type Item = Verb;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(verb) = self.verbs.next() {
            let verb = Verb::from_packed(verb, self.coords);
            let num_coords = verb.num_coordinates();
            self.coords = &self.coords[num_coords..];
            Some(verb)
        } else {
            None
        }
    }
}

#[cfg(feature = "textlayout")]
impl ttf_parser::OutlineBuilder for Path {
    fn move_to(&mut self, x: f32, y: f32) {
        self.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.bezier_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::{svg_arc_angle, Path, Verb, PI};

    fn line_path(length: f32) -> Path {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.line_to(length, 0.0);
        path
    }

    fn dashed_line_segments(path: &Path) -> Vec<((f32, f32), (f32, f32))> {
        let mut segments = Vec::new();
        let mut current = None;

        for verb in path.verbs() {
            match verb {
                Verb::MoveTo(x, y) => current = Some((x, y)),
                Verb::LineTo(x, y) => {
                    let start = current.expect("line segment should start with move_to");
                    let end = (x, y);
                    segments.push((start, end));
                    current = Some(end);
                }
                other => panic!("unexpected dashed path verb: {other:?}"),
            }
        }

        segments
    }

    fn assert_segment(segment: ((f32, f32), (f32, f32)), expected: ((f32, f32), (f32, f32))) {
        let epsilon = 0.001;
        assert!((segment.0 .0 - expected.0 .0).abs() < epsilon);
        assert!((segment.0 .1 - expected.0 .1).abs() < epsilon);
        assert!((segment.1 .0 - expected.1 .0).abs() < epsilon);
        assert!((segment.1 .1 - expected.1 .1).abs() < epsilon);
    }

    #[test]
    fn dashed_line_splits_visible_intervals() {
        let dashed = line_path(10.0).dashed(&[2.0, 1.0], 0.0);
        let segments = dashed_line_segments(&dashed);

        assert_eq!(segments.len(), 4);
        assert_segment(segments[0], ((0.0, 0.0), (2.0, 0.0)));
        assert_segment(segments[1], ((3.0, 0.0), (5.0, 0.0)));
        assert_segment(segments[2], ((6.0, 0.0), (8.0, 0.0)));
        assert_segment(segments[3], ((9.0, 0.0), (10.0, 0.0)));
    }

    #[test]
    fn dashed_line_applies_offset() {
        let dashed = line_path(8.0).dashed(&[2.0, 2.0], 1.0);
        let segments = dashed_line_segments(&dashed);

        assert_eq!(segments.len(), 3);
        assert_segment(segments[0], ((0.0, 0.0), (1.0, 0.0)));
        assert_segment(segments[1], ((3.0, 0.0), (5.0, 0.0)));
        assert_segment(segments[2], ((7.0, 0.0), (8.0, 0.0)));
    }

    #[test]
    fn odd_dash_pattern_repeats() {
        let dashed = line_path(12.0).dashed(&[2.0, 1.0, 3.0], 0.0);
        let segments = dashed_line_segments(&dashed);

        assert_eq!(segments.len(), 3);
        assert_segment(segments[0], ((0.0, 0.0), (2.0, 0.0)));
        assert_segment(segments[1], ((3.0, 0.0), (6.0, 0.0)));
        assert_segment(segments[2], ((8.0, 0.0), (9.0, 0.0)));
    }

    #[test]
    fn invalid_dash_pattern_keeps_path_solid() {
        let path = line_path(10.0);
        let dashed = path.dashed(&[0.0, 0.0], 0.0);

        assert_eq!(
            format!("{:?}", path.verbs().collect::<Vec<_>>()),
            format!("{:?}", dashed.verbs().collect::<Vec<_>>())
        );
    }

    // ---- SVG elliptical arc (svg_arc_to) tests ----

    /// Evaluates a cubic bezier at parameter `s` in [0, 1].
    fn bezier_point(p0: (f32, f32), c1: (f32, f32), c2: (f32, f32), p3: (f32, f32), s: f32) -> (f32, f32) {
        let u = 1.0 - s;
        let w0 = u * u * u;
        let w1 = 3.0 * u * u * s;
        let w2 = 3.0 * u * s * s;
        let w3 = s * s * s;
        (
            w0 * p0.0 + w1 * c1.0 + w2 * c2.0 + w3 * p3.0,
            w0 * p0.1 + w1 * c1.1 + w2 * c2.1 + w3 * p3.1,
        )
    }

    /// Densely samples every bezier segment of `path`, returning the sampled
    /// points along with the final endpoint of the path.
    fn sample_path(path: &Path) -> (Vec<(f32, f32)>, (f32, f32)) {
        let mut points = Vec::new();
        let mut cur = (0.0_f32, 0.0_f32);
        let mut endpoint = (0.0_f32, 0.0_f32);

        for verb in path.verbs() {
            match verb {
                Verb::MoveTo(x, y) => {
                    cur = (x, y);
                    endpoint = cur;
                }
                Verb::LineTo(x, y) => {
                    for i in 0..=16 {
                        let s = i as f32 / 16.0;
                        points.push((cur.0 + (x - cur.0) * s, cur.1 + (y - cur.1) * s));
                    }
                    cur = (x, y);
                    endpoint = cur;
                }
                Verb::BezierTo(c1x, c1y, c2x, c2y, x, y) => {
                    let p3 = (x, y);
                    for i in 0..=32 {
                        let s = i as f32 / 32.0;
                        points.push(bezier_point(cur, (c1x, c1y), (c2x, c2y), p3, s));
                    }
                    cur = p3;
                    endpoint = cur;
                }
                Verb::Close | Verb::Solid | Verb::Hole => {}
            }
        }

        (points, endpoint)
    }

    /// Implicit-form residual of a rotated ellipse for the point (px, py): zero
    /// when the point lies exactly on the ellipse boundary.
    ///
    /// Cubic beziers only approximate an elliptical arc; with the 4/3*tan(seg/4)
    /// handle length shared with `arc()`/`ellipse()`, a full 90-degree segment
    /// deviates from the true ellipse by at most ~5.5e-4 in this residual
    /// (≈0.017px on a 60px radius). Tests allow a little headroom over that worst
    /// case, but stay well inside the ~4e-3 a coarser handle length would give,
    /// so the tolerance keeps pinning the handle length and not just the sweep.
    const ELLIPSE_RESIDUAL_TOL: f32 = 1e-3;

    fn ellipse_residual(px: f32, py: f32, cx: f32, cy: f32, rx: f32, ry: f32, phi: f32) -> f32 {
        let (sin_phi, cos_phi) = phi.sin_cos();
        let dx = px - cx;
        let dy = py - cy;
        // Rotate the offset back into the ellipse's local axes.
        let u = cos_phi * dx + sin_phi * dy;
        let v = -sin_phi * dx + cos_phi * dy;
        (u * u) / (rx * rx) + (v * v) / (ry * ry) - 1.0
    }

    #[test]
    fn svg_arc_endpoint_is_exact() {
        let mut path = Path::new();
        path.move_to(100.0, 100.0);
        path.svg_arc_to(60.0, 40.0, 0.5, true, false, 240.0, 180.0);

        let (_, endpoint) = sample_path(&path);
        assert!((endpoint.0 - 240.0).abs() < 1e-3, "x endpoint {}", endpoint.0);
        assert!((endpoint.1 - 180.0).abs() < 1e-3, "y endpoint {}", endpoint.1);
    }

    #[test]
    fn svg_arc_points_lie_on_axis_aligned_ellipse() {
        let cx = 100.0;
        let cy = 100.0;
        let rx = 60.0;
        let ry = 40.0;

        // Start on the ellipse at angle 0, end at the top; the arc bulges out.
        let start = (cx + rx, cy);
        let end = (cx, cy - ry);

        let mut path = Path::new();
        path.move_to(start.0, start.1);
        path.svg_arc_to(rx, ry, 0.0, false, false, end.0, end.1);

        let (points, _) = sample_path(&path);
        assert!(!points.is_empty());
        for (px, py) in points {
            let r = ellipse_residual(px, py, cx, cy, rx, ry, 0.0);
            assert!(
                r.abs() < ELLIPSE_RESIDUAL_TOL,
                "point ({px}, {py}) off ellipse, residual {r}"
            );
        }
    }

    #[test]
    fn svg_arc_points_lie_on_rotated_ellipse() {
        let cx = 50.0;
        let cy = 70.0;
        let rx = 80.0;
        let ry = 30.0;
        let phi = 0.7_f32; // radians

        // Derive endpoints that genuinely lie on the rotated ellipse.
        let on_ellipse = |t: f32| -> (f32, f32) {
            let (sin_phi, cos_phi) = phi.sin_cos();
            let (sin_t, cos_t) = t.sin_cos();
            (
                cx + rx * cos_phi * cos_t - ry * sin_phi * sin_t,
                cy + rx * sin_phi * cos_t + ry * cos_phi * sin_t,
            )
        };
        let start = on_ellipse(0.3);
        let end = on_ellipse(2.4);

        let mut path = Path::new();
        path.move_to(start.0, start.1);
        path.svg_arc_to(rx, ry, phi, false, true, end.0, end.1);

        let (points, _) = sample_path(&path);
        assert!(!points.is_empty());
        for (px, py) in points {
            let r = ellipse_residual(px, py, cx, cy, rx, ry, phi);
            assert!(
                r.abs() < ELLIPSE_RESIDUAL_TOL,
                "point ({px}, {py}) off rotated ellipse, residual {r}"
            );
        }
    }

    /// Signed perpendicular offset of the arc's midpoint from the chord
    /// (start -> end). Its sign tells us which side of the chord the arc bulges
    /// toward; positive and negative are the two half-planes.
    fn chord_bulge_side(start: (f32, f32), end: (f32, f32), mid: (f32, f32)) -> f32 {
        let cx = end.0 - start.0;
        let cy = end.1 - start.1;
        let mx = mid.0 - start.0;
        let my = mid.1 - start.1;
        cx * my - cy * mx
    }

    /// Total angle swept by `points` about the ellipse (cx, cy, rx, ry, phi),
    /// measured in the ellipse's parametric angle and unwrapped increment by
    /// increment. Consecutive samples along the emitted beziers are at most a
    /// few degrees apart — far from the +-pi wrap-around — so the sum recovers
    /// the true swept angle including direction and extra revolutions, which
    /// on-ellipse residual checks are blind to.
    fn swept_parametric_angle(points: &[(f64, f64)], cx: f64, cy: f64, rx: f64, ry: f64, phi: f64) -> f64 {
        let (sin_phi, cos_phi) = phi.sin_cos();
        let angle = |p: (f64, f64)| -> f64 {
            let dx = p.0 - cx;
            let dy = p.1 - cy;
            let u = (cos_phi * dx + sin_phi * dy) / rx;
            let v = (-sin_phi * dx + cos_phi * dy) / ry;
            v.atan2(u)
        };
        let mut total = 0.0;
        for pair in points.windows(2) {
            let mut increment = angle(pair[1]) - angle(pair[0]);
            if increment > std::f64::consts::PI {
                increment -= 2.0 * std::f64::consts::PI;
            } else if increment < -std::f64::consts::PI {
                increment += 2.0 * std::f64::consts::PI;
            }
            total += increment;
        }
        total
    }

    #[test]
    fn svg_arc_flags_select_sweep_angle_and_direction() {
        // On the radius-50 circle the chord from (150, 100) to (100, 150)
        // subtends 90 degrees: large_arc=false must sweep exactly a quarter
        // turn and large_arc=true exactly three quarters, while the sweep flag
        // picks the direction (F.6.5.6: sweep=true is the positive-angle
        // direction). This pins down the swept angle itself, which endpoint
        // and on-ellipse checks cannot: an arc that takes an extra full
        // revolution or runs the long way in the wrong direction (e.g. a
        // broken F.6.5.6 modulo adjustment) still keeps every sample on the
        // ellipse and still lands exactly on the endpoint.
        let start = (150.0, 100.0);
        let end = (100.0, 150.0);
        let r = 50.0;

        for (large_arc, sweep) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut path = Path::new();
            path.move_to(start.0, start.1);
            path.svg_arc_to(r, r, 0.0, large_arc, sweep, end.0, end.1);

            let (points, _) = sample_path(&path);
            let points: Vec<(f64, f64)> = points.iter().map(|p| (f64::from(p.0), f64::from(p.1))).collect();
            let (cx, cy, srx, sry) = svg_arc_center_f64(start, end, r, r, 0.0, large_arc, sweep);
            let total = swept_parametric_angle(&points, cx, cy, srx, sry, 0.0);

            let magnitude = if large_arc {
                1.5 * std::f64::consts::PI
            } else {
                0.5 * std::f64::consts::PI
            };
            let expected = if sweep { magnitude } else { -magnitude };
            assert!(
                (total - expected).abs() < 0.02,
                "large_arc={large_arc} sweep={sweep}: swept {total} rad, expected {expected}"
            );
        }
    }

    #[test]
    fn svg_arc_flag_combinations_are_distinct() {
        let start = (100.0, 100.0);
        let end = (200.0, 150.0);
        let rx = 80.0;
        let ry = 80.0;

        let cases = [(false, false), (false, true), (true, false), (true, true)];
        let mut midpoints = Vec::new();
        let mut sides = Vec::new();
        for &(large, sweep) in &cases {
            let mut path = Path::new();
            path.move_to(start.0, start.1);
            path.svg_arc_to(rx, ry, 0.0, large, sweep, end.0, end.1);
            let (points, _) = sample_path(&path);
            let mid = points[points.len() / 2];
            midpoints.push(mid);
            sides.push(chord_bulge_side(start, end, mid));
        }

        // The four arcs must reach four distinct midpoints.
        for i in 0..cases.len() {
            for j in (i + 1)..cases.len() {
                let d = (midpoints[i].0 - midpoints[j].0).hypot(midpoints[i].1 - midpoints[j].1);
                assert!(
                    d > 1.0,
                    "arcs {:?} and {:?} have coincident midpoints",
                    cases[i],
                    cases[j]
                );
            }
        }

        // The sweep flag controls which side of the chord the arc bulges to, so
        // for a fixed large_arc flag the two sweep values must land on opposite
        // half-planes. sides indices: 0=(F,F) 1=(F,T) 2=(T,F) 3=(T,T).
        assert!(
            sides[0] * sides[1] < 0.0,
            "small arcs should bulge to opposite sides: {:?}",
            sides
        );
        assert!(
            sides[2] * sides[3] < 0.0,
            "large arcs should bulge to opposite sides: {:?}",
            sides
        );
    }

    #[test]
    fn svg_arc_large_flag_selects_longer_sweep() {
        let start = (100.0, 100.0);
        let end = (160.0, 100.0);
        let rx = 50.0;
        let ry = 50.0;

        let arc_length = |large: bool| -> f32 {
            let mut path = Path::new();
            path.move_to(start.0, start.1);
            path.svg_arc_to(rx, ry, 0.0, large, true, end.0, end.1);
            let (points, _) = sample_path(&path);
            points
                .windows(2)
                .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
                .sum()
        };

        let small = arc_length(false);
        let large = arc_length(true);
        assert!(
            large > small,
            "large arc ({large}) should be longer than small ({small})"
        );
    }

    #[test]
    fn svg_arc_out_of_range_radii_are_scaled() {
        // Endpoints 200 apart but radius only 10: F.6.6 must scale the radii up
        // so the arc still reaches the endpoint exactly.
        let start = (0.0, 0.0);
        let end = (200.0, 0.0);

        let mut path = Path::new();
        path.move_to(start.0, start.1);
        path.svg_arc_to(10.0, 10.0, 0.0, false, true, end.0, end.1);

        let (points, endpoint) = sample_path(&path);
        assert!((endpoint.0 - end.0).abs() < 1e-3);
        assert!((endpoint.1 - end.1).abs() < 1e-3);

        // With radii scaled to exactly span the chord, the arc is a half-circle of
        // radius 100 centered at (100, 0); every sampled point must lie on it.
        for (px, py) in points {
            let r = ellipse_residual(px, py, 100.0, 0.0, 100.0, 100.0, 0.0);
            assert!(
                r.abs() < ELLIPSE_RESIDUAL_TOL,
                "scaled-radii point ({px}, {py}) off circle, residual {r}"
            );
        }
    }

    #[test]
    fn svg_arc_zero_radius_is_straight_line() {
        let mut path = Path::new();
        path.move_to(10.0, 20.0);
        path.svg_arc_to(0.0, 40.0, 0.0, true, true, 80.0, 90.0);

        let verbs: Vec<_> = path.verbs().collect();
        // move_to + a single line_to, no bezier segments.
        assert_eq!(verbs.len(), 2);
        assert!(matches!(verbs[0], Verb::MoveTo(10.0, 20.0)));
        assert!(matches!(verbs[1], Verb::LineTo(80.0, 90.0)));
    }

    #[test]
    fn svg_arc_identical_endpoints_add_nothing() {
        let mut path = Path::new();
        path.move_to(30.0, 30.0);
        let before = path.verbs().count();
        path.svg_arc_to(50.0, 50.0, 0.0, true, true, 30.0, 30.0);
        let after = path.verbs().count();
        assert_eq!(before, after, "identical endpoints must not add any verbs");
    }

    #[test]
    fn svg_arc_negative_radii_use_absolute_value() {
        let rx = 70.0;
        let ry = 45.0;
        let start = (rx, 0.0);
        let end = (0.0, ry);

        // Negative radii must drop their sign (F.6.2), producing geometry
        // identical to the equivalent positive-radii arc.
        let mut neg = Path::new();
        neg.move_to(start.0, start.1);
        neg.svg_arc_to(-rx, -ry, 0.0, false, true, end.0, end.1);

        let mut pos = Path::new();
        pos.move_to(start.0, start.1);
        pos.svg_arc_to(rx, ry, 0.0, false, true, end.0, end.1);

        let (neg_pts, _) = sample_path(&neg);
        let (pos_pts, _) = sample_path(&pos);
        assert_eq!(neg_pts.len(), pos_pts.len());
        for (n, p) in neg_pts.iter().zip(pos_pts.iter()) {
            assert!((n.0 - p.0).abs() < 1e-5 && (n.1 - p.1).abs() < 1e-5);
        }

        // ...and that geometry lies on the origin-centered ellipse.
        for (px, py) in pos_pts {
            let r = ellipse_residual(px, py, 0.0, 0.0, rx, ry, 0.0);
            assert!(
                r.abs() < ELLIPSE_RESIDUAL_TOL,
                "negative-radii point ({px}, {py}) off ellipse, residual {r}"
            );
        }
    }

    #[test]
    fn svg_arc_on_empty_path_starts_at_origin() {
        let mut path = Path::new();
        path.svg_arc_to(50.0, 50.0, 0.0, false, true, 100.0, 0.0);

        let verbs: Vec<_> = path.verbs().collect();
        // An implicit move_to(0, 0) is inserted before the bezier segments.
        assert!(matches!(verbs[0], Verb::MoveTo(0.0, 0.0)));
        assert!(verbs.len() > 1);

        let (_, endpoint) = sample_path(&path);
        assert!((endpoint.0 - 100.0).abs() < 1e-3);
        assert!((endpoint.1).abs() < 1e-3);
    }

    #[test]
    fn svg_arc_angle_collinear_opposite_is_positive_pi() {
        // F.6.5.4 boundary: for collinear-opposite vectors the cross product
        // ux*vy - uy*vx is zero and the spec mandates the POSITIVE branch, i.e.
        // +PI. This is exactly the start->end radius-vector pair of the standard
        // `move_to(0,0); svg_arc_to(50,50,0,_,_,100,0)` semicircle, where the
        // start vector is (-1, 0) and the end vector is (1, 0).
        //
        // The product `(-1)*0 - 0*1` evaluates to floating-point -0.0, so a
        // `signum()`-based sign returns -1.0 and yields -PI here, violating the
        // spec. The corrected `< 0.0 -> -1 else +1` rule treats -0.0 as the
        // positive branch and returns +PI. (The reciprocal pair (1,0)->(-1,0)
        // has a +0.0 cross product and is +PI under both rules.)
        let angle = svg_arc_angle(-1.0, 0.0, 1.0, 0.0);
        assert!(
            (angle - std::f64::consts::PI).abs() < 1e-6,
            "collinear-opposite angle should be +PI, got {angle}"
        );

        let reciprocal = svg_arc_angle(1.0, 0.0, -1.0, 0.0);
        assert!(
            (reciprocal - std::f64::consts::PI).abs() < 1e-6,
            "reciprocal collinear-opposite angle should be +PI, got {reciprocal}"
        );
    }

    #[test]
    fn svg_arc_small_semicircle_reaches_apex_both_directions() {
        // A 180-degree arc is the boundary case where the start and end radius
        // vectors are collinear and opposite, so the F.6.5.4 cross product is
        // zero (see svg_arc_angle_collinear_opposite_is_positive_pi). Both sweep
        // directions of `move_to(0,0); svg_arc_to(50,50,0,false,_,100,0)` must
        // trace a genuine semicircle of radius 50 centered at (50, 0) rather than
        // collapsing onto the chord.
        let start = (0.0, 0.0);
        let end = (100.0, 0.0);
        let r = 50.0;
        let center = (50.0, 0.0);

        let semicircle = |sweep: bool| -> (Vec<(f32, f32)>, (f32, f32), (f32, f32)) {
            let mut path = Path::new();
            path.move_to(start.0, start.1);
            path.svg_arc_to(r, r, 0.0, false, sweep, end.0, end.1);
            let (points, endpoint) = sample_path(&path);
            let mid = points[points.len() / 2];
            (points, mid, endpoint)
        };

        let (sweep_pts, sweep_mid, sweep_end) = semicircle(true);
        let (nsweep_pts, nsweep_mid, nsweep_end) = semicircle(false);

        // Both directions must actually reach the requested endpoint.
        assert!((sweep_end.0 - end.0).abs() < 1e-3 && (sweep_end.1 - end.1).abs() < 1e-3);
        assert!((nsweep_end.0 - end.0).abs() < 1e-3 && (nsweep_end.1 - end.1).abs() < 1e-3);

        // The apex of a true semicircle is the chord midpoint offset by the
        // radius perpendicular to the chord: (50, +-50). A collapsed (chord)
        // arc would instead leave the midpoint at (50, 0), so the apex distance
        // from the chord guards against any future regression to a degenerate
        // 180-degree arc. The sweep flag picks the half-plane: sweep=true bulges
        // to -y, sweep=false to +y.
        assert!(
            (sweep_mid.0 - center.0).abs() < 0.5 && (sweep_mid.1 + r).abs() < 0.5,
            "sweep=true semicircle midpoint {sweep_mid:?} should reach apex (50, -50)"
        );
        assert!(
            (nsweep_mid.0 - center.0).abs() < 0.5 && (nsweep_mid.1 - r).abs() < 0.5,
            "sweep=false semicircle midpoint {nsweep_mid:?} should reach apex (50, 50)"
        );

        // The two sweep directions must bulge into opposite half-planes.
        let sweep_side = chord_bulge_side(start, end, sweep_mid);
        let nsweep_side = chord_bulge_side(start, end, nsweep_mid);
        assert!(
            sweep_side * nsweep_side < 0.0,
            "semicircles must bulge to opposite sides: {sweep_side} vs {nsweep_side}"
        );

        // Every sampled point of both arcs lies on the radius-50 circle.
        for (px, py) in sweep_pts.into_iter().chain(nsweep_pts.into_iter()) {
            let resid = ellipse_residual(px, py, center.0, center.1, r, r, 0.0);
            assert!(
                resid.abs() < ELLIPSE_RESIDUAL_TOL,
                "semicircle point ({px}, {py}) off radius-50 circle, residual {resid}"
            );
        }
    }

    // ---- deterministic property fuzz ----

    /// Minimal deterministic PRNG (Knuth's MMIX LCG constants, high 32 bits
    /// used) so the fuzz below needs no external crates and replays
    /// identically on every run.
    struct Lcg(u64);

    impl Lcg {
        fn next_u32(&mut self) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 32) as u32
        }

        /// Uniform in [0, 1).
        fn unit(&mut self) -> f32 {
            (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
        }

        fn range(&mut self, lo: f32, hi: f32) -> f32 {
            lo + (hi - lo) * self.unit()
        }

        fn flag(&mut self) -> bool {
            self.next_u32() & 1 == 1
        }

        fn one_in(&mut self, n: u32) -> bool {
            self.next_u32() % n == 0
        }
    }

    fn fuzz_coord(rng: &mut Lcg, extreme: bool) -> f32 {
        if extreme && rng.one_in(3) {
            let magnitude = if rng.flag() { 1e30 } else { 1e-30 };
            if rng.flag() {
                magnitude
            } else {
                -magnitude
            }
        } else {
            rng.range(-1e4, 1e4)
        }
    }

    fn fuzz_radius(rng: &mut Lcg, extreme: bool) -> f32 {
        if extreme && rng.one_in(3) {
            if rng.flag() {
                1e30
            } else {
                1e-30
            }
        } else if rng.one_in(40) {
            0.0
        } else {
            rng.range(0.0, 2e3)
        }
    }

    /// Independent f64 evaluation of the F.6.5/F.6.6 center parametrization,
    /// used as ground truth by the fuzz test: returns the arc's center and the
    /// (possibly F.6.6-scaled) radii.
    #[allow(clippy::too_many_arguments)]
    fn svg_arc_center_f64(
        start: (f32, f32),
        end: (f32, f32),
        rx: f32,
        ry: f32,
        phi: f32,
        large_arc: bool,
        sweep: bool,
    ) -> (f64, f64, f64, f64) {
        let (x1, y1) = (f64::from(start.0), f64::from(start.1));
        let (x2, y2) = (f64::from(end.0), f64::from(end.1));
        let mut rx = f64::from(rx).abs();
        let mut ry = f64::from(ry).abs();
        let (sin_phi, cos_phi) = f64::from(phi).sin_cos();

        let dx2 = (x1 - x2) * 0.5;
        let dy2 = (y1 - y2) * 0.5;
        let x1p = cos_phi * dx2 + sin_phi * dy2;
        let y1p = -sin_phi * dx2 + cos_phi * dy2;

        let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
        if lambda > 1.0 {
            let s = lambda.sqrt();
            rx *= s;
            ry *= s;
        }

        let rx2 = rx * rx;
        let ry2 = ry * ry;
        let numerator = (rx2 * ry2 - rx2 * y1p * y1p - ry2 * x1p * x1p).max(0.0);
        let denominator = rx2 * y1p * y1p + ry2 * x1p * x1p;
        let mut coef = (numerator / denominator).sqrt();
        if large_arc == sweep {
            coef = -coef;
        }
        let cxp = coef * (rx * y1p / ry);
        let cyp = coef * -(ry * x1p / rx);

        let cx = cos_phi * cxp - sin_phi * cyp + (x1 + x2) * 0.5;
        let cy = sin_phi * cxp + cos_phi * cyp + (y1 + y2) * 0.5;
        (cx, cy, rx, ry)
    }

    /// f64 twin of `ellipse_residual` for the fuzz ground-truth comparison.
    fn ellipse_residual_f64(px: f64, py: f64, cx: f64, cy: f64, rx: f64, ry: f64, phi: f64) -> f64 {
        let (sin_phi, cos_phi) = phi.sin_cos();
        let dx = px - cx;
        let dy = py - cy;
        let u = cos_phi * dx + sin_phi * dy;
        let v = -sin_phi * dx + cos_phi * dy;
        (u * u) / (rx * rx) + (v * v) / (ry * ry) - 1.0
    }

    fn bezier_point_f64(p0: (f64, f64), c1: (f64, f64), c2: (f64, f64), p3: (f64, f64), s: f64) -> (f64, f64) {
        let u = 1.0 - s;
        let w0 = u * u * u;
        let w1 = 3.0 * u * u * s;
        let w2 = 3.0 * u * s * s;
        let w3 = s * s * s;
        (
            w0 * p0.0 + w1 * c1.0 + w2 * c2.0 + w3 * p3.0,
            w0 * p0.1 + w1 * c1.1 + w2 * c2.1 + w3 * p3.1,
        )
    }

    #[test]
    fn svg_arc_deterministic_property_fuzz() {
        const ITERATIONS: u32 = 20_000;

        let mut rng = Lcg(0x5eed_1e57_ab1e_f00d);

        for iteration in 0..ITERATIONS {
            // ~1 in 50 iterations swap some inputs for extreme magnitudes.
            let extreme = rng.one_in(50);

            let start = (fuzz_coord(&mut rng, extreme), fuzz_coord(&mut rng, extreme));
            let end = (fuzz_coord(&mut rng, extreme), fuzz_coord(&mut rng, extreme));
            let rx = fuzz_radius(&mut rng, extreme);
            let ry = fuzz_radius(&mut rng, extreme);
            let phi = rng.range(-10.0, 10.0);
            let large_arc = rng.flag();
            let sweep = rng.flag();

            let case = format!(
                "iteration {iteration}: move_to({}, {}); \
                 svg_arc_to({rx:e}, {ry:e}, {phi}, {large_arc}, {sweep}, {}, {})",
                start.0, start.1, end.0, end.1
            );

            // (a) must not panic.
            let mut path = Path::new();
            path.move_to(start.0, start.1);
            path.svg_arc_to(rx, ry, phi, large_arc, sweep, end.0, end.1);

            // (b) every emitted coordinate is finite.
            let mut last = start;
            let mut segments = Vec::new();
            for verb in path.verbs() {
                let coords: Vec<f32> = match verb {
                    Verb::MoveTo(x, y) | Verb::LineTo(x, y) => vec![x, y],
                    Verb::BezierTo(c1x, c1y, c2x, c2y, x, y) => vec![c1x, c1y, c2x, c2y, x, y],
                    Verb::Close | Verb::Solid | Verb::Hole => vec![],
                };
                assert!(
                    coords.iter().all(|value| value.is_finite()),
                    "non-finite coordinate in {verb:?} for {case}"
                );
                match verb {
                    Verb::MoveTo(x, y) | Verb::LineTo(x, y) => last = (x, y),
                    Verb::BezierTo(c1x, c1y, c2x, c2y, x, y) => {
                        segments.push((last, (c1x, c1y), (c2x, c2y), (x, y)));
                        last = (x, y);
                    }
                    _ => {}
                }
            }

            // (c) for non-degenerate finite inputs the path must land on the
            // requested endpoint within 1e-2 relative tolerance.
            let degenerate = start == end || rx.abs() <= 1e-6 || ry.abs() <= 1e-6;
            if !degenerate {
                let tolerance = 1e-2 * (end.0.abs() + end.1.abs()).max(1.0);
                assert!(
                    (last.0 - end.0).abs() <= tolerance && (last.1 - end.1).abs() <= tolerance,
                    "endpoint {last:?} != requested {end:?} for {case}"
                );
            }

            // (d) well-conditioned cases: every sampled point of the emitted
            // bezier chain lies on the ground-truth rotated ellipse.
            let radii_in_range = |r: f64| (1.0..=2e3).contains(&r);
            if !extreme && radii_in_range(f64::from(rx)) && radii_in_range(f64::from(ry)) && start != end {
                let (cx, cy, srx, sry) = svg_arc_center_f64(start, end, rx, ry, phi, large_arc, sweep);
                // F.6.6 scaling can push the effective radii outside the
                // well-conditioned band; only check while they stay inside it.
                if radii_in_range(srx) && radii_in_range(sry) {
                    assert!(
                        !segments.is_empty(),
                        "well-conditioned arc must emit bezier segments for {case}"
                    );
                    // Base tolerance matches ELLIPSE_RESIDUAL_TOL (the worst-case
                    // per-segment handle error); the second term covers f32
                    // quantization of coordinates of magnitude ~point_scale
                    // amplified by the implicit-form gradient (~1/min_radius).
                    let point_scale = cx.abs() + cy.abs() + srx + sry;
                    let min_radius = srx.min(sry);
                    let tolerance =
                        f64::from(ELLIPSE_RESIDUAL_TOL) + 8.0 * f64::from(f32::EPSILON) * point_scale / min_radius;
                    let mut sampled = Vec::new();
                    for (p0, c1, c2, p3) in &segments {
                        for step in 0..=4 {
                            let s = f64::from(step) / 4.0;
                            let to_f64 = |p: (f32, f32)| (f64::from(p.0), f64::from(p.1));
                            let (px, py) = bezier_point_f64(to_f64(*p0), to_f64(*c1), to_f64(*c2), to_f64(*p3), s);
                            let residual = ellipse_residual_f64(px, py, cx, cy, srx, sry, f64::from(phi));
                            assert!(
                                residual.abs() < tolerance,
                                "sampled point ({px}, {py}) at s={s} off ground-truth ellipse \
                                 (residual {residual}, tolerance {tolerance}) for {case}"
                            );
                            sampled.push((px, py));
                        }
                    }

                    // (e) the swept angle obeys the flags. On-ellipse
                    // residuals and the exact endpoint are blind to an arc
                    // that circles the wrong way around or takes an extra
                    // full revolution (both stay on the ellipse and reach the
                    // endpoint), so measure the sweep itself: never more than
                    // a full turn, direction matching the sweep flag, and
                    // magnitude relative to pi matching the large_arc flag
                    // (F.6.5.6). Sweeps close to the boundaries are skipped:
                    // near-zero or near-pi magnitudes cannot discriminate.
                    let total = swept_parametric_angle(&sampled, cx, cy, srx, sry, f64::from(phi));
                    assert!(
                        total.abs() < 2.0 * std::f64::consts::PI + 0.1,
                        "swept more than a full turn ({total} rad) for {case}"
                    );
                    if total.abs() > 0.05 {
                        assert_eq!(
                            total > 0.0,
                            sweep,
                            "sweep direction violated (swept {total} rad) for {case}"
                        );
                    }
                    if (total.abs() - std::f64::consts::PI).abs() > 0.05 {
                        assert_eq!(
                            total.abs() > std::f64::consts::PI,
                            large_arc,
                            "large_arc magnitude violated (swept {total} rad) for {case}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn svg_arc_tiny_radii_scale_up_without_overflow() {
        // Minimized from the deterministic fuzz (iteration 78 of seed
        // 0x5eed_1e57_ab1e_f00d): radii far smaller than the chord must be
        // scaled up per F.6.6, but computing lambda = (x1p/rx)^2 + (y1p/ry)^2
        // in f32 overflows to infinity for rx = ry = 1e-30, which then poisons
        // the center parametrization into NaN control points. The spec-correct
        // result is a half-circle spanning the chord, exactly as if the radii
        // had been given as chord/2.
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.svg_arc_to(1e-30, 1e-30, 0.0, false, true, 100.0, 0.0);

        let (points, endpoint) = sample_path(&path);
        assert!(!points.is_empty());
        assert!((endpoint.0 - 100.0).abs() < 1e-3 && endpoint.1.abs() < 1e-3);
        for (px, py) in points {
            assert!(px.is_finite() && py.is_finite(), "non-finite point ({px}, {py})");
            let residual = ellipse_residual(px, py, 50.0, 0.0, 50.0, 50.0, 0.0);
            assert!(
                residual.abs() < ELLIPSE_RESIDUAL_TOL,
                "tiny-radii point ({px}, {py}) off scaled half-circle, residual {residual}"
            );
        }
    }

    #[test]
    fn svg_arc_huge_coordinates_stay_finite() {
        // Squaring f32-range coordinates in the F.6.5.1 midpoint terms
        // overflows f32 ((1e30)^2 = 1e60 > f32::MAX) even though the resulting
        // arc geometry itself is comfortably representable. The center math
        // must not hand non-finite control points to the path.
        let mut path = Path::new();
        path.move_to(-1e30, 0.0);
        path.svg_arc_to(2e30, 2e30, 0.0, false, true, 1e30, 0.0);

        let (points, endpoint) = sample_path(&path);
        assert!(!points.is_empty());
        for (px, py) in &points {
            assert!(px.is_finite() && py.is_finite(), "non-finite point ({px}, {py})");
        }
        let tolerance = 1e30 * 1e-2;
        assert!((endpoint.0 - 1e30).abs() < tolerance && endpoint.1.abs() < tolerance);
    }

    #[test]
    fn svg_arc_unrepresentable_geometry_degrades_to_chord() {
        // F.6.6 scaling of wildly mismatched radii (rx = 1e30, ry = 1e-30 with
        // a ~141-unit chord) yields an effective rx of ~5e61: the arc's control
        // points exceed f32 range even though both endpoints are finite. The
        // builder must degrade to the chord rather than emit non-finite
        // vertices.
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.svg_arc_to(1e30, 1e-30, 0.0, true, true, 100.0, 100.0);

        let verbs: Vec<_> = path.verbs().collect();
        assert_eq!(verbs.len(), 2, "expected move_to + line_to, got {verbs:?}");
        assert!(matches!(verbs[0], Verb::MoveTo(0.0, 0.0)));
        assert!(matches!(verbs[1], Verb::LineTo(100.0, 100.0)));
    }

    #[test]
    fn svg_arc_non_finite_arguments_leave_path_unchanged() {
        // Canvas-spec rule: path methods return early, adding nothing, when any
        // argument is infinite or NaN. Exercise every float argument position
        // with every non-finite value.
        let bad_values = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY];

        for arg_index in 0..5 {
            for &bad in &bad_values {
                let mut args = [50.0, 40.0, 0.3, 90.0, 60.0];
                args[arg_index] = bad;

                let mut path = Path::new();
                path.move_to(10.0, 20.0);
                let before = path.verbs().count();
                path.svg_arc_to(args[0], args[1], args[2], true, false, args[3], args[4]);
                assert_eq!(
                    before,
                    path.verbs().count(),
                    "non-finite arg {arg_index} ({bad}) must add no verbs"
                );

                // The path must remain fully usable afterwards: a follow-up
                // line_to continues from the pre-arc current point.
                path.line_to(70.0, 80.0);
                let verbs: Vec<_> = path.verbs().collect();
                assert_eq!(verbs.len(), before + 1);
                assert!(matches!(verbs.last(), Some(Verb::LineTo(70.0, 80.0))));

                // On an empty path the guard must also fire before the
                // implicit move_to(0, 0) is inserted.
                let mut empty = Path::new();
                empty.svg_arc_to(args[0], args[1], args[2], true, false, args[3], args[4]);
                assert!(
                    empty.is_empty(),
                    "non-finite arg {arg_index} ({bad}) must not add an implicit move_to"
                );
            }
        }
    }

    #[test]
    fn svg_arc_large_semicircle_sweeps_the_long_way() {
        // The SVG path `A 50 50 0 1 1 100 0` from (0,0): rx=ry=50 exactly spans
        // the 100-unit chord, so the large-arc flag still yields a 180-degree
        // semicircle, with the large+sweep flags forcing the long way around.
        // This exercises the same collinear-opposite F.6.5.4 boundary as the
        // small form. The apex must reach (50, -50) and the endpoint (100, 0).
        let start = (0.0, 0.0);
        let end = (100.0, 0.0);
        let r = 50.0;
        let center = (50.0, 0.0);

        let mut path = Path::new();
        path.move_to(start.0, start.1);
        path.svg_arc_to(r, r, 0.0, true, true, end.0, end.1);

        let (points, endpoint) = sample_path(&path);
        assert!(
            (endpoint.0 - end.0).abs() < 1e-3 && (endpoint.1 - end.1).abs() < 1e-3,
            "large-arc semicircle endpoint {endpoint:?} should reach (100, 0)"
        );

        let mid = points[points.len() / 2];
        assert!(
            (mid.0 - center.0).abs() < 0.5 && (mid.1 + r).abs() < 0.5,
            "large-arc semicircle midpoint {mid:?} should reach apex (50, -50)"
        );

        // Total polyline length must be close to a half-circumference (pi*r),
        // confirming it swept ~180 degrees rather than collapsing to the chord.
        let length: f32 = points
            .windows(2)
            .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
            .sum();
        let half_circumference = PI * r;
        assert!(
            (length - half_circumference).abs() < 1.0,
            "swept length {length} should be ~pi*r ({half_circumference})"
        );

        for (px, py) in points {
            let resid = ellipse_residual(px, py, center.0, center.1, r, r, 0.0);
            assert!(
                resid.abs() < ELLIPSE_RESIDUAL_TOL,
                "large semicircle point ({px}, {py}) off radius-50 circle, residual {resid}"
            );
        }
    }

    /// Control points for a single-segment arc, from the web platform test
    /// `svg/path/interfaces/getPathData-normalize.html`, which pins the
    /// normalized output of `M 6 10 A 10 10 10 0 0 15 10` to within 0.0005.
    ///
    /// This is the tightest available oracle for the per-segment handle length:
    /// the reference values follow from `4/3 * tan(sweep/4)`, the same kappa
    /// `arc()` and `ellipse()` use, and a coarser handle length misses them by
    /// several thousandths.
    #[test]
    fn svg_arc_control_points_match_the_web_platform_test() {
        let mut path = Path::new();
        path.move_to(6.0, 10.0);
        path.svg_arc_to(10.0, 10.0, 10.0_f32.to_radians(), false, false, 15.0, 10.0);

        let curves: Vec<_> = path
            .verbs()
            .filter_map(|verb| match verb {
                Verb::BezierTo(a, b, c, d, e, f) => Some((a, b, c, d, e, f)),
                _ => None,
            })
            .collect();
        assert_eq!(curves.len(), 1, "a 51 degree sweep is one segment");

        let (c1x, c1y, c2x, c2y, ex, ey) = curves[0];
        let expected = [8.8305, 11.4263, 12.1695, 11.4263, 15.0, 10.0];
        for (actual, want) in [c1x, c1y, c2x, c2y, ex, ey].iter().zip(expected.iter()) {
            assert!(
                (actual - *want).abs() < 5e-4,
                "control points {:?} should match the web platform test {expected:?}",
                [c1x, c1y, c2x, c2y, ex, ey]
            );
        }
    }

    /// A quarter turn reduces to the circular kappa the other builders share:
    /// the handles sit at `KAPPA90` along each tangent. Deriving the value
    /// independently of the constant keeps this honest if `KAPPA90` ever moves.
    #[test]
    fn svg_arc_quarter_turn_uses_the_shared_kappa() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.svg_arc_to(1.0, 1.0, 0.0, false, true, 1.0, 1.0);

        let curve = path
            .verbs()
            .find_map(|verb| match verb {
                Verb::BezierTo(a, b, c, d, e, f) => Some((a, b, c, d, e, f)),
                _ => None,
            })
            .expect("one cubic");

        let k = super::KAPPA90;
        for (actual, want) in
            [curve.0, curve.1, curve.2, curve.3, curve.4, curve.5]
                .iter()
                .zip([k, 0.0, 1.0, 1.0 - k, 1.0, 1.0])
        {
            assert!((actual - want).abs() < 1e-5, "quarter turn handles: got {curve:?}");
        }
    }

    /// The same endpoints with only the sweep flag flipped must pick the other
    /// centre, so the two arcs bow to opposite sides. This is the sign choice in
    /// F.6.5.2, which a lost minus renders as a mirrored arc that still has the
    /// right endpoints and so survives every endpoint-only assertion.
    #[test]
    fn svg_arc_sweep_flag_selects_the_other_centre() {
        let mut sweep = Path::new();
        sweep.move_to(0.0, 0.0);
        sweep.svg_arc_to(1.0, 1.0, 0.0, false, true, 1.0, 1.0);

        let mut against = Path::new();
        against.move_to(0.0, 0.0);
        against.svg_arc_to(1.0, 1.0, 0.0, false, false, 1.0, 1.0);

        // Centre (0, 1) bows the arc right and down; centre (1, 0) bows it up.
        for (px, py) in sample_path(&sweep).0 {
            let resid = ellipse_residual(px, py, 0.0, 1.0, 1.0, 1.0, 0.0);
            assert!(resid.abs() < ELLIPSE_RESIDUAL_TOL, "sweep arc off centre (0, 1)");
        }
        for (px, py) in sample_path(&against).0 {
            let resid = ellipse_residual(px, py, 1.0, 0.0, 1.0, 1.0, 0.0);
            assert!(
                resid.abs() < ELLIPSE_RESIDUAL_TOL,
                "counter-sweep arc off centre (1, 0)"
            );
        }
    }

    /// Radii too small to span the endpoints are scaled by exactly sqrt(lambda)
    /// per F.6.6.2. Chosen so the arithmetic is exact: a unit circle asked to
    /// span four units gives lambda = 4 and a scale of exactly 2, leaving a
    /// half-circle of radius 2 centred midway.
    #[test]
    fn svg_arc_out_of_range_radii_scale_by_exactly_sqrt_lambda() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.svg_arc_to(1.0, 1.0, 0.0, false, false, 4.0, 0.0);

        let (points, endpoint) = sample_path(&path);
        assert!((endpoint.0 - 4.0).abs() < 1e-4 && endpoint.1.abs() < 1e-4);
        for (px, py) in points {
            let resid = ellipse_residual(px, py, 2.0, 0.0, 2.0, 2.0, 0.0);
            assert!(
                resid.abs() < ELLIPSE_RESIDUAL_TOL,
                "point ({px}, {py}) should lie on the radius-2 circle that sqrt(lambda) = 2 produces"
            );
        }
    }

    /// Lambda has to be computed from the endpoint offset *after* rotating it
    /// into the ellipse's own axes. Taking it from the unrotated offset is a
    /// silent bug: the arc still joins its endpoints, just through radii around
    /// a quarter too small. Here the correct scale is 4.0697 and the unrotated
    /// one would be 3.1623.
    #[test]
    fn svg_arc_lambda_is_measured_in_the_rotated_frame() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.svg_arc_to(10.0, 40.0, 90.0_f32.to_radians(), false, true, 60.0, 80.0);

        let scale = 4.069_705_1_f32;
        for (px, py) in sample_path(&path).0 {
            let resid = ellipse_residual(px, py, 30.0, 40.0, 10.0 * scale, 40.0 * scale, 90.0_f32.to_radians());
            assert!(
                resid.abs() < ELLIPSE_RESIDUAL_TOL,
                "point ({px}, {py}) off the ellipse that the rotated-frame lambda gives"
            );
        }
    }

    /// Two half turns must close a circle exactly, the construction an SVG uses
    /// when it has no circle element to hand. From Gecko's
    /// `layout/reftests/svg/path-05.svg` (bug 657862), which paints this over a
    /// radius-79 disc and under a radius-81 one and demands neither shows.
    #[test]
    fn svg_arc_two_half_turns_close_a_circle() {
        let mut path = Path::new();
        path.move_to(20.0, 100.0);
        path.svg_arc_to(80.0, 80.0, 0.0, false, true, 180.0, 100.0);
        path.svg_arc_to(80.0, 80.0, 0.0, false, true, 20.0, 100.0);

        let (points, endpoint) = sample_path(&path);
        assert!(
            (endpoint.0 - 20.0).abs() < 1e-3 && (endpoint.1 - 100.0).abs() < 1e-3,
            "the second half turn must return to the start, got {endpoint:?}"
        );
        for (px, py) in points {
            let resid = ellipse_residual(px, py, 100.0, 100.0, 80.0, 80.0, 0.0);
            assert!(
                resid.abs() < ELLIPSE_RESIDUAL_TOL,
                "point ({px}, {py}) off the radius-80 circle, residual {resid}"
            );
        }
    }

    /// Radii large enough that the sweep underflows must not throw the geometry
    /// across the canvas. From Gecko bug 1988564, whose reduced case is
    /// `a56270558080367.86,56270558080367.86,0,0,1,-1.2,1.4`; before the fix
    /// Firefox put the endpoint at x = -171085 instead of 77.2, and its reftest
    /// `bad-arc.html` expects nothing visible at all.
    #[test]
    fn svg_arc_radii_far_larger_than_the_chord_stay_put() {
        let mut path = Path::new();
        path.move_to(78.4, 207.2);
        path.svg_arc_to(
            56_270_558_080_367.86,
            56_270_558_080_367.86,
            0.0,
            false,
            true,
            77.2,
            208.6,
        );

        let (points, endpoint) = sample_path(&path);
        assert!(
            (endpoint.0 - 77.2).abs() < 1e-2 && (endpoint.1 - 208.6).abs() < 1e-2,
            "endpoint {endpoint:?} should stay at the requested point"
        );
        for (px, py) in points {
            assert!(px.is_finite() && py.is_finite(), "non-finite point ({px}, {py})");
            // The chord is under 2 units long; nothing may wander far from it.
            assert!(
                (77.0..=79.0).contains(&px) && (207.0..=209.0).contains(&py),
                "point ({px}, {py}) left the neighbourhood of a 1.8 unit chord"
            );
        }
    }

    /// The first arc of the SVG 1.1 specification's own `arcs01.svg`. Its radii
    /// need the F.6.6.2 correction, and afterwards the term under the root in
    /// F.6.5.2 lands a hair below zero in exact arithmetic, so an implementation
    /// that does not clamp it emits NaN for the whole shape.
    #[test]
    fn svg_arc_spec_example_with_a_negative_radicand_stays_finite() {
        let mut path = Path::new();
        path.move_to(650.0, 325.0);
        path.svg_arc_to(25.0, 25.0, -30.0_f32.to_radians(), false, true, 700.0, 300.0);

        let (points, endpoint) = sample_path(&path);
        assert!(!points.is_empty(), "the arc must emit geometry");
        assert!(
            (endpoint.0 - 700.0).abs() < 1e-2 && (endpoint.1 - 300.0).abs() < 1e-2,
            "endpoint {endpoint:?} should reach (700, 300)"
        );
        for (px, py) in points {
            assert!(px.is_finite() && py.is_finite(), "non-finite point ({px}, {py})");
        }
    }

    /// A corner that cannot be rounded contributes the line to it and nothing
    /// else. Continuing into the arc appends a second segment built from
    /// direction vectors that normalize to nothing, which paints as a seam
    /// across the fill. Upstream nanovg returns here; the port dropped it.
    #[test]
    fn arc_to_degenerate_corner_emits_only_the_line() {
        for (x1, y1, x2, y2, radius) in [
            (120.0f32, 130.0f32, 210.0f32, 130.0f32, 40.0f32), // collinear
            (30.0, 130.0, 210.0, 130.0, 40.0),                 // corner on the start point
            (120.0, 130.0, 120.0, 130.0, 40.0),                // corner on the end point
            (120.0, 60.0, 210.0, 130.0, 0.0),                  // no radius to round with
        ] {
            let mut path = Path::new();
            path.move_to(30.0, 130.0);
            path.arc_to(x1, y1, x2, y2, radius);

            let verbs: Vec<_> = path.verbs().collect();
            assert_eq!(
                verbs.len(),
                2,
                "({x1}, {y1}) r={radius} should add one line, got {verbs:?}"
            );
            assert!(
                matches!(verbs[1], Verb::LineTo(..)),
                "expected a line, got {:?}",
                verbs[1]
            );
        }
    }

    /// An arc that sweeps no angle still has to produce usable geometry. The
    /// handle length was computed as (1 - cos hda) / sin hda, which is 0/0 at a
    /// zero sweep and put NaN into every control point of the shape.
    #[test]
    fn arc_with_no_sweep_stays_finite() {
        for solidity in [super::Solidity::Solid, super::Solidity::Hole] {
            let mut path = Path::new();
            path.move_to(10.0, 10.0);
            path.arc(50.0, 50.0, 20.0, 1.0, 1.0, solidity);

            for verb in path.verbs() {
                let coords = match verb {
                    Verb::MoveTo(a, b) | Verb::LineTo(a, b) => vec![a, b],
                    Verb::BezierTo(a, b, c, d, e, f) => vec![a, b, c, d, e, f],
                    _ => vec![],
                };
                for value in coords {
                    assert!(value.is_finite(), "{solidity:?} zero sweep produced {value}");
                }
            }
        }
    }
}
