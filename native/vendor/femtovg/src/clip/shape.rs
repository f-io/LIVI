//! Clips the fragment shader can evaluate: a path that outlines a
//! parallelogram, a rounded rectangle or an ellipse is a box with elliptical
//! corners in a frame of its own, and its coverage is a few lines of
//! arithmetic per fragment - no stencil work, and an antialiased edge.

use crate::{
    geometry::{Bounds, Transform2D},
    Path, Verb,
};

/// How far an outline may stray from the box it is taken for, as a share of
/// the corner radius (the box's smaller half-size on a straight side). A
/// circle of four cubics is 0.03 % off the true one and one of eight
/// quadratics, as tiny-skia builds it, 0.31 %; a share keeps the answer the
/// same at every zoom.
const FIT_TOLERANCE: f32 = 0.005;

/// How far a polygon's corners may be from a parallelogram's, as a share of
/// its size: rounding in the coordinates, no more - straight sides are not
/// approximations of anything.
const CORNER_TOLERANCE: f32 = 1e-4;

/// The cubic handle length of a quarter circle of radius one.
const KAPPA90: f32 = 0.552_284_8;

/// Interior points sampled on each curve when checking a fit.
const CURVE_SAMPLES: usize = 7;

/// The cosine and sine of the angles a corner's arc is sampled at when
/// asking whether a box lies in another: a quarter turn in eight steps.
const CORNER_ARC: [[f32; 2]; 9] = [
    [1.0, 0.0],
    [0.980_785_3, 0.195_090_3],
    [0.923_879_5, 0.382_683_4],
    [0.831_469_6, 0.555_570_2],
    [std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2],
    [0.555_570_2, 0.831_469_6],
    [0.382_683_4, 0.923_879_5],
    [0.195_090_3, 0.980_785_3],
    [0.0, 1.0],
];

/// How far, in fringe widths, the one box nested clips are combined into
/// may stray from what they cover together: half a pixel, no further than
/// the stencil the pair would otherwise go to resolves an edge.
const CONTAINMENT_SLACK: f32 = 0.5;

/// How far past the clip's edge, in fringe widths, a draw may reach and
/// still be inside it, and how far the one box a clip and a scissor make may
/// stray from what the two leave: the rounding of two outlines given in
/// different coordinates, and on a pixel four steps of 255 at most.
const DRAW_SLACK: f32 = 1.0 / 64.0;

/// The verbs past which a path is not asked, each time it is filled under a
/// clip, whether it outlines a rect: one with its closing side spelled out
/// has six.
const MAX_FILL_VERBS: usize = 8;

/// The corner radius, in fringe widths, under which a corner is taken as
/// square: a distance no longer stands in for coverage that tight (it is
/// off by up to a fifth of a pixel, the square corner by a sixteenth).
/// Skia's analytic clips square off at the same radius (`kRadiusMin`).
const SQUARE_CORNER_BELOW: f32 = 0.5;

/// How close to parallel a frame's axes may come, as the squared sine of the
/// angle between them: under half a degree apart the box is a sliver whose
/// inverse map loses the precision the shader needs, and goes to the stencil.
const MIN_AXIS_SPREAD: f32 = 1e-4;

/// The half extent, in fringe widths, under which a box goes to the stencil:
/// thinner than a pixel, its two sides no longer cover a pixel one at a time.
const MIN_HALF_EXTENT: f32 = 0.5;

/// Where the uniform rows put the corners' centers of a box with square
/// corners: so far out that no fragment is past them, yet inside the range
/// GLSL ES guarantees a highp float (2^62).
const NO_CORNERS: f32 = 1e18;

/// A box's corners in order around it, as the signs of their coordinates.
const CORNERS: [[f32; 2]; 4] = [[1.0, 1.0], [-1.0, 1.0], [-1.0, -1.0], [1.0, -1.0]];

/// The side between corner `i` and corner `i + 1` of [`CORNERS`], as the
/// direction it faces.
const SIDES: [[f32; 2]; 4] = [[0.0, 1.0], [-1.0, 0.0], [0.0, -1.0], [1.0, 0.0]];

/// A box with elliptical corners: zero radii make a rectangle, radii equal
/// to the extent an ellipse.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct RoundedBox {
    /// Maps the box - centered on the origin, sides along the axes - to the
    /// space its outline was given in.
    pub(crate) frame: Transform2D,
    pub(crate) extent: [f32; 2],
    pub(crate) radii: [f32; 2],
}

/// What an upright rect filled under a clip is drawn as in its place
/// ([`RoundedBox::rect_fill`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum RectFill {
    /// The rect the fill shares with a rect clip, antialiased by its own
    /// fringe, with no clip.
    Shared(RoundedBox),
    /// A rect a fringe width around the clip the fill covers, with no fringe
    /// of its own, under the clip's coverage.
    Covered(RoundedBox),
}

/// A [`RoundedBox`] as a draw's fragment shader takes it: the map from
/// device pixels to the box's frame scaled so that one unit is one fringe
/// width across each side, and the box in those units - its corners square
/// when they are too tight to tell from square ([`SQUARE_CORNER_BELOW`]).
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub(crate) struct ClipCoverage {
    /// Rows of the linear part.
    pub(crate) linear: [f32; 4],
    pub(crate) offset: [f32; 2],
    pub(crate) extent: [f32; 2],
    pub(crate) radii: [f32; 2],
    /// How far outside the box's round corners, in fringe widths, the
    /// outline it was fitted to strays ([`RoundedBox::fit_outline`]): a
    /// draw that goes no further there is inside the clip as it was given.
    pub(crate) strays: f32,
}

enum Segment {
    Line([f32; 2]),
    Cubic([f32; 2], [f32; 2], [f32; 2]),
}

impl RoundedBox {
    /// The box `path` outlines, if it is one: a single contour that goes
    /// once around a parallelogram, or around an axis-aligned rounded
    /// rectangle or ellipse within [`FIT_TOLERANCE`].
    pub(crate) fn fit(path: &Path) -> Option<Self> {
        Self::fit_outline(path).map(|(shape, _)| shape)
    }

    /// The box `path` outlines ([`Self::fit`]), and how far outside the box
    /// the outline strays, in the path's units: nothing for straight sides,
    /// and for corners drawn as Bezier curves what those are off an ellipse
    /// by - 0.03 % of the radius for the usual cubic, 0.3 % for a quadratic
    /// an eighth of the way around.
    pub(crate) fn fit_outline(path: &Path) -> Option<(Self, f32)> {
        match Self::fit_with_cut(path)? {
            (shape, strays, None) => Some((shape, strays)),
            _ => None,
        }
    }

    /// The box `path` outlines and how far the outline strays outside it
    /// ([`Self::fit_outline`]), or for an axis-aligned rectangle rounded at
    /// the two corners of one side only - a tab, a sheet, a window's title
    /// bar - a rounded box that runs on past the square side and the
    /// rectangle with square corners that cuts it there.
    pub(crate) fn fit_with_cut(path: &Path) -> Option<(Self, f32, Option<Self>)> {
        let (start, segments) = contour(path)?;
        if segments.iter().all(|segment| matches!(segment, Segment::Line(_))) {
            return Self::parallelogram(start, &segments).map(|shape| (shape, 0.0, None));
        }
        Self::rounded(start, &segments)
    }

    fn parallelogram(start: [f32; 2], segments: &[Segment]) -> Option<Self> {
        // The contour is closed: the last segment ends on the start again.
        let mut points = Vec::with_capacity(segments.len());
        points.push(start);
        for segment in &segments[..segments.len() - 1] {
            if let Segment::Line(end) = segment {
                points.push(*end);
            }
        }
        let size = points
            .iter()
            .map(|p| (p[0] - start[0]).abs().max((p[1] - start[1]).abs()))
            .fold(0.0, f32::max);
        // Corners only: a point on the line through its neighbours adds nothing.
        let n = points.len();
        let mut turns = (0..n).filter(|&i| {
            let (a, b, c) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
            let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
            cross.abs() > CORNER_TOLERANCE * size * size
        });
        let [p0, p1, p2, p3] = [turns.next()?, turns.next()?, turns.next()?, turns.next()?].map(|i| points[i]);
        if turns.next().is_some() {
            return None;
        }
        let close = |a: f32, b: f32| (a - b).abs() <= CORNER_TOLERANCE * size;
        if !close(p0[0] + p2[0], p1[0] + p3[0]) || !close(p0[1] + p2[1], p1[1] + p3[1]) {
            return None;
        }
        let u = [(p1[0] - p0[0]) * 0.5, (p1[1] - p0[1]) * 0.5];
        let v = [(p3[0] - p0[0]) * 0.5, (p3[1] - p0[1]) * 0.5];
        let extent = [u[0].hypot(u[1]), v[0].hypot(v[1])];
        Some(Self {
            frame: Transform2D::new(
                u[0] / extent[0],
                u[1] / extent[0],
                v[0] / extent[1],
                v[1] / extent[1],
                (p0[0] + p2[0]) * 0.5,
                (p0[1] + p2[1]) * 0.5,
            ),
            extent,
            radii: [0.0, 0.0],
        })
    }

    fn rounded(start: [f32; 2], segments: &[Segment]) -> Option<(Self, f32, Option<Self>)> {
        // The outline in order - each segment's interior samples, then its
        // end - and the segment ends alone, where straight sides meet corners.
        let mut outline = vec![start];
        let mut anchors = vec![start];
        let mut from = start;
        for segment in segments {
            match *segment {
                Segment::Line(end) => {
                    outline.push([(from[0] + end[0]) * 0.5, (from[1] + end[1]) * 0.5]);
                    outline.push(end);
                    from = end;
                }
                Segment::Cubic(c1, c2, end) => {
                    for i in 1..=CURVE_SAMPLES {
                        let t = i as f32 / (CURVE_SAMPLES + 1) as f32;
                        let s = 1.0 - t;
                        let at = |k: usize| {
                            s * s * s * from[k] + 3.0 * s * s * t * c1[k] + 3.0 * s * t * t * c2[k] + t * t * t * end[k]
                        };
                        outline.push([at(0), at(1)]);
                    }
                    outline.push(end);
                    from = end;
                }
            }
            anchors.push(from);
        }

        let bound = |k: usize, pick: fn(f32, f32) -> f32, seed: f32| outline.iter().map(|p| p[k]).fold(seed, pick);
        let (min, max) = (
            [bound(0, f32::min, f32::INFINITY), bound(1, f32::min, f32::INFINITY)],
            [
                bound(0, f32::max, f32::NEG_INFINITY),
                bound(1, f32::max, f32::NEG_INFINITY),
            ],
        );
        let center = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5];
        let extent = [(max[0] - min[0]) * 0.5, (max[1] - min[1]) * 0.5];
        let sized = extent.iter().all(|half| *half > 0.0 && half.is_finite());
        if !sized {
            return None;
        }
        let slack = FIT_TOLERANCE * extent[0].min(extent[1]);

