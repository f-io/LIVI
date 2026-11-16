//! Clipping to a path: the clip stack, the boxes ([`shape`]) and coverage
//! masks ([`mask`]) that each draw's fragment shader evaluates, and the
//! stencil plane - its replay per target and the quads that arm and resolve
//! it - for what neither takes.

use super::*;
use crate::path::PathCache;
use rgb::FromSlice;

mod coverage;
mod fill;
mod mask;
mod shape;
pub(crate) use fill::{is_thin, FillMasks, THIN_FILL_SPAN};
use fill::{PlacedMask, Placement};
use mask::MaskOutline;
pub(crate) use mask::{ClipMasks, MaskImage};
pub(crate) use shape::{ClipCoverage, RectFill, RoundedBox};

#[derive(Debug)]
pub(crate) struct ClipGeometry {
    pub(crate) vertices: Box<[Vertex]>,
}

#[derive(Debug)]
pub(crate) struct ClipEntry {
    pub(crate) target: RenderTarget,
    pub(crate) kind: ClipKind,
}

#[derive(Debug)]
pub(crate) enum ClipKind {
    /// Rasterized into the target's stencil clip plane.
    Stencil {
        geometry: Rc<ClipGeometry>,
        fill_rule: FillRule,
        bounds: Bounds,
        prior_armed: Rect,
        armed: Rect,
    },
    /// Evaluated by each draw's fragment shader, in device space, as the
    /// coverage worked out when the clip was taken. The innermost one on a
    /// target is the one in force: each is stacked as what it leaves of the
    /// one before it. Where the clip and the shape make no one box, `cut` is
    /// a box with square corners and parallel sides that cuts the shape,
    /// carried by each draw in the scissor's place: the cuts on a target
    /// clip together as what all of them cover.
    Shape {
        shape: RoundedBox,
        coverage: ClipCoverage,
        cut: Option<RoundedBox>,
    },
    /// Read by each draw's fragment shader from a coverage mask whose
    /// corner lies at `origin` on the target. The innermost one on a target
    /// is the one in force: each is rasterized as what it leaves of the one
    /// before it.
    Mask { mask: Rc<MaskImage>, origin: [i32; 2] },
}

/// How much finer than a fill's outline a mask's is flattened: at a 64th of
/// the tolerance a curve's chords stay within a few hundredths of a pixel of
/// it, where a fill's are up to a fifth of a pixel inside - a ring of circles
/// 38 and 18 pixels in radius is then 0.005 of a pixel's coverage from its
/// area on average and 0.02 at worst, against 0.064 and 0.22.
const MASK_TESSELLATION: f32 = 1.0 / 64.0;

/// The coverage mask a draw carries: the image, where its corner lies on
/// the target, the pixels it spans there and what one of them is of the
/// image, which is no smaller. `hard` takes a pixel whole or not at all,
/// for an operation that coverage cannot bound.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct MaskCoverage {
    pub(crate) image: ImageId,
    pub(crate) origin: [f32; 2],
    pub(crate) size: [f32; 2],
    pub(crate) texel: [f32; 2],
    pub(crate) hard: bool,
}

impl MaskCoverage {
    /// The device rectangle outside which the mask is empty: the pixels it
    /// spans.
    pub(crate) fn reach(&self) -> Bounds {
        Bounds {
            minx: self.origin[0],
            miny: self.origin[1],
            maxx: self.origin[0] + self.size[0],
            maxy: self.origin[1] + self.size[1],
        }
    }
}

/// What clips a draw: the clip shape and the scissor - the boxes - and the
/// coverage mask.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct DrawClip {
    pub(crate) shape: Option<ClipCoverage>,
    pub(crate) scissor: Scissor,
    pub(crate) mask: Option<MaskCoverage>,
}

/// The boxes a draw meets: the clip shape - with the scissor, where the two
/// make one box - and the scissor left beside it, as the scissor it is and
/// as a box of its own. Worked out for a scissor and a shape, and kept for
/// the draws that follow under the same two.
#[derive(Copy, Clone, Debug)]
pub(crate) struct ClipBoxes {
    of: (Scissor, Option<RoundedBox>, Option<RoundedBox>),
    shape: Option<(RoundedBox, ClipCoverage)>,
    scissor: Scissor,
    scissor_box: Option<(RoundedBox, ClipCoverage)>,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ClipPlaneState {
    pub(crate) count: usize,
    pub(crate) dirty: bool,
    pub(crate) armed: Rect,
}

impl<T> Canvas<T>
where
    T: Renderer,
{
    /// Drops the clips above `depth`. Their target planes are reconciled only
    /// when drawing resumes on them, so consecutive restores coalesce.
    pub(crate) fn pop_clips_to(&mut self, depth: usize) {
        if self.clip_stack.len() <= depth {
            return;
        }

        let mut removed = HashMap::<RenderTarget, (usize, Rect)>::new();
        for entry in self.clip_stack.drain(depth..) {
            if let ClipKind::Stencil { prior_armed, .. } = entry.kind {
                removed
                    .entry(entry.target)
                    .and_modify(|(count, _)| *count += 1)
                    .or_insert((1, prior_armed));
            }
        }
        for (target, (count, armed)) in removed {
            let plane = self
                .clip_planes
                .get_mut(&target)
                .expect("a clip entry has a target plane");
            plane.count -= count;
            plane.dirty = true;
            plane.armed = armed;
        }
    }

    pub(crate) fn reconcile_current_clip_plane(&mut self) {
        // A suppressed layer records no current-target draws, so defer stencil
        // repair instead of marking an unrecorded replay clean.
        if self.commands_suppressed() {
            return;
        }
        let target = self.current_render_target;
        let dirty = self.clip_planes.get(&target).is_some_and(|plane| plane.dirty);
        if !dirty {
            return;
        }
        self.clip_planes.get_mut(&target).unwrap().dirty = false;
        self.replay_clip_stack();
        if self.clip_planes.get(&target).is_some_and(|plane| plane.count == 0) {
            self.clip_planes.remove(&target);
        }
    }

    /// Whether the stencil clip gates draws into the current render target:
    /// some clip on the stack was taken on it.
    pub(crate) fn clip_active(&self) -> bool {
        self.clip_planes
            .get(&self.current_render_target)
            .is_some_and(|plane| plane.count != 0)
    }

    pub(crate) fn forget_clip_target(&mut self, target: RenderTarget) {
        let removed: Vec<usize> = self
            .clip_stack
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| (entry.target == target).then_some(index))
            .collect();
        if !removed.is_empty() {
            for state in &mut self.state_stack {
                state.clip_depth -= removed.partition_point(|&index| index < state.clip_depth);
            }
            self.clip_stack.retain(|entry| entry.target != target);
        }
        self.clip_planes.remove(&target);
    }

    /// Intersects the clip region with `path` under the current transform,
    /// with `fill_rule` as the clip-rule: Canvas 2D `clip()`, SVG `clip-path`
    /// and `clip-rule`. Drawing after this call is limited to the
    /// intersection of every clip taken on the current render target.
    ///
    /// A clip's edge takes coverage as a fill's does. A clip whose path
    /// outlines a rectangle (under any transform), a rounded rectangle, one
    /// rounded at the two corners of a side only, or an ellipse is a shape
    /// that the fragment shader of each draw under it evaluates, and a
    /// rectangle with sides parallel to a shape that it cuts clips with it
    /// in the scissor's place; any other clip is rasterized into a coverage
    /// mask, one byte for each pixel of its bounds, that those shaders read.
    /// A mask is kept, within a budget
    /// ([`set_clip_mask_budget`](Self::set_clip_mask_budget)), while its clip
    /// does not change or moves by whole pixels; a clip the budget has no
    /// room for is taken on the stencil, where a pixel is either inside it or
    /// outside. So is a shape once something that reaches past it is drawn
    /// under it with a composite operation that changes the destination
    /// where the source is transparent ([`CompositeOperation::Copy`],
    /// `SourceIn`, `SourceOut`, `DestinationIn`, `DestinationAtop`); under a
    /// mask such an operation takes each pixel whole or not at all.
    ///
    /// A draw that stays inside a shape is not clipped: an edge it shares
    /// with the clip is its own. A draw that reaches past it takes the
    /// clip's coverage over its own, so that where the two share an edge a
    /// pixel half inside is a quarter covered - except an upright rectangle
    /// filled under an upright shape or cut, which is cut to a rectangular
    /// one and drawn as a rounded one it covers. A scissor meets a draw as a
    /// shape does, and one that makes one box with the shape - around it,
    /// inside it, or two rectangles with parallel sides - clips with it as
    /// that box: an edge the two share takes coverage once. A mask's
    /// coverage multiplies every draw under it.
    ///
    /// Clips are part of the saved state - [`restore`](Self::restore) drops
    /// the clips taken since the matching [`save`](Self::save) - and belong
    /// to the render target they were taken on: a clip on the canvas still
    /// applies when a layer opened on it is composited back, and a clip taken
    /// inside a layer or while rendering into an image gates only that layer
    /// or image.
    ///
    /// [`clear_rect`](Self::clear_rect) is not clipped; a Canvas 2D
    /// `clearRect` under a clip is a fill of the rect with an opaque paint
    /// under [`CompositeOperation::DestinationOut`].
    pub fn clip_path(&mut self, path: &Path, fill_rule: FillRule) {
        if self.saturated() {
            // Nothing draws past the depth limit, so nothing to gate; the
            // clip would only cost geometry.
            return;
        }
        if self.clip_to_shape(path) || self.clip_to_mask(&[(path, fill_rule)]) {
            return;
        }
        let (geometry, bounds) = self.stencil_geometry(path, &self.state().transform);
        self.clip_to_stencil(geometry, bounds, fill_rule);
    }

    /// Intersects the clip region with the union of `paths`, each under its
    /// own fill rule and all under the current transform: an SVG `clipPath`
    /// with several children, each with its `clip-rule`. One path clips as
    /// [`clip_path`](Self::clip_path) does, none leaves nothing to draw on.
    ///
    /// The union is taken in a coverage mask, where children that share an
    /// edge leave no seam along it. When the masks' budget
    /// ([`set_clip_mask_budget`](Self::set_clip_mask_budget)) has no room
    /// for it the children clip as one path under the first one's rule, on
    /// the stencil: children that overlap under even-odd, or wind opposite
    /// ways, then cut holes into one another.
    pub fn clip_paths(&mut self, paths: &[(&Path, FillRule)]) {
        if let [(path, fill_rule)] = paths {
            return self.clip_path(path, *fill_rule);
        }
        if self.saturated() || self.clip_to_mask(paths) {
            return;
        }
        let transform = self.state().transform;
        let mut vertices = Vec::new();
        let mut bounds = Bounds::default();
        for (path, _) in paths {
            let path_cache = path.cache(&transform, self.tess_tol, self.dist_tol);
            vertices.extend(path_cache.winding_triangles());
            bounds = Bounds {
                minx: bounds.minx.min(path_cache.bounds.minx),
                miny: bounds.miny.min(path_cache.bounds.miny),
                maxx: bounds.maxx.max(path_cache.bounds.maxx),
                maxy: bounds.maxy.max(path_cache.bounds.maxy),
            };
        }
        let geometry = Rc::new(ClipGeometry {
            vertices: vertices.into_boxed_slice(),
        });
        let fill_rule = paths.first().map_or(FillRule::NonZero, |(_, fill_rule)| *fill_rule);
        self.clip_to_stencil(geometry, bounds, fill_rule);
    }

    /// The masks' budget in bytes: a mask holds a byte for each pixel of
    /// its clip's bounds, and an image of as many.
    pub fn clip_mask_budget(&self) -> usize {
        self.clip_masks.budget()
    }

    /// What the masks hold now, in bytes, against their budget.
    pub fn clip_mask_bytes(&self) -> usize {
        self.clip_masks.bytes()
    }

    /// Sets the budget of the coverage masks that antialias clips to paths
    /// which are no rectangle, rounded rectangle or ellipse. A mask is kept
    /// while a clip holds it and for two frames after, so that a clip which
    /// has not changed costs a lookup; a clip the budget has no room for
    /// beside the masks of its frame is taken on the stencil, where its
    /// edge is not antialiased. A budget of zero takes every such clip
    /// there.
    pub fn set_clip_mask_budget(&mut self, bytes: usize) {
        self.clip_masks.set_budget(bytes);
    }

    /// Whether an antialiased thin fill - twice its area over its perimeter
    /// under two pixels, its bounds spanning at most 65,536 - is drawn
    /// through a coverage mask, as it is by default: the fringes of its
    /// edges would over-ink it. The masks share an atlas that
    /// grows from a quarter of a megabyte to four as frames need and shrinks
    /// when a moving scene's masks need less, and at most 256 are made a
    /// frame; past either a thin fill is filled as any other.
    pub fn set_thin_fill_masks(&mut self, enabled: bool) {
        self.fill_masks.set_enabled(enabled);
    }

    /// Intersects the stencil clip plane of the current render target with
    /// `geometry`, the winding fans of a clip, under `fill_rule`.
    fn clip_to_stencil(&mut self, geometry: Rc<ClipGeometry>, bounds: Bounds, fill_rule: FillRule) {
        self.reconcile_current_clip_plane();
        let target = self.current_render_target;
        let target_rect = self.render_target_rect();
        let path_rect = Self::clip_bounds(bounds, target_rect);
        let previous_armed = self.clip_planes.get(&target).map_or(target_rect, |plane| plane.armed);

        if !self.clip_active() {
            self.emit_clip_reset(true);
        }
        self.emit_clip_fill(&geometry, fill_rule, previous_armed);
        let armed = previous_armed.intersect(path_rect);
        self.clip_stack.push(ClipEntry {
            target,
            kind: ClipKind::Stencil {
                geometry,
                fill_rule,
                bounds,
                prior_armed: previous_armed,
                armed,
            },
        });
        let plane = self.clip_planes.entry(target).or_insert(ClipPlaneState {
            count: 0,
            dirty: false,
            armed: target_rect,
        });
        plane.count += 1;
        plane.armed = armed;
        self.state_mut().clip_depth = self.clip_stack.len();
    }

    /// Takes the clip as a coverage mask for the fragment shader: the union
    /// of `paths`, each under its rule, cut by the mask already in force on
    /// the target. `false` - the budget has no room, or no image could be
    /// made - leaves the clip to the stencil.
    fn clip_to_mask(&mut self, paths: &[(&Path, FillRule)]) -> bool {
        if self.clip_masks.budget() == 0 {
            return false;
        }
        let transform = self.state().transform;
        // Flattened finer than a fill's outline, whose fringe hides what a
        // mask's exact area would show: the path's own cache, kept at the
        // fill's tolerance, is not used.
        let tolerance = self.tess_tol * MASK_TESSELLATION;
        let target = self.current_render_target;
        let (width, height) = self.render_target_size();
        let mut within = [0, 0, width as i32, height as i32];
        let parent = self.clip_stack.iter().rev().find_map(|entry| match &entry.kind {
            ClipKind::Mask { mask, origin } if entry.target == target => Some((mask.clone(), *origin)),
            _ => None,
        });
        if let Some((parent, at)) = &parent {
            // Outside the mask in force nothing is left to clip.
            let [parent_width, parent_height] = parent.size();
            within = [
                within[0].max(at[0]),
                within[1].max(at[1]),
                within[2].min(at[0] + parent_width as i32),
                within[3].min(at[1] + parent_height as i32),
            ];
        }
        // The clip bit for bit: asked for as before, it has its mask. It is
        // built where the last one was, so that one found again allocates
        // nothing.
        let mut request = std::mem::take(&mut self.clip_masks.request);
        request.clear();
        request.push(paths.len() as u32);
        for &(path, fill_rule) in paths {
            request.push(u32::from(fill_rule == FillRule::EvenOdd));
            path.words(&mut request);
        }
        request.extend(transform.0.map(f32::to_bits));
        request.extend([tolerance.to_bits(), self.dist_tol.to_bits()]);
        request.extend(within.map(|side| side as u32));
        if let Some((parent, at)) = &parent {
            request.extend([parent.id as u32, (parent.id >> 32) as u32, at[0] as u32, at[1] as u32]);
        }
        if let Some((mask, origin)) = self.clip_masks.asked(&request) {
            self.clip_masks.request = request;
            self.clip_stack.push(ClipEntry {
                target,
                kind: ClipKind::Mask { mask, origin },
            });
            self.state_mut().clip_depth = self.clip_stack.len();
            return true;
        }
        // A new request is remembered by a copy, the buffer kept for the next.
        let remembered = request.clone();
        self.clip_masks.request = request;
        let mut outline = MaskOutline::default();
        for &(path, fill_rule) in paths {
            let mut path_cache = path::PathCache::new(path.verbs(), &transform, tolerance, self.dist_tol);
            outline.child(fill_rule, path_cache.outlines(fill_rule));
        }
        let rect = outline.rect(within);
        let origin = [rect[0], rect[1]];
        let inside = parent
            .as_ref()
            .map(|(parent, at)| (parent.id, [origin[0] - at[0], origin[1] - at[1]]));
        let key = outline.key(rect, inside);
        if let Some((parent, at)) = &parent {
            if *at == origin && parent.key.same_outline(&key) {
                // The clip in force over again clips to no less.
                return true;
            }
        }
        let mask = match self.clip_masks.get(&key) {
            Some(mask) => mask,
            None => {
                let Some(dropped) = self.clip_masks.make_room(key.size()) else {
                    return false;
                };
                for image in dropped {
                    self.images.remove(&mut self.renderer, image);
                }
                let mut pixels = key.rasterize(&mut self.clip_masks.coverage);
                if let (Some((parent, _)), Some((_, offset))) = (&parent, inside) {
                    parent.cut(&mut pixels, key.size(), offset);
                }
                let [mask_width, mask_height] = key.size();
                let (image, uploaded) = match self.clip_masks.spare_image(key.size()) {
                    Some(image) => {
                        let source = ImageSource::Gray(imgref::Img::new(pixels.as_gray(), mask_width, mask_height));
                        (image, self.images.update(&mut self.renderer, image, source, 0, 0))
                    }
                    None => {
                        let [width, height] = mask::image_size(key.size());
                        let info = ImageInfo::new(ImageFlags::NEAREST, width, height, PixelFormat::Gray8);
                        let Ok(image) = self.images.alloc(&mut self.renderer, info) else {
                            return false;
                        };
                        // A new image is uploaded whole, the mask in its
                        // corner: a backend clears a texture whose first
                        // upload leaves part of it unwritten, and on wgpu
                        // five such textures cost every frame after 8 ms
                        // of a 70 ms frame of blurred layers.
                        let mut whole = vec![0; width * height];
                        for (row, line) in pixels.chunks_exact(mask_width).enumerate() {
                            whole[row * width..][..mask_width].copy_from_slice(line);
                        }
                        let source = ImageSource::Gray(imgref::Img::new(whole.as_gray(), width, height));
                        (image, self.images.update(&mut self.renderer, image, source, 0, 0))
                    }
                };
                if uploaded.is_err() {
                    self.images.remove(&mut self.renderer, image);
                    return false;
                }
                self.clip_masks.keep(key, image, pixels)
            }
        };
        self.clip_masks.remember(remembered, &mask, origin);
        self.clip_stack.push(ClipEntry {
            target,
            kind: ClipKind::Mask { mask, origin },
        });
        self.state_mut().clip_depth = self.clip_stack.len();
        true
    }

