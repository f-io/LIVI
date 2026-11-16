use std::{f32::consts::PI, ops::Range};

use bitflags::bitflags;

use crate::{
    geometry::{Bounds, Position, Transform2D, Vector},
    renderer::Vertex,
    FillRule, LineCap, LineJoin, Solidity,
};

use super::Verb;

bitflags! {
    #[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct PointFlags: u8 {
        const CORNER        = 0x01;
        const LEFT          = 0x02;
        const BEVEL         = 0x04;
        const INNERBEVEL    = 0x08;
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct Point {
    pos: Position,
    dpos: Vector,
    len: f32,
    dmpos: Vector,
    flags: PointFlags,
}

impl Point {
    pub fn new(x: f32, y: f32, flags: PointFlags) -> Self {
        Self {
            pos: Position { x, y },
            flags,
            ..Default::default()
        }
    }

    pub fn is_left(p0: &Self, p1: &Self, x: f32, y: f32) -> f32 {
        let pos = Position { x, y };
        (p1.pos - p0.pos).dot((pos - p0.pos).orthogonal())
    }

    pub fn approx_eq(&self, other: &Self, tolerance: f32) -> bool {
        (other.pos - self.pos).mag2() < tolerance * tolerance
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum Convexity {
    Concave,
    Convex,
    #[default]
    Unknown,
}

/// How often a contour's edges turn back along one axis, the turn from its
/// last edge to its first included: a convex contour turns back twice along
/// each.
#[derive(Default)]
struct AxisTurns {
    sign: i8,
    first: i8,
    turns: u32,
}

impl AxisTurns {
    fn add(&mut self, d: f32) {
        let sign = if d > 0.0 {
            1
        } else if d < 0.0 {
            -1
        } else {
            return;
        };
        if self.sign == 0 {
            self.first = sign;
        } else if self.sign != sign {
            self.turns += 1;
        }
        self.sign = sign;
    }

    fn total(&self) -> u32 {
        self.turns + u32::from(self.sign != self.first)
    }
}

/// An outline's mean width under a fill rule (see [`PathCache::mean_width`])
/// and whether each of its contours is convex.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OutlineWidth {
    pub(crate) mean: f32,
    pub(crate) convex: bool,
}

#[derive(Clone, Debug)]
pub struct Contour {
    point_range: Range<usize>,
    closed: bool,
    bevel: usize,
    solidity: Option<Solidity>,
    /// Set for the duration of a fill: this contour's points were flipped to
    /// the orientation the fringe extrusion assumes, so the triangles built
    /// from them have to be flipped back.
    reversed: bool,
    /// Set for the duration of a fill: the points lie on one line, so the
    /// contour encloses nothing and a fill of it draws nothing.
    degenerate: bool,
    pub(crate) fill: Vec<Vertex>,
    pub(crate) stroke: Vec<Vertex>,
    pub(crate) convexity: Convexity,
}

impl Default for Contour {
    fn default() -> Self {
        Self {
            point_range: 0..0,
            closed: Default::default(),
            bevel: Default::default(),
            solidity: None,
            reversed: false,
            degenerate: false,
            fill: Vec::new(),
            stroke: Vec::new(),
            convexity: Convexity::default(),
        }
    }
}

impl Contour {
    fn point_pairs<'a>(&self, points: &'a [Point]) -> impl Iterator<Item = (&'a Point, &'a Point)> {
        PointPairsIter {
            curr: 0,
            points: &points[self.point_range.clone()],
        }
    }

    fn polygon_area(points: &[Point]) -> f32 {
        let mut area = 0.0;

        for (p0, p1) in (PointPairsIter { curr: 0, points }) {
            area += (p1.pos.x - p0.pos.x) * (p1.pos.y + p0.pos.y);
        }

        area * 0.5
    }

    /// Whether the points all lie on one line: fewer than three of them, or
    /// none farther than 1/256 px from the line through the first point and
    /// the point farthest from it. Such a contour encloses nothing, and
    /// browsers draw nothing for its fill - a bare `<line>` or an open path
    /// under SVG's default black fill (femtovg/femtovg#341). The band is a
    /// fixed device distance, so a thin sliver stays a sliver at any zoom,
    /// and a contour with a long retraced tail keeps whatever area it does
    /// enclose.
    fn collinear(points: &[Point]) -> bool {
        if points.len() < 3 {
            return true;
        }
        let origin = points[0].pos;
        let far = points
            .iter()
            .map(|p| p.pos - origin)
            .max_by(|a, b| a.mag2().total_cmp(&b.mag2()))
            .unwrap();
        let length = far.mag2().sqrt();
        if length <= 0.0 {
            return true;
        }
        let axis = far * (1.0 / length);
        points.iter().all(|p| (p.pos - origin).cross(axis).abs() <= 1.0 / 256.0)
    }

    fn point_count(&self) -> usize {
        self.point_range.end - self.point_range.start
    }

    /// Reverses a contour whose points know the edge that leaves each one:
    /// reversed, a point leaves along the edge that arrived at it, negated.
    /// That equals working the directions out again, but a zero component
    /// can come back as -0.0; reversing twice restores every bit.
    fn reverse(points: &mut [Point]) {
        points.reverse();
        let Some(first) = points.first().map(|p| (p.dpos, p.len)) else {
            return;
        };
        for i in 1..points.len() {
            let (dpos, len) = (points[i].dpos, points[i].len);
            let p = &mut points[i - 1];
            p.dpos = -dpos;
            p.len = len;
        }
        if let Some(last) = points.last_mut() {
            last.dpos = -first.0;
            last.len = first.1;
        }
    }
}

struct PointPairsIter<'a> {
    curr: usize,
    points: &'a [Point],
}

impl<'a> Iterator for PointPairsIter<'a> {
    type Item = (&'a Point, &'a Point);

    fn next(&mut self) -> Option<Self::Item> {
        let curr = self.points.get(self.curr);

        let prev = if self.curr == 0 {
            self.points.last()
        } else {
            self.points.get(self.curr - 1)
        };

        self.curr += 1;

        curr.and_then(|some_curr| prev.map(|some_prev| (some_prev, some_curr)))
    }
}

/// How far a chord is moved toward the side its control points lie on, as a
/// share of their mean distance from it: 3/8 of that is half the curve's
/// distance from the chord at its middle, half the sagitta on a circular arc.
const CHORD_SHIFT: f32 = 0.375;

#[derive(Clone, Debug, Default)]
pub struct PathCache {
    pub(crate) contours: Vec<Contour>,
    pub(crate) bounds: Bounds,
    points: Vec<Point>,
    // Per contour, the other contours' winding number and crossing count at
    // a point just inside it, and its own orientation sign - what decides
    // whether it bounds a hole under either fill rule. Computed once per
    // cache (per transform), not per fill.
    contour_sides: Option<Vec<(i32, u32, i32)>>,
    // The square of the farthest a point between two chords was moved off the curve.
    straddle: f32,
    // The outline's mean width under a fill rule, once a fill has asked for it.
    mean_width: Option<(FillRule, OutlineWidth)>,
}

impl PathCache {
    /// Whether the fill is one convex contour, which needs no stencil.
    pub(crate) fn is_convex(&self) -> bool {
        self.contours.len() == 1 && self.contours[0].convexity == Convexity::Convex
    }

    /// The bounds of the vertices the last expansion left, its fringe
    /// included: past the path's own bounds by up to the miter limit times
    /// half the fringe at a sharp corner.
    pub(crate) fn expanded_bounds(&self) -> Bounds {
        Bounds::from_points(
            self.contours
                .iter()
                .flat_map(|contour| contour.fill.iter().chain(&contour.stroke))
                .map(|vertex| (vertex.x, vertex.y)),
        )
    }