        // A corner's radius along an axis is what the straight side on the
        // other axis leaves of the extent on the corner's half: the side ends
        // where its farthest anchor there is. No anchor on a side means no
        // straight side - an ellipse.
        // One pass over the anchors: for each corner and axis, how far along
        // that axis the straight side next to it reaches.
        let mut reach = [[None::<f32>; 2]; 4];
        for p in &anchors {
            for across in 0..2 {
                let side = p[across] - center[across];
                if (side.abs() - extent[across]).abs() > slack {
                    continue;
                }
                let along = 1 - across;
                let at = p[along] - center[along];
                for (corner, sign) in CORNERS.iter().zip(&mut reach) {
                    if side * corner[across] > 0.0 && at * corner[along] >= 0.0 {
                        let far = &mut sign[along];
                        *far = Some(far.map_or(at.abs(), |far: f32| far.max(at.abs())));
                    }
                }
            }
        }
        let radius = |corner: usize, along: usize| {
            let radius = reach[corner][along]
                .map_or(extent[along], |reach| extent[along] - reach)
                .max(0.0);
            if extent[along] - radius <= slack {
                extent[along]
            } else if radius <= slack {
                0.0
            } else {
                radius
            }
        };
        let corners = [0, 1, 2, 3].map(|corner| [radius(corner, 0), radius(corner, 1)]);
        let same = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() <= slack && (a[1] - b[1]).abs() <= slack;
        let square = |r: [f32; 2]| r == [0.0, 0.0];
        let round = |r: [f32; 2]| r[0] > 0.0 && r[1] > 0.0;
        // The same corners all round, or one side's two square and the
        // other two the same: that side is cut. Any other mix is no box.
        let (radii, cut) = if corners.iter().all(|&r| same(r, corners[0])) {
            (corners[0], None)
        } else {
            let side = (0..4).find(|&i| {
                let (a, b, c, d) = (
                    corners[i],
                    corners[(i + 1) % 4],
                    corners[(i + 2) % 4],
                    corners[(i + 3) % 4],
                );
                square(a) && square(b) && round(c) && same(c, d)
            })?;
            (corners[(side + 2) % 4], Some(side))
        };
        if !square(radii) && !round(radii) {
            return None;
        }
        let corner_radii = |p: &[f32; 2]| {
            let Some(side) = cut else {
                return radii;
            };
            let quadrant = match (p[0] >= center[0], p[1] >= center[1]) {
                (true, true) => 0,
                (false, true) => 1,
                (false, false) => 2,
                (true, false) => 3,
            };
            if quadrant == side || quadrant == (side + 1) % 4 {
                [0.0, 0.0]
            } else {
                radii
            }
        };

        // How far outside the box the outline's samples lie, to first order
        // at a corner; the curve between two samples may reach a little
        // further, which the tenth on top allows for.
        let mut strays = 0.0f32;
        let on_box = outline.iter().all(|p| {
            let side = [
                (p[0] - center[0]).abs() - extent[0],
                (p[1] - center[1]).abs() - extent[1],
            ];
            let radii = corner_radii(p);
            let corner = [side[0] + radii[0], side[1] + radii[1]];
            if radii[0] > 0.0 && corner[0] > 0.0 && corner[1] > 0.0 {
                let k = [corner[0] / radii[0], corner[1] / radii[1]];
                let k1 = k[0].hypot(k[1]);
                strays = strays.max((k1 - 1.0) * k1 / (k[0] / radii[0]).hypot(k[1] / radii[1]));
                (k1 - 1.0).abs() <= FIT_TOLERANCE
            } else {
                strays = strays.max(side[0].max(side[1]));
                side[0].max(side[1]).abs() <= slack
            }
        });
        // Once around, never turning back: twice around is another region
        // under even-odd, and a doubled-back outline is not the box's.
        let angle = |p: &[f32; 2]| ((p[1] - center[1]) / extent[1]).atan2((p[0] - center[0]) / extent[0]);
        let mut turned = 0.0;
        let mut forward = 0usize;
        let mut backward = 0usize;
        for pair in outline.windows(2) {
            let mut step = angle(&pair[1]) - angle(&pair[0]);
            if step > std::f32::consts::PI {
                step -= std::f32::consts::TAU;
            } else if step < -std::f32::consts::PI {
                step += std::f32::consts::TAU;
            }
            turned += step;
            forward += usize::from(step > 1e-4);
            backward += usize::from(step < -1e-4);
        }
        let once = (turned.abs() - std::f32::consts::TAU).abs() < 0.01 && (forward == 0 || backward == 0);
        if !(on_box && once) {
            return None;
        }
        let shape = Self {
            frame: Transform2D::translation(center[0], center[1]),
            extent,
            radii,
        };
        let Some(side) = cut else {
            return Some((shape, strays * 1.1, None));
        };
        // The rounded box runs on past the square side by twice its radius
        // there, so that its far corners lie well outside the cut.
        let [sx, sy] = SIDES[side];
        let run = [2.0 * radii[0] * sx.abs(), 2.0 * radii[1] * sy.abs()];
        let rounded = Self {
            frame: Transform2D::translation(center[0] + sx * run[0] * 0.5, center[1] + sy * run[1] * 0.5),
            extent: [extent[0] + run[0] * 0.5, extent[1] + run[1] * 0.5],
            radii,
        };
        let cut = Self {
            radii: [0.0, 0.0],
            ..shape
        };
        Some((rounded, strays * 1.1, Some(cut)))
    }

    /// The box under `transform`: its frame carried along.
    pub(crate) fn transformed(self, transform: &Transform2D) -> Self {
        Self {
            frame: self.frame * *transform,
            ..self
        }
    }

    /// The box for a fragment shader whose positions are in the space the
    /// frame maps to; `None` when the frame collapses that space or all but
    /// does ([`MIN_AXIS_SPREAD`]), or the box is thinner than a pixel there.
    pub(crate) fn coverage(&self, fringe_width: f32) -> Option<ClipCoverage> {
        let Transform2D([a, b, c, d, ..]) = self.frame;
        let determinant = a * d - c * b;
        let spans = (a * a + b * b) * (c * c + d * d);
        let invertible = determinant * determinant > MIN_AXIS_SPREAD * spans && spans.is_finite();
        if !invertible || fringe_width <= 0.0 {
            return None;
        }
        // Each row of the inverse is the gradient of one box coordinate, so
        // dividing by its length makes that coordinate count device pixels
        // across the side it measures - under a skew too.
        let Transform2D([ia, ib, ic, id, ix, iy]) = self.frame.inverse();
        let per_unit = [ia.hypot(ic) * fringe_width, ib.hypot(id) * fringe_width];
        let extent = [self.extent[0] / per_unit[0], self.extent[1] / per_unit[1]];
        if extent[0] < MIN_HALF_EXTENT || extent[1] < MIN_HALF_EXTENT {
            return None;
        }
        let mut radii = [self.radii[0] / per_unit[0], self.radii[1] / per_unit[1]];
        if radii[0] < SQUARE_CORNER_BELOW || radii[1] < SQUARE_CORNER_BELOW {
            radii = [0.0, 0.0];
        }
        Some(ClipCoverage {
            linear: [ia / per_unit[0], ic / per_unit[0], ib / per_unit[1], id / per_unit[1]],
            offset: [ix / per_unit[0], iy / per_unit[1]],
            extent,
            radii,
            strays: 0.0,
        })
    }

    /// `other` in this box's frame when the two have parallel sides: its
    /// center, and its half sides and radii along this frame's axes.
    fn alongside(&self, other: &Self) -> Option<([f32; 2], [f32; 2], [f32; 2])> {
        let Transform2D([a, b, c, d, x, y]) = other.frame / self.frame;
        let scale = a.abs().max(b.abs()).max(c.abs()).max(d.abs());
        let aligned = |along: f32, across: f32| across.abs() <= 1e-4 * scale && along.abs() > 0.0;
        if aligned(a, b) && aligned(d, c) {
            Some((
                [x, y],
                [a.abs() * other.extent[0], d.abs() * other.extent[1]],
                [a.abs() * other.radii[0], d.abs() * other.radii[1]],
            ))
        } else if aligned(b, a) && aligned(c, d) {
            Some((
                [x, y],
                [c.abs() * other.extent[1], b.abs() * other.extent[0]],
                [c.abs() * other.radii[1], b.abs() * other.radii[0]],
            ))
        } else {
            None
        }
    }

    /// Whether the box's sides lie along the device axes.
    fn upright(&self) -> bool {
        let Transform2D([a, b, c, d, ..]) = self.frame;
        let scale = a.abs().max(b.abs()).max(c.abs()).max(d.abs());
        let none = |v: f32| v.abs() <= 1e-4 * scale;
        (none(b) && none(c)) || (none(a) && none(d))
    }

    /// One fringe width in the box's own coordinates, across each pair of
    /// its sides.
    fn per_fringe(&self, fringe_width: f32) -> [f32; 2] {
        let Transform2D([ia, ib, ic, id, ..]) = self.frame.inverse();
        [ia.hypot(ic) * fringe_width, ib.hypot(id) * fringe_width]
    }

    /// The upright rect `path` fills under `transform`, if it is one.
    pub(crate) fn upright_rect(path: &Path, transform: &Transform2D) -> Option<Self> {
        if path.verb_count() > MAX_FILL_VERBS {
            return None;
        }
        let fill = Self::fit(path)?.transformed(transform);
        (fill.upright() && fill.radii == [0.0, 0.0]).then_some(fill)
    }

    /// What an upright rect ([`Self::upright_rect`]) filled under this clip
    /// is drawn as, so that an edge the two have in common is antialiased
    /// once: under an upright rect clip, the rect the two share, when it is
    /// at least a pixel thick; under an upright box with round corners that
    /// the rect covers whole, the box itself. `None` for any other fill,
    /// which keeps its outline under the clip's coverage.
    pub(crate) fn rect_fill(&self, fill: &Self, fringe_width: f32) -> Option<RectFill> {
        if !self.upright() {
            return None;
        }
        let fill = *fill;
        if self.radii == [0.0, 0.0] {
            let both = self.intersection_within(&fill, fringe_width, DRAW_SLACK)?;
            return both.coverage(fringe_width).map(|_| RectFill::Shared(both));
        }
        // The rect covers the box when each of its sides lies at or past
        // the box's.
        let ([x, y], half, _) = self.alongside(&fill)?;
        let per_fringe = self.per_fringe(fringe_width);
        let covers = [(x, half[0], 0), (y, half[1], 1)]
            .into_iter()
            .all(|(center, half, axis)| {
                let reach = self.extent[axis] - DRAW_SLACK * per_fringe[axis];
                center - half <= -reach && center + half >= reach
            });
        covers.then(|| {
            RectFill::Covered(Self {
                frame: self.frame,
                extent: [self.extent[0] + per_fringe[0], self.extent[1] + per_fringe[1]],
                radii: [0.0, 0.0],
            })
        })
    }

    /// What both this box and `other` cover, as one box, when their sides
    /// are parallel: exactly for two parallelograms; with round corners, the
    /// box with the nearer of each pair of sides and the corners of one of
    /// the two, when its outline stays within `slack` fringe widths of the
    /// real intersection's - one box inside the other, or two that all but
    /// coincide. `None` for any other overlap, or none.
    fn intersection_within(&self, other: &Self, fringe_width: f32, slack: f32) -> Option<Self> {
        let ([x, y], half, radii) = self.alongside(other)?;
        let low = [(x - half[0]).max(-self.extent[0]), (y - half[1]).max(-self.extent[1])];
        let high = [(x + half[0]).min(self.extent[0]), (y + half[1]).min(self.extent[1])];
        if low[0] >= high[0] || low[1] >= high[1] {
            return None;
        }
        let center = Transform2D::translation((low[0] + high[0]) * 0.5, (low[1] + high[1]) * 0.5);
        let extent = [(high[0] - low[0]) * 0.5, (high[1] - low[1]) * 0.5];
        let with_corners = |radii: [f32; 2]| Self {
            frame: center * self.frame,
            extent,
            radii: [radii[0].min(extent[0]), radii[1].min(extent[1])],
        };
        if self.radii == [0.0, 0.0] && other.radii == [0.0, 0.0] {
            return Some(with_corners([0.0, 0.0]));
        }
        // A rect whose sides all lie at or past the other box's leaves that
        // box as it is.
        if other.radii == [0.0, 0.0] && low == [-self.extent[0], -self.extent[1]] && high == self.extent {
            return Some(*self);
        }
        if self.radii == [0.0, 0.0] && low == [x - half[0], y - half[1]] && high == [x + half[0], y + half[1]] {
            return Some(with_corners(radii));
        }
        // On the real intersection's outline the further of the two boxes'
        // edges is the one a point is on.
        let (mine, theirs) = (self.coverage(fringe_width)?, other.coverage(fringe_width)?);
        [radii, self.radii].into_iter().map(with_corners).find(|both| {
            both.outline()
                .all(|point| mine.distance(point).max(theirs.distance(point)).abs() <= slack)
        })
    }

    /// What this clip and `other`, nested in it, clip to together, as one
    /// box within [`CONTAINMENT_SLACK`] ([`Self::intersection_within`]).
    pub(crate) fn intersection(&self, other: &Self, fringe_width: f32) -> Option<Self> {
        self.intersection_within(other, fringe_width, CONTAINMENT_SLACK)
    }

    /// This clip and a scissor - a box of its own, `scissor` - as the one
    /// box and its coverage that draws under both carry in place of the two,
    /// when they make one within [`DRAW_SLACK`]: an edge the two share then
    /// takes coverage once, not the scissor's over the clip's.
    pub(crate) fn with_scissor(&self, scissor: &Self, fringe_width: f32) -> Option<(Self, ClipCoverage)> {
        let both = self.intersection_within(scissor, fringe_width, DRAW_SLACK)?;
        Some((both, both.coverage(fringe_width)?))
    }

    /// What of `scissor` - a box with square corners - still clips a draw
    /// under this box too: each side of the scissor that this box lies
    /// within, to [`DRAW_SLACK`], moves out past this box's reach, so that
    /// an edge the two share is cut once, by this box. `None` when this box
    /// lies within all four; the scissor as it is when its corners are round
    /// or its sides are not parallel to this box's.
    pub(crate) fn trimmed_scissor(&self, scissor: &Self, fringe_width: f32) -> Option<Self> {
        let Some(([x, y], half, _)) = scissor.alongside(self).filter(|_| scissor.radii == [0.0, 0.0]) else {
            return Some(*scissor);
        };
        let per_fringe = scissor.per_fringe(fringe_width);
        let mut low = [-scissor.extent[0], -scissor.extent[1]];
        let mut high = scissor.extent;
        let mut cuts = false;
        for (axis, (center, half)) in [(x, half[0]), (y, half[1])].into_iter().enumerate() {
            let slack = DRAW_SLACK * per_fringe[axis];
            // This box's coverage ends half a fringe past its side, and the
            // scissor's ramp is half a fringe wide either side of its own.
            let past = 2.0 * per_fringe[axis];
            if low[axis] <= center - half + slack {
                low[axis] = center - half - past;
            } else {
                cuts = true;
            }
            if high[axis] >= center + half - slack {
                high[axis] = center + half + past;
            } else {
                cuts = true;
            }
        }
        cuts.then(|| Self {
            frame: Transform2D::translation((low[0] + high[0]) * 0.5, (low[1] + high[1]) * 0.5) * scissor.frame,
            extent: [(high[0] - low[0]) * 0.5, (high[1] - low[1]) * 0.5],
            radii: [0.0, 0.0],
        })
    }

    /// What two boxes with square corners and parallel sides both cover,
    /// exactly; `None` when their sides are not parallel, and a box with no
    /// extent where they do not overlap.
    pub(crate) fn rect_intersection(&self, other: &Self) -> Option<Self> {
        if self.radii != [0.0, 0.0] || other.radii != [0.0, 0.0] {
            return None;
        }
        let ([x, y], half, _) = self.alongside(other)?;
        let low = [(x - half[0]).max(-self.extent[0]), (y - half[1]).max(-self.extent[1])];
        let high = [(x + half[0]).min(self.extent[0]), (y + half[1]).min(self.extent[1])];
        let high = [high[0].max(low[0]), high[1].max(low[1])];
        Some(Self {
            frame: Transform2D::translation((low[0] + high[0]) * 0.5, (low[1] + high[1]) * 0.5) * self.frame,
            extent: [(high[0] - low[0]) * 0.5, (high[1] - low[1]) * 0.5],
            radii: [0.0, 0.0],
        })
    }

    /// Whether this box's sides are parallel to `other`'s.
    pub(crate) fn parallel(&self, other: &Self) -> bool {
        self.alongside(other).is_some()
    }

    /// The part of an outline - `points` in order around a convex region,
    /// in the space this box's frame maps to - that lies inside this box
    /// with square corners: each side drops what lies beyond it, which
    /// leaves one convex outline (Sutherland and Hodgman).
    pub(crate) fn cut_outline(&self, points: impl Iterator<Item = [f32; 2]>) -> Vec<[f32; 2]> {
        let inverse = self.frame.inverse();
        let mut outline: Vec<([f32; 2], [f32; 2])> = points
            .map(|point| {
                let (x, y) = inverse.transform_point(point[0], point[1]);
                (point, [x, y])
            })
            .collect();
        for (axis, sign) in [(0, 1.0), (0, -1.0), (1, 1.0), (1, -1.0)] {
            let beyond = |at: &[f32; 2]| sign * at[axis] - self.extent[axis];
            let mut kept = Vec::with_capacity(outline.len() + 1);
            for (i, &(point, at)) in outline.iter().enumerate() {
                let (next, next_at) = outline[(i + 1) % outline.len()];
                let (here, there) = (beyond(&at), beyond(&next_at));
                if here <= 0.0 {
                    kept.push((point, at));
                }
                if (here <= 0.0) != (there <= 0.0) {
                    let t = here / (here - there);
                    let lerp = |a: [f32; 2], b: [f32; 2]| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
                    kept.push((lerp(point, next), lerp(at, next_at)));
                }
            }
            outline = kept;
            if outline.is_empty() {
                break;
            }
        }
        outline.into_iter().map(|(point, _)| point).collect()
    }

    /// The device rectangle a box's corners span, as a path's bounds are.
    pub(crate) fn bounds(&self) -> Bounds {
        Bounds::from_points(
            [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
                .map(|[x, y]| self.frame.transform_point(x * self.extent[0], y * self.extent[1])),
        )
    }

    /// The box as a path in its own frame, for the stencil.
    pub(crate) fn path(&self) -> Path {
        let ([ex, ey], [rx, ry]) = (self.extent, self.radii);
        let mut path = Path::new();
        if rx <= 0.0 {
            path.rect(-ex, -ey, 2.0 * ex, 2.0 * ey);
            return path;
        }
        // A quarter ellipse as one cubic: handles this far short of the corner.
        let (hx, hy) = (rx * (1.0 - KAPPA90), ry * (1.0 - KAPPA90));
        path.move_to(-ex + rx, -ey);
        path.line_to(ex - rx, -ey);
        path.bezier_to(ex - hx, -ey, ex, -ey + hy, ex, -ey + ry);
        path.line_to(ex, ey - ry);
        path.bezier_to(ex, ey - hy, ex - hx, ey, ex - rx, ey);
        path.line_to(-ex + rx, ey);
        path.bezier_to(-ex + hx, ey, -ex, ey - hy, -ex, ey - ry);
        path.line_to(-ex, -ey + ry);
        path.bezier_to(-ex, -ey + hy, -ex + hx, -ey, -ex + rx, -ey);
        path.close();
        path
    }

    /// The box's corners in the space its frame maps to.
    #[cfg(test)]
    fn corners(&self) -> [[f32; 2]; 4] {
        [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]].map(|[x, y]| {
            let (x, y) = self.frame.transform_point(x * self.extent[0], y * self.extent[1]);
            [x, y]
        })
    }

    /// Points along the box's outline in the space its frame maps to: each
    /// corner's arc from one side to the next, which for a sharp corner is
    /// the corner itself.
    fn outline(&self) -> impl Iterator<Item = [f32; 2]> + '_ {
        [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
            .into_iter()
            .flat_map(move |[sx, sy]: [f32; 2]| {
                CORNER_ARC.into_iter().map(move |[cos, sin]| {
                    let x = self.extent[0] - self.radii[0] * (1.0 - cos);
                    let y = self.extent[1] - self.radii[1] * (1.0 - sin);
                    let (x, y) = self.frame.transform_point(x * sx, y * sy);
                    [x, y]
                })
            })
    }
}