    /// Draws a thin fill (see [`fill`]) as a quad over its
    /// coverage mask in the atlas of fill masks. `false` - the frame has made
    /// its share of masks, or the atlas has no room - leaves the fill to its
    /// fringe. `transform` places the path and `paint_transform` the paint,
    /// as in `fill_path_internal`: a glyph drawn as an outline is placed by
    /// its own transform and painted by the run's.
    pub(crate) fn fill_through_mask(
        &mut self,
        path: &Path,
        path_cache: &mut PathCache,
        transform: &Transform2D,
        paint_transform: &Transform2D,
        paint_flavor: &PaintFlavor,
        fill_rule: FillRule,
    ) -> bool {
        let (width, height) = self.render_target_size();
        let within = [0, 0, width as i32, height as i32];
        let mut request = std::mem::take(&mut self.fill_masks.request);
        request.clear();
        request.push(u32::from(fill_rule == FillRule::EvenOdd));
        path.words(&mut request);
        request.extend(transform.0.map(f32::to_bits));
        request.extend([self.tess_tol.to_bits(), self.dist_tol.to_bits()]);
        request.extend(within.map(|side| side as u32));
        let mut mask = self.fill_masks.asked(&request);
        if self.fill_masks.image.is_none() {
            self.fill_masks.limit(self.renderer.max_texture_size());
        }
        // A fill whose mask the atlas would refuse - past the frame's share,
        // or larger than the atlas grows - builds no outline to find out.
        let b = path_cache.bounds;
        let r = mask::pixel_rect([b.minx, b.miny, b.maxx, b.maxy], within);
        let size = [(r[2] - r[0]) as usize, (r[3] - r[1]) as usize];
        if mask.is_none() && self.fill_masks.may_make(size) {
            mask = self.fill_mask(path_cache, fill_rule, within);
            if let Some(mask) = mask {
                self.fill_masks.remember(request.clone(), mask);
            }
        }
        self.fill_masks.request = request;
        let (Some(mask), Some(image)) = (mask, self.fill_masks.image) else {
            return false;
        };
        // The quad covers the mask's pixels exactly, each fragment reading
        // the texel of its own pixel.
        let [x0, y0, x1, y1] = mask.rect.map(|side| side as f32);
        let [width, height] = self.fill_masks.size().map(|side| side as f32);
        let [s0, t0] = [mask.atlas[0] as f32 / width, mask.atlas[1] as f32 / height];
        // A transposed mask runs down the atlas where the quad runs across.
        let [across, down] = if mask.transposed {
            [y1 - y0, x1 - x0]
        } else {
            [x1 - x0, y1 - y0]
        };
        let (s1, t1) = (s0 + across / width, t0 + down / height);
        let ((s_right, t_right), (s_below, t_below)) = if mask.transposed {
            ((s0, t1), (s1, t0))
        } else {
            ((s1, t0), (s0, t1))
        };
        let verts = [
            Vertex::new(x0, y0, s0, t0),
            Vertex::new(x1, y1, s1, t1),
            Vertex::new(x1, y0, s_right, t_right),
            Vertex::new(x0, y0, s0, t0),
            Vertex::new(x0, y1, s_below, t_below),
            Vertex::new(x1, y1, s1, t1),
        ];
        self.render_triangles(&verts, paint_transform, paint_flavor, GlyphTexture::AlphaMask(image));
        true
    }

    /// The mask of a fill not asked for before: found by its outline, or
    /// made where the atlas has room for it. The atlas is made once a mask
    /// has its place, as large as the mask needs: a first thin fill that is
    /// refused, or larger than the first atlas, makes none to no purpose.
    fn fill_mask(&mut self, path_cache: &mut PathCache, fill_rule: FillRule, within: [i32; 4]) -> Option<PlacedMask> {
        // The fill's own outline: flattened again 64 times finer, as a
        // clip's is, the corpus's thin fills came only 0.7 % nearer.
        let mut outline = MaskOutline::default();
        outline.child(fill_rule, path_cache.outlines(fill_rule));
        let rect = outline.rect(within);
        // A mask taller than it is wide is rasterized and kept transposed, so
        // that the rows written, copied and uploaded one at a time are long.
        let transposed = rect[3] - rect[1] > rect[2] - rect[0];
        let key = if transposed {
            outline.transpose();
            outline.key([rect[1], rect[0], rect[3], rect[2]], None)
        } else {
            outline.key(rect, None)
        };
        #[cfg(test)]
        {
            self.fill_masks.keyed += 1;
        }
        if let Some(atlas) = self.fill_masks.get(&key) {
            return Some(PlacedMask {
                atlas,
                rect,
                transposed,
            });
        }
        let atlas = loop {
            match self.fill_masks.place(key.size()) {
                Placement::At(at) => break at,
                Placement::Grow => {
                    // The masks made this frame go to the image their quads
                    // read before it is let go.
                    self.upload_fill_masks();
                    if !self.fill_masks.may_make(key.size()) {
                        return None;
                    }
                    self.fill_masks.grow(key.size());
                }
                Placement::Refused => return None,
            }
        };
        // An atlas that cannot be made places no more masks this frame: the
        // place just taken is never drawn, and the atlas starts over.
        if self.fill_mask_atlas().is_none() {
            self.fill_masks.fail();
            return None;
        }
        self.fill_masks.make(key, atlas);
        Some(PlacedMask {
            atlas,
            rect,
            transposed,
        })
    }

    /// The atlas of fill masks, made when the first mask has its place in
    /// it and uploaded whole, as a clip mask's image is, so that no backend
    /// clears it before each use.
    fn fill_mask_atlas(&mut self) -> Option<ImageId> {
        if let Some(image) = self.fill_masks.image {
            return Some(image);
        }
        let [width, height] = self.fill_masks.size();
        let info = ImageInfo::new(ImageFlags::NEAREST, width, height, PixelFormat::Gray8);
        let image = self.images.alloc(&mut self.renderer, info).ok()?;
        // Whole, the rows kept and blank ones below them: an atlas that grew
        // keeps the masks of the one before.
        let mut whole = vec![0; width * height];
        whole[..self.fill_masks.pixels.len()].copy_from_slice(&self.fill_masks.pixels);
        let source = ImageSource::Gray(imgref::Img::new(whole.as_gray(), width, height));
        if self.images.update(&mut self.renderer, image, source, 0, 0).is_err() {
            self.images.remove(&mut self.renderer, image);
            return None;
        }
        self.fill_masks.image = Some(image);
        Some(image)
    }

    /// Uploads the fill masks made since the last flush: the rectangle they
    /// span, in one update, before the commands that read them are drawn.
    pub(crate) fn upload_fill_masks(&mut self) {
        let Some(image) = self.fill_masks.image else {
            return;
        };
        let Some(([x, y], [width, height], rows)) = self.fill_masks.take_upload() else {
            return;
        };
        let source = ImageSource::Gray(imgref::Img::new(rows.as_gray(), width, height));
        // A failed upload leaves the frame's masks blank where their quads,
        // recorded already, read them: those thin fills are missing from
        // this frame, the later ones keep their fringe, and the atlas starts
        // over to make them again in the next.
        if self.images.update(&mut self.renderer, image, source, x, y).is_err() {
            self.fill_masks.fail();
        }
    }

    /// Takes the clip as a shape for the fragment shader, if `path` outlines
    /// one - or one cut by a box with square corners, as a rectangle rounded
    /// on one side is - and it combines with the shape already in force: a
    /// draw carries a single shape, and the cuts in the scissor's place.
    /// `false` leaves the clip to the stencil.
    fn clip_to_shape(&mut self, path: &Path) -> bool {
        let Some((shape, strays, cut)) = RoundedBox::fit_with_cut(path) else {
            return false;
        };
        let transform = self.state().transform;
        let mut shape = shape.transformed(&transform);
        let mut cut = cut.map(|cut| cut.transformed(&transform));
        let Some(mut coverage) = shape.coverage(self.fringe_width) else {
            return false;
        };
        // A unit of the path is no longer on the target than the longer of
        // the transform's axes.
        let scale = transform[0].hypot(transform[1]).max(transform[2].hypot(transform[3]));
        coverage = coverage.straying(strays * scale / self.fringe_width);
        let in_force = self.clip_shape();
        if let Some((current, current_coverage)) = in_force {
            if let Some(both) = current.intersection(&shape, self.fringe_width) {
                if both == current {
                    // The shape in force already clips to less.
                    (shape, coverage) = (current, current_coverage);
                } else {
                    let Some(both_coverage) = both.coverage(self.fringe_width) else {
                        return false;
                    };
                    // Each corner of what the two leave is a corner of one
                    // of them.
                    let strays = coverage.strays.max(current_coverage.strays);
                    (shape, coverage) = (both, both_coverage.straying(strays));
                }
            } else if coverage.contains(&current) {
                (shape, coverage) = (current, current_coverage);
            } else if !current_coverage.contains(&shape) {
                // The two make no one box. A box with square corners and
                // parallel sides cuts the other one, which is in force.
                let rect = |b: &RoundedBox| b.radii == [0.0, 0.0] && b.parallel(&current);
                let cutting = if rect(&shape) {
                    let cutting = shape;
                    (shape, coverage) = (current, current_coverage);
                    cutting
                } else if rect(&current) {
                    current
                } else {
                    return false;
                };
                cut = match cut {
                    Some(cut) => match cut.rect_intersection(&cutting) {
                        Some(both) => Some(both),
                        None => return false,
                    },
                    None => Some(cutting),
                };
            }
        }
        if let Some(this) = cut {
            // A cut thinner than a pixel is for the stencil, as such a
            // shape is; one the shape lies within cuts nothing; one has to
            // be parallel to the cuts in force to clip with them as a box.
            let Some(this_coverage) = this.coverage(self.fringe_width) else {
                return false;
            };
            if this_coverage.contains(&shape) {
                cut = None;
            } else if self.clip_cut().is_some_and(|cuts| !cuts.parallel(&this)) {
                return false;
            }
        }
        if cut.is_none() && in_force.is_some_and(|(current, _)| current == shape) {
            return true;
        }
        self.clip_stack.push(ClipEntry {
            target: self.current_render_target,
            kind: ClipKind::Shape { shape, coverage, cut },
        });
        self.state_mut().clip_depth = self.clip_stack.len();
        true
    }

    /// A path's winding fans under `transform`, for the stencil, and their bounds.
    fn stencil_geometry(&self, path: &Path, transform: &Transform2D) -> (Rc<ClipGeometry>, Bounds) {
        let path_cache = path.cache(transform, self.tess_tol, self.dist_tol);
        let geometry = ClipGeometry {
            vertices: path_cache.winding_triangles().into_boxed_slice(),
        };
        (Rc::new(geometry), path_cache.bounds)
    }

    /// The boxes in force as a draw meets them. The scissor is a box like
    /// the clip shape: where the two make one ([`RoundedBox::with_scissor`])
    /// that box stands for both, and no scissor is left beside it. Otherwise
    /// the scissor, cut by the cuts in force, clips beside the shape, along
    /// the sides of it that cut into the shape
    /// ([`RoundedBox::trimmed_scissor`]): an edge it shares with the shape
    /// is the shape's.
    fn clip_boxes(&mut self, shape: Option<(RoundedBox, ClipCoverage)>) -> ClipBoxes {
        let scissor = self.state().scissor;
        let cut = self.clip_cut();
        let of = (scissor, shape.map(|(shape, _)| shape), cut);
        if let Some(last) = self.last_clip_boxes.as_ref().filter(|last| last.of == of) {
            return *last;
        }
        let scissor_box = scissor.extent.map(|extent| RoundedBox {
            frame: scissor.transform,
            extent,
            radii: [scissor.radius; 2],
        });
        let scissor_box = match (scissor_box, cut) {
            (Some(scissor_box), Some(cut)) => match scissor_box.rect_intersection(&cut) {
                Some(both) => Some(both),
                None => {
                    // A rounded scissor, or one at an angle to the cuts:
                    // they clip on the stencil, and the shapes with them.
                    self.shapes_to_stencil();
                    return self.clip_boxes(None);
                }
            },
            (scissor_box, cut) => scissor_box.or(cut),
        };
        // The round corners a square scissor leaves are the shape's; a
        // rounded scissor's own are exact.
        let both = match (shape, scissor_box) {
            (Some((shape, coverage)), Some(scissor_box)) => {
                let strays = if scissor_box.radii == [0.0; 2] {
                    coverage.strays
                } else {
                    0.0
                };
                shape
                    .with_scissor(&scissor_box, self.fringe_width)
                    .map(|(both, both_coverage)| (both, both_coverage.straying(strays)))
            }
            _ => None,
        };
        let boxes = match both {
            Some(both) => ClipBoxes {
                of,
                shape: Some(both),
                scissor: Scissor::default(),
                scissor_box: None,
            },
            None => {
                let scissor_box = match (shape, scissor_box) {
                    (Some((shape, _)), Some(scissor_box)) => shape.trimmed_scissor(&scissor_box, self.fringe_width),
                    (_, scissor_box) => scissor_box,
                };
                ClipBoxes {
                    of,
                    shape,
                    scissor: scissor_box.map_or(Scissor::default(), |scissor_box| Scissor {
                        transform: scissor_box.frame,
                        extent: Some(scissor_box.extent),
                        radius: scissor_box.radii[0],
                    }),
                    scissor_box: scissor_box
                        .and_then(|scissor_box| Some((scissor_box, scissor_box.coverage(self.fringe_width)?))),
                }
            }
        };
        self.last_clip_boxes = Some(boxes);
        boxes
    }

    /// The clip shape and the scissor a draw over `bounds` carries; `bounds`
    /// is asked for only under a shape or a scissor. `outline` is the draw's
    /// path, flattened, and how far the draw reaches around it.
    pub(crate) fn draw_clip(
        &mut self,
        bounds: impl FnOnce() -> Bounds,
        outline: Option<(&PathCache, f32)>,
    ) -> DrawClip {
        self.fill_clip(bounds, outline, None).0
    }

