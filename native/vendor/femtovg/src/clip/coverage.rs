//! The share of each pixel that outlines cover, worked out on the CPU: what
//! a clip that is no box, or a thin fill, takes its edge from.
//!
//! Each edge adds, row by row, the signed area it sweeps to its right into
//! the cells it crosses, so that a cell's winding is the sum of what the
//! cells up to it in its row hold - the accumulation of font-rs and of
//! FreeType's smooth rasterizer. The rows are resolved top to bottom from
//! the edges that cross each, into one row of cells and a bit for each cell
//! an edge touched: the touched cells are summed in order, and the cells
//! between two of them cost a fill and no arithmetic. An edge pixel gets the
//! exact area of the polygon inside it; where two contours cross in one
//! pixel their areas add before the fill rule folds them.

use crate::FillRule;

/// One part in this many of a pixel is what an outline's points are
/// rounded to before they are rasterized, so that an outline can be told
/// from another by its points.
pub(crate) const SUBPIXELS: f32 = 256.0;

/// An edge from its top to its bottom: `x` and `y` at the top, the change
/// in x a unit of y, and 1 if it was drawn downwards, -1 if upwards.
#[derive(Clone, Copy, Debug, Default)]
struct Edge {
    x: f32,
    y: f32,
    bottom: f32,
    slope: f32,
    down: f32,
}

impl Edge {
    fn at(&self, y: f32) -> f32 {
        self.x + (y - self.y) * self.slope
    }
}

/// One row being resolved: its area by cell, which cells hold any, and the
/// first and last word of `touched` set.
struct Row<'a> {
    width: usize,
    area: &'a mut [f32],
    touched: &'a mut [u64],
    words: [usize; 2],
}

impl Row<'_> {
    /// Adds the piece of an edge inside the row: from `xa` at its top to
    /// `xb` at its bottom, `area` the height it spans there, signed. What
    /// lies left of the mask counts for every cell of the row, what lies
    /// right of it for none.
    fn span(&mut self, xa: f32, xb: f32, area: f32) {
        let width = self.width as f32;
        let (lo, hi) = if xa <= xb { (xa, xb) } else { (xb, xa) };
        if lo >= width {
            return;
        }
        if hi <= 0.0 {
            return self.add(0, area);
        }
        if lo < 0.0 {
            let outside = lo / (lo - hi);
            self.add(0, area * outside);
            let (xa, xb) = if xa < xb { (0.0, xb) } else { (xa, 0.0) };
            return self.span(xa, xb, area * (1.0 - outside));
        }
        if hi > width {
            let inside = (width - lo) / (hi - lo);
            let (xa, xb) = if xa < xb { (xa, width) } else { (width, xb) };
            return self.span(xa, xb, area * inside);
        }
        let first = lo.floor();
        let end = hi.ceil();
        let (cell, last) = (first as usize, end as usize);
        if last <= cell + 1 {
            // Inside one cell: what is right of the edge's middle is the cell's.
            let middle = 0.5 * (lo + hi) - first;
            self.add(cell, area * (1.0 - middle));
            self.add(cell + 1, area * middle);
            return;
        }
        // Across cells the edge's height is spread evenly along x: a
        // triangle in the first and the last cell, a trapezoid in between.
        let per_cell = (hi - lo).recip();
        let enters = 1.0 - (lo - first);
        let leaves = hi - (end - 1.0);
        let head = 0.5 * per_cell * enters * enters;
        let tail = 0.5 * per_cell * leaves * leaves;
        self.add(cell, area * head);
        if last == cell + 2 {
            self.add(cell + 1, area * (1.0 - head - tail));
        } else {
            let second = per_cell * (enters + 0.5);
            self.add(cell + 1, area * (second - head));
            for between in cell + 2..last - 1 {
                self.add(between, area * per_cell);
            }
            let before_last = second + (last - cell - 3) as f32 * per_cell;
            self.add(last - 1, area * (1.0 - before_last - tail));
        }
        self.add(last, area * tail);
    }

    fn add(&mut self, cell: usize, area: f32) {
        if cell < self.width && area != 0.0 {
            self.area[cell] += area;
            let word = cell / 64;
            self.touched[word] |= 1 << (cell % 64);
            self.words = [self.words[0].min(word), self.words[1].max(word)];
        }
    }
}