impl ClipCoverage {
    /// A coverage of one at every position: a box no fragment is outside.
    pub(crate) const EVERYWHERE: Self = Self {
        linear: [0.0; 4],
        offset: [0.0; 2],
        extent: [1.0; 2],
        radii: [0.0; 2],
        strays: 0.0,
    };

    /// The coverage of a box whose fitted outline strays `strays` fringe
    /// widths outside its round corners.
    pub(crate) fn straying(self, strays: f32) -> Self {
        Self { strays, ..self }
    }

    /// The ten floats both backends' shaders read (`clipMask`): where the
    /// corners' ellipses are centered, the linear part by rows, the offset,
    /// and the half extents grown by the half fringe a side's ramp reaches.
    pub(crate) fn uniform_rows(&self) -> [f32; 10] {
        let inner = if self.radii[0] > 0.0 {
            [self.extent[0] - self.radii[0], self.extent[1] - self.radii[1]]
        } else {
            [NO_CORNERS; 2]
        };
        let [a, b, c, d] = self.linear;
        let outer = [self.extent[0] + 0.5, self.extent[1] + 0.5];
        [
            inner[0],
            inner[1],
            a,
            b,
            c,
            d,
            self.offset[0],
            self.offset[1],
            outer[0],
            outer[1],
        ]
    }

    /// Signed distance from the box's edge at a device position, in fringe
    /// widths and negative inside: what the fragment shaders' coverage is
    /// half a fringe minus.
    pub(crate) fn distance(&self, point: [f32; 2]) -> f32 {
        self.locate(point).0
    }

    /// A position's [`Self::distance`], and whether it lies off one of the
    /// box's round corners - the ends of the arc with it - or off a side.
    fn locate(&self, [x, y]: [f32; 2]) -> (f32, bool) {
        let u = [
            self.linear[0] * x + self.linear[1] * y + self.offset[0],
            self.linear[2] * x + self.linear[3] * y + self.offset[1],
        ];
        let side = [u[0].abs() - self.extent[0], u[1].abs() - self.extent[1]];
        let corner = [side[0] + self.radii[0], side[1] + self.radii[1]];
        let round = self.radii[0] > 0.0 && corner[0] >= 0.0 && corner[1] >= 0.0;
        if round && corner[0] > 0.0 && corner[1] > 0.0 {
            // k1 is one on the corner's ellipse; its gradient in device
            // pixels is g along the frame's rows, which are unit vectors but
            // stand at a right angle only without a skew.
            let k = [corner[0] / self.radii[0], corner[1] / self.radii[1]];
            let k1 = k[0].hypot(k[1]);
            let g = [k[0] / self.radii[0], k[1] / self.radii[1]];
            let lean =
                (self.linear[0] * self.linear[2] + self.linear[1] * self.linear[3]) * u[0].signum() * u[1].signum();
            let distance = (k1 - 1.0) * k1 / (g[0] * g[0] + g[1] * g[1] + 2.0 * g[0] * g[1] * lean).sqrt();
            (distance, true)
        } else {
            (side[0].max(side[1]), round)
        }
    }

    /// How far a device position is outside the clip as it was given, in
    /// fringe widths: its [`Self::distance`] from the box, less off a round
    /// corner what the outline the box was fitted to strays outside it and
    /// the `straddle` the point may stand off its curve.
    fn outside(&self, point: [f32; 2], straddle: f32) -> f32 {
        let (distance, round) = self.locate(point);
        if round {
            distance - self.strays - straddle
        } else {
            distance
        }
    }