    /// What clips a draw over `bounds`, and for an antialiased fill -
    /// `fill`, its path and transform - what to fill in the path's place
    /// when it is an upright rect ([`RectFill`]).
    ///
    /// A box - the shape, the scissor, or the one the two make - takes
    /// nothing from a draw it holds whole - by its bounds or, failing that,
    /// by the points of its outline - or from the rect such a fill shares
    /// with it, so an edge the draw has in common with the box is
    /// antialiased once. Such a draw carries no scissor, and no shape - or,
    /// after a draw that carried one, a coverage of one everywhere, so that
    /// the renderer goes on with the shader variant it has bound. A mask
    /// multiplies every draw under it. An operation that changes the
    /// destination where its source is transparent would change the pixels
    /// outside the clip too: before a draw that reaches them the shapes
    /// move to the stencil, and a mask takes each pixel whole or not at all.
    pub(crate) fn fill_clip(
        &mut self,
        bounds: impl FnOnce() -> Bounds,
        outline: Option<(&PathCache, f32)>,
        fill: Option<(&Path, &Transform2D)>,
    ) -> (DrawClip, Option<RectFill>) {
        if self.state().scissor.extent.is_none() && self.clip_stack.is_empty() {
            self.shape_carried = false;
            let clip = DrawClip {
                scissor: self.state().scissor,
                ..DrawClip::default()
            };
            return (clip, None);
        }
        let (shape, mask) = self.clips_in_force();
        let mask = mask.map(|mask| MaskCoverage {
            hard: !self.state().composite_operation.takes_coverage(),
            ..mask
        });
        let boxes = self.clip_boxes(shape);
        if boxes.shape.is_none() && boxes.scissor_box.is_none() {
            self.shape_carried = false;
            let clip = DrawClip {
                shape: None,
                scissor: boxes.scissor,
                mask,
            };
            return (clip, None);
        }
        let bounds = bounds();
        let fringe_width = self.fringe_width;
        let holds = |coverage: &ClipCoverage| {
            coverage.holds(&bounds)
                || outline.is_some_and(|(path, spread)| {
                    coverage.holds_outline(
                        &bounds,
                        path.positions_between_chords(),
                        spread / fringe_width,
                        path.straddle() / fringe_width,
                    )
                })
        };
        let mut scissor = boxes.scissor;
        // The fill as an upright rect: worked out for a box that cuts it, once.
        let mut upright: Option<Option<RoundedBox>> = None;
        let fill_rect = |upright: &mut Option<Option<RoundedBox>>| {
            *upright.get_or_insert_with(|| fill.and_then(|(path, transform)| RoundedBox::upright_rect(path, transform)))
        };
        let mut rect = None;
        if let Some((scissor_box, coverage)) = boxes.scissor_box {
            if holds(&coverage) {
                scissor = Scissor::default();
            } else {
                match fill_rect(&mut upright).and_then(|fill| scissor_box.rect_fill(&fill, self.fringe_width)) {
                    // The rect the fill shares with the scissor, which then
                    // clips no more; a shape still clips it.
                    Some(RectFill::Shared(both)) => {
                        (upright, rect, scissor) = (Some(Some(both)), Some(RectFill::Shared(both)), Scissor::default());
                    }
                    covered @ Some(RectFill::Covered(_)) if boxes.shape.is_none() => rect = covered,
                    _ => {}
                }
            }
        }
        let Some((shape, coverage)) = boxes.shape else {
            self.shape_carried = false;
            let clip = DrawClip {
                shape: None,
                scissor,
                mask,
            };
            return (clip, rect);
        };
        let held = match rect {
            Some(RectFill::Shared(both)) => coverage.holds(&both.bounds()),
            _ => holds(&coverage),
        };
        let shared = if held {
            None
        } else {
            fill_rect(&mut upright).and_then(|fill| shape.rect_fill(&fill, self.fringe_width))
        };
        let rect = shared.or(rect);
        if held || matches!(shared, Some(RectFill::Shared(_))) {
            let clip = DrawClip {
                shape: self.shape_carried.then_some(ClipCoverage::EVERYWHERE),
                scissor,
                mask,
            };
            return (clip, rect);
        }
        self.shape_carried = self.state().composite_operation.takes_coverage();
        if self.shape_carried {
            let clip = DrawClip {
                shape: Some(coverage),
                scissor,
                mask,
            };
            return (clip, rect);
        }
        self.shapes_to_stencil();
        // The scissor clips on its own again, as it was set.
        let clip = DrawClip {
            shape: None,
            scissor: self.state().scissor,
            mask,
        };
        (clip, None)
    }

    /// Moves the current render target's clip shapes, and their cuts, to the
    /// stencil. The shapes beneath the one in force contain it, but each
    /// would be in force once it is gone: all of them move.
    fn shapes_to_stencil(&mut self) {
        let target = self.current_render_target;
        let mut moved = false;
        for index in 0..self.clip_stack.len() {
            let ClipEntry {
                target: on,
                kind: ClipKind::Shape { shape, cut, .. },
            } = self.clip_stack[index]
            else {
                continue;
            };
            if on != target {
                continue;
            }
            let (geometry, bounds) = match cut {
                None => self.stencil_geometry(&shape.path(), &shape.frame),
                Some(cut) => {
                    // What the two cover, both convex: the shape's outline,
                    // flattened, cut by the box.
                    let outline = shape.path();
                    let outline = outline.cache(&shape.frame, self.tess_tol, self.dist_tol);
                    let mut path = Path::new();
                    for (i, [x, y]) in cut.cut_outline(outline.positions()).into_iter().enumerate() {
                        if i == 0 {
                            path.move_to(x, y);
                        } else {
                            path.line_to(x, y);
                        }
                    }
                    path.close();
                    self.stencil_geometry(&path, &Transform2D::identity())
                }
            };
            // The replay below works out what each entry leaves armed.
            self.clip_stack[index].kind = ClipKind::Stencil {
                geometry,
                fill_rule: FillRule::NonZero,
                bounds,
                prior_armed: Rect::default(),
                armed: Rect::default(),
            };
            let armed = self.render_target_rect();
            let plane = self.clip_planes.entry(target).or_insert(ClipPlaneState {
                count: 0,
                dirty: false,
                armed,
            });
            plane.count += 1;
            plane.dirty = true;
            moved = true;
        }
        if moved {
            self.reconcile_current_clip_plane();
        }
    }

    /// The shape and the mask in force on the current render target: the
    /// innermost of each.
    fn clips_in_force(&self) -> (Option<(RoundedBox, ClipCoverage)>, Option<MaskCoverage>) {
        let target = self.current_render_target;
        let (mut shape, mut mask) = (None, None);
        for entry in self.clip_stack.iter().rev().filter(|entry| entry.target == target) {
            match &entry.kind {
                ClipKind::Shape {
                    shape: box_, coverage, ..
                } if shape.is_none() => shape = Some((*box_, *coverage)),
                ClipKind::Mask { mask: image, origin } if mask.is_none() => {
                    let [width, height] = image.size();
                    let [image_width, image_height] = mask::image_size(image.size());
                    mask = Some(MaskCoverage {
                        image: image.image,
                        origin: [origin[0] as f32, origin[1] as f32],
                        size: [width as f32, height as f32],
                        texel: [1.0 / image_width as f32, 1.0 / image_height as f32],
                        hard: false,
                    });
                }
                _ => {}
            }
            if shape.is_some() && mask.is_some() {
                break;
            }
        }
        (shape, mask)
    }

    /// The device rect outside which the clip on the current render target's
    /// stencil leaves nothing, or `None` when the stencil doesn't clip.
    pub(crate) fn stencil_clip_reach(&self) -> Option<Bounds> {
        self.clip_planes
            .get(&self.current_render_target)
            .filter(|plane| plane.count != 0)
            .map(|plane| Bounds {
                minx: plane.armed.x,
                miny: plane.armed.y,
                maxx: plane.armed.x + plane.armed.w,
                maxy: plane.armed.y + plane.armed.h,
            })
    }

    /// The shape clip in force on the current render target, and its coverage.
    pub(crate) fn clip_shape(&self) -> Option<(RoundedBox, ClipCoverage)> {
        let target = self.current_render_target;
        self.clip_stack.iter().rev().find_map(|entry| match entry.kind {
            ClipKind::Shape { shape, coverage, .. } if entry.target == target => Some((shape, coverage)),
            _ => None,
        })
    }

    /// The cuts in force on the current render target, as one box: what all
    /// of them cover. They are parallel, as each was taken.
    pub(crate) fn clip_cut(&self) -> Option<RoundedBox> {
        let target = self.current_render_target;
        self.clip_stack
            .iter()
            .filter(|entry| entry.target == target)
            .filter_map(|entry| match entry.kind {
                ClipKind::Shape { cut, .. } => cut,
                ClipKind::Stencil { .. } | ClipKind::Mask { .. } => None,
            })
            .reduce(|both, cut| both.rect_intersection(&cut).unwrap_or(both))
    }

    /// Re-establishes the current render target's stencil clip plane from
    /// the stack: disarmed when no clip on it survives, otherwise reset to
    /// visible and re-intersected with the survivors (a few stencil-only
    /// draws, no color work).
    pub(crate) fn replay_clip_stack(&mut self) {
        let target = self.current_render_target;
        let entries: Vec<_> = self
            .clip_stack
            .iter()
            .filter(|entry| entry.target == target)
            .filter_map(|entry| match &entry.kind {
                ClipKind::Stencil {
                    geometry,
                    fill_rule,
                    bounds,
                    ..
                } => Some((geometry.clone(), *fill_rule, *bounds)),
                ClipKind::Shape { .. } | ClipKind::Mask { .. } => None,
            })
            .collect();
        if entries.is_empty() {
            self.emit_clip_reset(false);
            return;
        }
        self.emit_clip_reset(true);
        let mut previous_armed = self.render_target_rect();
        let target_rect = previous_armed;
        let mut armed_values = Vec::with_capacity(entries.len());
        for (geometry, fill_rule, bounds) in entries {
            self.emit_clip_fill(&geometry, fill_rule, previous_armed);
            let armed = previous_armed.intersect(Self::clip_bounds(bounds, target_rect));
            armed_values.push((previous_armed, armed));
            previous_armed = armed;
        }
        let stencils = self
            .clip_stack
            .iter_mut()
            .filter(|entry| entry.target == target)
            .filter_map(|entry| match &mut entry.kind {
                ClipKind::Stencil { prior_armed, armed, .. } => Some((prior_armed, armed)),
                ClipKind::Shape { .. } | ClipKind::Mask { .. } => None,
            });
        for ((prior_armed, armed), (prior, now)) in stencils.zip(armed_values) {
            *prior_armed = prior;
            *armed = now;
        }
        if let Some(plane) = self.clip_planes.get_mut(&target) {
            plane.armed = previous_armed;
        }
    }

    pub(crate) fn render_target_rect(&self) -> Rect {
        let (width, height) = self.render_target_size();
        Rect::new(0.0, 0.0, width, height)
    }

    pub(crate) fn clip_bounds(bounds: Bounds, target: Rect) -> Rect {
        if ![bounds.minx, bounds.miny, bounds.maxx, bounds.maxy]
            .into_iter()
            .all(f32::is_finite)
            || bounds.minx > bounds.maxx
            || bounds.miny > bounds.maxy
        {
            return Rect::default();
        }
        Rect::new(
            bounds.minx - 1.0,
            bounds.miny - 1.0,
            bounds.maxx - bounds.minx + 2.0,
            bounds.maxy - bounds.miny + 2.0,
        )
        .intersect(target)
    }

    /// Pushes a triangle strip over the whole current render target and
    /// returns its vertex range: the stencil quads that arm, disarm and
    /// resolve the clip plane must reach every pixel of the target, which
    /// for a layer store is the store, not the canvas.
    pub(crate) fn push_target_quad(&mut self) -> (usize, usize) {
        self.push_clip_quad(self.render_target_rect())
    }

    pub(crate) fn push_clip_quad(&mut self, rect: Rect) -> (usize, usize) {
        let offset = self.verts.len();
        let x1 = rect.x + rect.w;
        let y1 = rect.y + rect.h;
        self.verts.push(Vertex::new(rect.x, y1, 0.5, 1.0));
        self.verts.push(Vertex::new(x1, y1, 0.5, 1.0));
        self.verts.push(Vertex::new(rect.x, rect.y, 0.5, 1.0));
        self.verts.push(Vertex::new(x1, rect.y, 0.5, 1.0));
        (offset, 4)
    }

    pub(crate) fn emit_clip_reset(&mut self, visible: bool) {
        let mut cmd = Command::new(CommandType::ClipReset { visible });
        cmd.triangles_verts = Some(self.push_target_quad());
        self.append_cmd(cmd);
    }

    pub(crate) fn emit_clip_fill(&mut self, geometry: &ClipGeometry, fill_rule: FillRule, resolve: Rect) {
        let mut cmd = Command::new(CommandType::ClipFill);
        cmd.fill_rule = fill_rule;

        let offset = self.verts.len();
        self.verts.extend_from_slice(&geometry.vertices);
        if !geometry.vertices.is_empty() {
            cmd.drawables.push(Drawable {
                fill_verts: Some((offset, geometry.vertices.len())),
                ..Drawable::default()
            });
        }

        cmd.triangles_verts = Some(self.push_clip_quad(resolve));
        self.append_cmd(cmd);
    }
}

/// A clip only the stencil can take: the rect with a notch in its top side,
/// so it has the rect's bounds and is no box.
#[cfg(test)]
pub(crate) fn notched_rect(x: f32, y: f32, w: f32, h: f32) -> Path {
    let mut path = Path::new();
    path.move_to(x, y);
    path.line_to(x, y + h);
    path.line_to(x + w, y + h);
    path.line_to(x + w, y);
    path.line_to(x + w * 0.5, y + h * 0.25);
    path.close();
    path
}

/// `clear_rect` clears the whole stencil unless a clip is armed on the
/// target it clears, when only the winding bits may go: the command carries
/// that decision so a tiler takes its tile clear whenever it can.
#[test]
fn clear_rect_keeps_the_clip_plane_only_while_a_clip_is_armed() {
    let renderer = RecordingRenderer::default();
    let recorded_commands = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    // These are the stencil's workings: no clip is taken as a mask.
    canvas.set_clip_mask_budget(0);
    canvas.set_size(100, 100, 1.0);
    let keep_clips = |canvas: &mut Canvas<RecordingRenderer>| -> Vec<bool> {
        canvas.flush_to_output(());
        let commands = recorded_commands.borrow();
        commands
            .iter()
            .filter_map(|cmd| match cmd.cmd_type {
                CommandType::ClearRect { keep_clip, .. } => Some(keep_clip),
                _ => None,
            })
            .collect()
    };

    canvas.clear_rect(0, 0, 100, 100, Color::white());
    assert_eq!(
        keep_clips(&mut canvas),
        vec![false],
        "no clip: the whole stencil is cleared"
    );

    let clip = notched_rect(10.0, 10.0, 50.0, 50.0);
    canvas.save();
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.clear_rect(0, 0, 100, 100, Color::white());
    assert_eq!(
        keep_clips(&mut canvas),
        vec![true],
        "a clip on the screen survives the clear"
    );

    let image = canvas
        .create_image_empty(64, 64, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_render_target(RenderTarget::Image(image));
    canvas.clear_rect(0, 0, 64, 64, Color::white());
    assert_eq!(
        keep_clips(&mut canvas),
        vec![false],
        "the screen's clip does not gate an image target"
    );
    canvas.set_render_target(RenderTarget::Screen);
    canvas.restore();

    canvas.clear_rect(0, 0, 100, 100, Color::white());
    assert_eq!(
        keep_clips(&mut canvas),
        vec![false],
        "the clip is popped: the whole stencil is cleared again"
    );
}

#[test]
fn consecutive_clip_restores_replay_once_before_the_next_draw() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    // These are the stencil's workings: no clip is taken as a mask.
    canvas.set_clip_mask_budget(0);
    canvas.set_size(100, 100, 1.0);
    let clip = notched_rect(10.0, 10.0, 80.0, 80.0);

    for _ in 0..64 {
        canvas.save();
        canvas.clip_path(&clip, FillRule::NonZero);
    }
    canvas.flush_to_output(());
    for _ in 0..64 {
        canvas.restore();
    }
    assert!(canvas.commands.is_empty());

    canvas.fill_path(&clip, &Paint::color(Color::white()));
    assert_eq!(
        canvas
            .commands
            .iter()
            .filter(|command| matches!(command.cmd_type, CommandType::ClipReset { visible: false }))
            .count(),
        1
    );
    assert!(!canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::ClipFill)));
}