    pub fn new(verbs: impl Iterator<Item = Verb>, transform: &Transform2D, tess_tol: f32, dist_tol: f32) -> Self {
        let mut cache = Self::default();

        // Convert path verbs to a set of contours
        for verb in verbs {
            match verb {
                Verb::MoveTo(x, y) => {
                    cache.add_contour();
                    let (x, y) = transform.transform_point(x, y);
                    cache.add_point(x, y, PointFlags::CORNER, dist_tol);
                }
                Verb::LineTo(x, y) => {
                    let (x, y) = transform.transform_point(x, y);
                    cache.add_point(x, y, PointFlags::CORNER, dist_tol);
                }
                Verb::BezierTo(c1x, c1y, c2x, c2y, x, y) => {
                    if let Some(last) = cache.points.last().copied() {
                        let (c1x, c1y) = transform.transform_point(c1x, c1y);
                        let (c2x, c2y) = transform.transform_point(c2x, c2y);
                        let (x, y) = transform.transform_point(x, y);

                        // A cubic whose four points coincide is the point it starts from. Its
                        // chord is zero, so it would fail the flatness test to the depth limit.
                        let start = (last.pos.x, last.pos.y);
                        if [(x, y), (c1x, c1y), (c2x, c2y)] == [start; 3] {
                            continue;
                        }

                        cache.tesselate_bezier(
                            [last.pos.x, last.pos.y, c1x, c1y, c2x, c2y, x, y],
                            PointFlags::CORNER,
                            tess_tol,
                            dist_tol,
                        );

                        // cache.tesselate_bezier_afd(
                        //     last.pos.x,
                        //     last.pos.y,
                        //     c1x,
                        //     c1y,
                        //     c2x,
                        //     c2y,
                        //     x,
                        //     y,
                        //     PointFlags::CORNER,
                        //     tess_tol,
                        //     dist_tol,
                        // );
                    }
                }
                Verb::Close => {
                    if let Some(contour) = cache.contours.last_mut() {
                        contour.closed = true;
                    }
                }
                Verb::Solid => {
                    if let Some(contour) = cache.contours.last_mut() {
                        contour.solidity = Some(Solidity::Solid);
                    }
                }
                Verb::Hole => {
                    if let Some(contour) = cache.contours.last_mut() {
                        contour.solidity = Some(Solidity::Hole);
                    }
                }
            }
        }

        let all_points = &mut cache.points;
        let bounds = &mut cache.bounds;

        cache.contours.retain_mut(|contour| {
            let mut points = &mut all_points[contour.point_range.clone()];

            // If the first and last points are the same, remove the last, mark as closed contour.
            if let (Some(p0), Some(p1)) = (points.last(), points.first()) {
                if p0.approx_eq(p1, dist_tol) {
                    contour.point_range.end -= 1;
                    contour.closed = true;
                    points = &mut all_points[contour.point_range.clone()];
                }
            }

            if points.len() < 2 {
                return false;
            }

            // Enforce solidity by reversing the winding.
            if let Some(solidity) = contour.solidity {
                let area = Contour::polygon_area(points);

                if solidity == Solidity::Solid && area < 0.0 {
                    points.reverse();
                }

                if solidity == Solidity::Hole && area > 0.0 {
                    points.reverse();
                }
            }

            // Each point's direction and length to the next, the last's to the
            // first, and the bounds, kept in registers until the contour ends.
            let first = points[0].pos;
            let (mut minx, mut miny, mut maxx, mut maxy) = (bounds.minx, bounds.miny, bounds.maxx, bounds.maxy);
            let count = points.len();
            for i in 0..count {
                let next = if i + 1 < count { points[i + 1].pos } else { first };
                let p = &mut points[i];
                p.dpos = next - p.pos;
                p.len = p.dpos.normalize();
                minx = minx.min(p.pos.x);
                miny = miny.min(p.pos.y);
                maxx = maxx.max(p.pos.x);
                maxy = maxy.max(p.pos.y);
            }
            *bounds = Bounds { minx, miny, maxx, maxy };

            true
        });

        // Classified once: a contour whose points lie on one line encloses
        // nothing, so it neither draws nor counts in the others' classification.
        for contour in &mut cache.contours {
            contour.degenerate = Contour::collinear(&cache.points[contour.point_range.clone()]);
        }

        cache
    }

    /// Emits independent triangle-list fans whose signed coverage can be
    /// accumulated in the stencil buffer to recover the path's winding. The
    /// contours share one vertex list without introducing triangles between
    /// them.
    pub(crate) fn winding_triangles(&self) -> Vec<Vertex> {
        let triangles = self
            .contours
            .iter()
            .map(|contour| contour.point_count().saturating_sub(2))
            .sum::<usize>();
        let mut vertices = Vec::with_capacity(triangles * 3);

        for contour in &self.contours {
            let points = &self.points[contour.point_range.clone()];
            if let Some((center, tail)) = points.split_first() {
                let center = Vertex::pos(center.pos, 0.5, 1.0);
                for edge in tail.windows(2) {
                    vertices.extend_from_slice(&[
                        center,
                        Vertex::pos(edge[0].pos, 0.5, 1.0),
                        Vertex::pos(edge[1].pos, 0.5, 1.0),
                    ]);
                }
            }
        }

        vertices
    }