    /// The device rectangle outside which the coverage is zero - the box out
    /// to where its sides' ramps end - or `None` for a coverage of one
    /// everywhere.
    pub(crate) fn reach(&self) -> Option<Bounds> {
        self.bounds(0.5)
    }

    /// The device bounds of the box with each side `margin` fringe widths
    /// further out; `None` for a coverage of one everywhere.
    fn bounds(&self, margin: f32) -> Option<Bounds> {
        let [a, b, c, d] = self.linear;
        let determinant = a * d - b * c;
        if determinant == 0.0 {
            return None;
        }
        // The inverse of the linear part takes the box's half sides back
        // to device pixels.
        let outer = [self.extent[0] + margin, self.extent[1] + margin];
        let half = [
            (d.abs() * outer[0] + b.abs() * outer[1]) / determinant.abs(),
            (c.abs() * outer[0] + a.abs() * outer[1]) / determinant.abs(),
        ];
        let center = [
            (b * self.offset[1] - d * self.offset[0]) / determinant,
            (c * self.offset[0] - a * self.offset[1]) / determinant,
        ];
        Some(Bounds {
            minx: center[0] - half[0],
            miny: center[1] - half[1],
            maxx: center[0] + half[0],
            maxy: center[1] + half[1],
        })
    }

    /// Whether the clip as it was given ([`Self::outside`]) holds a
    /// rectangle of device pixels whole, within [`DRAW_SLACK`]: a draw that
    /// stays inside it has nothing clipped, and along an edge it shares
    /// with the box its own antialiasing is its coverage.
    pub(crate) fn holds(&self, bounds: &Bounds) -> bool {
        if self.radii == [0.0; 2] {
            // Between two parallel sides the rectangle's corners reach as
            // far from its center as its half sides do along the row.
            let center = [(bounds.minx + bounds.maxx) * 0.5, (bounds.miny + bounds.maxy) * 0.5];
            let half = [(bounds.maxx - bounds.minx) * 0.5, (bounds.maxy - bounds.miny) * 0.5];
            return [0, 1].into_iter().all(|axis| {
                let [a, b] = [self.linear[2 * axis], self.linear[2 * axis + 1]];
                let at = a * center[0] + b * center[1] + self.offset[axis];
                at.abs() + a.abs() * half[0] + b.abs() * half[1] - self.extent[axis] <= DRAW_SLACK
            });
        }
        [
            [bounds.minx, bounds.miny],
            [bounds.maxx, bounds.miny],
            [bounds.maxx, bounds.maxy],
            [bounds.minx, bounds.maxy],
        ]
        .into_iter()
        .all(|corner| self.outside(corner, 0.0) <= DRAW_SLACK)
    }

    /// Whether the clip as it was given holds a draw whole, within
    /// [`DRAW_SLACK`], by the points of its outline: each one inside, with
    /// the `spread` - in fringe widths - the draw reaches around it, a
    /// stroke's half width, and off a round corner with the `straddle` a
    /// point between two chords may stand off its curve. The box is convex,
    /// so what lies between the points is inside with them; a rounded rect
    /// filled under its own outline as a clip is held where its bounds,
    /// which stand past the corners, are not. The points
    /// are looked at only where they can tell more than `bounds`, the
    /// draw's, do: not for a draw that reaches past the box's own bounds,
    /// nor under an upright box with square corners, which holds the points
    /// when it holds the bounds. Around a point the distance must be exact,
    /// which it is with square or circular corners and no skew: no other
    /// box holds a stroke this way.
    pub(crate) fn holds_outline(
        &self,
        bounds: &Bounds,
        points: impl Iterator<Item = ([f32; 2], bool)>,
        spread: f32,
        straddle: f32,
    ) -> bool {
        // The draw's bounds come from its points, so they stand out as far.
        let Some(own) = self.bounds(DRAW_SLACK + self.strays + straddle) else {
            return true;
        };
        let [a, b, c, d] = self.linear;
        let upright = (b == 0.0 && c == 0.0) || (a == 0.0 && d == 0.0);
        let square = self.radii == [0.0; 2];
        let past = bounds.minx < own.minx || bounds.miny < own.miny || bounds.maxx > own.maxx || bounds.maxy > own.maxy;
        if past || (upright && square) {
            return false;
        }
        let exact = square || (self.radii[0] == self.radii[1] && (a * c + b * d).abs() <= 1e-4);
        if spread > 0.0 && !exact {
            return false;
        }
        let mut points = points.peekable();
        points.peek().is_some()
            && points.all(|(point, between_chords)| {
                self.outside(point, if between_chords { straddle } else { 0.0 }) + spread <= DRAW_SLACK
            })
    }

    /// Whether `other` lies inside this box, within [`CONTAINMENT_SLACK`],
    /// so that clipping to both is clipping to `other`.
    pub(crate) fn contains(&self, other: &RoundedBox) -> bool {
        other.outline().all(|point| self.distance(point) <= CONTAINMENT_SLACK)
    }
}