#[test]
fn same_size_set_size_does_not_replay_a_clip() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    // These are the stencil's workings: no clip is taken as a mask.
    canvas.set_clip_mask_budget(0);
    canvas.set_size(100, 100, 1.0);
    let clip = notched_rect(10.0, 10.0, 80.0, 80.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.flush_to_output(());

    canvas.set_size(100, 100, 1.0);
    assert!(!canvas
        .commands
        .iter()
        .any(|command| { matches!(command.cmd_type, CommandType::ClipFill | CommandType::ClipReset { .. }) }));

    canvas.set_size(120, 120, 1.0);
    assert!(!canvas
        .commands
        .iter()
        .any(|command| { matches!(command.cmd_type, CommandType::ClipFill | CommandType::ClipReset { .. }) }));
    canvas.fill_path(&clip, &Paint::color(Color::white()));
    assert_eq!(
        canvas
            .commands
            .iter()
            .filter(|command| matches!(command.cmd_type, CommandType::ClipFill))
            .count(),
        1
    );
}

#[test]
fn nested_clip_resolve_is_bounded_by_the_outer_clip() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    // These are the stencil's workings: no clip is taken as a mask.
    canvas.set_clip_mask_budget(0);
    canvas.set_size(100, 100, 1.0);
    let outer = notched_rect(10.0, 20.0, 30.0, 40.0);
    let inner = notched_rect(15.0, 25.0, 10.0, 10.0);
    canvas.clip_path(&outer, FillRule::NonZero);
    canvas.clip_path(&inner, FillRule::NonZero);

    let (resolve_start, resolve_len) = canvas
        .commands
        .iter()
        .filter(|command| matches!(command.cmd_type, CommandType::ClipFill))
        .nth(1)
        .and_then(|command| command.triangles_verts)
        .unwrap();
    assert_eq!(resolve_len, 4);
    let resolve = &canvas.verts[resolve_start..resolve_start + resolve_len];
    assert!(resolve.iter().all(|vertex| (9.0..=41.0).contains(&vertex.x)));
    assert!(resolve.iter().all(|vertex| (19.0..=61.0).contains(&vertex.y)));
}

#[test]
fn many_contour_clip_uses_one_winding_drawable() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    // These are the stencil's workings: no clip is taken as a mask.
    canvas.set_clip_mask_budget(0);
    canvas.set_size(100, 100, 1.0);
    let mut clip = Path::new();
    for inset in 0..256 {
        let inset = inset as f32 * 0.01;
        clip.rect(inset, inset, 10.0, 10.0);
    }

    canvas.clip_path(&clip, FillRule::NonZero);

    let command = canvas
        .commands
        .iter()
        .find(|command| matches!(command.cmd_type, CommandType::ClipFill))
        .unwrap();
    assert_eq!(command.drawables.len(), 1);
    assert_eq!(command.drawables[0].fill_verts.map(|(_, len)| len), Some(256 * 6));
}

#[test]
fn reallocating_an_image_marks_its_clip_plane_for_replay() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    // These are the stencil's workings: no clip is taken as a mask.
    canvas.set_clip_mask_budget(0);
    canvas.set_size(64, 64, 1.0);
    let image = canvas
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_render_target(RenderTarget::Image(image));
    let clip = notched_rect(0.0, 0.0, 16.0, 32.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.flush_to_output(());

    canvas
        .realloc_image(image, 48, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    assert_eq!(canvas.renderer.image_deletion_count, 1);
    assert!(canvas.clip_planes[&RenderTarget::Image(image)].dirty);
    canvas.fill_path(&clip, &Paint::color(Color::white()));
    assert!(!canvas.clip_planes[&RenderTarget::Image(image)].dirty);
    assert!(canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::ClipFill)));
}

#[test]
fn filtering_an_image_marks_its_clip_plane_for_replay() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    // These are the stencil's workings: no clip is taken as a mask.
    canvas.set_clip_mask_budget(0);
    canvas.set_size(64, 64, 1.0);
    let source = canvas
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let target = canvas
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_render_target(RenderTarget::Image(target));
    let clip = notched_rect(0.0, 0.0, 16.0, 32.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.flush_to_output(());

    canvas.filter_image(target, ImageFilter::identity(), source);
    assert!(canvas.clip_planes[&RenderTarget::Image(target)].dirty);
    canvas.fill_path(&clip, &Paint::color(Color::white()));

    let filter = canvas
        .commands
        .iter()
        .position(|command| matches!(command.cmd_type, CommandType::RenderFilteredImage { .. }))
        .unwrap();
    let replay = canvas
        .commands
        .iter()
        .rposition(|command| matches!(command.cmd_type, CommandType::ClipFill))
        .unwrap();
    assert!(replay > filter);
}

/// A filter pass draws into its own target image, never into the target a
/// clip gates, so it must be recorded ungated under an active clip: a backend
/// that stencil-tests it (OpenGL) would otherwise test the filter quad against
/// the target image's blank plane and drop the whole pass - every layer
/// filter and every blurred shadow drawn under a clip.
#[test]
fn filter_passes_are_not_gated_by_the_active_clip() {
    use renderer::CommandType;

    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    // These are the stencil's workings: no clip is taken as a mask.
    canvas.set_clip_mask_budget(0);
    canvas.set_size(100, 100, 1.0);
    let source = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let target = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();

    let clip = notched_rect(0.0, 0.0, 50.0, 50.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.filter_image(target, ImageFilter::gaussian_blur(2.0), source);
    let mut path = Path::new();
    path.rect(10.0, 10.0, 30.0, 30.0);
    canvas.fill_path(&path, &Paint::color(Color::rgb(255, 0, 0)));
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    let filter = commands
        .iter()
        .find(|c| matches!(c.cmd_type, CommandType::RenderFilteredImage { .. }))
        .expect("the filter pass is recorded");
    assert!(!filter.clip_active, "a filter pass is not gated by the clip");
    let fill = commands
        .iter()
        .find(|c| matches!(c.cmd_type, CommandType::ConvexFill { .. }))
        .expect("the fill is recorded");
    assert!(fill.clip_active, "the fill under the clip is gated");
}

#[test]
fn a_suppressed_draw_does_not_consume_a_dirty_clip_replay() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    // These are the stencil's workings: no clip is taken as a mask.
    canvas.set_clip_mask_budget(0);
    canvas.set_size(64, 64, 1.0);
    let mask = canvas
        .create_image_empty(1, 1, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();

    let outer = notched_rect(4.0, 4.0, 56.0, 56.0);
    canvas.clip_path(&outer, FillRule::NonZero);
    canvas.save();
    let inner = notched_rect(8.0, 8.0, 32.0, 32.0);
    canvas.clip_path(&inner, FillRule::NonZero);
    canvas.flush_to_output(());
    canvas.restore();
    assert!(canvas.clip_planes[&RenderTarget::Screen].dirty);

    canvas.set_transient_image_budget(0);
    let masked = LayerEffects::new().with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 64.0, 64.0);
    assert!(canvas.begin_layer(&masked));
    let mut rect = Path::new();
    rect.rect(0.0, 0.0, 64.0, 64.0);
    canvas.fill_path(&rect, &Paint::color(Color::white()));
    assert!(canvas.clip_planes[&RenderTarget::Screen].dirty);
    canvas.end_layer();

    canvas.fill_path(&rect, &Paint::color(Color::white()));
    assert!(!canvas.clip_planes[&RenderTarget::Screen].dirty);
    assert!(canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::ClipReset { visible: true })));
    assert!(canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::ClipFill)));
}

/// Balanced save / clip_path / restore past the limit records no clip and
/// no geometry, and reset() drops the entries past the limit with the
/// layers it discards.
#[cfg(test)]
#[test]
fn clips_past_the_depth_limit_cost_nothing_and_reset_clears_the_overflow() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    while canvas.state_stack.len() < MAX_STATE_DEPTH {
        canvas.save();
    }
    let clip = notched_rect(0.0, 0.0, 32.0, 64.0);
    for _ in 0..32 {
        canvas.save();
        canvas.clip_path(&clip, FillRule::NonZero);
        canvas.restore();
    }
    assert!(canvas.clip_stack.is_empty());
    assert!(canvas.clip_planes.is_empty());
    assert!(!canvas.saturated());

    for _ in 0..3 {
        canvas.save();
    }
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    assert_eq!(canvas.overflow.len(), 4);
    canvas.reset();
    assert!(!canvas.saturated());
    assert!(canvas.layers.is_empty());
    assert!(canvas.warned_overflow, "one warning per canvas");
}

/// What the draws of a flush carried as their clip shape, in order.
#[cfg(test)]
fn drawn_clips(commands: &[Command]) -> Vec<Option<ClipCoverage>> {
    commands
        .iter()
        .filter_map(|cmd| match &cmd.cmd_type {
            CommandType::ConvexFill { params }
            | CommandType::Stroke { params }
            | CommandType::Triangles { params }
            | CommandType::ConcaveFill {
                fill_params: params, ..
            }
            | CommandType::StencilStroke { params1: params, .. } => Some(params.clip),
            _ => None,
        })
        .collect()
}

/// What each draw of a flush carried: no shape, a coverage of one
/// everywhere, or a shape's.
#[cfg(test)]
fn carried(commands: &[Command]) -> Vec<&'static str> {
    drawn_clips(commands)
        .iter()
        .map(|clip| match clip {
            None => "none",
            Some(coverage) if *coverage == ClipCoverage::EVERYWHERE => "everywhere",
            Some(_) => "shape",
        })
        .collect()
}

#[cfg(test)]
fn stencil_clip_commands(commands: &[Command]) -> usize {
    commands
        .iter()
        .filter(|cmd| matches!(cmd.cmd_type, CommandType::ClipFill | CommandType::ClipReset { .. }))
        .count()
}

/// A rectangle, a rounded rectangle and an ellipse clip without the stencil:
/// the draws under them carry the shape, in device pixels, until the restore.
#[test]
fn a_box_clip_is_a_shape_on_the_draws_under_it() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(200, 200, 2.0);
    // No rect: a rect under a rect clip would be drawn as what they share.
    let fill = notched_rect(0.0, 0.0, 200.0, 200.0);
    let mut stroke = Path::new();
    stroke.move_to(0.0, 0.0);
    stroke.line_to(200.0, 200.0);
    let paint = Paint::color(Color::black());

    let mut rounded = Path::new();
    rounded.rounded_rect(20.0, 40.0, 80.0, 40.0, 10.0);
    let mut circle = Path::new();
    circle.circle(50.0, 50.0, 30.0);
    let mut rect = Path::new();
    rect.rect(20.0, 40.0, 80.0, 40.0);
    let half_pixel = 0.5; // the fringe at a device pixel ratio of two
    for (clip, extent, radii) in [
        (&rect, [20.0, 40.0], [0.0, 0.0]), // a rect's first side runs down
        (&rounded, [40.0, 20.0], [10.0, 10.0]),
        (&circle, [30.0, 30.0], [30.0, 30.0]),
    ] {
        canvas.save();
        canvas.translate(10.0, 0.0);
        canvas.clip_path(clip, FillRule::NonZero);
        assert!(!canvas.clip_active(), "the stencil plane is not armed");
        canvas.reset_transform();
        canvas.fill_path(&fill, &paint);
        canvas.stroke_path(&stroke, &paint);
        canvas.restore();
        canvas.fill_path(&fill, &paint);
        canvas.flush_to_output(());

        let commands = recorded.borrow();
        assert_eq!(stencil_clip_commands(&commands), 0);
        let clips = drawn_clips(&commands);
        let [Some(filled), Some(stroked), None] = clips[..] else {
            panic!("fill and stroke clipped, the fill after the restore not: {clips:?}");
        };
        assert_eq!(filled, stroked);
        for (got, want) in filled
            .extent
            .iter()
            .chain(&filled.radii)
            .zip(extent.iter().chain(&radii))
        {
            assert!((got - want / half_pixel).abs() < 1e-2, "{filled:?}");
        }
        // Taken under the translation, 10 further right: the point lies 15
        // units inside the boxes and 25 inside the circle, two pixels a unit.
        let (center_x, depth) = if radii[0] == 30.0 { (60.0, 50.0) } else { (70.0, 30.0) };
        assert!((filled.distance([center_x, 55.0]) + depth).abs() < 1e-2, "{filled:?}");
    }
}

/// A draw the shape holds whole carries no shape - a fill, a stroke with its
/// width and its miters, glyph quads, by their bounds or by the points of
/// their outline - and one that reaches past the shape carries it. After a
/// draw that carried the shape, a held one carries a coverage of one, for
/// the renderer to stay on the variant it has bound.
/// A vertex where the path put it takes no slack from the path's curves:
/// past a round corner's arc by more than the clip's slack, it is clipped,
/// though the chords of a circle in the same path straddle by more.
#[test]
fn a_vertex_takes_no_slack_from_the_paths_curves() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let mut clip = Path::new();
    clip.rounded_rect(10.0, 10.0, 80.0, 80.0, 20.0);
    canvas.clip_path(&clip, FillRule::NonZero);

    let mut path = Path::new();
    path.circle(50.0, 50.0, 25.0);
    // A twentieth of a pixel past the top left corner's arc.
    let tip = 30.0 - 20.05 * std::f32::consts::FRAC_1_SQRT_2;
    path.move_to(tip, tip);
    path.line_to(40.0, 30.0);
    path.line_to(30.0, 40.0);
    path.close();
    assert!(
        path.cache(&Transform2D::identity(), canvas.tess_tol, canvas.dist_tol)
            .straddle()
            > 0.05
    );
    canvas.fill_path(&path, &Paint::color(Color::black()));
    canvas.flush_to_output(());
    assert_eq!(carried(&recorded.borrow()), ["shape"]);
}

/// Held, a draw under an operation coverage cannot bound leaves the shape a
/// shape.
#[test]
fn a_draw_the_shape_holds_carries_no_shape() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    let mut clip = Path::new();
    clip.rounded_rect(10.0, 10.0, 80.0, 80.0, 20.0);
    canvas.clip_path(&clip, FillRule::NonZero);

    let triangle = |x: f32, y: f32, size: f32| {
        let mut path = Path::new();
        path.move_to(x, y);
        path.line_to(x + size, y);
        path.line_to(x, y + size);
        path.close();
        path
    };
    let line = |x0: f32, y0: f32, x1: f32, y1: f32, x2: f32, y2: f32| {
        let mut path = Path::new();
        path.move_to(x0, y0);
        path.line_to(x1, y1);
        path.line_to(x2, y2);
        path
    };
    let wide = paint.clone().with_line_width(6.0).with_line_join(LineJoin::Round);
    let mitered = paint.clone().with_line_width(6.0).with_miter_limit(4.0);
    let quad = |x: f32, y: f32| {
        [(0.0, 0.0), (8.0, 8.0), (8.0, 0.0), (0.0, 0.0), (0.0, 8.0), (8.0, 8.0)]
            .map(|(dx, dy)| Vertex::new(x + dx, y + dy, 0.0, 0.0))
    };
    let flavor = PaintFlavor::Color(Color::black());

    // The clip's outline set in by half the wide stroke's width.
    let mut inset = Path::new();
    inset.rounded_rect(13.0, 13.0, 74.0, 74.0, 17.0);

    // Held: inside the straight sides, clear of the corner arcs - and a
    // stroke along the clip's edge from inside, by its outline.
    canvas.fill_path(&triangle(30.0, 30.0, 40.0), &paint);
    canvas.stroke_path(&line(30.0, 40.0, 50.0, 60.0, 70.0, 40.0), &wide);
    canvas.render_triangles(&quad(40.0, 40.0), &Transform2D::identity(), &flavor, GlyphTexture::None);
    canvas.stroke_path(&inset, &wide);
    // Not held: past a side, in a corner the arc cuts, a stroke whose width
    // or whose miter reaches out - a miter's limit counts, drawn or not -
    // and a quad on the edge.
    canvas.fill_path(&triangle(30.0, 30.0, 70.0), &paint);
    canvas.fill_path(&triangle(11.0, 11.0, 10.0), &paint);
    canvas.stroke_path(&line(12.0, 40.0, 12.0, 50.0, 12.0, 60.0), &wide);
    canvas.stroke_path(&line(30.0, 16.0, 50.0, 22.0, 70.0, 16.0), &mitered);
    canvas.stroke_path(&inset, &mitered);
    canvas.render_triangles(&quad(86.0, 40.0), &Transform2D::identity(), &flavor, GlyphTexture::None);

    canvas.global_composite_operation(CompositeOperation::Copy);
    canvas.fill_path(&triangle(30.0, 30.0, 40.0), &paint);
    assert!(
        canvas.clip_shape().is_some() && !canvas.clip_active(),
        "held: nothing to bound"
    );
    canvas.fill_path(&triangle(30.0, 30.0, 70.0), &paint);
    assert!(
        canvas.clip_shape().is_none() && canvas.clip_active(),
        "past the shape: the stencil"
    );

    canvas.flush_to_output(());
    assert_eq!(
        carried(&recorded.borrow()),
        [
            "none",
            "none",
            "none",
            "none",
            "shape",
            "shape",
            "shape",
            "shape",
            "shape",
            "shape",
            "everywhere",
            "none"
        ],
        "four held, six not, and the two copies: one held, one on the stencil"
    );

    // Past the clip's restore a draw carries nothing again.
    canvas.reset();
    canvas.save();
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.fill_path(&triangle(30.0, 30.0, 70.0), &paint);
    canvas.fill_path(&triangle(30.0, 30.0, 40.0), &paint);
    canvas.restore();
    canvas.fill_path(&triangle(30.0, 30.0, 40.0), &paint);
    canvas.flush_to_output(());
    assert_eq!(carried(&recorded.borrow()), ["shape", "everywhere", "none"]);
}