    /// Each contour's points in order, closed from the last point to the
    /// first: the outline a mask of a fill under `fill_rule` is rasterized
    /// from. A mask adds up the winding over each pixel before the rule
    /// folds it, exact where the pixel sees at most two windings, one apart:
    /// a ring thinner than a pixel, both circles wound alike, sees 0, 1 and
    /// 2, and its hole counted as ink. Even-odd does not ask which way a
    /// contour is wound, so a hole's contour is wound against those around
    /// it, and nested contours wind 0 or 1. A degenerate contour is left
    /// out, as [`Self::expand_fill`] leaves it: the mask is of what the fill
    /// would draw.
    pub(crate) fn outlines(&mut self, fill_rule: FillRule) -> impl Iterator<Item = Vec<[f32; 2]>> + '_ {
        let reverse: Vec<bool> = if fill_rule == FillRule::EvenOdd && self.contours.len() > 1 {
            let hole = self.hole_contours(fill_rule);
            // As `expand_fill` winds them: what is filled as `polygon_area`
            // calls positive (own -1), a hole the other way.
            self.contour_sides()
                .iter()
                .zip(hole)
                .map(|(&(_, _, own), hole)| own == if hole { -1 } else { 1 })
                .collect()
        } else {
            Vec::new()
        };
        let cache = &*self;
        let drawn = cache
            .contours
            .iter()
            .enumerate()
            .filter(|(_, contour)| !contour.degenerate);
        drawn.map(move |(i, contour)| {
            let points = cache.points[contour.point_range.clone()].iter();
            let outline = points.map(|point| [point.pos.x, point.pos.y]);
            if reverse.get(i) == Some(&true) {
                outline.rev().collect()
            } else {
                outline.collect()
            }
        })
    }

    /// Twice the area a fill of the outline under `fill_rule` covers, over
    /// the outline's perimeter, in device pixels: a sliver's width, a disc's
    /// radius. Each contour adds the area inside it, or takes it off if it
    /// bounds a hole, however it is wound. A degenerate contour, of which the
    /// fill draws nothing, adds no perimeter either, and a path of them alone
    /// has no width to measure: it fills nothing, and needs no mask. Each
    /// contour is convex when it turns the same way at every point and back
    /// twice along each axis, as [`Self::calculate_joins`] finds once the
    /// contours are oriented. Worked out once a cache, which a path keeps
    /// while its transform does.
    pub(crate) fn mean_width(&mut self, fill_rule: FillRule) -> OutlineWidth {
        if let Some((rule, width)) = self.mean_width {
            if rule == fill_rule {
                return width;
            }
        }
        // One contour bounds no hole: no need to ask which do.
        let hole = if self.contours.len() > 1 {
            self.hole_contours(fill_rule)
        } else {
            Vec::new()
        };
        let (mut area, mut perimeter, mut convex) = (0.0, 0.0, true);
        for (i, contour) in self.contours.iter().enumerate() {
            let points = &self.points[contour.point_range.clone()];
            let (Some(first), Some(last)) = (points.first(), points.last()) else {
                continue;
            };
            if contour.degenerate {
                continue;
            }
            // From the contour's first point, so that the products stay small.
            let origin = first.pos;
            let mut from = last.pos - origin;
            let mut twice = 0.0f32;
            let (mut leaving, mut left, mut right) = (last.dpos, 0, 0);
            let (mut x, mut y) = (AxisTurns::default(), AxisTurns::default());
            for point in points {
                let to = point.pos - origin;
                twice += from.x * to.y - to.x * from.y;
                // Each point knows the length of the edge that leaves it.
                perimeter += point.len;
                from = to;
                let cross = leaving.cross(point.dpos);
                left += usize::from(cross > 0.0);
                right += usize::from(cross < 0.0);
                x.add(point.dpos.x);
                y.add(point.dpos.y);
                leaving = point.dpos;
            }
            convex &= (left == points.len() || right == points.len()) && x.total() == 2 && y.total() == 2;
            area += if hole.get(i) == Some(&true) {
                -twice.abs()
            } else {
                twice.abs()
            };
        }
        let mean = if perimeter > 0.0 {
            area.abs() / perimeter
        } else {
            f32::INFINITY
        };
        let width = OutlineWidth { mean, convex };
        self.mean_width = Some((fill_rule, width));
        width
    }

    /// The farthest a point between two chords stands off its curve (see
    /// [`Self::tesselate_bezier`]), in device pixels.
    pub(crate) fn straddle(&self) -> f32 {
        self.straddle.sqrt()
    }

    /// The points of every contour: the flattened outline.
    pub(crate) fn positions(&self) -> impl Iterator<Item = [f32; 2]> + '_ {
        self.points.iter().map(|point| [point.pos.x, point.pos.y])
    }

    /// The points of every contour, each with whether it lies between two
    /// chords, off its curve, rather than where the path put it.
    pub(crate) fn positions_between_chords(&self) -> impl Iterator<Item = ([f32; 2], bool)> + '_ {
        let between_chords = |point: &Point| !point.flags.contains(PointFlags::CORNER);
        self.points
            .iter()
            .map(move |point| ([point.pos.x, point.pos.y], between_chords(point)))
    }

    fn add_contour(&mut self) {
        let mut contour = Contour::default();

        contour.point_range.start = self.points.len();
        contour.point_range.end = self.points.len();

        self.contours.push(contour);
    }

    fn add_point(&mut self, x: f32, y: f32, flags: PointFlags, dist_tol: f32) {
        if let Some(contour) = self.contours.last() {
            // If the contour's last point equals this new point just OR the flags and ignore the new point
            if let Some(last_point) = self.points[contour.point_range.clone()].last_mut() {
                if last_point.approx_eq(&Point::new(x, y, flags), dist_tol) {
                    last_point.flags |= flags;
                    return;
                }
            }
        }

        self.push_point(x, y, flags);
    }

    /// `add_point` for a point known to lie beyond `dist_tol` of the contour's last one.
    fn push_point(&mut self, x: f32, y: f32, flags: PointFlags) {
        if let Some(contour) = self.contours.last_mut() {
            self.points.push(Point::new(x, y, flags));
            contour.point_range.end += 1;
        }
    }

    /// Flattens a cubic into chords: a piece whose control points lie close
    /// enough to the line between its ends is taken as that line, and any
    /// other is halved, to ten halvings, the left half first. A chord lies
    /// on the inner side of its piece's bend, off the curve by up to its
    /// sagitta, so each point between two chords is moved outward, toward
    /// their control points, by the mean of their [`CHORD_SHIFT`]s: the
    /// chords then cross the curve and stray about half the sagitta either
    /// side. The cubic's end stays on it and takes `flags`.
    fn tesselate_bezier(&mut self, curve: [f32; 8], flags: PointFlags, tess_tol: f32, dist_tol: f32) {
        // The right halves still to flatten, the nearest last, each with its
        // depth: one at most per depth.
        let mut pending = [([0.0; 8], 0u8); 10];
        let mut count = 0;
        let (mut piece, mut depth) = (curve, 0u8);
        // The end of the last chord, held until the next chord's shift is
        // known, and its own shift.
        let mut held: Option<([f32; 2], [f32; 2])> = None;
        // The contour's last point: the cubic's start, then each point taken.
        let mut last = [curve[0], curve[1]];

        loop {
            #[cfg(test)]
            tests::BEZIER_PIECES.with(|pieces| pieces.set(pieces.get() + 1));

            let [x1, y1, x2, y2, x3, y3, x4, y4] = piece;
            let dx = x4 - x1;
            let dy = y4 - y1;
            // The control points' distances to the right of the chord, times its length.
            let d2 = (x2 - x4) * dy - (y2 - y4) * dx;
            let d3 = (x3 - x4) * dy - (y3 - y4) * dx;
            let chord = dx * dx + dy * dy;

            if (d2.abs() + d3.abs()) * (d2.abs() + d3.abs()) < tess_tol * chord {
                let bend = if chord > 0.0 {
                    -CHORD_SHIFT * 0.5 * (d2 + d3) / chord
                } else {
                    0.0
                };
                let shift = [-dy * bend, dx * bend];
                if let Some((at, previous)) = held.replace(([x4, y4], shift)) {
                    let mean = [(previous[0] + shift[0]) * 0.5, (previous[1] + shift[1]) * 0.5];
                    self.straddle = self.straddle.max(mean[0] * mean[0] + mean[1] * mean[1]);
                    // A point within dist_tol of the last is dropped, as `add_point` would; it has no flags to merge.
                    let point = [at[0] + mean[0], at[1] + mean[1]];
                    let (ex, ey) = (point[0] - last[0], point[1] - last[1]);
                    if ex * ex + ey * ey >= dist_tol * dist_tol {
                        self.push_point(point[0], point[1], PointFlags::empty());
                        last = point;
                    }
                }
            } else if depth < 10 {
                let x12 = (x1 + x2) * 0.5;
                let y12 = (y1 + y2) * 0.5;
                let x23 = (x2 + x3) * 0.5;
                let y23 = (y2 + y3) * 0.5;
                let x34 = (x3 + x4) * 0.5;
                let y34 = (y3 + y4) * 0.5;
                let x123 = (x12 + x23) * 0.5;
                let y123 = (y12 + y23) * 0.5;
                let x234 = (x23 + x34) * 0.5;
                let y234 = (y23 + y34) * 0.5;
                let x1234 = (x123 + x234) * 0.5;
                let y1234 = (y123 + y234) * 0.5;

                depth += 1;
                pending[count] = ([x1234, y1234, x234, y234, x34, y34, x4, y4], depth);
                count += 1;
                piece = [x1, y1, x12, y12, x123, y123, x1234, y1234];
                continue;
            }

            // A piece is taken or, past ten halvings, dropped: on to the
            // nearest right half left.
            let Some(next) = count.checked_sub(1) else {
                break;
            };
            count = next;
            (piece, depth) = pending[count];
        }

        if let Some((at, _)) = held {
            self.add_point(at[0], at[1], flags, dist_tol);
        }
    }

    // fn tesselate_bezier_afd(
    //     &mut self,
    //     x1: f32,
    //     y1: f32,
    //     x2: f32,
    //     y2: f32,
    //     x3: f32,
    //     y3: f32,
    //     x4: f32,
    //     y4: f32,
    //     flags: PointFlags,
    //     tess_tol: f32,
    //     dist_tol: f32,
    // ) {
    //     let ax = -x1 + 3.*x2 - 3.*x3 + x4;
    //     let ay = -y1 + 3.*y2 - 3.*y3 + y4;
    //     let bx = 3.*x1 - 6.*x2 + 3.*x3;
    //     let by = 3.*y1 - 6.*y2 + 3.*y3;
    //     let cx = -3.*x1 + 3.*x2;
    //     let cy = -3.*y1 + 3.*y2;

    //     // Transform to forward difference basis (stepsize 1)
    //     let mut px = x1;
    //     let mut py = y1;
    //     let mut dx = ax + bx + cx;
    //     let mut dy = ay + by + cy;
    //     let mut ddx = 6.*ax + 2.*bx;
    //     let mut ddy = 6.*ay + 2.*by;
    //     let mut dddx = 6.*ax;
    //     let mut dddy = 6.*ay;

    //     //printf("dx: %f, dy: %f\n", dx, dy);
    //     //printf("ddx: %f, ddy: %f\n", ddx, ddy);
    //     //printf("dddx: %f, dddy: %f\n", dddx, dddy);

    //     const AFD_ONE: i32 = 1<<10;

    //     let mut t = 0;
    //     let mut dt = AFD_ONE;

    //     let tol = tess_tol * 4.0;

    //     while t < AFD_ONE {

    //         // Flatness measure.
    //         let mut d = ddx*ddx + ddy*ddy + dddx*dddx + dddy*dddy;

    //         // Go to higher resolution if we're moving a lot
    //         // or overshooting the end.
    //         while (d > tol && dt > 1) || (t+dt > AFD_ONE) {

    //             // Apply L to the curve. Increase curve resolution.
    //             dx = 0.5 * dx - (1.0/8.0)*ddx + (1.0/16.0)*dddx;
    //             dy = 0.5 * dy - (1.0/8.0)*ddy + (1.0/16.0)*dddy;
    //             ddx = (1.0/4.0) * ddx - (1.0/8.0) * dddx;
    //             ddy = (1.0/4.0) * ddy - (1.0/8.0) * dddy;
    //             dddx = (1.0/8.0) * dddx;
    //             dddy = (1.0/8.0) * dddy;

    //             // Half the stepsize.
    //             dt >>= 1;

    //             // Recompute d
    //             d = ddx*ddx + ddy*ddy + dddx*dddx + dddy*dddy;

    //         }

    //         // Go to lower resolution if we're really flat
    //         // and we aren't going to overshoot the end.
    //         // XXX: tol/32 is just a guess for when we are too flat.
    //         while (d > 0.0 && d < tol/32.0 && dt < AFD_ONE) && (t+2*dt <= AFD_ONE) {

    //             // printf("down\n");

    //             // Apply L^(-1) to the curve. Decrease curve resolution.
    //             dx = 2. * dx + ddx;
    //             dy = 2. * dy + ddy;
    //             ddx = 4. * ddx + 4. * dddx;
    //             ddy = 4. * ddy + 4. * dddy;
    //             dddx = 8. * dddx;
    //             dddy = 8. * dddy;

    //             // Double the stepsize.
    //             dt <<= 1;

    //             // Recompute d
    //             d = ddx*ddx + ddy*ddy + dddx*dddx + dddy*dddy;

    //         }

    //         // Forward differencing.
    //         px += dx;
    //         py += dy;
    //         dx += ddx;
    //         dy += ddy;
    //         ddx += dddx;
    //         ddy += dddy;

    //         // Output a point.
    //         self.add_point(px, py, flags, dist_tol);

    //         // Advance along the curve.
    //         t += dt;

    //         // Ensure we don't overshoot.
    //         debug_assert!(t <= AFD_ONE);

    //     }
    // }

    pub fn contains_point(&self, x: f32, y: f32, fill_rule: FillRule) -> bool {
        // Early out if point is outside the bounding rectangle
        if !self.bounds.contains(x, y) {
            return false;
        }

        if fill_rule == FillRule::EvenOdd {
            for contour in &self.contours {
                let mut crossing = false;

                for (p0, p1) in contour.point_pairs(&self.points) {
                    if (p1.pos.y > y) != (p0.pos.y > y)
                        && (x < (p0.pos.x - p1.pos.x) * (y - p1.pos.y) / (p0.pos.y - p1.pos.y) + p1.pos.x)
                    {
                        crossing = !crossing;
                    }
                }

                if crossing {
                    return true;
                }
            }
        } else {
            // NonZero
            for contour in &self.contours {
                let mut winding_number: i32 = 0;

                for (p0, p1) in contour.point_pairs(&self.points) {
                    if p0.pos.y <= y {
                        if p1.pos.y > y && Point::is_left(p0, p1, x, y) > 0.0 {
                            winding_number = winding_number.wrapping_add(1);
                        }
                    } else if p1.pos.y <= y && Point::is_left(p0, p1, x, y) < 0.0 {
                        winding_number = winding_number.wrapping_sub(1);
                    }
                }

                if winding_number != 0 {
                    return true;
                }
            }
        }

        false
    }

    /// The other contours' winding number and crossing count at the midpoint
    /// of each contour's first edge, plus the contour's own orientation sign,
    /// computed once per cache. A contour whose box misses the sample point
    /// cannot wind around it (its crossings of the ray cancel), so only
    /// overlapping boxes are walked: near-linear for the usual
    /// many-disjoint-contours path. A degenerate contour draws nothing, so
    /// it gets an empty box and counts nowhere: a sliver within the band
    /// could otherwise wind once around a sample point on its line and turn
    /// the contour that owns the point inside out.
    fn contour_sides(&mut self) -> &[(i32, u32, i32)] {
        if self.contour_sides.is_none() {
            let n = self.contours.len();
            let boxes: Vec<Bounds> = self
                .contours
                .iter()
                .map(|contour| {
                    let mut b = Bounds::default();
                    if contour.degenerate {
                        return b;
                    }
                    for point in &self.points[contour.point_range.clone()] {
                        b.minx = b.minx.min(point.pos.x);
                        b.miny = b.miny.min(point.pos.y);
                        b.maxx = b.maxx.max(point.pos.x);
                        b.maxy = b.maxy.max(point.pos.y);
                    }
                    b
                })
                .collect();
            let sides = (0..n)
                .map(|i| {
                    let points = &self.points[self.contours[i].point_range.clone()];
                    if n < 2 || points.len() < 3 {
                        return (0, 0, 0);
                    }
                    let (a, b) = (points[0].pos, points[1].pos);
                    let (x, y) = ((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
                    // `is_left` winds the orientation `polygon_area` calls positive as -1.
                    let own = if Contour::polygon_area(points) > 0.0 { -1 } else { 1 };
                    let (mut winding, mut crossings) = (0i32, 0u32);
                    for (j, contour) in self.contours.iter().enumerate() {
                        if j == i || !boxes[j].contains(x, y) {
                            continue;
                        }
                        for (p0, p1) in contour.point_pairs(&self.points) {
                            if p0.pos.y <= y {
                                if p1.pos.y > y && Point::is_left(p0, p1, x, y) > 0.0 {
                                    winding += 1;
                                    crossings += 1;
                                }
                            } else if p1.pos.y <= y && Point::is_left(p0, p1, x, y) < 0.0 {
                                winding -= 1;
                                crossings += 1;
                            }
                        }
                    }
                    (winding, crossings, own)
                })
                .collect();
            self.contour_sides = Some(sides);
        }
        self.contour_sides.as_deref().unwrap_or(&[])
    }

    /// For each contour, whether the region just inside it is unfilled under
    /// `fill_rule` - a hole boundary. A path with one contour has none.
    fn hole_contours(&mut self, fill_rule: FillRule) -> Vec<bool> {
        if self.contours.len() < 2 {
            return vec![false; self.contours.len()];
        }
        self.contour_sides()
            .iter()
            .map(|&(winding, crossings, own)| match fill_rule {
                FillRule::NonZero => own != 0 && winding + own == 0,
                FillRule::EvenOdd => own != 0 && crossings % 2 == 1,
            })
            .collect()
    }

    pub(crate) fn expand_fill(
        &mut self,
        fringe_width: f32,
        line_join: LineJoin,
        miter_limit: f32,
        fill_rule: FillRule,
    ) {
        let has_fringe = fringe_width > 0.0;

        // Which contours bound a hole: their interior is unfilled under the
        // fill rule, so the fringe must extrude away from it. Sampled at the
        // midpoint of each contour's first edge, which is adjacent to its
        // interior, against every other contour of the path.
        let hole = self.hole_contours(fill_rule);

        // Every offset below is taken along a point's miter vector, and that
        // vector's direction follows the order the contour's points are in. A
        // contour wound the other way extrudes its fringe outward instead of
        // inward, so the fill lands a whole fringe too wide - the clockwise
        // square in #308, a pixel bigger all round than the identical
        // counter-clockwise one. nanovg never meets this because it normalizes
        // every contour up front ("Enforce winding" in nvg__flattenPaths, with
        // nvg__addPath defaulting each path to NVG_CCW). femtovg cannot just do
        // that: unlike nanovg it lets the authored winding express holes, the
        // way SVG and Canvas write them, and the stencil pass reads that
        // winding back off these triangles. So normalize for the geometry, put
        // the triangles back the way the caller wound them, and restore the
        // points - a later stroke of the same cached path must still see its
        // own direction, which decides dash phase and which end gets which cap.
        // A contour whose interior is filled is oriented so the miter vectors
        // point inward; a hole's boundary the other way round, so its fringe
        // lands in the fill on the far side of the edge rather than in the
        // hole - nanovg's NVG_SOLID / NVG_HOLE distinction, derived here from
        // the winding the caller authored instead of declared.
        for (contour, is_hole) in self.contours.iter_mut().zip(hole) {
            let points = &mut self.points[contour.point_range.clone()];
            contour.reversed = !contour.degenerate && (Contour::polygon_area(points) < 0.0) != is_hole;
            if contour.reversed {
                Contour::reverse(points);
            }
        }

        self.calculate_joins(fringe_width, line_join, miter_limit);

        let convex = self.is_convex();

        for contour in &mut self.contours {
            contour.stroke.clear();
            contour.fill.clear();
            if contour.degenerate {
                continue;
            }

            // The fill is a fan about its first vertex: one vertex per point,
            // two at a bevel.
            let point_count = contour.point_count();
            contour.fill.reserve(point_count + contour.bevel);
            if has_fringe {
                contour.stroke.reserve((point_count + contour.bevel * 5 + 1) * 2);
            }

            // TODO: woff = 0.0 produces no artifaacts for small sizes
            let woff = 0.5 * fringe_width;
            //let woff = 0.0; // Makes everything thicker

            if has_fringe {
                for (p0, p1) in contour.point_pairs(&self.points) {
                    if p1.flags.contains(PointFlags::BEVEL) {
                        if p1.flags.contains(PointFlags::LEFT) {
                            let lpos = p1.pos + p1.dmpos * woff;
                            contour.fill.push(Vertex::pos(lpos, 0.5, 1.0));
                        } else {
                            let lpos0 = p1.pos + p0.dpos.orthogonal() * woff;
                            let lpos1 = p1.pos + p1.dpos.orthogonal() * woff;
                            contour.fill.push(Vertex::pos(lpos0, 0.5, 1.0));
                            contour.fill.push(Vertex::pos(lpos1, 0.5, 1.0));
                        }
                    } else {
                        contour.fill.push(Vertex::pos(p1.pos + p1.dmpos * woff, 0.5, 1.0));
                    }
                }
            } else {
                let points = &self.points[contour.point_range.clone()];

                for point in points {
                    contour.fill.push(Vertex::pos(point.pos, 0.5, 1.0));
                }
            }

            if contour.fill.len() > 2 {
                // Only the stencil pass reads this winding back, to tell a hole
                // from a solid; reversing the rim flips every triangle. A convex
                // path skips the stencil, where flipped triangles would just
                // face away.
                if contour.reversed && !convex {
                    contour.fill[1..].reverse();
                }
            } else {
                contour.fill.clear();
            }

            if has_fringe {
                let rw = fringe_width - woff;
                let ru = 1.0;

                let (lw, lu) = if convex {
                    // Create only half a fringe for convex shapes so that
                    // the shape can be rendered without stenciling.
                    (woff, 0.5)
                } else {
                    (fringe_width + woff, 0.0)
                };

                for (p0, p1) in contour.point_pairs(&self.points) {
                    if p1.flags.contains(PointFlags::BEVEL | PointFlags::INNERBEVEL) {
                        bevel_join(&mut contour.stroke, p0, p1, lw, rw, lu, ru);
                    } else {
                        contour.stroke.push(Vertex::pos(p1.pos + p1.dmpos * lw, lu, 1.0));
                        contour.stroke.push(Vertex::pos(p1.pos - p1.dmpos * rw, ru, 1.0));
                    }
                }

                // Loop it
                let p0 = contour.stroke[0];
                let p1 = contour.stroke[1];
                contour.stroke.push(Vertex::new(p0.x, p0.y, lu, 1.0));
                contour.stroke.push(Vertex::new(p1.x, p1.y, ru, 1.0));
            }
        }

        // The cache outlives this call; a stroke of the same path reads these
        // same points, so give them back exactly as they arrived.
        for contour in &mut self.contours {
            if contour.reversed {
                let points = &mut self.points[contour.point_range.clone()];
                Contour::reverse(points);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn expand_stroke(
        &mut self,
        stroke_width: f32,
        fringe_width: f32,
        line_cap_start: LineCap,
        line_cap_end: LineCap,
        line_join: LineJoin,
        miter_limit: f32,
        tess_tol: f32,
    ) {
        let ncap = curve_divisions(stroke_width, PI, tess_tol);

        let stroke_width = stroke_width + (fringe_width * 0.5);

        // Disable the gradient used for antialiasing when antialiasing is not enabled.
        let (u0, u1) = if fringe_width == 0.0 { (0.5, 0.5) } else { (0.0, 1.0) };

        self.calculate_joins(stroke_width, line_join, miter_limit);

        for contour in &mut self.contours {
            contour.stroke.clear();

            for (i, (p0, p1)) in contour.point_pairs(&self.points).enumerate() {
                // Add start cap
                if !contour.closed && i == 1 {
                    match line_cap_start {
                        LineCap::Butt => butt_cap_start(
                            &mut contour.stroke,
                            p0,
                            p0,
                            stroke_width,
                            -fringe_width * 0.5,
                            fringe_width,
                            u0,
                            u1,
                        ),
                        LineCap::Square => butt_cap_start(
                            &mut contour.stroke,
                            p0,
                            p0,
                            stroke_width,
                            stroke_width - fringe_width,
                            fringe_width,
                            u0,
                            u1,
                        ),
                        LineCap::Round => {
                            round_cap_start(&mut contour.stroke, p0, p0, stroke_width, ncap as usize, u0, u1)
                        }
                    }
                }

                if (i > 0 && i < contour.point_count() - 1) || contour.closed {
                    if p1.flags.contains(PointFlags::BEVEL) || p1.flags.contains(PointFlags::INNERBEVEL) {
                        if line_join == LineJoin::Round {
                            round_join(
                                &mut contour.stroke,
                                p0,
                                p1,
                                stroke_width,
                                stroke_width,
                                u0,
                                u1,
                                ncap as usize,
                            );
                        } else {
                            bevel_join(&mut contour.stroke, p0, p1, stroke_width, stroke_width, u0, u1);
                        }
                    } else {
                        contour
                            .stroke
                            .push(Vertex::pos(p1.pos + p1.dmpos * stroke_width, u0, 1.0));
                        contour
                            .stroke
                            .push(Vertex::pos(p1.pos - p1.dmpos * stroke_width, u1, 1.0));
                    }
                }

                // Add end cap
                if !contour.closed && i == contour.point_count() - 1 {
                    match line_cap_end {
                        LineCap::Butt => butt_cap_end(
                            &mut contour.stroke,
                            p1,
                            p0,
                            stroke_width,
                            -fringe_width * 0.5,
                            fringe_width,
                            u0,
                            u1,
                        ),
                        LineCap::Square => butt_cap_end(
                            &mut contour.stroke,
                            p1,
                            p0,
                            stroke_width,
                            stroke_width - fringe_width,
                            fringe_width,
                            u0,
                            u1,
                        ),
                        LineCap::Round => {
                            round_cap_end(&mut contour.stroke, p1, p0, stroke_width, ncap as usize, u0, u1)
                        }
                    }
                }
            }

            if contour.closed {
                contour
                    .stroke
                    .push(Vertex::new(contour.stroke[0].x, contour.stroke[0].y, u0, 1.0));
                contour
                    .stroke
                    .push(Vertex::new(contour.stroke[1].x, contour.stroke[1].y, u1, 1.0));
            }
        }
    }

    fn calculate_joins(&mut self, stroke_width: f32, line_join: LineJoin, miter_limit: f32) {
        let inv_stroke_width = if stroke_width > 0.0 { 1.0 / stroke_width } else { 0.0 };

        for contour in &mut self.contours {
            let points = &mut self.points[contour.point_range.clone()];
            let mut nleft = 0;

            contour.bevel = 0;

            let (mut x_turns, mut y_turns) = (AxisTurns::default(), AxisTurns::default());

            for i in 0..points.len() {
                let p0 = if i == 0 {
                    points.last().copied().unwrap()
                } else {
                    points.get(i - 1).copied().unwrap()
                };

                let p1 = points.get_mut(i).unwrap();

                let dlpos0 = p0.dpos.orthogonal();
                let dlpos1 = p1.dpos.orthogonal();

                // Calculate extrusions
                p1.dmpos = (dlpos0 + dlpos1) * 0.5;
                let dmr2 = p1.dmpos.mag2();

                if dmr2 > 0.000_001 {
                    let scale = (1.0 / dmr2).min(600.0);

                    p1.dmpos *= scale;
                }

                // Clear flags, but keep the corner.
                p1.flags = if p1.flags.contains(PointFlags::CORNER) {
                    PointFlags::CORNER
                } else {
                    PointFlags::empty()
                };

                // Keep track of left turns.
                let cross = p0.dpos.cross(p1.dpos);

                if cross > 0.0 {
                    nleft += 1;
                    p1.flags |= PointFlags::LEFT;
                }

                // Determine sign for convexity
                x_turns.add(p1.dpos.x);
                y_turns.add(p1.dpos.y);

                // Calculate if we should use bevel or miter for inner join.
                let limit = (p0.len.min(p1.len) * inv_stroke_width).max(1.01);

                if (dmr2 * limit * limit) < 1.0 {
                    p1.flags |= PointFlags::INNERBEVEL;
                }

                #[allow(clippy::collapsible_if)]
                // Check to see if the corner needs to be beveled.
                if p1.flags.contains(PointFlags::CORNER) {
                    if (dmr2 * miter_limit * miter_limit) < 1.0
                        || line_join == LineJoin::Bevel
                        || line_join == LineJoin::Round
                    {
                        p1.flags |= PointFlags::BEVEL;
                    }
                }

                if p1.flags.contains(PointFlags::BEVEL | PointFlags::INNERBEVEL) {
                    contour.bevel += 1;
                }
            }

            let convex = x_turns.total() == 2 && y_turns.total() == 2;

            contour.convexity = if nleft == points.len() && convex {
                Convexity::Convex
            } else {
                Convexity::Concave
            };
        }
    }

    /// If this path is merely a rectangle, return it
    pub(crate) fn path_fill_is_rect(&self) -> Option<crate::Rect> {
        if self.contours.len() != 1 {
            return None;
        }

        // A fan of four: the corners in turn.
        let &[top_left, bottom_left, bottom_right, top_right] = self.contours[0].fill.as_slice() else {
            return None;
        };

        if top_left.x == bottom_left.x
            && top_left.y == top_right.y
            && bottom_right.x == top_right.x
            && bottom_right.y == bottom_left.y
        {
            // A mirrored transform hands the corners over in the opposite
            // order, so the names hold only for an upright rect: take the
            // extent by min/max, not by place in the fan. The blit maps its
            // texture coordinates through the paint transform, flip included.
            let (x0, x1) = (top_left.x, top_right.x);
            let (y0, y1) = (top_left.y, bottom_left.y);
            Some(crate::Rect::new(
                x0.min(x1),
                y0.min(y1),
                (x1 - x0).abs(),
                (y1 - y0).abs(),
            ))
        } else {
            None
        }
    }
}

fn curve_divisions(radius: f32, arc: f32, tol: f32) -> u32 {
    let da = (radius / (radius + tol)).acos() * 2.0;

    ((arc / da).ceil() as u32).max(2)
}

#[allow(clippy::too_many_arguments)]
fn butt_cap_start(verts: &mut Vec<Vertex>, p0: &Point, p1: &Point, w: f32, d: f32, aa: f32, u0: f32, u1: f32) {
    let ppos = p0.pos - p1.dpos * d;
    let dlpos = p1.dpos.orthogonal();

    verts.push(Vertex::pos(ppos + dlpos * w - p1.dpos * aa, u0, 0.0));
    verts.push(Vertex::pos(ppos - dlpos * w - p1.dpos * aa, u1, 0.0));
    verts.push(Vertex::pos(ppos + dlpos * w, u0, 1.0));
    verts.push(Vertex::pos(ppos - dlpos * w, u1, 1.0));
}

#[allow(clippy::too_many_arguments)]
fn butt_cap_end(verts: &mut Vec<Vertex>, p0: &Point, p1: &Point, w: f32, d: f32, aa: f32, u0: f32, u1: f32) {
    let ppos = p0.pos + p1.dpos * d;
    let dlpos = p1.dpos.orthogonal();

    verts.push(Vertex::pos(ppos + dlpos * w, u0, 1.0));
    verts.push(Vertex::pos(ppos - dlpos * w, u1, 1.0));
    verts.push(Vertex::pos(ppos + dlpos * w + p1.dpos * aa, u0, 0.0));
    verts.push(Vertex::pos(ppos - dlpos * w + p1.dpos * aa, u1, 0.0));
}

fn round_cap_start(verts: &mut Vec<Vertex>, p0: &Point, p1: &Point, w: f32, ncap: usize, u0: f32, u1: f32) {
    let ppos = p0.pos;
    let dlpos = p1.dpos.orthogonal();

    for i in 0..ncap {
        let a = i as f32 / (ncap as f32 - 1.0) * PI;
        let offset = Vector::from_angle(a).with_basis(-dlpos, -p1.dpos) * w;

        verts.push(Vertex::pos(ppos + offset, u0, 1.0));
        verts.push(Vertex::pos(ppos, 0.5, 1.0));
    }

    verts.push(Vertex::pos(ppos + dlpos * w, u0, 1.0));
    verts.push(Vertex::pos(ppos - dlpos * w, u1, 1.0));
}

fn round_cap_end(verts: &mut Vec<Vertex>, p0: &Point, p1: &Point, w: f32, ncap: usize, u0: f32, u1: f32) {
    let ppos = p0.pos;
    let dlpos = p1.dpos.orthogonal();

    verts.push(Vertex::pos(ppos + dlpos * w, u0, 1.0));
    verts.push(Vertex::pos(ppos - dlpos * w, u1, 1.0));

    for i in 0..ncap {
        let a = i as f32 / (ncap as f32 - 1.0) * PI;
        let offset = Vector::from_angle(a).with_basis(-dlpos, p1.dpos) * w;

        verts.push(Vertex::pos(ppos, 0.5, 1.0));
        verts.push(Vertex::pos(ppos + offset, u0, 1.0));
    }
}

fn choose_bevel(bevel: bool, p0: &Point, p1: &Point, w: f32) -> (Position, Position) {
    if bevel {
        (p1.pos + p0.dpos.orthogonal() * w, p1.pos + p1.dpos.orthogonal() * w)
    } else {
        let pos = p1.pos + p1.dmpos * w;
        (pos, pos)
    }
}

#[allow(clippy::too_many_arguments)]
fn round_join(verts: &mut Vec<Vertex>, p0: &Point, p1: &Point, lw: f32, rw: f32, lu: f32, ru: f32, ncap: usize) {
    let dlpos0 = p0.dpos.orthogonal();
    let dlpos1 = p1.dpos.orthogonal();

    let a0;
    let mut a1;

    if p1.flags.contains(PointFlags::LEFT) {
        let (lpos0, lpos1) = choose_bevel(p1.flags.contains(PointFlags::INNERBEVEL), p0, p1, lw);
        a0 = (-dlpos0).angle();
        a1 = (-dlpos1).angle();

        if a1 > a0 {
            a1 -= PI * 2.0;
        }

        verts.push(Vertex::pos(lpos0, lu, 1.0));
        verts.push(Vertex::pos(p1.pos - dlpos0 * rw, ru, 1.0));

        let n = ((((a0 - a1) / PI) * ncap as f32).ceil() as usize).max(2).min(ncap);

        for i in 0..n {
            let u = i as f32 / (n - 1) as f32;
            let a = a0 + u * (a1 - a0);
            let rpos = p1.pos + Vector::from_angle(a) * rw;

            verts.push(Vertex::pos(p1.pos, 0.5, 1.0));
            verts.push(Vertex::pos(rpos, ru, 1.0));
        }

        verts.push(Vertex::pos(lpos1, lu, 1.0));
        verts.push(Vertex::pos(p1.pos - dlpos1 * rw, ru, 1.0));
    } else {
        let (rpos0, rpos1) = choose_bevel(p1.flags.contains(PointFlags::INNERBEVEL), p0, p1, -rw);
        a0 = dlpos0.angle();
        a1 = dlpos1.angle();

        if a1 < a0 {
            a1 += PI * 2.0;
        }

        verts.push(Vertex::pos(p1.pos + dlpos0 * rw, lu, 1.0));
        verts.push(Vertex::pos(rpos0, ru, 1.0));

        let n = ((((a1 - a0) / PI) * ncap as f32).ceil() as usize).max(2).min(ncap);

        for i in 0..n {
            let u = i as f32 / (n - 1) as f32;
            let a = a0 + u * (a1 - a0);
            let lpos = p1.pos + Vector::from_angle(a) * lw;

            verts.push(Vertex::pos(lpos, lu, 1.0));
            verts.push(Vertex::pos(p1.pos, 0.5, 1.0));
        }

        verts.push(Vertex::pos(p1.pos + dlpos1 * rw, lu, 1.0));
        verts.push(Vertex::pos(rpos1, ru, 1.0));
    }
}

#[allow(clippy::branches_sharing_code)]
fn bevel_join(verts: &mut Vec<Vertex>, p0: &Point, p1: &Point, lw: f32, rw: f32, lu: f32, ru: f32) {
    let dlpos0 = p0.dpos.orthogonal();
    let dlpos1 = p1.dpos.orthogonal();

    if p1.flags.contains(PointFlags::LEFT) {
        let (lpos0, lpos1) = choose_bevel(p1.flags.contains(PointFlags::INNERBEVEL), p0, p1, lw);

        verts.push(Vertex::pos(lpos0, lu, 1.0));
        verts.push(Vertex::pos(p1.pos - dlpos0 * rw, ru, 1.0));

        if p1.flags.contains(PointFlags::BEVEL) {
            verts.push(Vertex::pos(lpos0, lu, 1.0));
            verts.push(Vertex::pos(p1.pos - dlpos0 * rw, ru, 1.0));

            verts.push(Vertex::pos(lpos1, lu, 1.0));
            verts.push(Vertex::pos(p1.pos - dlpos1 * rw, ru, 1.0));
        } else {
            let rpos0 = p1.pos - p1.dmpos * rw;

            verts.push(Vertex::pos(p1.pos, 0.5, 1.0));
            verts.push(Vertex::pos(p1.pos - dlpos0 * rw, ru, 1.0));

            verts.push(Vertex::pos(rpos0, ru, 1.0));
            verts.push(Vertex::pos(rpos0, ru, 1.0));

            verts.push(Vertex::pos(p1.pos, 0.5, 1.0));
            verts.push(Vertex::pos(p1.pos - dlpos1 * rw, ru, 1.0));
        }

        verts.push(Vertex::pos(lpos1, lu, 1.0));
        verts.push(Vertex::pos(p1.pos - dlpos1 * rw, ru, 1.0));
    } else {
        let (rpos0, rpos1) = choose_bevel(p1.flags.contains(PointFlags::INNERBEVEL), p0, p1, -rw);

        verts.push(Vertex::pos(p1.pos + dlpos0 * lw, lu, 1.0));
        verts.push(Vertex::pos(rpos0, ru, 1.0));

        if p1.flags.contains(PointFlags::BEVEL) {
            verts.push(Vertex::pos(p1.pos + dlpos0 * lw, lu, 1.0));
            verts.push(Vertex::pos(rpos0, ru, 1.0));

            verts.push(Vertex::pos(p1.pos + dlpos1 * lw, lu, 1.0));
            verts.push(Vertex::pos(rpos1, ru, 1.0));
        } else {
            let lpos0 = p1.pos + p1.dmpos * lw;

            verts.push(Vertex::pos(p1.pos + dlpos0 * lw, lu, 1.0));
            verts.push(Vertex::pos(p1.pos, 0.5, 1.0));

            verts.push(Vertex::pos(lpos0, lu, 1.0));
            verts.push(Vertex::pos(lpos0, lu, 1.0));

            verts.push(Vertex::pos(p1.pos + dlpos1 * lw, lu, 1.0));
            verts.push(Vertex::pos(p1.pos, 0.5, 1.0));
        }

        verts.push(Vertex::pos(p1.pos + dlpos1 * lw, lu, 1.0));
        verts.push(Vertex::pos(rpos1, ru, 1.0));
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::Path;
    use std::cell::Cell;

    thread_local! {
        /// The pieces `tesselate_bezier` has examined on this thread.
        pub(super) static BEZIER_PIECES: Cell<usize> = const { Cell::new(0) };
    }

    fn pieces_flattening(path: &Path, transform: &Transform2D) -> usize {
        BEZIER_PIECES.with(|pieces| pieces.set(0));
        PathCache::new(path.verbs(), transform, 0.25, 0.01);
        BEZIER_PIECES.with(Cell::get)
    }

    /// A cubic whose four points coincide is the point it starts from, so it is
    /// not subdivided. It never passes the flatness test - its chord is zero -
    /// and was halved to the depth limit, 2,047 pieces that added no point:
    /// the square corner of a `rounded_rect_varying`, any curve under a
    /// transform that scales to nothing.
    #[test]
    fn a_cubic_that_is_a_point_is_not_subdivided() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.line_to(10.0, 0.0);
        path.bezier_to(10.0, 0.0, 10.0, 0.0, 10.0, 0.0);
        path.line_to(10.0, 10.0);
        assert_eq!(pieces_flattening(&path, &Transform2D::identity()), 0);
        let cache = PathCache::new(path.verbs(), &Transform2D::identity(), 0.25, 0.01);
        assert_eq!(cache.contours[0].point_count(), 3);

        // A tab's square corners cost nothing: half the pieces of the same
        // rect rounded all round.
        let identity = Transform2D::identity();
        let mut tab = Path::new();
        tab.rounded_rect_varying(0.0, 0.0, 60.0, 24.0, 6.0, 6.0, 0.0, 0.0);
        let mut rounded = Path::new();
        rounded.rounded_rect(0.0, 0.0, 60.0, 24.0, 6.0);
        assert_eq!(
            2 * pieces_flattening(&tab, &identity),
            pieces_flattening(&rounded, &identity)
        );

        assert_eq!(pieces_flattening(&rounded, &Transform2D::scaling(0.0, 0.0)), 0);
    }

    /// A cubic whose ends meet but whose control points do not is a loop, and
    /// is still flattened.
    #[test]
    fn a_loop_whose_ends_meet_is_flattened() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.bezier_to(10.0, -10.0, 10.0, 10.0, 0.0, 0.0);
        let cache = PathCache::new(path.verbs(), &Transform2D::identity(), 0.25, 0.01);
        assert_eq!(cache.contours.len(), 1);
        assert!(cache.contours[0].point_count() > 4);
    }

    // A fixed sequence of numbers in [0, 1), so the tests below are repeatable.
    fn unit(seed: &mut u32) -> f32 {
        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (*seed >> 8) as f32 / (1 << 24) as f32
    }

    // The recursive flattener the loop replaced, as the reference it must
    // match: the pieces it takes as chords, in turn.
    fn chords_recursively(chords: &mut Vec<[f32; 8]>, c: [f32; 8], level: usize, tol: f32) {
        if level > 10 {
            return;
        }
        let [x1, y1, x2, y2, x3, y3, x4, y4] = c;
        let x12 = (x1 + x2) * 0.5;
        let y12 = (y1 + y2) * 0.5;
        let x23 = (x2 + x3) * 0.5;
        let y23 = (y2 + y3) * 0.5;
        let x34 = (x3 + x4) * 0.5;
        let y34 = (y3 + y4) * 0.5;
        let x123 = (x12 + x23) * 0.5;
        let y123 = (y12 + y23) * 0.5;
        let dx = x4 - x1;
        let dy = y4 - y1;
        let d2 = ((x2 - x4) * dy - (y2 - y4) * dx).abs();
        let d3 = ((x3 - x4) * dy - (y3 - y4) * dx).abs();
        if (d2 + d3) * (d2 + d3) < tol * (dx * dx + dy * dy) {
            chords.push(c);
            return;
        }
        let x234 = (x23 + x34) * 0.5;
        let y234 = (y23 + y34) * 0.5;
        let x1234 = (x123 + x234) * 0.5;
        let y1234 = (y123 + y234) * 0.5;
        chords_recursively(chords, [x1, y1, x12, y12, x123, y123, x1234, y1234], level + 1, tol);
        chords_recursively(chords, [x1234, y1234, x234, y234, x34, y34, x4, y4], level + 1, tol);
    }

    // How far a chord is moved toward the side its control points lie on.
    fn chord_shift([x1, y1, x2, y2, x3, y3, x4, y4]: [f32; 8]) -> [f32; 2] {
        let (dx, dy) = (x4 - x1, y4 - y1);
        let d2 = (x2 - x4) * dy - (y2 - y4) * dx;
        let d3 = (x3 - x4) * dy - (y3 - y4) * dx;
        let chord = dx * dx + dy * dy;
        let bend = if chord > 0.0 {
            -CHORD_SHIFT * 0.5 * (d2 + d3) / chord
        } else {
            0.0
        };
        [-dy * bend, dx * bend]
    }

    // Each point's direction and length to the next, the last's to the first.
    fn work_out_directions(points: &mut [Point]) {
        for i in 0..points.len() {
            let next = points[(i + 1) % points.len()].pos;
            let p = &mut points[i];
            p.dpos = next - p.pos;
            p.len = p.dpos.normalize();
        }
    }

    #[test]
    fn reversing_gives_the_directions_worked_out_again() {
        let mut seed = 7;
        for case in 0..600 {
            // Every third contour on a coarse grid, for axis-aligned edges and repeated points.
            let grid = case % 3 == 0;
            let mut points: Vec<Point> = (0..1 + case % 9)
                .map(|_| {
                    let (x, y) = (unit(&mut seed) * 50.0, unit(&mut seed) * 50.0);
                    let (x, y) = if grid {
                        ((x / 10.0).floor(), (y / 10.0).floor())
                    } else {
                        (x, y)
                    };
                    Point::new(x, y, PointFlags::empty())
                })
                .collect();
            work_out_directions(&mut points);
            let bits = |p: &Point| [p.pos.x, p.pos.y, p.dpos.x, p.dpos.y, p.len].map(f32::to_bits);
            let original: Vec<_> = points.iter().map(bits).collect();

            let mut expected = points.clone();
            expected.reverse();
            work_out_directions(&mut expected);
            Contour::reverse(&mut points);
            for (p, q) in points.iter().zip(&expected) {
                let same =
                    [p.pos.x, p.pos.y, p.dpos.x, p.dpos.y, p.len] == [q.pos.x, q.pos.y, q.dpos.x, q.dpos.y, q.len];
                assert!(same, "case {case}: {p:?} reversed, {q:?} worked out again");
            }

            Contour::reverse(&mut points);
            assert_eq!(
                points.iter().map(bits).collect::<Vec<_>>(),
                original,
                "case {case}: reversed twice"
            );
        }
    }

    #[test]
    fn a_fill_hands_a_reversed_contour_back_as_it_found_it() {
        // Wound against a solid's direction, with a curve and edges along both axes.
        let mut path = Path::new();
        path.move_to(10.0, 10.0);
        path.line_to(90.0, 10.0);
        path.bezier_to(110.0, 30.0, 110.0, 70.0, 90.0, 90.0);
        path.line_to(10.0, 90.0);
        path.close();
        let mut cache = PathCache::new(path.verbs(), &Transform2D::identity(), 0.25, 0.01);
        let snapshot = |cache: &PathCache| {
            let bits = |p: &Point| [p.pos.x, p.pos.y, p.dpos.x, p.dpos.y, p.len].map(f32::to_bits);
            cache.points.iter().map(bits).collect::<Vec<_>>()
        };
        let before = snapshot(&cache);

        cache.expand_fill(1.0, LineJoin::Miter, 2.4, FillRule::NonZero);

        assert!(
            cache.contours[0].reversed,
            "the fill has to reverse this contour for the test to mean anything"
        );
        assert_eq!(snapshot(&cache), before);
    }

    #[test]
    fn the_loop_flattens_a_cubic_as_the_recursion_did() {
        let mut seed = 1;
        for case in 0..3000 {
            let mut c: [f32; 8] = std::array::from_fn(|_| unit(&mut seed) * 400.0 - 200.0);
            match case % 6 {
                // Control points at the chord's ends, a closed loop, a tiny curve, and
                // curves so large they need about ten halvings and more than ten.
                1 => [c[2], c[3], c[4], c[5]] = [c[0], c[1], c[6], c[7]],
                2 => [c[6], c[7]] = [c[0], c[1]],
                3 => c.iter_mut().for_each(|v| *v *= 1e-3),
                4 => c.iter_mut().for_each(|v| *v *= 300.0),
                5 => c.iter_mut().for_each(|v| *v *= 3000.0),
                _ => {}
            }
            let tol = [0.25, 0.0625, 0.25 / 3.0][case / 6 % 3];

            let mut path = Path::new();
            path.move_to(c[0], c[1]);
            path.bezier_to(c[2], c[3], c[4], c[5], c[6], c[7]);
            let cache = PathCache::new(path.verbs(), &Transform2D::identity(), tol, 0.01);
            // Each point between two chords moved by the mean of their shifts,
            // and the last chord's end where it is, with the curve's flags.
            let mut chords = Vec::new();
            chords_recursively(&mut chords, c, 0, tol);
            let mut expected = vec![([c[0], c[1]], PointFlags::CORNER)];
            for pair in chords.windows(2) {
                let ([a0, a1], [b0, b1]) = (chord_shift(pair[0]), chord_shift(pair[1]));
                let mean = [(a0 + b0) * 0.5, (a1 + b1) * 0.5];
                expected.push(([pair[0][6] + mean[0], pair[0][7] + mean[1]], PointFlags::empty()));
            }
            if let Some(last) = chords.last() {
                expected.push(([last[6], last[7]], PointFlags::CORNER));
            }

            let bits = |(p, flags): ([f32; 2], PointFlags)| (p.map(f32::to_bits), flags);
            let got: Vec<_> = cache
                .points
                .iter()
                .map(|p| bits(([p.pos.x, p.pos.y], p.flags)))
                .collect();
            let expected: Vec<_> = expected.into_iter().map(bits).collect();
            assert_eq!(got, expected, "case {case}: {c:?} at {tol}");
        }
    }

    #[test]
    fn self_intersecting_polygon_is_concave() {
        // star
        let mut path = Path::new();
        path.move_to(50.0, 0.0);
        path.line_to(21.0, 90.0);
        path.line_to(98.0, 35.0);
        path.line_to(2.0, 35.0);
        path.line_to(79.0, 90.0);
        path.close();

        let transform = Transform2D::identity();

        let mut path_cache = PathCache::new(path.verbs(), &transform, 0.25, 0.01);
        path_cache.expand_fill(1.0, LineJoin::Miter, 10.0, FillRule::NonZero);

        assert_eq!(path_cache.contours[0].convexity, Convexity::Concave);
    }

    /// A rect stays a rect under a mirrored transform: the corners arrive in
    /// the opposite order, and the fast image-blit path used to read that as
    /// a negative extent and draw nothing.
    #[test]
    fn a_mirrored_rect_fill_is_still_a_rect() {
        let mut path = Path::new();
        path.rect(10.0, 20.0, 30.0, 40.0);

        let upright = Transform2D::identity();
        let mut cache = PathCache::new(path.verbs(), &upright, 0.25, 0.01);
        cache.expand_fill(0.0, LineJoin::Miter, 10.0, FillRule::NonZero);
        let rect = cache.path_fill_is_rect().expect("an upright rect");
        assert_eq!((rect.x, rect.y, rect.w, rect.h), (10.0, 20.0, 30.0, 40.0));

        // y' = 100 - y: the rect lands at y in 40..80; x' = -x: at x in -40..-10.
        let mirrored = Transform2D::new(-1.0, 0.0, 0.0, -1.0, 0.0, 100.0);
        let mut cache = PathCache::new(path.verbs(), &mirrored, 0.25, 0.01);
        cache.expand_fill(0.0, LineJoin::Miter, 10.0, FillRule::NonZero);
        let rect = cache.path_fill_is_rect().expect("a mirrored rect is still a rect");
        assert_eq!((rect.x, rect.y, rect.w, rect.h), (-40.0, 40.0, 30.0, 40.0));
    }

    /// A vertex repeated within the distance tolerance merges into the one
    /// before it, as in nanovg's nvg__addPoint.
    #[test]
    fn a_repeated_vertex_is_merged() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.line_to(10.0, 0.0);
        path.line_to(10.0, 0.0);
        path.line_to(10.0, 10.0);

        let cache = PathCache::new(path.verbs(), &Transform2D::identity(), 0.25, 0.01);
        assert_eq!(cache.contours[0].point_count(), 3);
    }

    /// The merged point takes the repeat's flags: a curve's end landing on
    /// its last flattened point is still a corner.
    #[test]
    fn a_merged_vertex_keeps_the_corner_flag() {
        let mut cache = PathCache::default();
        cache.add_contour();
        cache.add_point(0.0, 0.0, PointFlags::CORNER, 0.01);
        cache.add_point(10.0, 0.0, PointFlags::empty(), 0.01);
        cache.add_point(10.0, 0.005, PointFlags::CORNER, 0.01);

        assert_eq!(cache.contours[0].point_count(), 2);
        assert!(cache.points[1].flags.contains(PointFlags::CORNER));
    }

    /// A curve shorter than the tolerance merges into the point it starts from.
    #[test]
    fn a_tiny_curve_is_merged() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.line_to(10.0, 0.0);
        path.bezier_to(10.0, 0.0, 10.0, 0.0, 10.005, 0.0);
        path.line_to(10.0, 10.0);

        let cache = PathCache::new(path.verbs(), &Transform2D::identity(), 0.25, 0.01);
        assert_eq!(cache.contours[0].point_count(), 3);
    }

    /// A pill from `rounded_rect` has zero-length sides, so each one repeats a
    /// vertex. Merged, the pill is convex and filled without the stencil.
    #[test]
    fn a_pill_is_convex() {
        let mut path = Path::new();
        path.rounded_rect(0.0, 0.0, 40.0, 10.0, 5.0);

        let mut cache = PathCache::new(path.verbs(), &Transform2D::identity(), 0.25, 0.01);
        cache.expand_fill(1.0, LineJoin::Miter, 10.0, FillRule::NonZero);
        assert_eq!(cache.contours[0].convexity, Convexity::Convex);
    }

    /// A contour's first point is never merged into the previous contour's
    /// last one.
    #[test]
    fn a_new_contour_keeps_its_first_point() {
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.line_to(10.0, 0.0);
        path.move_to(10.0, 0.0);
        path.line_to(10.0, 10.0);

        let cache = PathCache::new(path.verbs(), &Transform2D::identity(), 0.25, 0.01);
        let counts: Vec<usize> = cache.contours.iter().map(Contour::point_count).collect();
        assert_eq!(counts, [2, 2]);
    }

    #[test]
    fn winding_triangles_do_not_classify_contour_nesting() {
        let mut path = Path::new();
        for inset in 0..256 {
            let inset = inset as f32 * 0.01;
            path.rect(inset, inset, 10.0, 10.0);
        }

        let cache = PathCache::new(path.verbs(), &Transform2D::identity(), 0.25, 0.01);
        let vertices = cache.winding_triangles();

        assert!(cache.contour_sides.is_none());
        assert_eq!(vertices.len(), 256 * 6);
    }
}

/*
pub struct MutStridedChunks<'a, T: 'a> {
    buffer: &'a mut [T],
    rotated: bool,
    pos: usize,
}

impl<'a, T: 'a> MutStridedChunks<'a, T> {
    pub fn new(buffer: &'a mut [T]) -> Self {
        buffer.rotate_right(1);
        Self {
            buffer: buffer,
            rotated: false,
            pos: 0
        }
    }

    fn next(&mut self) -> Option<&mut [T]> {
        if self.pos == self.buffer.len() - 1 && !self.rotated {
            self.buffer.rotate_left(1);
            self.rotated = true;
            self.pos -= 1;
        }

        let len = self.buffer.len() - self.pos;

        if 2 <= len {
            let (start, end) = (self.pos, self.pos + 2);
            let subslice = &mut self.buffer[start..end];

            self.pos += 1;
            Some(subslice)
        } else {
            None
        }
    }
}

fn main() {
    let mut my_array = [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19];

    let mut iter = MutStridedChunks::new(&mut my_array);

    while let Some(subslice) = iter.next() {
        log::info!("{:?}", subslice);
    }
}
*/