/// The path's single contour as its start and its segments, closed; `None`
/// for an empty path or one with a second contour.
fn contour(path: &Path) -> Option<([f32; 2], Vec<Segment>)> {
    let mut start: Option<[f32; 2]> = None;
    let mut last = [0.0, 0.0];
    let mut segments = Vec::with_capacity(path.verb_count());
    let mut closed = false;
    for verb in path.verbs() {
        match verb {
            Verb::MoveTo(x, y) => {
                if !segments.is_empty() {
                    return None;
                }
                start = Some([x, y]);
                last = [x, y];
            }
            Verb::LineTo(x, y) => {
                if closed || start.is_none() {
                    return None;
                }
                if [x, y] != last {
                    segments.push(Segment::Line([x, y]));
                    last = [x, y];
                }
            }
            Verb::BezierTo(c1x, c1y, c2x, c2y, x, y) => {
                if closed || start.is_none() {
                    return None;
                }
                segments.push(Segment::Cubic([c1x, c1y], [c2x, c2y], [x, y]));
                last = [x, y];
            }
            Verb::Close => closed = true,
            Verb::Solid | Verb::Hole => {}
        }
    }
    let start = start?;
    if last != start {
        segments.push(Segment::Line(start));
    }
    (!segments.is_empty()).then_some((start, segments))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(a: [f32; 2], b: [f32; 2]) {
        assert!(
            (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3,
            "{a:?} vs {b:?}"
        );
    }

    /// An ellipse as tiny-skia builds one: eight quadratics, 0.31 % outside
    /// the true curve at their middles.
    fn quad_ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Path {
        let mut path = Path::new();
        let point = |i: usize, scale: f32| {
            let a = i as f32 * std::f32::consts::FRAC_PI_4;
            (cx + rx * scale * a.cos(), cy + ry * scale * a.sin())
        };
        let (x, y) = point(0, 1.0);
        path.move_to(x, y);
        for i in 0..8 {
            let a = (i as f32 + 0.5) * std::f32::consts::FRAC_PI_4;
            let reach = 1.0 / std::f32::consts::FRAC_PI_8.cos();
            let (x, y) = point(i + 1, 1.0);
            path.quad_to(cx + rx * reach * a.cos(), cy + ry * reach * a.sin(), x, y);
        }
        path.close();
        path
    }

    /// A rectangle rounded at the two corners of one side - any side - fits
    /// as a rounded box that runs on past the square side and the rectangle
    /// that cuts it there: inside both is inside the outline, everywhere.
    /// It is no box on its own, and no other mix of corners fits.
    #[test]
    fn a_rect_rounded_on_one_side_is_a_rounded_box_and_a_cut() {
        let (x, y, w, h, r) = (10.0, 20.0, 60.0, 30.0, 8.0);
        // Inside the rect, and in a round corner's square inside its circle.
        let inside_outline = |px: f32, py: f32, [tl, tr, br, bl]: [f32; 4]| {
            let corner = |r: f32, ox: f32, oy: f32, sx: f32, sy: f32| {
                r == 0.0 || (px - ox) * sx <= 0.0 || (py - oy) * sy <= 0.0 || (px - ox).hypot(py - oy) <= r
            };
            (x..=x + w).contains(&px)
                && (y..=y + h).contains(&py)
                && corner(tl, x + tl, y + tl, -1.0, -1.0)
                && corner(tr, x + w - tr, y + tr, 1.0, -1.0)
                && corner(br, x + w - br, y + h - br, 1.0, 1.0)
                && corner(bl, x + bl, y + h - bl, -1.0, 1.0)
        };
        for corners in [[r, r, 0.0, 0.0], [0.0, r, r, 0.0], [0.0, 0.0, r, r], [r, 0.0, 0.0, r]] {
            let mut path = Path::new();
            path.rounded_rect_varying(x, y, w, h, corners[0], corners[1], corners[2], corners[3]);
            assert!(RoundedBox::fit_outline(&path).is_none(), "{corners:?}: no box alone");
            let (shape, _, cut) = RoundedBox::fit_with_cut(&path).unwrap();
            let cut = cut.unwrap();
            assert_eq!(shape.radii, [r, r], "{corners:?}");
            assert_eq!(cut.radii, [0.0, 0.0], "{corners:?}");
            let (shape_coverage, cut_coverage) = (shape.coverage(1.0).unwrap(), cut.coverage(1.0).unwrap());
            for i in 0..=80 {
                for j in 0..=50 {
                    let (px, py) = (x - 5.0 + i as f32 * 0.875, y - 5.0 + j as f32 * 0.8);
                    let both = shape_coverage.distance([px, py]) <= 0.0 && cut_coverage.distance([px, py]) <= 0.0;
                    let want = inside_outline(px, py, corners);
                    // Points within a hair of the outline may fall either way.
                    let near = shape_coverage
                        .distance([px, py])
                        .abs()
                        .min(cut_coverage.distance([px, py]).abs())
                        < 0.05;
                    assert!(
                        both == want || near,
                        "{corners:?}: ({px}, {py}) inside both {both}, the outline {want}"
                    );
                }
            }
        }
        for corners in [[r, r, r, 0.0], [r, 0.0, r, 0.0], [r, r, 4.0, 4.0]] {
            let mut path = Path::new();
            path.rounded_rect_varying(x, y, w, h, corners[0], corners[1], corners[2], corners[3]);
            assert!(
                RoundedBox::fit_with_cut(&path).is_none(),
                "{corners:?}: no box, cut or not"
            );
        }
    }

    /// A scissor keeps the sides that cut into a box and loses those the box
    /// lies within, which move out past the box's reach; a box within all
    /// four leaves no scissor. A rounded scissor, or one at an angle, stays
    /// as it is.
    #[test]
    fn a_scissor_keeps_only_the_sides_that_cut_into_the_box() {
        let window = RoundedBox {
            frame: Transform2D::translation(50.0, 45.0),
            extent: [40.0, 35.0],
            radii: [12.0, 12.0],
        };
        let rect = |left: f32, top: f32, right: f32, bottom: f32| RoundedBox {
            frame: Transform2D::translation((left + right) * 0.5, (top + bottom) * 0.5),
            extent: [(right - left) * 0.5, (bottom - top) * 0.5],
            radii: [0.0, 0.0],
        };
        let sides = |b: RoundedBox| {
            let (x, y) = b.frame.transform_point(0.0, 0.0);
            [x - b.extent[0], y - b.extent[1], x + b.extent[0], y + b.extent[1]]
        };
        let trimmed = window.trimmed_scissor(&rect(10.0, 30.5, 90.0, 100.5), 1.0).unwrap();
        assert_eq!(sides(trimmed), [8.0, 30.5, 92.0, 82.0], "only the top cuts");
        let trimmed = window.trimmed_scissor(&rect(25.0, 0.0, 60.0, 100.0), 1.0).unwrap();
        assert_eq!(sides(trimmed), [25.0, 8.0, 60.0, 82.0], "the left and right cut");
        assert!(
            window.trimmed_scissor(&rect(0.0, 10.0, 90.0, 120.0), 1.0).is_none(),
            "around the box"
        );
        let rounded = RoundedBox {
            radii: [4.0, 4.0],
            ..rect(10.0, 30.5, 90.0, 100.5)
        };
        assert_eq!(
            window.trimmed_scissor(&rounded, 1.0),
            Some(rounded),
            "a rounded scissor"
        );
        let mut turned = Transform2D::identity();
        turned.rotate(0.3);
        let at_an_angle = RoundedBox {
            frame: turned,
            ..rect(10.0, 30.5, 90.0, 100.5)
        };
        assert_eq!(
            window.trimmed_scissor(&at_an_angle, 1.0),
            Some(at_an_angle),
            "at an angle"
        );
    }

    /// An outline cut by a box keeps what lies inside the box: a square cut
    /// by a rect across one corner is the rect the two share.
    #[test]
    fn an_outline_cut_by_a_box_is_what_lies_inside_it() {
        let square = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        let cut = RoundedBox {
            frame: Transform2D::translation(100.0, 20.0),
            extent: [50.0, 30.0],
            radii: [0.0, 0.0],
        };
        let outline = cut.cut_outline(square.into_iter());
        let area = outline
            .iter()
            .zip(outline.iter().cycle().skip(1))
            .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
            .sum::<f32>()
            .abs()
            * 0.5;
        assert!((area - 50.0 * 50.0).abs() < 1e-3, "{area} {outline:?}");
        let away = RoundedBox {
            frame: Transform2D::translation(300.0, 300.0),
            ..cut
        };
        assert!(away.cut_outline(square.into_iter()).is_empty(), "nothing inside");
    }

    /// The fit tells how far the outline strays outside the box it found:
    /// nothing for straight sides, 0.03 % of the radius for corners drawn as
    /// the usual cubics and 0.3 % for an ellipse of eight quadratics - and a
    /// coverage that carries that holds the outline it was fitted to.
    #[test]
    fn a_fit_tells_how_far_the_outline_strays_outside_the_box() {
        let mut rect = Path::new();
        rect.rect(10.0, 20.0, 30.0, 40.0);
        assert_eq!(RoundedBox::fit_outline(&rect).unwrap().1, 0.0);
        let mut rounded = Path::new();
        rounded.rounded_rect(10.0, 20.0, 300.0, 400.0, 100.0);
        let (_, cubic) = RoundedBox::fit_outline(&rounded).unwrap();
        assert!((0.02..0.04).contains(&cubic), "{cubic} of a radius of 100");
        let (shape, quadratic) = RoundedBox::fit_outline(&quad_ellipse(200.0, 150.0, 100.0, 100.0)).unwrap();
        assert!((0.3..0.4).contains(&quadratic), "{quadratic} of a radius of 100");

        // The quadratics' own points, flattened: outside the circle by more
        // than the slack every draw has, and held once the coverage knows.
        let outline: Vec<[f32; 2]> = (0..64)
            .map(|i| {
                let (segment, t) = (i / 8, (i % 8) as f32 / 8.0);
                let at = |j: usize, scale: f32| {
                    let a = j as f32 * std::f32::consts::FRAC_PI_4;
                    [200.0 + 100.0 * scale * a.cos(), 150.0 + 100.0 * scale * a.sin()]
                };
                let a = (segment as f32 + 0.5) * std::f32::consts::FRAC_PI_4;
                let reach = 100.0 / std::f32::consts::FRAC_PI_8.cos();
                let (p0, c, p2) = (
                    at(segment, 1.0),
                    [200.0 + reach * a.cos(), 150.0 + reach * a.sin()],
                    at(segment + 1, 1.0),
                );
                let s = 1.0 - t;
                [
                    s * s * p0[0] + 2.0 * s * t * c[0] + t * t * p2[0],
                    s * s * p0[1] + 2.0 * s * t * c[1] + t * t * p2[1],
                ]
            })
            .collect();
        let bounds = outline.iter().fold(Bounds::default(), |bounds, [x, y]| Bounds {
            minx: bounds.minx.min(*x),
            miny: bounds.miny.min(*y),
            maxx: bounds.maxx.max(*x),
            maxy: bounds.maxy.max(*y),
        });
        let coverage = shape.coverage(1.0).unwrap();
        assert!(!coverage.holds_outline(&bounds, outline.iter().map(|&point| (point, false)), 0.0, 0.0));
        let knowing = coverage.straying(quadratic);
        assert!(knowing.holds_outline(&bounds, outline.iter().map(|&point| (point, false)), 0.0, 0.0));
    }

    /// A box holds a draw by the points of its outline where its bounds
    /// stand past the box's corners: the box's own outline, as a fill, under
    /// any frame, and not once a point is a sixteenth of a pixel out. A
    /// stroke is held with its half width around every point, by a box whose
    /// distance is exact - not by one with elliptical corners or a skew.
    #[test]
    fn a_box_holds_its_own_outline_and_a_stroke_set_in_from_it() {
        let mut turned = Transform2D::identity();
        turned.rotate(0.4);
        turned.translate(31.0, 47.5);
        let skewed = Transform2D::new(1.0, 0.0, 0.5, 1.0, 40.0, 30.0);
        let bounds_of = |points: &[[f32; 2]]| {
            points.iter().fold(Bounds::default(), |bounds, [x, y]| Bounds {
                minx: bounds.minx.min(*x),
                miny: bounds.miny.min(*y),
                maxx: bounds.maxx.max(*x),
                maxy: bounds.maxy.max(*y),
            })
        };
        for (frame, radii, strokes) in [
            (Transform2D::translation(40.25, 30.5), [6.0, 6.0], true),
            (Transform2D::translation(40.25, 30.5), [0.0, 0.0], true),
            (turned, [6.0, 6.0], true),
            (turned, [8.0, 3.0], false),
            (skewed, [0.0, 0.0], true),
            (skewed, [6.0, 6.0], false),
        ] {
            let shape = RoundedBox {
                frame,
                extent: [20.0, 12.5],
                radii,
            };
            let coverage = shape.coverage(1.0).unwrap();
            let outline: Vec<[f32; 2]> = shape.outline().collect();
            let bounds = bounds_of(&outline);
            // An upright box with square corners holds the points when it
            // holds the bounds, and is not asked about them.
            let by_bounds = radii == [0.0, 0.0] && frame[1] == 0.0 && frame[2] == 0.0;
            assert_eq!(coverage.holds(&bounds), by_bounds, "{frame:?} {radii:?}");
            assert_eq!(
                coverage.holds_outline(&bounds, outline.iter().map(|&point| (point, false)), 0.0, 0.0),
                !by_bounds,
                "{frame:?} {radii:?}"
            );
            // One point a sixteenth of a pixel further from the center.
            let mut out = outline.clone();
            let (cx, cy) = frame.transform_point(0.0, 0.0);
            let [x, y] = out[3];
            let away = 0.0625 / (x - cx).hypot(y - cy);
            out[3] = [x + (x - cx) * away, y + (y - cy) * away];
            let leaves = coverage.distance(out[3]) > DRAW_SLACK;
            assert!(
                leaves && !coverage.holds_outline(&bounds_of(&out), out.iter().map(|&point| (point, false)), 0.0, 0.0)
            );
            assert!(
                !coverage.holds_outline(&bounds, std::iter::empty(), 0.0, 0.0),
                "no points, nothing held"
            );

            // The outline of the box two pixels in from each side, stroked
            // four wide.
            let per_fringe = shape.per_fringe(1.0);
            let inset = RoundedBox {
                extent: [20.0 - 2.0 * per_fringe[0], 12.5 - 2.0 * per_fringe[1]],
                radii: [(radii[0] - 2.0).max(0.0), (radii[1] - 2.0).max(0.0)],
                ..shape
            };
            let path: Vec<[f32; 2]> = inset.outline().collect();
            let stroke_bounds = bounds_of(&path);
            let stroke_bounds = Bounds {
                minx: stroke_bounds.minx - 2.0,
                miny: stroke_bounds.miny - 2.0,
                maxx: stroke_bounds.maxx + 2.0,
                maxy: stroke_bounds.maxy + 2.0,
            };
            assert_eq!(
                coverage.holds(&stroke_bounds)
                    || coverage.holds_outline(&stroke_bounds, path.iter().map(|&point| (point, false)), 2.0, 0.0),
                strokes,
                "{frame:?} {radii:?}: a stroke on the box's edge from inside"
            );
            assert!(!coverage.holds_outline(&stroke_bounds, path.iter().map(|&point| (point, false)), 2.1, 0.0));
        }
    }

    /// What the fitted outline strays outside the box counts off its round
    /// corners alone. Past a straight side - a rounded box's, or a square
    /// one's, as the box a scissor leaves of a rounded clip is - a draw is
    /// held within [`DRAW_SLACK`] and no further.
    #[test]
    fn an_outlines_stray_gives_no_way_past_a_straight_side() {
        let past_a_side = Bounds {
            minx: 30.0,
            miny: 25.0,
            maxx: 60.25,
            maxy: 35.0,
        };
        let corners = [[30.0, 25.0], [60.25, 25.0], [60.25, 35.0], [30.0, 35.0]];
        for radii in [[0.0, 0.0], [6.0, 6.0]] {
            let shape = RoundedBox {
                frame: Transform2D::translation(40.0, 30.0),
                extent: [20.0, 12.5],
                radii,
            };
            let coverage = shape.coverage(1.0).unwrap().straying(0.5);
            assert!(!coverage.holds(&past_a_side), "{radii:?}");
            // Nor do points between chords take their straddle there.
            for (between_chords, straddle) in [(false, 0.0), (true, 0.5)] {
                let points = corners.map(|point| (point, between_chords)).into_iter();
                assert!(
                    !coverage.holds_outline(&past_a_side, points, 0.0, straddle),
                    "{radii:?}"
                );
            }
        }

        // A quarter pixel off the corner's arc, and on the axis of a circle,
        // where the arc of one corner ends and the next begins.
        let rounded = RoundedBox {
            frame: Transform2D::translation(40.0, 30.0),
            extent: [20.0, 12.5],
            radii: [6.0, 6.0],
        };
        let circle = RoundedBox {
            frame: Transform2D::translation(40.0, 30.0),
            extent: [20.0, 20.0],
            radii: [20.0, 20.0],
        };
        let diagonal = 6.25 * std::f32::consts::FRAC_1_SQRT_2;
        for (shape, point) in [(rounded, [54.0 + diagonal, 36.5 + diagonal]), (circle, [60.25, 30.0])] {
            let points = [[40.0, 30.0], point, [40.0, 34.0]];
            let bounds = Bounds {
                minx: 40.0,
                miny: 30.0,
                maxx: point[0],
                maxy: point[1].max(34.0),
            };
            let coverage = shape.coverage(1.0).unwrap();
            assert!(
                !coverage.holds_outline(&bounds, points.map(|point| (point, false)).into_iter(), 0.0, 0.0),
                "{point:?}"
            );
            let knowing = coverage.straying(0.5);
            assert!(
                knowing.holds_outline(&bounds, points.map(|point| (point, false)).into_iter(), 0.0, 0.0),
                "{point:?}"
            );
        }
    }

    /// A point between two chords may stand off a round corner by the
    /// straddle, past the box's own bounds too; a point where the path put it
    /// may not.
    #[test]
    fn only_a_point_between_chords_stands_off_by_the_straddle() {
        let circle = RoundedBox {
            frame: Transform2D::translation(40.0, 30.0),
            extent: [20.0, 20.0],
            radii: [20.0, 20.0],
        };
        let coverage = circle.coverage(1.0).unwrap();
        let out = [60.04, 30.0];
        let bounds = Bounds {
            minx: 40.0,
            miny: 30.0,
            maxx: out[0],
            maxy: 34.0,
        };
        let points = |between_chords| [([40.0, 30.0], false), (out, between_chords), ([40.0, 34.0], false)].into_iter();
        assert!(coverage.holds_outline(&bounds, points(true), 0.0, 0.05));
        assert!(!coverage.holds_outline(&bounds, points(true), 0.0, 0.0));
        assert!(!coverage.holds_outline(&bounds, points(false), 0.0, 0.05));
    }

    /// A coverage is zero outside its reach, and the reach is no wider than
    /// that: the box grown by the half fringe its sides' ramps run past it.
    #[test]
    fn a_coverage_reaches_as_far_as_its_ramps_and_no_further() {
        let mut turned = Transform2D::identity();
        turned.rotate(0.4);
        turned.translate(31.0, 47.5);
        let mut skewed = Transform2D::new(3.0, 0.5, -1.25, 0.75, 0.0, 0.0);
        skewed.translate(40.25, 52.0);
        for frame in [Transform2D::translation(40.25, 30.5), turned, skewed] {
            for radius in [0.0, 6.0] {
                let shape = RoundedBox {
                    frame,
                    extent: [20.0, 12.5],
                    radii: [radius; 2],
                };
                let coverage = shape.coverage(1.0).unwrap();
                let reach = coverage.reach().unwrap();
                let per_fringe = shape.per_fringe(1.0);
                let ramps = RoundedBox {
                    extent: [20.0 + 0.5 * per_fringe[0], 12.5 + 0.5 * per_fringe[1]],
                    radii: [0.0; 2],
                    ..shape
                }
                .corners();
                let pick = |f: fn(f32, f32) -> f32, axis: usize| ramps.iter().map(|c| c[axis]).reduce(f).unwrap();
                assert_close([reach.minx, reach.miny], [pick(f32::min, 0), pick(f32::min, 1)]);
                assert_close([reach.maxx, reach.maxy], [pick(f32::max, 0), pick(f32::max, 1)]);
                for step in 0..(200 * 200) {
                    let at = [(step % 200) as f32 * 0.75 - 40.0, (step / 200) as f32 * 0.75 - 40.0];
                    let inside =
                        reach.minx <= at[0] && at[0] <= reach.maxx && reach.miny <= at[1] && at[1] <= reach.maxy;
                    assert!(
                        inside || coverage.distance(at) >= 0.5,
                        "{at:?} is covered outside {reach:?}"
                    );
                }
            }
        }
        assert!(ClipCoverage::EVERYWHERE.reach().is_none());
    }

    #[test]
    fn a_rect_and_any_parallelogram_fit_with_their_own_frame() {
        let mut rect = Path::new();
        rect.rect(10.0, 20.0, 30.0, 40.0);
        let fit = RoundedBox::fit(&rect).unwrap();
        assert_eq!(
            (fit.extent, fit.radii),
            ([20.0, 15.0], [0.0, 0.0]),
            "sides in path order"
        );
        assert_close(
            [
                fit.frame.transform_point(0.0, 0.0).0,
                fit.frame.transform_point(0.0, 0.0).1,
            ],
            [25.0, 40.0],
        );

        // Open, with a collinear point on a side, and skewed: still one.
        let mut skewed = Path::new();
        skewed.move_to(0.0, 0.0);
        skewed.line_to(5.0, 0.0);
        skewed.line_to(10.0, 0.0);
        skewed.line_to(14.0, 8.0);
        skewed.line_to(4.0, 8.0);
        let fit = RoundedBox::fit(&skewed).unwrap();
        assert_close(fit.extent, [5.0, 20.0_f32.sqrt()]);
        let mut corners = fit.corners().to_vec();
        corners.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut expected = vec![[0.0, 0.0], [10.0, 0.0], [14.0, 8.0], [4.0, 8.0]];
        expected.sort_by(|a: &[f32; 2], b| a.partial_cmp(b).unwrap());
        for (corner, expected) in corners.iter().zip(expected) {
            assert_close(*corner, expected);
        }
    }

    #[test]
    fn ellipses_and_rounded_rects_fit_whatever_they_are_built_from() {
        let mut circle = Path::new();
        circle.circle(50.0, 40.0, 25.0);
        let fit = RoundedBox::fit(&circle).unwrap();
        assert_close(fit.extent, [25.0, 25.0]);
        assert_close(fit.radii, [25.0, 25.0]);

        let mut ellipse = Path::new();
        ellipse.ellipse(0.0, 0.0, 30.0, 10.0);
        let fit = RoundedBox::fit(&ellipse).unwrap();
        assert_close(fit.extent, [30.0, 10.0]);
        assert_close(fit.radii, [30.0, 10.0]);

        let fit = RoundedBox::fit(&quad_ellipse(100.0, 100.0, 50.0, 30.0)).unwrap();
        assert!((fit.extent[0] - 50.0).abs() < 0.2 && (fit.extent[1] - 30.0).abs() < 0.2);
        assert_eq!(fit.radii, fit.extent, "no straight side: an ellipse");

        let mut rounded = Path::new();
        rounded.rounded_rect(10.0, 10.0, 80.0, 40.0, 12.0);
        let fit = RoundedBox::fit(&rounded).unwrap();
        assert_close(fit.extent, [40.0, 20.0]);
        assert_close(fit.radii, [12.0, 12.0]);
    }

    #[test]
    fn a_fit_does_not_depend_on_scale() {
        for scale in [0.01, 1.0, 400.0] {
            let fit = RoundedBox::fit(&quad_ellipse(0.0, 0.0, 50.0 * scale, 30.0 * scale));
            assert!(fit.is_some(), "scale {scale}");
            let mut rounded = Path::new();
            rounded.rounded_rect(0.0, 0.0, 80.0 * scale, 40.0 * scale, 12.0 * scale);
            assert!(RoundedBox::fit(&rounded).is_some(), "scale {scale}");
        }
    }

    #[test]
    fn other_outlines_do_not_fit() {
        let mut triangle = Path::new();
        triangle.move_to(0.0, 0.0);
        triangle.line_to(10.0, 0.0);
        triangle.line_to(5.0, 8.0);
        triangle.close();
        assert_eq!(RoundedBox::fit(&triangle), None);

        let mut trapezoid = Path::new();
        trapezoid.move_to(0.0, 0.0);
        trapezoid.line_to(10.0, 0.0);
        trapezoid.line_to(8.0, 8.0);
        trapezoid.line_to(2.0, 8.0);
        assert_eq!(RoundedBox::fit(&trapezoid), None);

        let mut two = Path::new();
        two.rect(0.0, 0.0, 10.0, 10.0);
        two.rect(20.0, 0.0, 10.0, 10.0);
        assert_eq!(RoundedBox::fit(&two), None, "two contours");

        let mut varying = Path::new();
        varying.rounded_rect_varying(0.0, 0.0, 80.0, 40.0, 4.0, 8.0, 12.0, 16.0);
        assert_eq!(RoundedBox::fit(&varying), None, "corners of different radii");

        // A circle of four quadratics is 6 % off: a blob, not a circle.
        let mut blob = Path::new();
        blob.move_to(10.0, 0.0);
        blob.quad_to(10.0, 10.0, 0.0, 10.0);
        blob.quad_to(-10.0, 10.0, -10.0, 0.0);
        blob.quad_to(-10.0, -10.0, 0.0, -10.0);
        blob.quad_to(10.0, -10.0, 10.0, 0.0);
        assert_eq!(RoundedBox::fit(&blob), None);

        // Twice around is empty under even-odd.
        let mut twice = Path::new();
        twice.move_to(10.0, 0.0);
        for _ in 0..2 {
            twice.arc(0.0, 0.0, 10.0, 0.0, std::f32::consts::TAU, crate::Solidity::Hole);
        }
        assert_eq!(RoundedBox::fit(&twice), None);

        // A corner a thousandth of the size off makes it another quadrilateral.
        let mut off = Path::new();
        off.move_to(0.0, 0.0);
        off.line_to(100.0, 0.0);
        off.line_to(100.0, 50.1);
        off.line_to(0.0, 50.0);
        assert_eq!(RoundedBox::fit(&off), None);

        // A polygon is not a circle, however many sides it has.
        let mut fine = Path::new();
        fine.move_to(10.0, 0.0);
        for i in 1..=200 {
            let a = i as f32 / 200.0 * std::f32::consts::TAU;
            fine.line_to(10.0 * a.cos(), 10.0 * a.sin());
        }
        assert_eq!(RoundedBox::fit(&fine), None);

        let mut line = Path::new();
        line.move_to(0.0, 0.0);
        line.line_to(10.0, 0.0);
        assert_eq!(RoundedBox::fit(&line), None);
        assert_eq!(RoundedBox::fit(&Path::new()), None);
    }

    #[test]
    fn coverage_counts_device_pixels_across_each_side() {
        // A 40 x 20 box at (30, 20), rotated a quarter turn and doubled, at
        // a device pixel ratio of two: a pixel is half a unit.
        let mut transform = Transform2D::rotation(std::f32::consts::FRAC_PI_2);
        transform.scale(2.0, 2.0);
        let shape = RoundedBox {
            frame: Transform2D::translation(30.0, 20.0),
            extent: [20.0, 10.0],
            radii: [0.0, 0.0],
        }
        .transformed(&transform);
        let coverage = shape.coverage(0.5).unwrap();
        assert_close(coverage.extent, [80.0, 40.0]);
        let (x, y) = transform.transform_point(30.0, 20.0);
        assert!(
            (coverage.distance([x, y]) + 40.0).abs() < 1e-3,
            "center: the nearer side is 40 px away"
        );
        let (x, y) = transform.transform_point(50.25, 20.0);
        assert!(
            (coverage.distance([x, y]) - 1.0).abs() < 1e-3,
            "a quarter unit out is one pixel"
        );

        // Under a skew a coordinate still counts pixels perpendicular to its side.
        let mut skew = Transform2D::identity();
        skew.skew_x(std::f32::consts::FRAC_PI_4);
        let coverage = shape_at_origin().transformed(&skew).coverage(1.0).unwrap();
        assert!((coverage.distance([10.0 + std::f32::consts::SQRT_2, 0.0]) - 1.0).abs() < 1e-3);

        let flat = shape_at_origin().transformed(&Transform2D::scaling(1.0, 0.0));
        assert_eq!(flat.coverage(1.0), None, "a collapsed frame has no coverage");
    }

    fn shape_at_origin() -> RoundedBox {
        RoundedBox {
            frame: Transform2D::identity(),
            extent: [10.0, 10.0],
            radii: [0.0, 0.0],
        }
    }

    #[test]
    fn corner_distance_is_exact_for_a_circle_and_first_order_for_an_ellipse() {
        let circle = RoundedBox {
            frame: Transform2D::identity(),
            extent: [10.0, 10.0],
            radii: [10.0, 10.0],
        }
        .coverage(1.0)
        .unwrap();
        for (point, distance) in [
            ([13.0, 0.0], 3.0),
            ([6.0, 8.0], 0.0),
            ([3.0, 4.0], -5.0),
            ([9.0, 12.0], 5.0),
        ] {
            assert!((circle.distance(point) - distance).abs() < 1e-3, "{point:?}");
        }
        let ellipse = RoundedBox {
            frame: Transform2D::identity(),
            extent: [30.0, 10.0],
            radii: [30.0, 10.0],
        }
        .coverage(1.0)
        .unwrap();
        // Half a pixel off the curve along its normal at 45 degrees of parameter.
        let (x, y) = (30.0 * 0.5_f32.sqrt(), 10.0 * 0.5_f32.sqrt());
        let normal = [x / 900.0, y / 100.0];
        let length = normal[0].hypot(normal[1]);
        let out = [x + 0.5 * normal[0] / length, y + 0.5 * normal[1] / length];
        assert!((ellipse.distance(out) - 0.5).abs() < 0.02, "{}", ellipse.distance(out));
    }

    #[test]
    fn corner_distance_counts_device_pixels_under_a_skew() {
        // A circle turned and then stretched: its frame's axes are no longer
        // at a right angle, and the corner is an ellipse that lies askew in it.
        let mut transform = Transform2D::rotation(0.5);
        transform.scale(1.6, 0.7);
        let circle = RoundedBox {
            frame: Transform2D::identity(),
            extent: [40.0, 40.0],
            radii: [40.0, 40.0],
        }
        .transformed(&transform);
        let coverage = circle.coverage(1.0).unwrap();
        for step in 0..16 {
            let angle = (step as f32 + 0.5) * std::f32::consts::FRAC_PI_8;
            let (x, y) = transform.transform_point(40.0 * angle.cos(), 40.0 * angle.sin());
            // The tangent is the image of the circle's; the normal points away from the center.
            let (tx, ty) = transform.transform_point(-angle.sin(), angle.cos());
            let length = tx.hypot(ty);
            let mut normal = [ty / length, -tx / length];
            if normal[0] * x + normal[1] * y < 0.0 {
                normal = [-normal[0], -normal[1]];
            }
            for offset in [-0.5, 0.0, 0.5] {
                let at = [x + offset * normal[0], y + offset * normal[1]];
                assert!(
                    (coverage.distance(at) - offset).abs() < 0.03,
                    "{offset} px off the outline at step {step}: {}",
                    coverage.distance(at)
                );
            }
        }
    }

    #[test]
    fn a_corner_under_half_a_pixel_is_square() {
        let rounded = |radius: f32| RoundedBox {
            frame: Transform2D::identity(),
            extent: [20.0, 10.0],
            radii: [radius, radius],
        };
        assert_eq!(rounded(0.4).coverage(1.0).unwrap().radii, [0.0, 0.0]);
        assert_eq!(rounded(0.6).coverage(1.0).unwrap().radii, [0.6, 0.6]);
        // Half a pixel is a quarter of a unit at a device pixel ratio of two.
        assert_eq!(rounded(0.4).coverage(0.5).unwrap().radii, [0.8, 0.8]);
        // The radius on screen decides: a wide corner zoomed far out is as tight.
        let far = rounded(4.0).transformed(&Transform2D::scaling(0.1, 0.1));
        assert_eq!(far.coverage(1.0).unwrap().radii, [0.0, 0.0]);
        // Either radius: a corner tight on one axis alone is squared too.
        let lopsided = RoundedBox {
            frame: Transform2D::identity(),
            extent: [20.0, 10.0],
            radii: [8.0, 0.3],
        };
        assert_eq!(lopsided.coverage(1.0).unwrap().radii, [0.0, 0.0]);
    }

    #[test]
    fn a_box_thinner_than_a_pixel_has_no_coverage() {
        let slab = |half: f32| RoundedBox {
            frame: Transform2D::identity(),
            extent: [20.0, half],
            radii: [0.0, 0.0],
        };
        assert!(slab(0.5).coverage(1.0).is_some(), "one pixel thick");
        assert_eq!(slab(0.4).coverage(1.0), None);
        assert!(
            slab(0.4).coverage(0.5).is_some(),
            "a pixel is half a unit at a ratio of two"
        );
        let far = slab(4.0).transformed(&Transform2D::scaling(0.1, 0.1));
        assert_eq!(far.coverage(1.0), None, "the thickness on screen decides");
    }

    #[test]
    fn uniform_rows_carry_the_corners_centers_and_the_sides_ramps() {
        let rounded = RoundedBox {
            frame: Transform2D::translation(30.0, 40.0),
            extent: [20.0, 10.0],
            radii: [4.0, 3.0],
        };
        assert_eq!(
            rounded.coverage(1.0).unwrap().uniform_rows(),
            [16.0, 7.0, 1.0, 0.0, 0.0, 1.0, -30.0, -40.0, 20.5, 10.5]
        );
        // Square corners have no center a fragment could be past.
        let rows = shape_at_origin().coverage(1.0).unwrap().uniform_rows();
        assert_eq!(rows[..2], [NO_CORNERS; 2]);
        assert_eq!(rows[8..], [10.5, 10.5]);
    }

    #[test]
    fn a_frame_skewed_to_a_sliver_has_no_coverage() {
        let skewed = |degrees: f32| {
            let mut skew = Transform2D::identity();
            skew.skew_x((90.0 - degrees).to_radians());
            shape_at_origin().transformed(&skew).coverage(1.0)
        };
        assert!(skewed(5.0).is_some(), "axes five degrees apart");
        assert_eq!(skewed(0.3), None, "axes a third of a degree apart");
        // The test is of the angle, not the size.
        let tiny = shape_at_origin().transformed(&Transform2D::scaling(1e-4, 1e-4));
        assert!(tiny.coverage(1.0).is_some());
    }

    #[test]
    fn containment_needs_the_margin_on_every_corner() {
        let outer = RoundedBox {
            frame: Transform2D::translation(50.0, 50.0),
            extent: [40.0, 40.0],
            radii: [40.0, 40.0],
        };
        let inner = |half: f32| RoundedBox {
            frame: Transform2D::translation(50.0, 50.0),
            extent: [half, half],
            radii: [0.0, 0.0],
        };
        let coverage = outer.coverage(1.0).unwrap();
        assert!(coverage.contains(&inner(28.0)), "corners 39.6 from the center");
        assert!(
            coverage.contains(&inner(28.5)),
            "corners 0.3 px out: within what a stencil resolves"
        );
        assert!(!coverage.contains(&inner(29.0)), "corners a pixel out");
        assert!(!inner(28.0).coverage(1.0).unwrap().contains(&outer));

        // Round corners are followed, not boxed: a rounded rect lies inside
        // its twin, and inside one shifted by less than half a pixel.
        let rounded = |x: f32| RoundedBox {
            frame: Transform2D::translation(x, 50.0),
            extent: [40.0, 30.0],
            radii: [12.0, 12.0],
        };
        let twin = rounded(50.0).coverage(1.0).unwrap();
        assert!(twin.contains(&rounded(50.0)));
        assert!(twin.contains(&rounded(50.3)));
        assert!(!twin.contains(&rounded(51.0)));
        // At a device pixel ratio of two the same shift is over half a pixel.
        assert!(!rounded(50.0).coverage(0.5).unwrap().contains(&rounded(50.3)));
    }

    /// The box holds a draw whose bounds stay inside it - on its edge
    /// counts, a sixty-fourth of a pixel past it too - and a corner of the
    /// bounds past a round corner is outside.
    #[test]
    fn a_draw_is_held_when_its_bounds_stay_inside() {
        let bounds = |minx: f32, miny: f32, maxx: f32, maxy: f32| Bounds { minx, miny, maxx, maxy };
        let rect = RoundedBox {
            frame: Transform2D::translation(50.0, 40.0),
            extent: [40.0, 30.0],
            radii: [0.0, 0.0],
        };
        let coverage = rect.coverage(1.0).unwrap();
        assert!(coverage.holds(&bounds(20.0, 20.0, 60.0, 60.0)));
        assert!(coverage.holds(&bounds(10.0, 10.0, 90.0, 70.0)), "its own bounds");
        assert!(coverage.holds(&bounds(10.0, 10.0, 90.01, 70.0)), "rounding");
        assert!(
            !coverage.holds(&bounds(10.0, 10.0, 90.1, 70.0)),
            "a tenth of a pixel out"
        );
        // At a device pixel ratio of two the same tenth of a unit is still out.
        assert!(!rect.coverage(0.5).unwrap().holds(&bounds(10.0, 10.0, 90.02, 70.0)));

        let rounded = RoundedBox {
            radii: [12.0, 12.0],
            ..rect
        };
        let coverage = rounded.coverage(1.0).unwrap();
        assert!(!coverage.holds(&bounds(10.0, 10.0, 90.0, 70.0)), "its corners are cut");
        assert!(
            coverage.holds(&bounds(14.0, 14.0, 86.0, 66.0)),
            "inside the corner arcs"
        );
        assert!(coverage.holds(&bounds(10.0, 22.0, 90.0, 58.0)), "between the corners");

        // Under a turn the bounds' corners are what must be inside.
        let turned = rect.transformed(&Transform2D::rotation(0.5));
        let coverage = turned.coverage(1.0).unwrap();
        let (cx, cy) = turned.frame.transform_point(0.0, 0.0);
        assert!(coverage.holds(&bounds(cx - 20.0, cy - 20.0, cx + 20.0, cy + 20.0)));
        assert!(!coverage.holds(&bounds(cx - 38.0, cy - 28.0, cx + 38.0, cy + 28.0)));
    }

    /// An upright rect filled under an upright rect clip is the rect the two
    /// share - the clip's twin, a fill around it, one across its corner -
    /// whatever the scale, and under an upright box with round corners that
    /// it covers, the box a fringe larger. A turned pair, a rounded fill, a
    /// rect that leaves part of a rounded clip out, a fill that is no box
    /// and a sliver under a pixel keep their outline.
    #[test]
    fn an_upright_rect_fill_is_what_it_shares_with_the_clip_or_the_clip_it_covers() {
        let rect = |x: f32, y: f32, w: f32, h: f32| {
            let mut path = Path::new();
            path.rect(x, y, w, h);
            path
        };
        let clip_of = |path: &Path, transform: &Transform2D| RoundedBox::fit(path).unwrap().transformed(transform);
        let bounds = |drawn: RoundedBox| {
            let corners = drawn.corners();
            let pick = |k: usize, f: fn(f32, f32) -> f32, seed: f32| corners.iter().map(|c| c[k]).fold(seed, f);
            [
                pick(0, f32::min, f32::INFINITY),
                pick(1, f32::min, f32::INFINITY),
                pick(0, f32::max, f32::NEG_INFINITY),
                pick(1, f32::max, f32::NEG_INFINITY),
            ]
            .map(|v| (v * 100.0).round() / 100.0)
        };
        let shared = |fill: Option<RectFill>| match fill {
            Some(RectFill::Shared(rect)) => Some(bounds(rect)),
            other => panic!("{other:?}"),
        };
        let covered = |fill: Option<RectFill>| match fill {
            Some(RectFill::Covered(rect)) => Some(bounds(rect)),
            other => panic!("{other:?}"),
        };
        let identity = Transform2D::identity();
        let clip = clip_of(&rect(10.0, 10.0, 80.0, 60.0), &identity);
        let under = |fill: &Path| RoundedBox::upright_rect(fill, &identity).and_then(|fill| clip.rect_fill(&fill, 1.0));
        assert_eq!(
            shared(under(&rect(10.0, 10.0, 80.0, 60.0))),
            Some([10.0, 10.0, 90.0, 70.0]),
            "twin"
        );
        assert_eq!(
            shared(under(&rect(0.0, 0.0, 100.0, 100.0))),
            Some([10.0, 10.0, 90.0, 70.0]),
            "around"
        );
        assert_eq!(
            shared(under(&rect(50.0, 10.0, 80.0, 30.0))),
            Some([50.0, 10.0, 90.0, 40.0]),
            "across"
        );
        assert_eq!(under(&rect(89.5, 0.0, 50.0, 100.0)), None, "half a pixel shared");
        assert_eq!(under(&rect(120.0, 0.0, 50.0, 100.0)), None, "apart");

        // A quarter turn keeps both upright; the scale is the fill's own.
        let quarter = Transform2D::rotation(std::f32::consts::FRAC_PI_2);
        let scaled = Transform2D::new(2.0, 0.0, 0.0, 2.0, 5.0, 5.0);
        assert_eq!(
            shared(
                RoundedBox::upright_rect(&rect(20.0, -60.0, 100.0, 30.0), &quarter)
                    .and_then(|fill| clip.rect_fill(&fill, 1.0))
            ),
            Some([30.0, 20.0, 60.0, 70.0])
        );
        assert_eq!(
            shared(
                RoundedBox::upright_rect(&rect(0.0, 0.0, 20.0, 20.0), &scaled)
                    .and_then(|fill| clip.rect_fill(&fill, 1.0))
            ),
            Some([10.0, 10.0, 45.0, 45.0])
        );

        let mut rounded = Path::new();
        rounded.rounded_rect(10.0, 10.0, 80.0, 60.0, 14.0);
        let twin = rect(10.0, 10.0, 80.0, 60.0);
        let round = clip_of(&rounded, &identity);
        assert_eq!(
            covered(RoundedBox::upright_rect(&twin, &identity).and_then(|fill| round.rect_fill(&fill, 1.0))),
            Some([9.0, 9.0, 91.0, 71.0]),
            "on its sides"
        );
        assert_eq!(
            covered(
                RoundedBox::upright_rect(&rect(0.0, 0.0, 100.0, 100.0), &identity)
                    .and_then(|fill| round.rect_fill(&fill, 1.0))
            ),
            Some([9.0, 9.0, 91.0, 71.0])
        );
        assert_eq!(
            covered(RoundedBox::upright_rect(&twin, &identity).and_then(|fill| round.rect_fill(&fill, 0.5))),
            Some([9.5, 9.5, 90.5, 70.5]),
            "at a device pixel ratio of two the fringe is half a unit"
        );
        assert_eq!(
            RoundedBox::upright_rect(&rect(10.1, 10.0, 80.0, 60.0), &identity)
                .and_then(|fill| round.rect_fill(&fill, 1.0)),
            None,
            "a tenth of a unit short"
        );
        assert_eq!(
            RoundedBox::upright_rect(&rect(10.0, 10.0, 80.0, 30.0), &identity)
                .and_then(|fill| round.rect_fill(&fill, 1.0)),
            None,
            "its top half only"
        );
        assert_eq!(
            RoundedBox::upright_rect(&rounded, &identity).and_then(|fill| round.rect_fill(&fill, 1.0)),
            None,
            "its rounded twin"
        );
        let mut circle = Path::new();
        circle.circle(50.0, 40.0, 30.0);
        assert_eq!(
            covered(
                RoundedBox::upright_rect(&rect(0.0, 0.0, 100.0, 100.0), &identity)
                    .and_then(|fill| clip_of(&circle, &identity).rect_fill(&fill, 1.0))
            ),
            Some([19.0, 9.0, 81.0, 71.0])
        );

        let turned = Transform2D::rotation(0.3);
        assert_eq!(
            RoundedBox::upright_rect(&twin, &turned).and_then(|fill| clip_of(&twin, &turned).rect_fill(&fill, 1.0)),
            None,
            "a turned pair"
        );
        assert_eq!(
            RoundedBox::upright_rect(&twin, &turned).and_then(|fill| clip.rect_fill(&fill, 1.0)),
            None,
            "a turned fill"
        );
        assert_eq!(
            RoundedBox::upright_rect(&rounded, &identity).and_then(|fill| clip.rect_fill(&fill, 1.0)),
            None,
            "a rounded fill"
        );
        let mut triangle = Path::new();
        triangle.move_to(0.0, 0.0);
        triangle.line_to(100.0, 0.0);
        triangle.line_to(0.0, 100.0);
        triangle.close();
        assert_eq!(
            RoundedBox::upright_rect(&triangle, &identity).and_then(|fill| clip.rect_fill(&fill, 1.0)),
            None,
            "no box"
        );
        let mut long = Path::new();
        long.move_to(0.0, 0.0);
        for step in 1..=20 {
            long.line_to(step as f32 * 5.0, 0.0);
        }
        long.line_to(100.0, 100.0);
        long.line_to(0.0, 100.0);
        long.close();
        assert!(
            RoundedBox::fit(&long).is_some(),
            "a rect with its top side in twenty pieces"
        );
        assert_eq!(
            RoundedBox::upright_rect(&long, &identity).and_then(|fill| clip.rect_fill(&fill, 1.0)),
            None,
            "too many verbs to ask on every fill"
        );
    }

    /// A scissor and a clip make one box when they are rects with parallel
    /// sides, or when one all but holds the other; then that box stands for
    /// both. A pixel's width of one past the other's round corner does not.
    #[test]
    fn a_scissor_and_a_clip_make_one_box_when_one_holds_the_other_or_both_are_rects() {
        let boxed = |x: f32, y: f32, w: f32, h: f32, r: f32| RoundedBox {
            frame: Transform2D::translation(x + w * 0.5, y + h * 0.5),
            extent: [w * 0.5, h * 0.5],
            radii: [r, r],
        };
        let one = |clip: RoundedBox, scissor: RoundedBox| {
            clip.with_scissor(&scissor, 1.0).map(|(both, coverage)| {
                assert_eq!(both.coverage(1.0), Some(coverage));
                (both.extent, both.radii)
            })
        };
        let rect = boxed(10.0, 10.0, 80.0, 60.0, 0.0);
        assert_eq!(one(rect, rect), Some(([40.0, 30.0], [0.0, 0.0])), "twins");
        assert_eq!(
            one(rect, boxed(0.0, 0.0, 50.0, 100.0, 0.0)),
            Some(([20.0, 30.0], [0.0, 0.0]))
        );
        let rounded = boxed(10.0, 10.0, 80.0, 60.0, 12.0);
        assert_eq!(
            one(rounded, rounded),
            Some(([40.0, 30.0], [12.0, 12.0])),
            "rounded twins"
        );
        assert_eq!(
            one(rounded, boxed(0.0, 0.0, 100.0, 100.0, 0.0)),
            Some(([40.0, 30.0], [12.0, 12.0]))
        );
        assert_eq!(
            one(rounded, rect),
            Some(([40.0, 30.0], [12.0, 12.0])),
            "a scissor on the clip's sides"
        );
        assert_eq!(
            one(rect, rounded),
            Some(([40.0, 30.0], [12.0, 12.0])),
            "a rounded scissor on a rect's sides"
        );
        assert_eq!(
            one(rounded, boxed(11.0, 10.0, 80.0, 60.0, 0.0)),
            None,
            "a side across the corners"
        );
        assert_eq!(one(rect, rect.transformed(&Transform2D::rotation(0.2))), None, "turned");
        assert_eq!(one(rect, boxed(100.0, 0.0, 20.0, 20.0, 0.0)), None, "apart");
        assert_eq!(
            one(rect, boxed(10.0, 10.0, 80.0, 0.5, 0.0)),
            None,
            "thinner than a pixel together"
        );
    }

    #[test]
    fn parallelograms_with_parallel_sides_intersect_as_one() {
        let rect = |x: f32, y: f32, w: f32, h: f32| {
            let mut path = Path::new();
            path.rect(x, y, w, h);
            RoundedBox::fit(&path).unwrap()
        };
        let corners = |a: RoundedBox, b: RoundedBox| {
            let mut corners = a.intersection(&b, 1.0).unwrap().corners().to_vec();
            corners.sort_by(|a, b| a.partial_cmp(b).unwrap());
            corners
        };
        let expected = [[40.0, 30.0], [40.0, 50.0], [60.0, 30.0], [60.0, 50.0]];
        // The same region whichever box is asked and however each was wound.
        let (a, b) = (rect(10.0, 10.0, 50.0, 40.0), rect(40.0, 30.0, 50.0, 40.0));
        let mut turned = Path::new();
        turned.move_to(90.0, 30.0);
        turned.line_to(90.0, 70.0);
        turned.line_to(40.0, 70.0);
        turned.line_to(40.0, 30.0);
        let turned = RoundedBox::fit(&turned).unwrap();
        for (first, second) in [(a, b), (b, a), (a, turned), (turned, a)] {
            for (corner, expected) in corners(first, second).iter().zip(expected) {
                assert_close(*corner, expected);
            }
        }
        // Under one transform the sides stay parallel.
        let mut spin = Transform2D::rotation(0.7);
        spin.scale(2.0, 0.5);
        let both = a.transformed(&spin).intersection(&b.transformed(&spin), 1.0).unwrap();
        let mut spun = both.corners().to_vec();
        spun.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut expected_spun = expected.map(|[x, y]| {
            let (x, y) = spin.transform_point(x, y);
            [x, y]
        });
        expected_spun.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for (corner, expected) in spun.iter().zip(expected_spun) {
            assert_close(*corner, expected);
        }

        assert_eq!(a.intersection(&rect(70.0, 10.0, 10.0, 10.0), 1.0), None, "apart");
        assert_eq!(
            a.intersection(&b.transformed(&Transform2D::rotation(0.3)), 1.0),
            None,
            "not parallel"
        );
        let mut circle = Path::new();
        circle.circle(55.0, 45.0, 20.0);
        assert_eq!(
            a.intersection(&RoundedBox::fit(&circle).unwrap(), 1.0),
            None,
            "a circle across a corner: what they share is no box"
        );
    }

    #[test]
    fn boxes_with_round_corners_intersect_as_one_when_one_all_but_holds_the_other() {
        let rounded = |x0: f32, y0: f32, x1: f32, y1: f32, radius: f32| RoundedBox {
            frame: Transform2D::translation((x0 + x1) * 0.5, (y0 + y1) * 0.5),
            extent: [(x1 - x0) * 0.5, (y1 - y0) * 0.5],
            radii: [radius, radius],
        };
        let sides = |shape: RoundedBox| {
            let (x, y) = shape.frame.transform_point(0.0, 0.0);
            [
                x - shape.extent[0],
                y - shape.extent[1],
                x + shape.extent[0],
                y + shape.extent[1],
            ]
        };
        let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-3);

        // An icon's frame and a second one a fraction of a pixel smaller and
        // a quarter pixel further right: each side of what they share is
        // the nearer one, which is neither frame.
        let outer = rounded(131.94, 33.23, 325.49, 226.78, 41.47);
        let inner = rounded(132.72, 33.23, 325.72, 226.22, 41.36);
        for (first, second) in [(outer, inner), (inner, outer)] {
            let both = first.intersection(&second, 1.0).unwrap();
            assert!(near(sides(both), [132.72, 33.23, 325.49, 226.22]), "{:?}", sides(both));
            assert!((41.3..41.5).contains(&both.radii[0]));
        }

        // One inside the other is the inner one, whichever is asked.
        let small = rounded(150.0, 60.0, 300.0, 200.0, 12.0);
        assert_eq!(outer.intersection(&small, 1.0), Some(small));
        assert_eq!(small.intersection(&outer, 1.0), Some(small));

        // A band across the straight part of a rounded box is a rect; one
        // that cuts through its corners shares no box with it.
        let band = rounded(100.0, 100.0, 400.0, 180.0, 0.0);
        let cut = outer.intersection(&band, 1.0).unwrap();
        assert!(near(sides(cut), [131.94, 100.0, 325.49, 180.0]) && cut.radii == [0.0, 0.0]);
        let across = rounded(100.0, 50.0, 400.0, 180.0, 0.0);
        assert_eq!(outer.intersection(&across, 1.0), None);
        // Nor do two frames a pixel apart at a corner.
        let shifted = rounded(132.94, 34.23, 326.49, 227.78, 41.47);
        assert_eq!(outer.intersection(&shifted, 1.0), None);
    }
}