/// An antialiased upright rect under an upright rect clip is drawn as the
/// rect the two share, with no clip: one convex fill with its fringe. Under
/// an upright clip with round corners that it covers it is drawn as the
/// clip: a quad without a fringe, a fringe's width around the clip, under
/// the clip's coverage. A fill without antialiasing, one that is no rect
/// and a rect across part of the rounded clip keep their outline, and their
/// fringe, and carry the shape. The rounded clip's twin lies inside it: it
/// keeps its outline and its fringe, and the clip takes nothing from it.
#[test]
fn an_upright_rect_under_an_upright_clip_is_what_they_share_or_the_clip_it_covers() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let drawn = renderer.last_verts.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    let rect = |x: f32, y: f32, w: f32, h: f32| {
        let mut path = Path::new();
        path.rect(x, y, w, h);
        path
    };
    let mut triangle = Path::new();
    triangle.move_to(0.0, 0.0);
    triangle.line_to(100.0, 0.0);
    triangle.line_to(0.0, 100.0);
    triangle.close();
    let bounds_drawn = |cmd: &Command| {
        let (start, count) = cmd.drawables[0].fill_verts.unwrap();
        let verts = &drawn.borrow()[start..start + count];
        let pick = |f: fn(f32, f32) -> f32, seed: f32, of: fn(&Vertex) -> f32| verts.iter().map(of).fold(seed, f);
        [
            pick(f32::min, f32::INFINITY, |v| v.x),
            pick(f32::min, f32::INFINITY, |v| v.y),
            pick(f32::max, f32::NEG_INFINITY, |v| v.x),
            pick(f32::max, f32::NEG_INFINITY, |v| v.y),
        ]
    };

    canvas.save();
    canvas.clip_path(&rect(10.0, 20.0, 80.0, 60.0), FillRule::NonZero);
    canvas.fill_path(&rect(0.0, 0.0, 100.0, 100.0), &paint);
    canvas.fill_path(&rect(50.0, 20.0, 80.0, 30.0), &paint);
    canvas.fill_path(&rect(0.0, 0.0, 100.0, 100.0), &paint.clone().with_anti_alias(false));
    canvas.fill_path(&triangle, &paint);
    canvas.restore();
    let mut rounded = Path::new();
    rounded.rounded_rect(10.0, 20.0, 80.0, 60.0, 15.0);
    canvas.clip_path(&rounded, FillRule::NonZero);
    canvas.fill_path(&rect(10.0, 20.0, 80.0, 60.0), &paint);
    canvas.fill_path(&rounded, &paint);
    canvas.fill_path(&rect(10.0, 20.0, 80.0, 30.0), &paint);
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    let fills: Vec<&Command> = commands
        .iter()
        .filter(|cmd| matches!(cmd.cmd_type, CommandType::ConvexFill { .. }))
        .collect();
    assert_eq!(
        carried(&commands),
        ["none", "none", "shape", "shape", "shape", "everywhere", "shape"],
        "what the clip cuts to a rect carries nothing, and what lies inside it a coverage of one"
    );
    let fringed: Vec<bool> = fills
        .iter()
        .map(|cmd| cmd.drawables[0].stroke_verts.is_some())
        .collect();
    assert_eq!(fringed, [true, true, false, true, false, true, true]);
    // A fringed fill's own vertices are a half fringe inside its outline.
    assert_eq!(
        bounds_drawn(fills[0]),
        [10.5, 20.5, 89.5, 79.5],
        "around: the clip's rect"
    );
    assert_eq!(
        bounds_drawn(fills[1]),
        [50.5, 20.5, 89.5, 49.5],
        "across: what both cover"
    );
    assert_eq!(
        bounds_drawn(fills[2]),
        [0.0, 0.0, 100.0, 100.0],
        "without antialiasing: as it is"
    );
    assert_eq!(
        bounds_drawn(fills[4]),
        [9.0, 19.0, 91.0, 81.0],
        "covering: a fringe around the clip"
    );
    assert_eq!(
        bounds_drawn(fills[6]),
        [10.5, 20.5, 89.5, 49.5],
        "across the top: its own outline"
    );
    assert!(
        canvas.clip_shape().is_some() && !canvas.clip_active(),
        "no stencil in any of this"
    );
}

/// A scissor and the clip shape that make one box reach a draw as that box
/// alone: a draw that crosses it carries the box and no scissor, one the
/// box holds neither. A scissor that makes no one box with the shape stays
/// a box of its own, for the draws it does not hold.
#[test]
fn a_scissor_and_the_shape_reach_a_draw_as_one_box() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    let mut triangle = Path::new();
    triangle.move_to(0.0, 0.0);
    triangle.line_to(100.0, 0.0);
    triangle.line_to(0.0, 100.0);
    triangle.close();
    let mut small = Path::new();
    small.move_to(30.0, 30.0);
    small.line_to(50.0, 30.0);
    small.line_to(30.0, 50.0);
    small.close();
    let mut clip = Path::new();
    clip.rounded_rect(10.0, 20.0, 80.0, 60.0, 15.0);
    let scissors = |commands: &[Command]| -> Vec<bool> {
        commands
            .iter()
            .filter_map(|cmd| match &cmd.cmd_type {
                CommandType::ConvexFill { params } => Some(params.scissor_mat != [0.0; 12]),
                _ => None,
            })
            .collect()
    };

    // The viewport of an export: a scissor with the clip's own bounds.
    canvas.scissor(10.0, 20.0, 80.0, 60.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.fill_path(&triangle, &paint);
    canvas.fill_path(&small, &paint);
    // A scissor across the clip's corners makes no one box with it.
    canvas.scissor(20.0, 20.0, 80.0, 60.0);
    canvas.fill_path(&triangle, &paint);
    canvas.fill_path(&small, &paint);
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    assert_eq!(carried(&commands), ["shape", "everywhere", "shape", "everywhere"]);
    assert_eq!(scissors(&commands), [false, false, true, false]);
    let carried = drawn_clips(&commands)[0].unwrap();
    assert_eq!(
        (carried.extent, carried.radii),
        ([40.0, 30.0], [15.0, 15.0]),
        "the clip, which the scissor holds"
    );
}

/// What a rounded clip's outline strays outside its box - cubic corners, a
/// tenth of a pixel off the circle at this size - gives a draw no way past
/// a scissor: not past the sides of the rect a scissor inside the clip
/// leaves, and not past a rounded scissor's corners, which are exact.
#[test]
fn a_scissor_takes_no_slack_from_the_clips_outline() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(1000, 1000, 1.0);
    let paint = Paint::color(Color::black());
    let mut clip = Path::new();
    clip.rounded_rect(100.0, 100.0, 800.0, 800.0, 400.0);
    let (_, strays) = RoundedBox::fit_outline(&clip).unwrap();
    assert!(strays > 0.08, "{strays}");
    let triangle = |reach: f32| {
        let mut path = Path::new();
        path.move_to(500.0, 400.0);
        path.line_to(reach, 500.0);
        path.line_to(500.0, 600.0);
        path.close();
        path
    };

    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.scissor(300.0, 300.0, 400.0, 400.0);
    canvas.fill_path(&triangle(700.0), &paint); // to the scissor's side
    canvas.fill_path(&triangle(700.06), &paint); // past it, by less than the outline strays
    canvas.rounded_scissor(100.0, 100.0, 800.0, 800.0, 400.0);
    canvas.fill_path(&clip, &paint); // the clip's twin, past the scissor's circle
    canvas.reset_scissor();
    canvas.fill_path(&clip, &paint); // and inside the clip alone
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    let clipped: Vec<bool> = commands
        .iter()
        .filter_map(|cmd| match &cmd.cmd_type {
            CommandType::ConvexFill { params }
            | CommandType::ConcaveFill {
                fill_params: params, ..
            } => Some(params.scissor_mat != [0.0; 12]),
            _ => None,
        })
        .zip(carried(&commands))
        .map(|(scissored, carried)| scissored || carried == "shape")
        .collect();
    assert_eq!(clipped, [false, true, true, false]);
}

/// The scissor meets a draw as a clip shape does: a draw it holds whole -
/// by its bounds, or by its outline as a rounded scissor's twin is -
/// carries none, an antialiased upright rect under an upright scissor is
/// filled as the rect the two share with no scissor, and one that covers a
/// rounded scissor as a quad without a fringe around it, under the scissor.
/// Everything else the scissor cuts carries it.
#[test]
fn a_scissor_meets_a_draw_as_a_clip_shape_does() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let drawn = renderer.last_verts.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    let rect = |x: f32, y: f32, w: f32, h: f32| {
        let mut path = Path::new();
        path.rect(x, y, w, h);
        path
    };
    let triangle = |x: f32, y: f32, size: f32| {
        let mut path = Path::new();
        path.move_to(x, y);
        path.line_to(x + size, y);
        path.line_to(x, y + size);
        path.close();
        path
    };
    let scissored = |cmd: &Command| match &cmd.cmd_type {
        CommandType::ConvexFill { params } | CommandType::Stroke { params } => Some(params.scissor_mat != [0.0; 12]),
        CommandType::StencilStroke { params1, .. } => Some(params1.scissor_mat != [0.0; 12]),
        _ => None,
    };
    let bounds_drawn = |cmd: &Command| {
        let (start, count) = cmd.drawables[0].fill_verts.unwrap();
        let verts = &drawn.borrow()[start..start + count];
        let pick = |f: fn(f32, f32) -> f32, seed: f32, of: fn(&Vertex) -> f32| verts.iter().map(of).fold(seed, f);
        [
            pick(f32::min, f32::INFINITY, |v| v.x),
            pick(f32::min, f32::INFINITY, |v| v.y),
            pick(f32::max, f32::NEG_INFINITY, |v| v.x),
            pick(f32::max, f32::NEG_INFINITY, |v| v.y),
        ]
    };
    let fringed = |cmd: &Command| cmd.drawables[0].stroke_verts.is_some();

    canvas.scissor(10.0, 20.0, 80.0, 60.0);
    canvas.fill_path(&triangle(10.0, 20.0, 40.0), &paint); // on the scissor's left side and top
    canvas.fill_path(&triangle(0.0, 20.0, 40.0), &paint); // across its left side
    canvas.fill_path(&rect(10.0, 20.0, 80.0, 60.0), &paint); // its twin
    canvas.fill_path(&rect(0.0, 0.0, 100.0, 100.0), &paint); // around it
    canvas.fill_path(&rect(50.0, 0.0, 80.0, 50.0), &paint); // across its corner
    canvas.fill_path(&rect(0.0, 0.0, 100.0, 100.0), &paint.clone().with_anti_alias(false));
    let mut line = Path::new();
    line.move_to(40.0, 50.0);
    line.line_to(60.0, 50.0);
    canvas.stroke_path(&line, &paint.clone().with_line_width(2.0)); // inside, with all a miter could reach
    canvas.stroke_path(&line, &paint.clone().with_line_width(30.0)); // its reach crosses the scissor
    canvas.rounded_scissor(10.0, 20.0, 80.0, 60.0, 15.0);
    canvas.fill_path(&rect(10.0, 20.0, 80.0, 60.0), &paint); // covers the rounded scissor
    canvas.fill_path(&rect(10.0, 20.0, 80.0, 30.0), &paint); // across its top
    canvas.fill_path(&rect(30.0, 30.0, 20.0, 20.0), &paint); // inside
    let mut twin = Path::new();
    twin.rounded_rect(10.0, 20.0, 80.0, 60.0, 15.0);
    canvas.fill_path(&twin, &paint); // its outline: inside, though its bounds are not
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    let draws: Vec<&Command> = commands.iter().filter(|cmd| scissored(cmd).is_some()).collect();
    let carried: Vec<bool> = draws.iter().map(|cmd| scissored(cmd).unwrap()).collect();
    assert_eq!(
        carried,
        [false, true, false, false, false, true, false, true, true, true, false, false]
    );
    assert!(fringed(draws[11]), "the rounded twin keeps its own fringe");
    assert_eq!(bounds_drawn(draws[2]), [10.5, 20.5, 89.5, 79.5], "the twin: as it is");
    assert_eq!(
        bounds_drawn(draws[3]),
        [10.5, 20.5, 89.5, 79.5],
        "around: the scissor's rect"
    );
    assert_eq!(
        bounds_drawn(draws[4]),
        [50.5, 20.5, 89.5, 49.5],
        "across: what both cover"
    );
    assert_eq!(
        bounds_drawn(draws[5]),
        [0.0, 0.0, 100.0, 100.0],
        "without antialiasing: as it is"
    );
    assert_eq!(
        bounds_drawn(draws[8]),
        [9.0, 19.0, 91.0, 81.0],
        "covering: a fringe around the scissor"
    );
    assert_eq!(
        [2usize, 3, 4, 8, 9].map(|index| fringed(draws[index])),
        [true, true, true, false, true],
        "the covering quad takes its edge from the scissor alone"
    );
    assert!(
        !canvas.clip_active() && canvas.clip_shape().is_none(),
        "a scissor is no clip"
    );
}

/// One shape per draw: a shape inside the one in force takes its place, one
/// around it adds nothing, a rect across a rect leaves their intersection,
/// and a circle across a rect is in force with the rect cutting it.
#[test]
fn nested_shape_clips_keep_one_shape_in_force() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let rect = |x: f32, y: f32, w: f32, h: f32| {
        let mut path = Path::new();
        path.rect(x, y, w, h);
        path
    };
    let fill = notched_rect(0.0, 0.0, 100.0, 100.0);
    let paint = Paint::color(Color::black());
    let half_width_in_force = |canvas: &mut Canvas<RecordingRenderer>| {
        canvas.fill_path(&fill, &paint);
        canvas.flush_to_output(());
        drawn_clips(&recorded.borrow()).last().unwrap().unwrap().extent[0]
    };

    canvas.clip_path(&rect(10.0, 10.0, 80.0, 80.0), FillRule::NonZero);
    canvas.save();
    canvas.clip_path(&rect(20.0, 20.0, 40.0, 40.0), FillRule::NonZero);
    assert_eq!(canvas.clip_stack.len(), 2, "inside: stacked");
    assert_eq!(half_width_in_force(&mut canvas), 20.0);
    canvas.clip_path(&rect(0.0, 0.0, 100.0, 100.0), FillRule::NonZero);
    assert_eq!(canvas.clip_stack.len(), 2, "around: nothing to add");
    assert_eq!(half_width_in_force(&mut canvas), 20.0);
    canvas.restore();
    assert_eq!(half_width_in_force(&mut canvas), 40.0, "the outer shape is back");

    // Overlapping rects with parallel sides: what both cover, x 50..90.
    canvas.clip_path(&rect(50.0, 50.0, 80.0, 80.0), FillRule::NonZero);
    assert_eq!(half_width_in_force(&mut canvas), 20.0);
    assert!(!canvas.clip_active(), "still no stencil");

    // A circle that overlaps the rect is no single shape with it: the rect
    // cuts the circle, which is in force.
    let mut circle = Path::new();
    circle.circle(50.0, 50.0, 30.0);
    canvas.clip_path(&circle, FillRule::NonZero);
    assert!(!canvas.clip_active(), "still no stencil");
    assert_eq!(half_width_in_force(&mut canvas), 30.0, "the circle in force");
    assert_eq!(
        drawn_scissor(recorded.borrow().last().unwrap()),
        Some([50.0, 50.0, 32.0, 32.0]),
        "the rect's sides that cut the circle, the others past its reach"
    );
    assert_eq!(stencil_clip_commands(&recorded.borrow()), 0);
}

/// Where a draw's scissor lies, as x, y, width and height, when its sides
/// lie along the device axes.
#[cfg(test)]
fn drawn_scissor(cmd: &Command) -> Option<[f32; 4]> {
    let params = match &cmd.cmd_type {
        CommandType::ConvexFill { params } | CommandType::Stroke { params } => params,
        CommandType::ConcaveFill { fill_params, .. } => fill_params,
        _ => return None,
    };
    if params.scissor_mat == [0.0; 12] {
        return None;
    }
    // The matrix takes device positions to the scissor's frame.
    let [a, b, _, _, c, d, _, _, x, y, ..] = params.scissor_mat;
    let device = Transform2D([a, b, c, d, x, y]).inverse();
    let [ex, ey] = params.scissor_ext;
    let corners = [[-ex, -ey], [ex, ey]].map(|[u, v]| device.transform_point(u, v));
    let (left, right) = (corners[0].0.min(corners[1].0), corners[0].0.max(corners[1].0));
    let (top, bottom) = (corners[0].1.min(corners[1].1), corners[0].1.max(corners[1].1));
    let upright = (a == 0.0 && d == 0.0) || (b == 0.0 && c == 0.0);
    upright.then_some([left, top, right - left, bottom - top].map(|v| (v * 1e3).round() / 1e3))
}