/// The edges added, and what resolving them reuses: the edges crossing the
/// row, the row's area by cell and which cells hold any. The buffers are
/// kept from one mask to the next.
#[derive(Debug, Default)]
pub(crate) struct Coverage {
    width: usize,
    height: usize,
    edges: Vec<Edge>,
    // Resolving: the edges by the row they start in, and those crossing it.
    starts: Vec<u32>,
    order: Vec<Edge>,
    active: Vec<Edge>,
    area: Vec<f32>,
    touched: Vec<u64>,
    /// A contour's points on their way in, kept for the next.
    pub(crate) points: Vec<[f32; 2]>,
}

impl Coverage {
    #[cfg(test)]
    pub(crate) fn new(width: usize, height: usize) -> Self {
        let mut coverage = Self::default();
        coverage.reset(width, height);
        coverage
    }

    /// Starts a mask of another size, with nothing added.
    pub(crate) fn reset(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.edges.clear();
        // Resolving leaves every cell and bit as it found them: clear.
        if self.area.len() < width {
            self.area.resize(width, 0.0);
            self.touched.resize(width.div_ceil(64), 0);
        }
    }

    /// Adds a closed contour, its points in pixels from the mask's corner.
    pub(crate) fn contour(&mut self, points: &[[f32; 2]]) {
        let Some(&last) = points.last() else {
            return;
        };
        let mut from = last;
        for &to in points {
            self.edge(from, to);
            from = to;
        }
    }

    /// Adds one edge: rows above and below the mask take nothing from it.
    fn edge(&mut self, [x0, y0]: [f32; 2], [x1, y1]: [f32; 2]) {
        if y0 == y1 || !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
            return;
        }
        // Downwards the edge has what is inside to its right counted up.
        let (down, (xa, ya), (xb, yb)) = if y0 < y1 {
            (1.0, (x0, y0), (x1, y1))
        } else {
            (-1.0, (x1, y1), (x0, y0))
        };
        if yb <= 0.0 || ya >= self.height as f32 {
            return;
        }
        self.edges.push(Edge {
            x: xa,
            y: ya,
            bottom: yb,
            slope: (xb - xa) / (yb - ya),
            down,
        });
    }

    /// Folds what was added by `rule` into coverage and adds it to `mask`,
    /// `width` bytes a row, up to full: outlines resolved one after another
    /// come to their union, each under its own rule, and two that share an
    /// edge leave no seam along it. Leaves nothing added.
    pub(crate) fn resolve(&mut self, rule: FillRule, mask: &mut [u8]) {
        debug_assert_eq!(mask.len(), self.width * self.height);
        let covered = |winding: f32| -> u8 {
            let winding = winding.abs();
            let share = match rule {
                FillRule::NonZero => winding.min(1.0),
                FillRule::EvenOdd => {
                    let folded = winding % 2.0;
                    if folded > 1.0 {
                        2.0 - folded
                    } else {
                        folded
                    }
                }
            };
            (share * 255.0 + 0.5) as u8
        };
        let fill = |cells: &mut [u8], coverage: u8| match coverage {
            0 => {}
            255 => cells.fill(255),
            _ => cells.iter_mut().for_each(|cell| *cell = cell.saturating_add(coverage)),
        };
        // Top to bottom, an edge joining the rows it crosses as they come:
        // gathered by the row it starts in with a counting sort.
        let row_of = |edge: &Edge| edge.y.max(0.0) as usize;
        self.starts.clear();
        self.starts.resize(self.height + 1, 0);
        for edge in &self.edges {
            self.starts[row_of(edge) + 1] += 1;
        }
        for row in 0..self.height {
            self.starts[row + 1] += self.starts[row];
        }
        self.order.clear();
        self.order.resize(self.edges.len(), Edge::default());
        for edge in &self.edges {
            let at = &mut self.starts[row_of(edge)];
            self.order[*at as usize] = *edge;
            *at += 1;
        }
        // Filling moved each row's start to its end: its edges are those before.
        let height = self.height as f32;
        let mut next = 0;
        for (row, line) in mask.chunks_exact_mut(self.width.max(1)).enumerate() {
            let end = self.starts[row] as usize;
            self.active.extend_from_slice(&self.order[next..end]);
            next = end;
            if self.active.is_empty() {
                continue;
            }
            let width = self.width;
            let mut cells = Row {
                width,
                area: &mut self.area[..width],
                touched: &mut self.touched[..width.div_ceil(64)],
                words: [usize::MAX, 0],
            };
            let (top, bottom) = (row as f32, (row + 1) as f32);
            let mut kept = 0;
            for index in 0..self.active.len() {
                let edge = self.active[index];
                let (y, next) = (edge.y.max(top), bottom.min(edge.bottom).min(height));
                cells.span(edge.at(y), edge.at(next), (next - y) * edge.down);
                if edge.bottom > bottom {
                    self.active[kept] = edge;
                    kept += 1;
                }
            }
            self.active.truncate(kept);

            let (mut winding, mut from) = (0.0, 0);
            let words = cells.words;
            for word in words[0]..=words[1] {
                let mut bits = std::mem::take(&mut cells.touched[word]);
                while bits != 0 {
                    let cell = word * 64 + bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    if cell > from {
                        fill(&mut line[from..cell], covered(winding));
                    }
                    winding += std::mem::take(&mut cells.area[cell]);
                    line[cell] = line[cell].saturating_add(covered(winding));
                    from = cell + 1;
                }
            }
            // Past the last edge the winding is what the mask's right side cut off.
            fill(&mut line[from..], covered(winding));
        }
        self.edges.clear();
        self.active.clear();
    }
}