/// A rect clip that cuts a clip with round corners across its straight
/// sides - the content of a window, under the window - and a rectangle
/// rounded on one side - its title bar - clip without the stencil: the
/// shape is in force, and the rect cuts it in the scissor's place, along the
/// sides of it that cut into the shape. A scissor joins the cut; one at an
/// angle to it, or a draw that coverage cannot bound, sends the clip to the
/// stencil, as what both leave.
#[test]
fn a_rect_that_cuts_a_rounded_clip_clips_beside_it_in_the_scissors_place() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let drawn = renderer.last_verts.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    let fill = notched_rect(0.0, 0.0, 100.0, 100.0);
    let last = |canvas: &mut Canvas<RecordingRenderer>| {
        canvas.fill_path(&fill, &paint);
        canvas.flush_to_output(());
        let commands = recorded.borrow();
        let shape = drawn_clips(&commands).last().copied().flatten();
        (
            shape.map(|shape| (shape.extent, shape.radii)),
            drawn_scissor(commands.last().unwrap()),
        )
    };
    let mut window = Path::new();
    window.rounded_rect(10.0, 10.0, 80.0, 70.0, 12.0);
    let mut content = Path::new();
    content.rect(10.0, 30.5, 80.0, 70.0);

    canvas.save();
    canvas.clip_path(&window, FillRule::NonZero);
    canvas.clip_path(&content, FillRule::NonZero);
    assert!(!canvas.clip_active(), "no stencil");
    assert_eq!(
        last(&mut canvas),
        (Some(([40.0, 35.0], [12.0, 12.0])), Some([8.0, 30.5, 84.0, 51.5])),
        "the window in force; the content's top cuts it, its other sides are the window's"
    );
    canvas.scissor(0.0, 0.0, 50.0, 100.0);
    assert_eq!(
        last(&mut canvas),
        (Some(([40.0, 35.0], [12.0, 12.0])), Some([8.0, 30.5, 42.0, 51.5])),
        "a scissor joins the cut"
    );
    canvas.scissor(0.0, 0.0, 100.0, 60.0);
    assert_eq!(
        last(&mut canvas),
        (Some(([40.0, 14.75], [0.0, 0.0])), None),
        "the scissor and the cut leave a rect of the window's straight sides: one box"
    );
    canvas.reset_scissor();
    // A rect whose top lies on the cut: filled as the rect it shares with
    // the cut, whose top is its own edge, under the window and no scissor.
    let mut on_the_cut = Path::new();
    on_the_cut.rect(0.0, 30.5, 100.0, 90.0);
    canvas.fill_path(&on_the_cut, &paint);
    canvas.flush_to_output(());
    {
        let commands = recorded.borrow();
        let fill = commands.last().unwrap();
        assert_eq!(drawn_scissor(fill), None, "no scissor");
        assert!(drawn_clips(&commands).last().copied().flatten().is_some(), "the window");
        let (start, count) = fill.drawables[0].fill_verts.unwrap();
        let top = drawn.borrow()[start..start + count]
            .iter()
            .map(|v| v.y)
            .fold(f32::INFINITY, f32::min);
        assert_eq!(top, 31.0, "the shared rect's top, inset by half its fringe");
    }
    canvas.restore();
    assert_eq!(last(&mut canvas), (None, None), "restored: nothing clips");

    // A title bar: round top corners, a square bottom.
    let mut bar = Path::new();
    bar.rounded_rect_varying(10.0, 10.0, 80.0, 20.25, 8.0, 8.0, 0.0, 0.0);
    canvas.save();
    canvas.clip_path(&bar, FillRule::NonZero);
    assert!(!canvas.clip_active(), "no stencil");
    let (shape, scissor) = last(&mut canvas);
    assert_eq!(shape.map(|(_, radii)| radii), Some([8.0, 8.0]), "round corners");
    assert_eq!(
        scissor,
        Some([8.0, 8.0, 84.0, 22.25]),
        "the bar's bottom cuts the shape, which runs on past it"
    );

    // A scissor at an angle to the cut: the clip goes to the stencil.
    canvas.save();
    canvas.rotate(0.3);
    canvas.scissor(0.0, 0.0, 100.0, 100.0);
    canvas.reset_transform();
    assert_eq!(last(&mut canvas).0, None, "no shape");
    assert!(canvas.clip_active(), "on the stencil");
    canvas.restore();
    canvas.restore();

    // A draw that coverage cannot bound sends the clip to the stencil, as
    // what the window and its content both cover.
    canvas.clip_path(&window, FillRule::NonZero);
    canvas.clip_path(&content, FillRule::NonZero);
    canvas.global_composite_operation(CompositeOperation::Copy);
    canvas.fill_path(&fill, &paint);
    let stencils: Vec<Bounds> = canvas
        .clip_stack
        .iter()
        .filter_map(|entry| match entry.kind {
            ClipKind::Stencil { bounds, .. } => Some(bounds),
            ClipKind::Shape { .. } | ClipKind::Mask { .. } => None,
        })
        .collect();
    assert_eq!(stencils.len(), 2);
    let both = stencils[1];
    assert!(
        (both.miny - 30.5).abs() < 1e-3 && (both.maxy - 80.0).abs() < 1e-3,
        "the content cut by the window: {both:?}"
    );
}

/// A rounded rect nested in its twin, or in one a fraction of a pixel off -
/// the double clip design tools export - stays one shape.
#[test]
fn a_rounded_clip_nested_in_its_twin_stays_a_shape() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(100, 100, 1.0);
    let rounded = |x: f32, y: f32| {
        let mut path = Path::new();
        path.rounded_rect(x, y, 80.0, 60.0, 16.0);
        path
    };
    canvas.clip_path(&rounded(10.0, 10.0), FillRule::NonZero);
    canvas.clip_path(&rounded(10.0, 10.0), FillRule::NonZero);
    assert_eq!(canvas.clip_stack.len(), 1, "its twin adds nothing");
    // Shifted along a side, the two share a narrower box with the same
    // corners: the left ones of one, the right ones of the other.
    canvas.clip_path(&rounded(10.3, 10.0), FillRule::NonZero);
    canvas.clip_path(&rounded(14.0, 10.0), FillRule::NonZero);
    assert!(!canvas.clip_active());
    let (shape, _) = canvas.clip_shape().unwrap();
    assert!(
        (shape.extent[0] - 38.0).abs() < 1e-3 && (shape.radii[0] - 16.0).abs() < 1e-3,
        "{shape:?}"
    );

    canvas.clip_path(&rounded(18.0, 14.0), FillRule::NonZero);
    assert!(
        matches!(canvas.clip_stack.last().unwrap().kind, ClipKind::Mask { .. }) && !canvas.clip_active(),
        "shifted across a corner, what they share has corners of its own: a mask's, beside the shape"
    );
    assert!(canvas.clip_shape().is_some());
}

/// A composite operation that changes the destination where the source is
/// transparent cannot be held to a shape by coverage: the target's shapes
/// move to the stencil before the draw and stay there.
#[test]
fn a_draw_coverage_cannot_bound_moves_the_shapes_to_the_stencil() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let fill = notched_rect(0.0, 0.0, 100.0, 100.0);
    let paint = Paint::color(Color::black());
    let mut outer = Path::new();
    outer.circle(50.0, 50.0, 45.0);
    let mut inner = Path::new();
    inner.rect(40.0, 40.0, 20.0, 20.0);

    canvas.save();
    canvas.clip_path(&outer, FillRule::NonZero);
    canvas.clip_path(&inner, FillRule::NonZero);
    for operation in [
        CompositeOperation::SourceOver,
        CompositeOperation::Atop,
        CompositeOperation::DestinationOver,
        CompositeOperation::DestinationOut,
        CompositeOperation::Lighter,
        CompositeOperation::Xor,
    ] {
        canvas.global_composite_operation(operation);
        canvas.fill_path(&fill, &paint);
    }
    assert!(!canvas.clip_active(), "coverage bounds all of these");

    canvas.global_composite_operation(CompositeOperation::Copy);
    canvas.fill_path(&fill, &paint);
    assert_eq!(canvas.clip_planes[&RenderTarget::Screen].count, 2, "both shapes moved");
    assert_eq!(canvas.clip_shape(), None);
    canvas.global_composite_operation(CompositeOperation::SourceOver);
    canvas.fill_path(&fill, &paint);
    canvas.flush_to_output(());
    {
        let commands = recorded.borrow();
        let clips = drawn_clips(&commands);
        assert_eq!(clips.iter().filter(|clip| clip.is_some()).count(), 6);
        assert_eq!(
            clips[6..],
            [None, None],
            "the copy and what follows it are gated by the stencil"
        );
        assert_eq!(stencil_clip_commands(&commands), 3, "armed once, filled twice");
        let copy = commands
            .iter()
            .rposition(|cmd| matches!(cmd.cmd_type, CommandType::ClipFill))
            .unwrap();
        assert!(
            commands[copy + 1..].iter().all(|cmd| cmd.clip_active),
            "after the clip fills every draw is stencil-gated"
        );
    }

    canvas.restore();
    canvas.fill_path(&fill, &paint);
    assert!(
        canvas.clip_stack.is_empty() && !canvas.clip_active(),
        "the restore pops them as stencil clips"
    );

    for operation in [
        CompositeOperation::SourceIn,
        CompositeOperation::SourceOut,
        CompositeOperation::DestinationIn,
        CompositeOperation::DestinationAtop,
    ] {
        assert!(
            !CompositeOperationState::new(operation).takes_coverage(),
            "{operation:?}"
        );
    }
}

/// A shape belongs to the target it was taken on, as a stencil clip does:
/// it gates a layer's composite and not its content, and a shape taken
/// inside the layer gates only that.
#[test]
fn a_shape_clip_gates_only_draws_into_its_target() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let fill = notched_rect(0.0, 0.0, 100.0, 100.0);
    let paint = Paint::color(Color::black());
    let mut outer = Path::new();
    outer.circle(50.0, 50.0, 40.0);
    let mut inner = Path::new();
    inner.rect(60.0, 60.0, 30.0, 30.0);

    canvas.clip_path(&outer, FillRule::NonZero);
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    canvas.fill_path(&fill, &paint);
    canvas.clip_path(&inner, FillRule::NonZero);
    canvas.fill_path(&fill, &paint);
    canvas.end_layer();
    canvas.flush_to_output(());
    let clips = drawn_clips(&recorded.borrow());
    let [None, Some(in_layer), Some(composite)] = clips[..] else {
        panic!("{clips:?}");
    };
    assert_eq!(in_layer.radii, [0.0, 0.0], "the layer's own rect, in its store's space");
    assert_eq!(composite.radii, [40.0, 40.0], "the canvas's circle on the composite");

    // An image target's shape leaves with the image.
    let image = canvas
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_render_target(RenderTarget::Image(image));
    canvas.clip_path(&inner, FillRule::NonZero);
    assert_eq!(canvas.clip_stack.len(), 2);
    canvas.set_render_target(RenderTarget::Screen);
    canvas.delete_image(image);
    assert_eq!(canvas.clip_stack.len(), 1, "only the screen's circle is left");
}

/// A command whose draws carry a shape is bounded by the pixels the shape
/// reaches - its fringe included, cut to the target, and never less than a
/// pixel of it; a draw that carries none, or a coverage of one everywhere,
/// is not bounded.
#[test]
fn a_draw_under_a_shape_is_bounded_by_the_pixels_the_shape_reaches() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 80, 1.0);
    let paint = Paint::color(Color::black());
    let mut clip = Path::new();
    clip.rounded_rect(20.25, 30.5, 40.5, 20.0, 5.0);
    let mut dented = Path::new();
    dented.move_to(-8.0, -8.0);
    dented.line_to(108.0, -8.0);
    dented.line_to(108.0, 88.0);
    dented.line_to(-8.0, 88.0);
    dented.line_to(30.0, 40.0);
    dented.close();
    let mut triangle = Path::new();
    triangle.move_to(0.0, 0.0);
    triangle.line_to(90.0, 40.0);
    triangle.line_to(0.0, 70.0);
    triangle.close();
    let mut held = Path::new();
    held.rect(30.0, 35.0, 10.0, 10.0);
    let mut line = Path::new();
    line.move_to(0.0, 40.0);
    line.line_to(100.0, 40.0);

    canvas.fill_path(&dented, &paint); // before the clip
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.fill_path(&dented, &paint); // concave
    canvas.fill_path(&triangle, &paint); // convex
    canvas.stroke_path(&line, &paint.clone().with_line_width(3.0));
    canvas.fill_path(&held, &paint); // carries a coverage of one everywhere
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    let bounds: Vec<_> = commands
        .iter()
        .filter(|cmd| {
            !matches!(
                cmd.cmd_type,
                CommandType::SetRenderTarget(_) | CommandType::ClearRect { .. }
            )
        })
        .map(|cmd| cmd.clip_bounds([100, 80]))
        .collect();
    // The clip spans 20.25 to 60.75 and 30.5 to 50.5, and half a pixel of ramp.
    let reached = Some([19, 30, 43, 21]);
    assert_eq!(bounds, [None, reached, reached, reached, None]);
    let concave = commands
        .iter()
        .find(|cmd| cmd.clip_bounds([100, 80]).is_some())
        .unwrap();
    assert!(matches!(concave.cmd_type, CommandType::ConcaveFill { .. }));
    assert_eq!(
        concave.clip_bounds([40, 40]),
        Some([19, 30, 21, 10]),
        "cut to the target"
    );
    assert_eq!(
        concave.clip_bounds([10, 10]),
        Some([9, 9, 1, 1]),
        "a pixel of a target it misses"
    );
    assert_eq!(concave.clip_bounds([0, 0]), None);
}

/// The unclipped image blit would bypass a shape as it would the stencil.
#[test]
fn an_image_blit_under_a_shape_clip_takes_the_masked_path() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let image = canvas
        .create_image_empty(32, 32, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let mut clip = Path::new();
    clip.circle(16.0, 16.0, 12.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    let mut blit = Path::new();
    blit.rect(0.0, 0.0, 32.0, 32.0);
    let mut paint = Paint::image(image, 0.0, 0.0, 32.0, 32.0, 0.0, 1.0);
    paint.set_anti_alias(false);
    canvas.fill_path(&blit, &paint);
    canvas.flush_to_output(());
    let clips = drawn_clips(&recorded.borrow());
    assert!(matches!(clips[..], [Some(_)]), "{clips:?}");
    assert!(recorded.borrow().iter().all(|cmd| !matches!(
        cmd.cmd_type,
        CommandType::Triangles {
            params: Params {
                shader_type: ShaderType::TextureCopyUnclipped,
                ..
            }
        }
    )));
}

/// The mask each fill of a flush carried.
#[cfg(test)]
fn drawn_masks(commands: &[Command]) -> Vec<Option<MaskCoverage>> {
    commands
        .iter()
        .filter_map(|cmd| match &cmd.cmd_type {
            CommandType::ConvexFill { params }
            | CommandType::ConcaveFill {
                fill_params: params, ..
            } => Some(params.clip_mask),
            _ => None,
        })
        .collect()
}

/// A clip to a path that is no box is a coverage mask: no stencil command,
/// the draws under it carry the mask - where its corner lies, the pixels
/// it spans - until the restore, and the same outline a whole number of
/// pixels away is the same mask at another place. A fraction of a pixel
/// away it is another.
#[test]
fn a_path_clip_is_a_mask_found_again_by_its_outline() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    let fill = notched_rect(0.0, 0.0, 100.0, 100.0);
    let clip = notched_rect(10.25, 20.5, 40.0, 30.0);

    canvas.save();
    canvas.clip_path(&clip, FillRule::NonZero);
    assert!(matches!(canvas.clip_stack.last().unwrap().kind, ClipKind::Mask { .. }));
    assert!(!canvas.clip_active(), "no stencil");
    canvas.fill_path(&fill, &paint);
    canvas.restore();
    canvas.fill_path(&fill, &paint);
    canvas.save();
    canvas.translate(7.0, -3.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.fill_path(&fill, &paint);
    canvas.restore();
    assert_eq!(canvas.clip_masks.len(), 1);
    canvas.save();
    canvas.translate(0.5, 0.0);
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.restore();
    assert_eq!(canvas.clip_masks.len(), 2, "half a pixel away is another mask");
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    assert_eq!(stencil_clip_commands(&commands), 0);
    let masks = drawn_masks(&commands);
    let first = masks[0].expect("the fill under the clip carries its mask");
    assert_eq!(
        (first.origin, first.size, first.hard),
        ([9.0, 19.0], [43.0, 33.0], false),
        "the clip's bounds and a pixel around them"
    );
    assert_eq!(masks[1], None, "restored");
    let moved = masks[2].unwrap();
    assert_eq!(moved.image, first.image, "the same mask");
    assert_eq!((moved.origin, moved.size), ([16.0, 16.0], [43.0, 33.0]));
}

/// A clip asked for bit for bit as before - the same path, transform and
/// surroundings - has its mask by the request alone; moved by whole pixels
/// it asks anew and finds the mask by its outline; and a request that goes
/// a frame unasked is forgotten, as a mask no clip took is.
#[test]
fn a_clip_asked_for_as_before_has_its_mask_by_the_request() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(100, 100, 1.0);
    let clip = notched_rect(10.25, 20.5, 40.0, 30.0);
    let frame = |canvas: &mut Canvas<RecordingRenderer>, dx: f32, twice: bool| {
        for _ in 0..if twice { 2 } else { 1 } {
            canvas.save();
            canvas.translate(dx, 0.0);
            canvas.clip_path(&clip, FillRule::NonZero);
            assert!(matches!(canvas.clip_stack.last().unwrap().kind, ClipKind::Mask { .. }));
            canvas.restore();
        }
        let masks = (canvas.clip_masks.len(), canvas.clip_masks.asked_len());
        canvas.flush_to_output(());
        masks
    };
    assert_eq!(
        frame(&mut canvas, 0.0, true),
        (1, 1),
        "asked twice, one request and one mask"
    );
    assert_eq!(frame(&mut canvas, 0.0, false), (1, 1), "and the same a frame later");
    assert_eq!(
        frame(&mut canvas, 3.0, false),
        (1, 2),
        "three pixels on: another request, the same mask"
    );
    assert_eq!(
        frame(&mut canvas, 3.5, false),
        (2, 2),
        "half a pixel more: another mask, and the first request, a frame unasked, is forgotten"
    );
    assert_eq!(
        frame(&mut canvas, 3.5, false),
        (1, 1),
        "as are the first mask and the second request"
    );
    canvas.flush_to_output(());
    canvas.flush_to_output(());
    assert_eq!((canvas.clip_masks.len(), canvas.clip_masks.asked_len()), (0, 0));

    // A request is the clip bit for bit: another path at the same place,
    // the same path under another rule or inside another mask asks anew.
    canvas.clip_path(&clip, FillRule::NonZero);
    canvas.reset();
    canvas.clip_path(&clip, FillRule::EvenOdd);
    canvas.reset();
    canvas.clip_path(&notched_rect(10.25, 20.5, 40.0, 30.5), FillRule::NonZero);
    canvas.reset();
    canvas.clip_path(&notched_rect(0.0, 0.0, 90.0, 90.0), FillRule::NonZero);
    canvas.clip_path(&clip, FillRule::NonZero);
    assert_eq!(canvas.clip_masks.asked_len(), 5);
}

/// A mask nests in the one in force as what both cover, cut to its bounds;
/// its twin adds nothing; and beside a clip shape a draw carries both.
#[test]
fn masks_nest_and_ride_beside_a_shape() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    let fill = notched_rect(0.0, 0.0, 100.0, 100.0);
    let outer = notched_rect(10.0, 10.0, 60.0, 60.0);
    let inner = notched_rect(40.0, 40.0, 50.0, 50.0);

    canvas.clip_path(&outer, FillRule::NonZero);
    canvas.clip_path(&outer, FillRule::NonZero);
    assert_eq!(canvas.clip_stack.len(), 1, "its twin adds nothing");
    canvas.clip_path(&inner, FillRule::NonZero);
    assert_eq!(canvas.clip_stack.len(), 2);
    let ClipKind::Mask { mask, origin } = &canvas.clip_stack[1].kind else {
        panic!("a mask in a mask is a mask");
    };
    // The inner clip's bounds, 39..91, cut to the outer mask's, 9..71.
    assert_eq!((*origin, mask.size()), ([39, 39], [32, 32]));
    assert_eq!(mask.pixels[16 * 32 + 16], 255, "inside both");
    assert_eq!(mask.pixels[31 * 32 + 31], 0, "the outer mask's border");
    let mut rounded = Path::new();
    rounded.rounded_rect(20.0, 20.0, 60.0, 60.0, 10.0);
    canvas.clip_path(&rounded, FillRule::NonZero);
    canvas.fill_path(&fill, &paint);
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    assert_eq!(stencil_clip_commands(&commands), 0);
    assert_eq!(carried(&commands), ["shape"]);
    let mask = drawn_masks(&commands)[0].unwrap();
    assert_eq!((mask.origin, mask.size), ([39.0, 39.0], [32.0, 32.0]));
    // The draw is bounded by what both leave it: the mask spans 39 to 71,
    // the shape ends, ramp included, at 80.5.
    let bounds: Vec<_> = commands.iter().filter_map(|cmd| cmd.clip_bounds([100, 100])).collect();
    assert_eq!(bounds, [[39, 39, 32, 32]]);
    assert_eq!(commands.last().unwrap().clip_bounds([60, 50]), Some([39, 39, 21, 11]));
}

/// Several paths clip as one mask of their union; one path as that path
/// would alone; none as a mask of one pixel that nothing covers. Without a
/// budget for masks the paths are one clip on the stencil.
#[test]
fn clip_paths_take_one_mask_or_one_stencil_clip() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let rect = |x: f32, y: f32, w: f32, h: f32| {
        let mut path = Path::new();
        path.rect(x, y, w, h);
        path
    };
    let (left, right) = (rect(10.0, 10.0, 30.5, 40.0), rect(40.5, 10.0, 30.0, 40.0));

    canvas.save();
    canvas.clip_paths(&[(&left, FillRule::NonZero), (&right, FillRule::EvenOdd)]);
    assert_eq!(canvas.clip_stack.len(), 1);
    let ClipKind::Mask { mask, origin } = &canvas.clip_stack[0].kind else {
        panic!("the union of two boxes is a mask");
    };
    assert_eq!((*origin, mask.size()), ([9, 9], [63, 42]));
    assert_eq!(
        &mask.pixels[20 * 63 + 29..][..5],
        [255; 5],
        "no seam where the two meet, at 40.5"
    );
    canvas.restore();

    canvas.save();
    canvas.clip_paths(&[(&left, FillRule::NonZero)]);
    assert!(canvas.clip_shape().is_some(), "one box is a shape");
    canvas.restore();

    canvas.save();
    canvas.clip_paths(&[]);
    let ClipKind::Mask { mask, .. } = &canvas.clip_stack[0].kind else {
        panic!("no path is a mask");
    };
    assert_eq!((mask.size(), mask.pixels.as_slice()), ([1, 1], &[0u8][..]));
    canvas.restore();

    canvas.set_clip_mask_budget(0);
    canvas.clip_paths(&[(&left, FillRule::NonZero), (&right, FillRule::EvenOdd)]);
    assert!(canvas.clip_active(), "no budget: the stencil");
    canvas.fill_path(&notched_rect(0.0, 0.0, 100.0, 100.0), &Paint::color(Color::black()));
    canvas.flush_to_output(());
    let commands = recorded.borrow();
    assert_eq!(
        stencil_clip_commands(&commands),
        2,
        "armed, and filled once with both paths"
    );
    let fans = commands
        .iter()
        .find(|cmd| matches!(cmd.cmd_type, CommandType::ClipFill))
        .and_then(|cmd| cmd.drawables[0].fill_verts)
        .unwrap();
    assert_eq!(fans.1, 12, "two triangles a rect, two rects");
    assert_eq!(drawn_masks(&commands), [None]);
}

/// Masks are kept within their budget: a clip the budget has no room for
/// goes to the stencil, and once a frame has passed without it a mask no
/// clip holds makes room - its image serves the mask that takes its place.
#[test]
fn a_mask_the_budget_has_no_room_for_goes_to_the_stencil() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(100, 100, 1.0);
    // One mask of 43 x 33 pixels: those, and an image of 64 x 64.
    canvas.set_clip_mask_budget(43 * 33 + 64 * 64);
    let first = notched_rect(10.25, 20.5, 40.0, 30.0);
    let second = notched_rect(10.5, 20.5, 40.0, 30.0);
    let image_of = |canvas: &Canvas<RecordingRenderer>| match &canvas.clip_stack[0].kind {
        ClipKind::Mask { mask, .. } => Some(mask.image),
        _ => None,
    };

    canvas.save();
    canvas.clip_path(&first, FillRule::NonZero);
    let first_image = image_of(&canvas).expect("a mask");
    canvas.restore();
    canvas.save();
    canvas.clip_path(&second, FillRule::NonZero);
    assert!(
        matches!(canvas.clip_stack[0].kind, ClipKind::Stencil { .. }),
        "the first mask was used in this frame"
    );
    canvas.restore();
    canvas.flush_to_output(());

    let (made, deleted) = (
        canvas.renderer.image_allocation_attempts,
        canvas.renderer.image_deletion_count,
    );
    canvas.save();
    canvas.clip_path(&second, FillRule::NonZero);
    assert_eq!(
        image_of(&canvas),
        Some(first_image),
        "a frame later, in the first mask's image"
    );
    assert_eq!(
        (
            canvas.renderer.image_allocation_attempts,
            canvas.renderer.image_deletion_count
        ),
        (made, deleted),
        "no image was made or deleted for it"
    );
    assert_eq!(canvas.clip_masks.len(), 1);
    canvas.restore();

    // An image that cannot be made leaves the clip to the stencil too.
    canvas.set_clip_mask_budget(1 << 20);
    canvas.renderer.fail_image_allocations = true;
    canvas.clip_path(&notched_rect(10.0, 20.0, 80.0, 70.0), FillRule::NonZero);
    assert!(matches!(canvas.clip_stack[0].kind, ClipKind::Stencil { .. }));
    assert_eq!(canvas.clip_masks.len(), 1);

    // Two frames after the last clip took it a mask is let go, and a frame
    // after that its image.
    canvas.renderer.fail_image_allocations = false;
    canvas.reset();
    canvas.flush_to_output(());
    canvas.flush_to_output(());
    assert_eq!(canvas.clip_masks.len(), 0);
    assert_eq!(canvas.renderer.image_deletion_count, deleted);
    canvas.flush_to_output(());
    assert_eq!(canvas.renderer.image_deletion_count, deleted + 1);
}

/// An operation that changes the destination where its source is
/// transparent takes a mask whole or not at all - the draw says so - and
/// leaves it a mask: the stencil is not involved.
#[test]
fn a_draw_coverage_cannot_bound_takes_a_mask_whole() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    let fill = notched_rect(0.0, 0.0, 100.0, 100.0);
    canvas.clip_path(&notched_rect(10.0, 10.0, 60.0, 60.0), FillRule::NonZero);
    canvas.fill_path(&fill, &paint);
    canvas.global_composite_operation(CompositeOperation::Copy);
    canvas.fill_path(&fill, &paint);
    canvas.global_composite_operation(CompositeOperation::SourceOver);
    canvas.fill_path(&fill, &paint);
    canvas.flush_to_output(());

    let commands = recorded.borrow();
    assert_eq!(stencil_clip_commands(&commands), 0);
    let hard: Vec<bool> = drawn_masks(&commands).iter().map(|mask| mask.unwrap().hard).collect();
    assert_eq!(hard, [false, true, false]);
}

/// A ring thinner than a pixel, both circles wound alike, covers the ring's
/// area under even-odd, as a clip's mask and as a thin fill's: added up
/// before the rule folds it, the winding counted the hole into each pixel
/// the ring crosses.
#[test]
fn an_even_odd_ring_wound_alike_holds_its_area() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(100, 100, 1.0);
    let mut ring = Path::new();
    ring.circle(50.0, 50.0, 30.0);
    ring.circle(50.0, 50.0, 29.6);
    let area = std::f32::consts::PI * (30.0f32.powi(2) - 29.6f32.powi(2));
    let ink = |pixels: &[u8]| pixels.iter().map(|&p| f32::from(p) / 255.0).sum::<f32>();

    canvas.fill_path(&ring, &Paint::color(Color::black()).with_fill_rule(FillRule::EvenOdd));
    let filled = ink(&canvas.fill_masks.pixels);
    assert!(
        (filled - area).abs() < 0.02 * area,
        "the fill's mask: {filled:.1} for {area:.1}"
    );

    canvas.clip_path(&ring, FillRule::EvenOdd);
    let ClipKind::Mask { mask, .. } = &canvas.clip_stack[0].kind else {
        panic!("a ring is a mask");
    };
    let clipped = ink(&mask.pixels);
    assert!(
        (clipped - area).abs() < 0.02 * area,
        "the clip's mask: {clipped:.1} for {area:.1}"
    );
}

/// A strip `width` across along (10, 10) to (60, 40), shifted by `dx`.
#[cfg(test)]
fn thin_strip(dx: f32, width: f32) -> Path {
    let (x0, y0, x1, y1) = (10.0 + dx, 10.0, 60.0 + dx, 40.0);
    let length = f32::hypot(x1 - x0, y1 - y0);
    let (nx, ny) = (-(y1 - y0) / length * width / 2.0, (x1 - x0) / length * width / 2.0);
    let mut path = Path::new();
    path.move_to(x0 + nx, y0 + ny);
    path.line_to(x1 + nx, y1 + ny);
    path.line_to(x1 - nx, y1 - ny);
    path.line_to(x0 - nx, y0 - ny);
    path.close();
    path
}

#[cfg(test)]
fn mask_quads(commands: &[Command], atlas: Option<ImageId>) -> usize {
    commands
        .iter()
        .filter(|command| matches!(command.glyph_texture, GlyphTexture::AlphaMask(id) if Some(id) == atlas))
        .count()
}

/// A thin fill that has not changed, or has moved by whole pixels, finds its
/// mask again: the second frame rasterizes nothing.
#[test]
fn a_thin_fill_is_rasterized_once() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    for dx in [0.0, 0.0, 3.0] {
        canvas.fill_path(&thin_strip(dx, 0.3), &paint);
        canvas.flush_to_output(());
        assert_eq!(mask_quads(&recorded.borrow(), canvas.fill_masks.image), 1);
    }
    assert_eq!(canvas.fill_masks.rasterized, 1);

    // A fraction of a pixel over is another mask.
    canvas.fill_path(&thin_strip(0.5, 0.3), &paint);
    canvas.flush_to_output(());
    assert_eq!(canvas.fill_masks.rasterized, 2);
}

/// At most 256 masks are made a frame; past them a thin fill is filled.
#[test]
fn a_frame_makes_its_share_of_masks() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    // Each strip a tenth of a pixel longer than the last: its own outline.
    for i in 0..300 {
        let mut strip = Path::new();
        strip.move_to(10.0, 10.0);
        strip.line_to(30.0 + i as f32 * 0.1, 10.2);
        strip.line_to(30.0 + i as f32 * 0.1, 10.5);
        strip.line_to(10.0, 10.3);
        strip.close();
        canvas.fill_path(&strip, &paint);
    }
    canvas.flush_to_output(());
    let commands = recorded.borrow();
    assert_eq!(mask_quads(&commands, canvas.fill_masks.image), 256);
    assert_eq!(fills(&commands), 44);
    assert_eq!(canvas.fill_masks.keyed, 256, "the 44 past the share build no outline");
}

/// One still thin fill among moving ones finds its own mask every frame, but
/// each frame makes more masks than it finds: the atlas starts over every
/// other frame and stays as small as a frame's masks need.
#[test]
fn a_still_thin_fill_among_moving_ones_does_not_hold_the_atlas() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(1000, 100, 1.0);
    let paint = Paint::color(Color::black());
    let mut most = 0;
    for frame in 0..400 {
        canvas.fill_path(&thin_strip(0.0, 0.3), &paint);
        for i in 1..=3 {
            // Each at its own speed, so that no two share a mask.
            canvas.fill_path(
                &thin_strip(60.0 * i as f32 + frame as f32 * (0.26 + 0.11 * i as f32), 0.3),
                &paint,
            );
        }
        canvas.flush_to_output(());
        most = most.max(canvas.fill_masks.held());
    }
    assert_eq!(canvas.fill_masks.size(), [512, 512]);
    assert!(most <= 8, "{most} masks held at most");
}