/// Multiplies `mask` by `other` where the two overlap, and clears it
/// elsewhere: the coverage of what both cover, as a product. `offset` is
/// where `mask`'s corner lies in `other`.
pub(crate) fn intersect(mask: &mut [u8], size: [usize; 2], other: &[u8], other_size: [usize; 2], offset: [i32; 2]) {
    let [width, height] = size;
    let [other_width, other_height] = other_size;
    for row in 0..height {
        let line = &mut mask[row * width..][..width];
        let other_row = row as i64 + i64::from(offset[1]);
        if other_row < 0 || other_row >= other_height as i64 {
            line.fill(0);
            continue;
        }
        // The columns of this row that lie inside `other`.
        let start = (-i64::from(offset[0])).clamp(0, width as i64) as usize;
        let end = (other_width as i64 - i64::from(offset[0])).clamp(0, width as i64) as usize;
        line[..start].fill(0);
        line[end.max(start)..].fill(0);
        if start >= end {
            continue;
        }
        let other_start = (start as i64 + i64::from(offset[0])) as usize;
        let other_line = &other[other_row as usize * other_width + other_start..][..end - start];
        for (cell, &by) in line[start..end].iter_mut().zip(other_line) {
            if by != 255 && *cell != 0 {
                *cell = ((u16::from(*cell) * u16::from(by) + 127) / 255) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The area of a convex or concave simple polygon inside the unit pixel
    /// at (`px`, `py`), by clipping it to the pixel's four sides.
    fn area_inside(polygon: &[[f32; 2]], px: usize, py: usize) -> f64 {
        let mut points: Vec<[f64; 2]> = polygon.iter().map(|p| [f64::from(p[0]), f64::from(p[1])]).collect();
        let (x0, y0) = (px as f64, py as f64);
        // (axis, bound, keep below)
        for (axis, bound, below) in [(0, x0, false), (0, x0 + 1.0, true), (1, y0, false), (1, y0 + 1.0, true)] {
            let inside = |p: &[f64; 2]| if below { p[axis] <= bound } else { p[axis] >= bound };
            let mut out = Vec::with_capacity(points.len() + 4);
            for (index, current) in points.iter().enumerate() {
                let previous = &points[(index + points.len() - 1) % points.len()];
                let crossing = || {
                    let t = (bound - previous[axis]) / (current[axis] - previous[axis]);
                    [
                        previous[0] + t * (current[0] - previous[0]),
                        previous[1] + t * (current[1] - previous[1]),
                    ]
                };
                match (inside(previous), inside(current)) {
                    (true, true) => out.push(*current),
                    (true, false) => out.push(crossing()),
                    (false, true) => {
                        out.push(crossing());
                        out.push(*current);
                    }
                    (false, false) => {}
                }
            }
            points = out;
            if points.is_empty() {
                return 0.0;
            }
        }
        let twice: f64 = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
            .sum();
        twice.abs() / 2.0
    }

    fn rasterized(size: [usize; 2], contours: &[&[[f32; 2]]], rule: FillRule) -> Vec<u8> {
        let mut coverage = Coverage::new(size[0], size[1]);
        for contour in contours {
            coverage.contour(contour);
        }
        let mut mask = vec![0; size[0] * size[1]];
        coverage.resolve(rule, &mut mask);
        mask
    }

    /// Every pixel of a simple polygon gets the area of the polygon inside
    /// it, to half a level of 255: upright and turned, clockwise and not,
    /// thinner than a pixel, a sliver across many cells of a row.
    #[test]
    fn a_pixel_gets_the_area_inside_it() {
        let polygons: [&[[f32; 2]]; 7] = [
            &[[2.25, 3.5], [12.75, 3.5], [12.75, 9.125], [2.25, 9.125]],
            &[[2.25, 9.125], [12.75, 9.125], [12.75, 3.5], [2.25, 3.5]],
            &[[8.0, 1.3], [14.6, 13.2], [1.4, 10.7]],
            &[[1.1, 1.2], [14.9, 2.3], [13.7, 14.8], [0.4, 12.9]],
            &[[3.0, 5.2], [13.0, 5.5], [13.0, 5.9], [3.0, 5.6]],
            &[[0.5, 7.25], [15.5, 7.5], [15.5, 7.75]],
            &[[5.5, 0.5], [5.75, 0.5], [9.75, 15.5], [9.5, 15.5]],
        ];
        for polygon in polygons {
            let mask = rasterized([16, 16], &[polygon], FillRule::NonZero);
            for py in 0..16 {
                for px in 0..16 {
                    let exact = area_inside(polygon, px, py);
                    let got = f64::from(mask[py * 16 + px]) / 255.0;
                    assert!(
                        (got - exact).abs() <= 0.5 / 255.0 + 1e-4,
                        "{polygon:?} at ({px}, {py}): {got} for {exact}"
                    );
                }
            }
        }
    }

    /// What reaches past the mask is cut at its sides: the cells inside get
    /// what they would in a mask around the whole outline.
    #[test]
    fn an_outline_past_the_mask_is_cut_at_its_sides() {
        let polygon: &[[f32; 2]] = &[[-6.5, -3.25], [21.25, 2.5], [18.5, 22.75], [-4.0, 14.5]];
        let shifted: Vec<[f32; 2]> = polygon.iter().map(|p| [p[0] + 8.0, p[1] + 8.0]).collect();
        let whole = rasterized([32, 32], &[&shifted], FillRule::NonZero);
        let cut = rasterized([16, 16], &[polygon], FillRule::NonZero);
        for py in 0..16 {
            for px in 0..16 {
                let (inside, around) = (cut[py * 16 + px], whole[(py + 8) * 32 + px + 8]);
                assert!(inside.abs_diff(around) <= 1, "({px}, {py}): {inside} for {around}");
            }
        }
        // A contour wholly to the left covers the rows it spans; one wholly
        // above, below or to the right covers nothing.
        let left = rasterized(
            [4, 4],
            &[&[[-9.0, 1.0], [-2.0, 1.0], [-2.0, 3.0], [-9.0, 3.0]]],
            FillRule::NonZero,
        );
        assert_eq!(left, [0; 16], "left of the mask, closed there");
        let across = rasterized(
            [4, 4],
            &[&[[-9.0, 1.0], [9.0, 1.0], [9.0, 3.0], [-9.0, 3.0]]],
            FillRule::NonZero,
        );
        assert_eq!(across, [0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0]);
    }

    /// The fill rule folds the winding: a square inside a square the same
    /// way round is one square under nonzero and a frame under even-odd; the
    /// other way round it is a frame under both.
    #[test]
    fn the_fill_rule_folds_the_winding() {
        let outer: &[[f32; 2]] = &[[1.0, 1.0], [7.0, 1.0], [7.0, 7.0], [1.0, 7.0]];
        let inner: &[[f32; 2]] = &[[3.0, 3.0], [5.0, 3.0], [5.0, 5.0], [3.0, 5.0]];
        let reversed: Vec<[f32; 2]> = inner.iter().rev().copied().collect();
        let at = |mask: &[u8], x: usize, y: usize| mask[y * 8 + x];
        let same = rasterized([8, 8], &[outer, inner], FillRule::NonZero);
        assert_eq!((at(&same, 2, 2), at(&same, 4, 4), at(&same, 0, 0)), (255, 255, 0));
        let folded = rasterized([8, 8], &[outer, inner], FillRule::EvenOdd);
        assert_eq!((at(&folded, 2, 2), at(&folded, 4, 4), at(&folded, 0, 0)), (255, 0, 0));
        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            let frame = rasterized([8, 8], &[outer, &reversed], rule);
            assert_eq!((at(&frame, 2, 2), at(&frame, 4, 4)), (255, 0), "{rule:?}");
        }
    }

    /// Outlines resolved one after another add up to their union: each under
    /// its own rule, whichever way round it runs, with no seam where two
    /// share an edge inside a pixel and no more than full where they overlap.
    #[test]
    fn outlines_resolved_in_turn_come_to_their_union() {
        let left: &[[f32; 2]] = &[[1.0, 1.0], [4.4, 1.0], [4.4, 7.0], [1.0, 7.0]];
        let right: &[[f32; 2]] = &[[4.4, 1.0], [7.0, 1.0], [7.0, 7.0], [4.4, 7.0]];
        let backwards: Vec<[f32; 2]> = right.iter().rev().copied().collect();
        let over: &[[f32; 2]] = &[[2.0, 2.0], [6.0, 2.0], [6.0, 6.0], [2.0, 6.0]];
        let mut coverage = Coverage::new(8, 8);
        let mut mask = vec![0; 64];
        coverage.contour(left);
        coverage.resolve(FillRule::NonZero, &mut mask);
        coverage.contour(&backwards);
        coverage.resolve(FillRule::EvenOdd, &mut mask);
        assert_eq!(
            &mask[3 * 8..4 * 8],
            [0, 255, 255, 255, 255, 255, 255, 0],
            "no seam at 4.4"
        );
        coverage.contour(over);
        coverage.resolve(FillRule::NonZero, &mut mask);
        assert_eq!(
            &mask[3 * 8..4 * 8],
            [0, 255, 255, 255, 255, 255, 255, 0],
            "full is full"
        );
    }

    /// Two masks intersect as the product of their coverages where they
    /// overlap, and as nothing where they do not.
    #[test]
    fn masks_intersect_as_a_product() {
        let mut mask = vec![255, 128, 255, 255, 64, 255, 255, 255, 255];
        let other = vec![128, 255, 0, 255];
        // `mask`'s corner is one cell left of and above `other`'s.
        intersect(&mut mask, [3, 3], &other, [2, 2], [-1, -1]);
        assert_eq!(mask, [0, 0, 0, 0, 32, 255, 0, 0, 255]);
    }
}