/// An atlas whose image cannot be made places no more masks in the frame:
/// its thin fills keep their fringe, none past the first builds an outline,
/// and the next frame makes the atlas and the masks.
#[test]
fn an_atlas_that_cannot_be_made_leaves_the_frame_to_the_fringe() {
    let renderer = RecordingRenderer {
        fail_image_allocations: true,
        ..Default::default()
    };
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    let draw = |canvas: &mut Canvas<RecordingRenderer>| {
        canvas.fill_path(&thin_strip(0.0, 0.3), &paint);
        canvas.fill_path(&thin_strip(20.0, 0.3), &paint);
        canvas.flush_to_output(());
    };
    draw(&mut canvas);
    assert_eq!(fills(&recorded.borrow()), 2);
    assert_eq!(canvas.fill_masks.keyed, 1, "the second builds no outline");
    canvas.renderer.fail_image_allocations = false;
    draw(&mut canvas);
    assert_eq!(mask_quads(&recorded.borrow(), canvas.fill_masks.image), 2);
}

/// An upload that fails before the atlas grows leaves the mask made before it
/// blank in its frame - its quad is recorded already - and the frame's later
/// thin fills to their fringe, the atlas not grown; the next frame makes them
/// all again.
#[test]
fn a_failed_upload_leaves_the_rest_of_the_frame_to_the_fringe() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(1000, 100, 1.0);
    let paint = Paint::color(Color::black());
    let sliver = |x: f32, y: f32, length: f32| {
        let mut path = Path::new();
        path.rect(x, y, length, 0.4);
        path
    };
    let quads = |commands: &[Command]| {
        commands
            .iter()
            .filter(|command| matches!(command.glyph_texture, GlyphTexture::AlphaMask(_)))
            .count()
    };
    canvas.fill_path(&sliver(10.0, 50.0, 40.0), &paint);
    canvas.flush_to_output(());
    let frame = |canvas: &mut Canvas<RecordingRenderer>| {
        canvas.fill_path(&sliver(100.0, 50.0, 41.0), &paint);
        // Wider than the first atlas: it grows, uploading the one before.
        canvas.fill_path(&sliver(10.0, 10.0, 700.0), &paint);
        canvas.fill_path(&sliver(10.0, 80.0, 30.0), &paint);
        canvas.flush_to_output(());
    };
    canvas.renderer.fail_image_updates = true;
    frame(&mut canvas);
    assert_eq!((quads(&recorded.borrow()), fills(&recorded.borrow())), (1, 2));
    assert_eq!(canvas.fill_masks.size(), [512, 512], "not grown");
    canvas.renderer.fail_image_updates = false;
    frame(&mut canvas);
    assert_eq!((quads(&recorded.borrow()), fills(&recorded.borrow())), (3, 0));
}

/// The atlas starts small and doubles when a mask finds no room in it, in
/// the frame: the masks it held keep their places, the quads drawn before
/// read the image it outgrew, freed once the frame is drawn, and the next
/// frame finds every mask in the new one.
#[test]
fn the_atlas_grows_when_a_mask_finds_no_room() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(1000, 100, 1.0);
    let paint = Paint::color(Color::black());
    let mut small = Path::new();
    small.rect(10.0, 50.0, 40.0, 0.4);
    let mut bar = Path::new();
    bar.rect(10.0, 10.0, 700.0, 0.4);
    let images = |commands: &[Command]| -> Vec<ImageId> {
        commands
            .iter()
            .filter_map(|command| match command.glyph_texture {
                GlyphTexture::AlphaMask(id) => Some(id),
                _ => None,
            })
            .collect()
    };
    let deleted = canvas.renderer.image_deletion_count;
    canvas.fill_path(&small, &paint);
    canvas.fill_path(&bar, &paint);
    canvas.flush_to_output(());
    let atlas = canvas.fill_masks.image;
    let drawn = images(&recorded.borrow());
    assert_eq!(drawn.len(), 2, "both through masks");
    assert_ne!(Some(drawn[0]), atlas, "the first quad reads the atlas outgrown");
    assert_eq!(Some(drawn[1]), atlas);
    assert_eq!(canvas.fill_masks.size(), [1024, 512], "wider, not taller");
    assert_eq!(
        canvas.renderer.image_deletion_count,
        deleted + 1,
        "the outgrown atlas freed"
    );

    canvas.fill_path(&small, &paint);
    canvas.fill_path(&bar, &paint);
    canvas.flush_to_output(());
    assert_eq!(images(&recorded.borrow()), [atlas.unwrap(); 2]);
    assert_eq!(canvas.fill_masks.rasterized, 2, "nothing made again");
}

/// The atlas stays within the backend's largest image: past it a mask keeps
/// its fringe, and the atlas grows only as far as the backend allows.
#[test]
fn the_atlas_stays_within_the_backends_largest_image() {
    let renderer = RecordingRenderer {
        max_texture_size: 600,
        ..Default::default()
    };
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(1000, 100, 1.0);
    let paint = Paint::color(Color::black());
    let mut too_wide = Path::new();
    too_wide.rect(10.0, 10.0, 700.0, 0.4);
    let mut wide = Path::new();
    wide.rect(10.0, 50.0, 560.0, 0.4);
    canvas.fill_path(&too_wide, &paint);
    canvas.fill_path(&wide, &paint);
    canvas.flush_to_output(());
    let image = canvas.fill_masks.image;
    assert_eq!(mask_quads(&recorded.borrow(), image), 1, "the wide one only");
    assert_eq!(canvas.fill_masks.size(), [600, 512]);
}

/// A scene in motion makes masks that no later frame asks for: the atlas
/// starts over after each such frame instead of filling with them, and a
/// still one keeps its mask.
#[test]
fn a_moving_thin_fill_does_not_fill_the_atlas() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    for frame in 0..10 {
        canvas.fill_path(&thin_strip(frame as f32 * 0.37, 0.3), &paint);
        canvas.flush_to_output(());
    }
    assert_eq!(canvas.fill_masks.held(), 0);
    for _ in 0..3 {
        canvas.fill_path(&thin_strip(0.0, 0.3), &paint);
        canvas.flush_to_output(());
    }
    assert_eq!(canvas.fill_masks.held(), 1);
}

#[cfg(test)]
fn fills(commands: &[Command]) -> usize {
    commands
        .iter()
        .filter(|command| {
            matches!(
                command.cmd_type,
                CommandType::ConvexFill { .. } | CommandType::ConcaveFill { .. }
            )
        })
        .count()
}

/// A degenerate contour, of which a fill draws nothing, plays no part in a
/// mask: a path of one alone makes no atlas, one beside a square does not
/// make the square thin, and one beside a thin fill leaves its outline, and
/// its mask, as they were.
#[test]
fn degenerate_contours_play_no_part_in_masks() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());

    // #341's bare collinear contour.
    let mut line = Path::new();
    line.move_to(0.2, 0.2);
    line.line_to(0.2, 1.8);
    line.close();
    canvas.fill_path(&line, &paint);
    canvas.flush_to_output(());
    assert_eq!(canvas.fill_masks.image, None);
    assert_eq!(canvas.renderer.image_allocation_attempts, 0);

    // A square 20 px wide beside a line traced 900 px back and forth, whose
    // perimeter would have made the square's mean width 0.8.
    let mut square = Path::new();
    square.rect(10.0, 10.0, 20.0, 20.0);
    square.move_to(0.0, 50.0);
    for _ in 0..5 {
        square.line_to(90.0, 50.0);
        square.line_to(0.0, 50.0);
    }
    square.close();
    canvas.fill_path(&square, &paint);
    canvas.flush_to_output(());
    assert_eq!(canvas.fill_masks.image, None, "the square keeps its fringe");
    assert_eq!(fills(&recorded.borrow()), 1);

    canvas.fill_path(&thin_strip(0.0, 0.3), &paint);
    canvas.flush_to_output(());
    let mut beside = thin_strip(0.0, 0.3);
    beside.move_to(70.0, 80.0);
    beside.line_to(90.0, 80.0);
    beside.close();
    canvas.fill_path(&beside, &paint);
    canvas.flush_to_output(());
    assert_eq!(mask_quads(&recorded.borrow(), canvas.fill_masks.image), 1);
    assert_eq!(canvas.fill_masks.rasterized, 1, "found by its outline");
}

/// A mask that finds no room in the atlas at its largest leaves the room
/// there is to the smaller masks after it in the frame; the atlas starts
/// over once the frame is drawn.
#[test]
fn a_mask_that_finds_no_room_leaves_it_to_smaller_ones() {
    let renderer = RecordingRenderer {
        max_texture_size: 64,
        ..Default::default()
    };
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    let diagonal = |side: f32| {
        let mut path = Path::new();
        path.move_to(2.0, 2.2);
        path.line_to(2.0 + side, 2.2 + side);
        path.line_to(2.2 + side, 2.0 + side);
        path.line_to(2.2, 2.0);
        path.close();
        path
    };
    let mut small = Path::new();
    small.rect(40.0, 5.0, 20.0, 0.3);
    // 31 x 31 in the corner, then 59 x 59, which no place in a 64 x 64
    // atlas has room for beside it, then 22 x 3, which the corner's right
    // has.
    canvas.fill_path(&diagonal(28.0), &paint);
    canvas.fill_path(&diagonal(56.0), &paint);
    canvas.fill_path(&small, &paint);
    canvas.flush_to_output(());
    assert_eq!(mask_quads(&recorded.borrow(), canvas.fill_masks.image), 2);
    assert_eq!(fills(&recorded.borrow()), 1, "the large one keeps its fringe");
    assert_eq!(canvas.fill_masks.held(), 0, "started over");
}

/// The atlas is made once a mask has its place in it, as large as that
/// mask needs: a first thin fill that is refused makes none, and one wider
/// than the first atlas makes only the one it needs.
#[test]
fn the_atlas_is_made_for_the_first_mask_placed() {
    let renderer = RecordingRenderer {
        max_texture_size: 600,
        ..Default::default()
    };
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(1000, 100, 1.0);
    let paint = Paint::color(Color::black());
    let mut too_wide = Path::new();
    too_wide.rect(10.0, 10.0, 700.0, 0.4);
    canvas.fill_path(&too_wide, &paint);
    canvas.flush_to_output(());
    assert_eq!(canvas.renderer.image_allocation_attempts, 0, "refused, no atlas");

    let mut wide = Path::new();
    wide.rect(10.0, 50.0, 560.0, 0.4);
    canvas.fill_path(&wide, &paint);
    canvas.flush_to_output(());
    assert_eq!(canvas.renderer.image_allocation_attempts, 1);
    assert_eq!(canvas.renderer.image_deletion_count, 0, "none made to be let go");
    assert_eq!(canvas.fill_masks.size(), [600, 512]);
}

/// The atlas grows only the way its masks need: a long sliver makes it wide,
/// and so does a tall one, kept transposed; masks that fit it but find no
/// room lengthen its shorter side.
#[test]
fn the_atlas_grows_only_the_way_its_masks_need() {
    let paint = Paint::color(Color::black());
    let grown = |width: u32, height: u32, paths: &[Path]| {
        let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
        canvas.set_size(width, height, 1.0);
        for path in paths {
            canvas.fill_path(path, &paint);
        }
        canvas.flush_to_output(());
        canvas.fill_masks.size()
    };
    let mut wide = Path::new();
    wide.rect(10.0, 10.0, 1500.0, 0.4);
    assert_eq!(grown(2000, 100, &[wide]), [2048, 512]);
    let mut tall = Path::new();
    tall.rect(10.0, 10.0, 0.4, 1500.0);
    assert_eq!(grown(100, 2000, &[tall]), [2048, 512]);
    // Strips 500 x 100 across, each a pixel shorter than the last, so that
    // each is a mask of its own: four fit 512 x 512, the fifth finds no room.
    let strips: Vec<Path> = (0..6)
        .map(|i| {
            let (y, end) = (10.0 + 90.0 * i as f32, 510.0 - i as f32);
            let mut path = Path::new();
            path.move_to(10.0, y);
            path.line_to(end, y + 100.0);
            path.line_to(end, y + 100.3);
            path.line_to(10.0, y + 0.3);
            path.close();
            path
        })
        .collect();
    assert_eq!(grown(600, 700, &strips), [512, 1024]);
}

/// The atlas keeps its rows on the CPU only down to its lowest mask: a
/// sliver three rows tall in an atlas 1024 wide keeps three rows of it.
#[test]
fn the_atlas_keeps_only_the_rows_its_masks_reach() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(1000, 100, 1.0);
    let mut sliver = Path::new();
    sliver.rect(10.0, 10.0, 700.0, 0.4);
    canvas.fill_path(&sliver, &Paint::color(Color::black()));
    canvas.flush_to_output(());
    let [width, _] = canvas.fill_masks.size();
    assert_eq!(width, 1024);
    assert_eq!(canvas.fill_masks.pixels.len(), 3 * width);
}

/// An atlas a long sliver made wide is made small again once the masks after
/// it have fitted a smaller one at eight start-overs in a row, not before:
/// here a short sliver in motion, whose atlas starts over every other frame.
#[test]
fn the_atlas_is_made_smaller_once_its_masks_stay_small() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(2000, 100, 1.0);
    let paint = Paint::color(Color::black());
    let mut long = Path::new();
    long.rect(10.0, 10.0, 1500.0, 0.4);
    canvas.fill_path(&long, &paint);
    canvas.flush_to_output(());
    assert_eq!(canvas.fill_masks.size(), [2048, 512]);
    let deleted = canvas.renderer.image_deletion_count;
    for frame in 0..30 {
        canvas.fill_path(&thin_strip(frame as f32 * 0.37, 0.3), &paint);
        canvas.flush_to_output(());
        if frame == 10 {
            assert_eq!(canvas.fill_masks.size(), [2048, 512], "five start-overs are not enough");
        }
    }
    assert_eq!(canvas.fill_masks.size(), [512, 512]);
    assert_eq!(
        canvas.renderer.image_deletion_count,
        deleted + 1,
        "the wide atlas freed"
    );
}

/// A convex fill a pixel wide and more keeps its fringe, which draws it as
/// it is - a rectangle of it exactly - while a concave one under two pixels
/// wide, a mark whose mean width hides its hairlines, is drawn through a
/// mask, and one past two keeps its fringe again.
#[test]
fn a_concave_fill_under_two_pixels_wide_is_thin_and_a_convex_one_over_a_pixel_is_not() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let paint = Paint::color(Color::black());
    let masked = |canvas: &mut Canvas<RecordingRenderer>, path: &Path| {
        canvas.fill_path(path, &paint);
        canvas.flush_to_output(());
        mask_quads(&recorded.borrow(), canvas.fill_masks.image)
    };
    let mut bar = Path::new();
    bar.rect(10.0, 10.3, 60.0, 1.2);
    assert_eq!(masked(&mut canvas, &bar), 0, "a rectangle 1.2 px tall");
    let mut bars = Path::new();
    bars.rect(10.0, 20.3, 60.0, 1.5);
    bars.rect(10.0, 30.3, 60.0, 1.5);
    assert_eq!(masked(&mut canvas, &bars), 0, "two of them, 1.5 px tall");
    // A U whose strokes are `w` across.
    let u = |w: f32| {
        let mut path = Path::new();
        path.move_to(40.0, 40.0);
        path.line_to(40.0 + w, 40.0);
        path.line_to(40.0 + w, 70.0 - w);
        path.line_to(70.0 - w, 70.0 - w);
        path.line_to(70.0 - w, 40.0);
        path.line_to(70.0, 40.0);
        path.line_to(70.0, 70.0);
        path.line_to(40.0, 70.0);
        path.close();
        path
    };
    assert_eq!(masked(&mut canvas, &u(1.5)), 1, "a U of strokes 1.5 px across");
    assert_eq!(masked(&mut canvas, &u(2.5)), 0, "a U of strokes 2.5 px across");
}

/// A mask taller than it is wide is kept transposed, and its quad reads it
/// so: across the target is down the atlas, and down the target across it.
#[test]
fn a_tall_mask_is_kept_transposed() {
    let renderer = RecordingRenderer::default();
    let (recorded, verts) = (renderer.last_commands.clone(), renderer.last_verts.clone());
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);
    let mut tall = Path::new();
    tall.rect(20.0, 10.0, 0.4, 70.0);
    canvas.fill_path(&tall, &Paint::color(Color::black()));
    canvas.flush_to_output(());
    let commands = recorded.borrow();
    let quad = commands
        .iter()
        .find(|command| matches!(command.glyph_texture, GlyphTexture::AlphaMask(_)))
        .expect("drawn through a mask");
    let (offset, count) = quad.triangles_verts.unwrap();
    let verts = &verts.borrow()[offset..offset + count];
    let at = |x: f32, y: f32| *verts.iter().find(|v| v.x == x && v.y == y).unwrap();
    let ([x0, y0], [x1, y1]) = ([19.0, 9.0], [22.0, 81.0]);
    let (corner, right, below) = (at(x0, y0), at(x1, y0), at(x0, y1));
    assert_eq!(
        (right.u, right.v - corner.v),
        (corner.u, 3.0 / 512.0),
        "across the target, down the atlas"
    );
    assert_eq!(
        (below.u - corner.u, below.v),
        (72.0 / 512.0, corner.v),
        "down the target, across the atlas"
    );
}
