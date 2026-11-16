//! Layers: a group captured into a transient store and composited with
//! opacity, filters, a mask or a blend mode.

use super::*;

/// Effects applied to a layer when [`Canvas::end_layer`] composites it back.
///
/// Declared up front at [`Canvas::begin_layer`] - like Canvas 2D's
/// `beginLayer(filter)` proposal - so the layer's backing store can be sized
/// for the effects (a blur needs kernel-reach padding). Construct with
/// [`LayerEffects::new`] (what `Default` gives too) and the builder methods;
/// more effect kinds can be added without breaking callers.
#[derive(Clone, Debug)]
pub struct LayerEffects {
    pub(crate) opacity: f32,
    // Shared, so a layer record clones a pointer rather than the list:
    // a scene of thousands of filtered groups costs one copy per group
    // declaration, not one per open layer.
    pub(crate) filters: Rc<[ImageFilter]>,
    pub(crate) mask: Option<LayerMask>,
    // Set by `with_blend`: the composite is source-over with this mode, even
    // where the blend itself has to be omitted.
    pub(crate) blend: Option<BlendMode>,
}

/// How a layer mask's coverage is derived from its image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskKind {
    /// Coverage is the mask content's luminance (Rec. 709 weights on its
    /// sRGB values) times its alpha - SVG `mask`'s default `mask-type`.
    Luminance,
    /// Coverage is the mask content's own alpha channel.
    Alpha,
}

/// The transients a mask draws its coverage through: the mask normalized
/// into layer space and, for a luminance mask, the alpha it converts to.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MaskImages {
    pub(crate) normalized: ImageId,
    pub(crate) converted: Option<ImageId>,
}

/// The transients a filter chain draws through: its result and the
/// scratches its passes run through.
#[derive(Clone, Debug)]
pub(crate) struct FilterImages {
    pub(crate) target: ImageId,
    pub(crate) scratch: FilterScratchImages,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LayerMask {
    pub(crate) image: ImageId,
    pub(crate) kind: MaskKind,
    // Device-space placement of the mask image.
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl LayerEffects {
    /// No-op effects: full opacity, no filters, no mask.
    pub fn new() -> Self {
        Self {
            opacity: 1.0,
            filters: Rc::default(),
            mask: None,
            blend: None,
        }
    }

    /// Sets the group opacity the composite applies to the layer as a whole.
    ///
    /// This is SVG group-opacity / Canvas layer semantics: overlapping
    /// children inside the layer do NOT double-blend; the finished layer is
    /// faded as one image. Non-finite values are ignored; the value clamps
    /// to [0, 1].
    #[must_use]
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        if opacity.is_finite() {
            self.opacity = opacity.clamp(0.0, 1.0);
        }
        self
    }

    /// Sets an image-filter chain applied to the captured layer before it is
    /// composited, executing through
    /// [`filter_image_chain`](Canvas::filter_image_chain) - runs of color
    /// matrices still fold to one pass. The chain's result and scratches are
    /// reserved at [`begin_layer`](Canvas::begin_layer) with the layer's store.
    /// Under resource pressure a source-reading chain is omitted while group
    /// opacity is preserved; a source-replacing turbulence chain fails closed.
    #[must_use]
    pub fn with_filters(mut self, filters: &[ImageFilter]) -> Self {
        self.filters = filters.iter().take(MAX_FILTER_PASSES + 1).copied().collect();
        self
    }

    /// Masks the layer by `image`, placed at the device-space rect
    /// `(x, y, width, height)` - device space because a mask is a raster
    /// captured the way the layer is - with coverage derived per `kind`:
    /// SVG `mask` semantics, [`MaskKind::Luminance`] being SVG's default
    /// mask-type, computed on the mask's sRGB values as SVG's default
    /// `color-interpolation` has it. Applied after the filter chain, SVG's
    /// order for a group carrying both `filter` and `mask`. Pixels the mask
    /// rect does not cover are fully masked out. The rect stays root device
    /// space - the space of the target the outermost open layer draws on -
    /// however deep the masked layer nests: an enclosing layer's capture
    /// origin does not shift it.
    ///
    /// The mask image is borrowed, not owned: render mask content into your
    /// own image (upload or render target - its `ImageFlags` orientation is
    /// respected). A deletion requested while the layer is open is deferred
    /// through its composite flush. If coverage storage cannot be reserved
    /// after the layer is captured, the capture is discarded rather than
    /// composited unmasked.
    #[must_use]
    pub fn with_mask(mut self, image: ImageId, kind: MaskKind, x: f32, y: f32, width: f32, height: f32) -> Self {
        self.mask = Some(LayerMask {
            image,
            kind,
            x,
            y,
            width,
            height,
        });
        self
    }

    /// Composites the layer with `mode`: CSS `mix-blend-mode`, SVG's on a
    /// group. The finished layer, at its opacity, is blended with what the
    /// target it was opened on holds under the store, under source-over: a
    /// blend mode is the composite operation, so the one in effect at
    /// `begin_layer` is not applied, whether the blend runs or not. The
    /// layer's content is isolated, as in a stacking context.
    ///
    /// The backdrop must be readable: a layer opened while rendering into
    /// an image or inside another captured layer. On the screen, or when
    /// the blend's transients (the backdrop copy and, without a filter
    /// chain, the result) do not fit the budget, the blend is omitted and
    /// the layer composites source-over at its opacity.
    #[must_use]
    pub fn with_blend(mut self, mode: BlendMode) -> Self {
        self.blend = Some(mode);
        self
    }

    /// The colour-space conversion this layer's composite applies in place
    /// of its filter chain: a chain that is one conversion and nothing else
    /// converts each texel as the composite samples it, rounded to 8 bits as
    /// the conversion's pass stores its result - the same pixels with no pass
    /// and no image for the result. Not with a blend mode, whose pass reads
    /// the chain's result.
    pub(crate) fn composite_transfer(&self) -> Option<Transfer> {
        if self.blend.is_some_and(BlendMode::reads_backdrop) {
            return None;
        }
        match *self.filters {
            [ImageFilter::LinearRgbToSrgb] => Some(Transfer::ToSrgb),
            [ImageFilter::SrgbToLinearRgb] => Some(Transfer::ToLinearRgb),
            _ => None,
        }
    }
}

// Hand-written so the default is `new()`'s no-op effects: a derived Default
// would zero the opacity and make `LayerEffects::default()` a layer that
// composites nothing.
impl Default for LayerEffects {
    fn default() -> Self {
        Self::new()
    }
}

/// A blend mode's transients: the backdrop copy and the result. The blend
/// pass turns what it reads over once, so its result is read through the
/// opposite of what its source is. A chain whose result reads without
/// FLIP_Y blends back into the capture (free after the chain's first pass,
/// and its FLIP_Y suits the turned-over output), so `result` is `None`;
/// a chain whose result reads through FLIP_Y, like no chain at all, blends
/// into a plain `result`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BlendImages {
    pub(crate) backdrop: ImageId,
    pub(crate) result: Option<ImageId>,
}

#[derive(Debug)]
pub(crate) struct LayerRecord {
    // None marks a layer without a capture. It either passes through or, when
    // `discard` is set, suppresses draws that would be unsafe to expose.
    pub(crate) image: Option<ImageId>,
    // Optional effect storage reserved with the capture.
    pub(crate) mask_images: Option<MaskImages>,
    pub(crate) filter_images: Option<FilterImages>,
    pub(crate) blend_images: Option<BlendImages>,
    pub(crate) reserved_filter_work: u64,
    pub(crate) discard: bool,
    pub(crate) previous_target: RenderTarget,
    // Where the store lands on the previous target: the composite's origin,
    // in that target's device space.
    pub(crate) origin: (f32, f32),
    // Root device coordinates of the store's (0, 0): `origin` plus the shift
    // of every enclosing capture. A pass-through layer draws on the enclosing
    // target unchanged and carries that target's root origin along. A mask
    // rect is root device space, so it is placed against this, not `origin`.
    pub(crate) root_origin: (f32, f32),
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) effects: LayerEffects,
    pub(crate) outer_alpha: f32,
    // The state stack's length while the layer's own entry - the save
    // `begin_layer` pushes - is on top. That entry is the layer's boundary:
    // `restore()` at it closes the layer, `end_layer()` restores to it, and
    // the saves above it belong to the layer's content.
    pub(crate) state_depth: usize,
}

impl LayerRecord {
    /// Every transient the layer holds - its capture, the mask's coverage
    /// images and the filter chain's - which is what a flush keeps live and
    /// what `end_layer` or a discard returns to the pool.
    pub(crate) fn images(&self) -> impl Iterator<Item = ImageId> + '_ {
        let mask = self.mask_images;
        let filter = self.filter_images.as_ref();
        let blend = self.blend_images;
        self.image
            .into_iter()
            .chain(mask.map(|images| images.normalized))
            .chain(mask.and_then(|images| images.converted))
            .chain(filter.map(|images| images.target))
            .chain(filter.into_iter().flat_map(|images| images.scratch.images()))
            .chain(blend.map(|images| images.backdrop))
            .chain(blend.and_then(|images| images.result))
    }
}

impl<T> Canvas<T>
where
    T: Renderer,
{
    /// Drops every open layer without compositing it: the state stack
    /// rebalances, the layers' images return to the pool and drawing
    /// continues on the target that was current before the outermost layer.
    pub(crate) fn discard_open_layers(&mut self) {
        let mut outermost_target = None;
        while let Some(record) = self.layers.pop() {
            self.restore_to_layer_boundary(&record);
            self.refund_filter_work(record.reserved_filter_work);
            for image in record.images() {
                self.release_transient_image(image);
            }
            outermost_target = Some(record.previous_target);
        }
        if let Some(target) = outermost_target {
            self.set_render_target(target);
        }
    }

    /// Restores to `record`'s boundary: the saves left open inside the layer
    /// go with it, then the layer's own entry. The stack cannot be below the
    /// boundary while the layer is open - `restore()` closes the layer
    /// rather than cross it - so this pops at least the layer's entry.
    pub(crate) fn restore_to_layer_boundary(&mut self, record: &LayerRecord) {
        debug_assert!(self.state_stack.len() >= record.state_depth);
        // Entries past the limit sit above every real boundary.
        self.overflow.clear();
        self.state_stack.truncate(record.state_depth);
        self.pop_state();
    }

    /// Draws through `draw` on `target`, then goes back to the target that
    /// was current - the store of an open layer included, which is not
    /// otherwise reachable. For a side pass, such as rendering a mask or a
    /// filter's input, while a layer is open.
    pub fn with_render_target<R>(&mut self, target: RenderTarget, draw: impl FnOnce(&mut Self) -> R) -> R {
        let previous = self.current_render_target;
        self.set_render_target(target);
        let result = draw(self);
        self.set_render_target(previous);
        result
    }

    /// Runs `draw` on `target` as an effect's own pass - a backdrop placed,
    /// a mask normalised, a shadow's coverage drawn - under a state of its
    /// own: `transform`, full alpha, no scissor, source-over, no shadow,
    /// outside the state stack. The depth limit and a pass-through layer's
    /// suppression concern drawing on the caller's target and do not apply
    /// inside. The caller's state and target are back when it returns.
    pub(crate) fn offscreen_pass<R>(
        &mut self,
        target: RenderTarget,
        transform: Transform2D,
        draw: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.offscreen_passes += 1;
        let state = *self.state();
        let result = self.with_render_target(target, |canvas| {
            canvas.enter_offscreen_state(transform);
            draw(canvas)
        });
        *self.state_mut() = state;
        self.offscreen_passes -= 1;
        result
    }

    /// Runs `passes` (a [`filter_passes`] plan) from `source_image` into
    /// `target_image`, ping-ponging through the `scratch` images acquired for
    /// that plan, and releases them once the chain is recorded: they are free
    /// for the next chain (or layer) of this size.
    /// Draws `backdrop` at `rect` into `into`, an image of the chain's size,
    /// over transparent, so a blend pass samples it at the same pixel
    /// coordinates as the image it filters. An ordinary draw, as a mask's
    /// normalization is: the backdrop's own storage convention (an upload or
    /// a render target) never enters the pass's orientation rule.
    pub(crate) fn place_blend_backdrop(&mut self, into: ImageId, backdrop: ImageId, rect: (f32, f32, f32, f32)) {
        let Some(info) = self.images.info(into) else {
            return;
        };
        let (x, y, width, height) = rect;
        self.offscreen_pass(RenderTarget::Image(into), Transform2D::identity(), |canvas| {
            canvas.clear_rect(
                0,
                0,
                info.width() as u32,
                info.height() as u32,
                Color::rgbaf(0.0, 0.0, 0.0, 0.0),
            );
            let paint = Paint::image(backdrop, x, y, width, height, 0.0, 1.0);
            canvas.fill_device_rect(x, y, width, height, &paint.flavor);
        });
    }

    /// Opens a layer that [`end_layer`](Self::end_layer) composites with the
    /// declared opacity, filters, mask and blend mode. The current scissor
    /// bounds the capture when it is an axis-aligned rectangle; blur reach
    /// expands it.
    ///
    /// Returns `false` only when no capture fits and ordinary content passes
    /// through. Its current alpha is scaled by the requested opacity as an
    /// approximation; overlapping draws need a capture for true group opacity.
    /// A `true` layer is isolated or safely suppressed. A captured layer keeps
    /// group opacity, omits an ordinary filter or a blend mode if needed, and
    /// suppresses content whose mask or source-replacing filter cannot be
    /// applied.
    #[must_use = "false means ordinary content is using the pass-through fallback"]
    pub fn begin_layer(&mut self, effects: &LayerEffects) -> bool {
        if self.push_past_depth_limit(true) {
            // Past the depth limit the layer is one entry that end_layer()
            // pairs with, and its content is suppressed: the safe reading
            // of every effect it could have declared, so `true`.
            return true;
        }
        let state = *self.state();
        // The store spans what is being drawn into: the canvas, or an image
        // render target of another size (a layer opened while rendering to
        // an offscreen larger than the canvas must capture all of it).
        let (canvas_w, canvas_h) = self.render_target_size();
        // Root device coordinates of that target's (0, 0): its own when no
        // layer is open, else the enclosing layer's store origin in root
        // space - which a pass-through enclosing layer inherits unchanged.
        let root = self.layers.last().map_or((0.0, 0.0), |layer| layer.root_origin);

        // A layer opened under a non-invertible transform is not rasterizable
        // at all - not even for draws that set a valid transform inside it
        // (WPT 2d.layer.non-invertible-matrix). Capture into a small void
        // store that end_layer never composites (outer alpha 0).
        let [a, b, c, d, _, _] = state.transform.0;
        if (a * d - b * c).abs() < 1e-6 {
            let void = transient::LAYER_GRANULARITY;
            let image = self
                .acquire_transient_image(void, void, ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y)
                .ok();
            self.layers.push(LayerRecord {
                image,
                mask_images: None,
                filter_images: None,
                blend_images: None,
                reserved_filter_work: 0,
                discard: image.is_none(),
                previous_target: self.current_render_target,
                origin: (0.0, 0.0),
                root_origin: root,
                width: void,
                height: void,
                effects: effects.clone(),
                outer_alpha: 0.0,
                state_depth: self.state_stack.len() + 1,
            });
            self.save();
            if let Some(image) = image {
                self.set_render_target(RenderTarget::Image(image));
                self.clear_rect(0, 0, void as u32, void as u32, Color::rgbaf(0.0, 0.0, 0.0, 0.0));
            }
            return true;
        }

        // Reach padding, per axis: 3 sigma of the blurs covers >99.7% of the
        // kernel - each blur's true sigma (the chain runs a blur above the
        // shader's per-pass bound at the size where it fits one, so its
        // reach is real), clamped only at the chain ceiling, and successive
        // Gaussians compound in quadrature, n blurs of sigma reaching like
        // one of sigma * sqrt(n) - plus each dilation's radius and each
        // offset's shift, which carry content outward by that much.
        let pad = chain_reach(&effects.filters).map_or([0.0; 2], |reach| reach.map(|axis| axis + FRINGE_PAD));

        // A rounded or rotated scissor has no device rect: the store spans
        // the canvas and the scissor applies once, at the composite, after
        // the filters - SVG's clip-path over a filtered group and Canvas 2D's
        // clip under `ctx.filter` both clip the result, not the input.
        let rect = state
            .scissor
            .device_bounds(canvas_w, canvas_h)
            .unwrap_or_else(|| Rect::new(0.0, 0.0, canvas_w, canvas_h));
        // The true reach can push a full-width store past the backend's
        // texture limit (2048 px on a VideoCore IV); bound the pad so the
        // layer still captures with its reach truncated at the store edge,
        // instead of passing through with every effect dropped.
        let limit = self.renderer.max_texture_size();
        let pad = [
            bounded_pad(pad[0], rect.w, limit, transient::LAYER_GRANULARITY),
            bounded_pad(pad[1], rect.h, limit, transient::LAYER_GRANULARITY),
        ];
        let span =
            StoreSpan::padded(rect.x, rect.y, rect.x + rect.w, rect.y + rect.h, pad).clamped(canvas_w, canvas_h, pad);
        let visible = span.store(transient::LAYER_GRANULARITY);
        // The shadow the composite casts comes from wherever the store's
        // content lands once shifted by the offset and spread by the blur:
        // the capture takes in the source that reaches into the scissor from
        // outside it - up to the offset away, plus the blur's reach - or the
        // shadow of content the scissor leaves out would be missing. The
        // reach takes what the texture limit leaves after the visible
        // capture, so a store that fit without it keeps fitting.
        let mut plan = if state.shadow_color.a > 0.0 {
            span.with_shadow_reach(state.shadow_offset, state.shadow_blur * 1.5, limit)
                .store(transient::LAYER_GRANULARITY)
        } else {
            visible
        };

        // Past the backend's texture limit (2048 px on a VideoCore IV), an
        // ordinary layer passes through; effects that cannot safely expose
        // their source are suppressed below.
        // Render-target storage is premultiplied and vertically flipped;
        // FLIP_Y makes the unfiltered composite sample it upright.
        let flags = ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y;
        let mut image = plan
            .fits(limit)
            .then(|| self.acquire_transient_image(plan.width, plan.height, flags).ok())
            .flatten();
        if image.is_none() && plan != visible {
            // The reach did not fit the transient budget; the visible
            // capture alone may, and it is what the layer had before.
            plan = visible;
            image = plan
                .fits(limit)
                .then(|| self.acquire_transient_image(plan.width, plan.height, flags).ok())
                .flatten();
        }
        let (minx, miny) = plan.origin;
        let (width, height) = (plan.width, plan.height);

        let fail_closed = image.is_none()
            && (effects.mask.is_some()
                || effects
                    .filters
                    .iter()
                    .any(|filter| matches!(filter, ImageFilter::Turbulence { .. }))
                || state.alpha * effects.opacity <= 0.0);
        let mut record = LayerRecord {
            image,
            mask_images: None,
            filter_images: None,
            blend_images: None,
            reserved_filter_work: 0,
            discard: fail_closed,
            previous_target: self.current_render_target,
            origin: (minx, miny),
            root_origin: root,
            width,
            height,
            effects: effects.clone(),
            outer_alpha: state.alpha,
            state_depth: self.state_stack.len() + 1,
        };
        if let Some(image) = image {
            record.root_origin = (root.0 + minx, root.1 + miny);
            let headroom = self
                .images
                .info(image)
                .map(|info| self.renderer.transient_image_cost(info))
                .unwrap_or(0);

            if let Some(mask) = effects.mask {
                let work = match mask.kind {
                    MaskKind::Alpha => 0,
                    MaskKind::Luminance => {
                        filter_work(&[Pass::at(ImageFilter::luminance_to_alpha(), FULL)], width, height)
                    }
                };
                if self.reserve_filter_work(work) {
                    if let Some(images) = self.reserve_mask_images(width, height, mask.kind, headroom) {
                        record.mask_images = Some(images);
                        record.reserved_filter_work = work;
                    } else {
                        self.refund_filter_work(work);
                        record.discard = true;
                    }
                } else {
                    record.discard = true;
                }
            }

            // A chain the composite runs (`composite_transfer`) needs no
            // images and no filter work.
            if !record.discard && !effects.filters.is_empty() && effects.composite_transfer().is_none() {
                let replacing = effects
                    .filters
                    .iter()
                    .any(|filter| matches!(filter, ImageFilter::Turbulence { .. }));
                if let Some(passes) = filter_passes(&effects.filters) {
                    let work = filter_work(&passes, width, height);
                    if self.reserve_filter_work(work) {
                        if let Some(images) = self.reserve_filter_images(width, height, &effects.filters, headroom) {
                            if self.prepare_turbulence_lattices(&passes).is_ok() {
                                record.filter_images = Some(images);
                                record.reserved_filter_work = record.reserved_filter_work.saturating_add(work);
                            } else {
                                for image in std::iter::once(images.target).chain(images.scratch.images()) {
                                    self.rollback_transient_image(image);
                                }
                                self.refund_filter_work(work);
                                record.discard = replacing;
                            }
                        } else {
                            self.refund_filter_work(work);
                            record.discard = replacing;
                        }
                    } else {
                        record.discard = replacing;
                    }
                } else {
                    record.discard = replacing;
                }
            }

            // The backdrop under the store is readable from an image target
            // only; without it, or the room, the layer composites source-over.
            if !record.discard
                && effects.blend.is_some_and(BlendMode::reads_backdrop)
                && matches!(record.previous_target, RenderTarget::Image(_))
            {
                let work = blend_work(width, height);
                if self.reserve_filter_work(work) {
                    let capture_serves = record.filter_images.as_ref().is_some_and(|images| {
                        !self
                            .images
                            .info(images.target)
                            .is_some_and(|info| info.flags().contains(ImageFlags::FLIP_Y))
                    });
                    match self.reserve_blend_images(width, height, capture_serves, headroom) {
                        Some(images) => {
                            record.blend_images = Some(images);
                            record.reserved_filter_work = record.reserved_filter_work.saturating_add(work);
                        }
                        None => self.refund_filter_work(work),
                    }
                }
            }

            if record.discard {
                let effect_images: Vec<_> = record
                    .mask_images
                    .into_iter()
                    .flat_map(|images| [Some(images.normalized), images.converted])
                    .flatten()
                    .chain(
                        record
                            .filter_images
                            .iter()
                            .flat_map(|images| std::iter::once(images.target).chain(images.scratch.images())),
                    )
                    .collect();
                for held in effect_images {
                    self.rollback_transient_image(held);
                }
                self.refund_filter_work(record.reserved_filter_work);
                record.mask_images = None;
                record.filter_images = None;
                record.reserved_filter_work = 0;
            }
        }
        let image = record.image;
        let discard = record.discard;
        self.layers.push(record);

        self.save();
        let Some(image) = image else {
            if !discard {
                self.state_mut().alpha *= effects.opacity;
                if effects.blend.is_some() {
                    self.state_mut().composite_operation = CompositeOperationState::default();
                }
            }
            return discard;
        };
        self.set_render_target(RenderTarget::Image(image));
        self.clear_rect(0, 0, width as u32, height as u32, Color::rgbaf(0.0, 0.0, 0.0, 0.0));

        // Shift device space so the captured rect's origin lands on (0, 0),
        // exactly like the shadow pass maps its padded bbox. Shadow state is
        // a layer rendering attribute too: it applies to the layer's result
        // at end_layer, so it must not also apply to every draw inside, or
        // the layer's children would each cast their own shadow and then the
        // layer would cast one more over the lot.
        let mut layer_transform = Transform2D::translation(-minx, -miny);
        layer_transform.premultiply(&state.transform);
        self.enter_offscreen_state(layer_transform);
        true
    }

    /// Closes the innermost [`begin_layer`](Self::begin_layer) and composites
    /// the captured layer onto the previous target with the layer's declared
    /// effects, honoring the outer scissor and composite operation.
    ///
    /// The state is restored to what it was at `begin_layer`: a `save()`
    /// left open inside the layer is discarded with it, like Skia's
    /// `restoreToCount()`. With no layer open this does nothing - a
    /// `restore()` that reached the layer's entry has already closed it.
    pub fn end_layer(&mut self) {
        if self.pop_layer_past_depth_limit() {
            // A layer past the limit closes with the saves left open inside
            // it; nothing was drawn to composite.
            return;
        }
        let Some(mut record) = self.layers.pop() else {
            return;
        };
        self.restore_to_layer_boundary(&record);
        self.set_render_target(record.previous_target);
        let Some(image) = record.image else {
            return; // pass-through layer: nothing captured
        };

        if record.discard {
            self.release_layer_images(&record, None);
            return;
        }

        let alpha = record.outer_alpha * record.effects.opacity;
        if alpha <= 0.0 {
            self.refund_filter_work(record.reserved_filter_work);
            self.release_layer_images(&record, None);
            return;
        }

        // Run the filter chain, if any, through the images reserved for it at
        // begin_layer: the chain releases the scratches, the result goes back
        // with the composite. The result's FLIP_Y flag was chosen at
        // begin_layer from how the plan's passes leave it, so the composite,
        // the mask and the blend read it the right way up through it.
        let filtered = match record.filter_images.take() {
            Some(FilterImages { target, scratch }) => {
                let passes =
                    filter_passes(&record.effects.filters).expect("an admitted layer has a bounded filter plan");
                self.run_filter_passes(target, &passes, image, scratch, true, record.root_origin);
                Some(target)
            }
            None => None,
        };
        let source = filtered.unwrap_or(image);

        let (minx, miny) = record.origin;

        // The mask applies after the filter chain - SVG's order for a group
        // carrying both - and multiplies the layer's alpha in place.
        if let (Some(mask), Some(images)) = (record.effects.mask, record.mask_images) {
            self.apply_layer_mask(source, &record, mask, images);
        }

        // A blend mode composites the layer's contribution over the backdrop,
        // under source-over and at full alpha: the opacity went into the pass.
        let blended = match (record.effects.blend, record.blend_images, record.previous_target) {
            (Some(mode), Some(images), RenderTarget::Image(parent)) => {
                self.blend_layer_with_backdrop(mode, source, image, &record, images, parent, alpha)
            }
            _ => None,
        };
        let (composited, alpha) = match blended {
            Some(contribution) => (contribution, 1.0),
            None => (source, alpha),
        };
        let tint = Color::rgbaf(1.0, 1.0, 1.0, alpha);
        let mut layer_paint = Paint::image_tint(
            composited,
            minx,
            miny,
            record.width as f32,
            record.height as f32,
            0.0,
            tint,
        )
        .with_transfer(record.effects.composite_transfer());
        layer_paint.set_anti_alias(false);

        // Composite in plain device space at the captured origin; the state
        // restored above supplies the outer scissor and composite operation.
        // The restore above put back the shadow state that was current at
        // begin_layer, and this composite deliberately runs under it: the
        // shadow pass builds coverage from the paint's real alpha, so the
        // layer image casts one shadow for the whole group, the way Canvas
        // 2D's beginLayer applies the shadow to the layer's result and SVG's
        // feDropShadow applies to a filtered group. Inside the layer the
        // shadow state was reset, so nothing has been shadowed twice.
        let composite_operation = self.state().composite_operation;
        match record.effects.blend {
            // plus-lighter is the GPU's saturating addition: no backdrop pass.
            Some(BlendMode::PlusLighter) => {
                self.state_mut().composite_operation = CompositeOperationState::new(CompositeOperation::Lighter);
            }
            Some(_) => {
                self.state_mut().composite_operation = CompositeOperationState::new(CompositeOperation::SourceOver);
            }
            None => {}
        }
        self.fill_device_rect(
            minx,
            miny,
            record.width as f32,
            record.height as f32,
            &layer_paint.flavor,
        );
        self.state_mut().composite_operation = composite_operation;

        // The composite that reads the layer is recorded; its images can back
        // the next layer of this size.
        self.release_layer_images(&record, filtered);
    }

    /// Returns a finished layer's images, and its chain's result `filtered`,
    /// to the transient pool once every command reading them is recorded.
    pub(crate) fn release_layer_images(&mut self, record: &LayerRecord, filtered: Option<ImageId>) {
        for image in record.images().chain(filtered) {
            self.release_transient_image(image);
        }
    }

    /// Blends `source` (the capture or its chain's result, masked), scaled
    /// by `alpha` first, with the region of `parent` under the store, and
    /// returns the image holding its contribution over that backdrop; `None`
    /// when the parent cannot be read.
    pub(crate) fn blend_layer_with_backdrop(
        &mut self,
        mode: BlendMode,
        source: ImageId,
        capture: ImageId,
        record: &LayerRecord,
        images: BlendImages,
        parent: ImageId,
        alpha: f32,
    ) -> Option<ImageId> {
        debug_assert!(mode.reads_backdrop(), "{mode:?} composites without a backdrop pass");
        let (parent_width, parent_height) = self.image_size(parent).ok()?;
        let (minx, miny) = record.origin;
        let (width, height) = (record.width as f32, record.height as f32);
        // The parent shifted so the store's origin lands on (0, 0).
        self.place_blend_backdrop(
            images.backdrop,
            parent,
            (-minx, -miny, parent_width as f32, parent_height as f32),
        );
        let source_flipped = self
            .images
            .info(source)
            .is_some_and(|info| info.flags().contains(ImageFlags::FLIP_Y));
        let target = images.result.unwrap_or(capture);
        debug_assert!(target != source);
        debug_assert_eq!(
            self.images
                .info(target)
                .is_some_and(|info| info.flags().contains(ImageFlags::FLIP_Y)),
            !source_flipped,
            "the blend's result lands the other way up from its source"
        );
        let blend = ImageFilter::Blend {
            mode,
            backdrop: images.backdrop,
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        let pass = BlendPass {
            backdrop_flipped: !source_flipped,
            source_alpha: alpha,
            contribution: true,
        };
        let draw = PassDraw {
            backdrop: Some((images.backdrop, pass)),
            ..PassDraw::default()
        };
        self.filter_image_with_scratch(target, blend, source, None, draw)
            .then_some(target)
    }

    /// Puts the current state into the shape every offscreen pass draws
    /// under: `transform` as the pass's device space, full alpha, no scissor,
    /// source-over and no shadow. The caller's `save()` holds the state this
    /// replaces.
    pub(crate) fn enter_offscreen_state(&mut self, transform: Transform2D) {
        let state = self.state_mut();
        state.transform = transform;
        state.alpha = 1.0;
        state.scissor = Scissor::default();
        state.composite_operation = CompositeOperationState::default();
        state.shadow_color = Color::rgbaf(0.0, 0.0, 0.0, 0.0);
        state.shadow_blur = 0.0;
        state.shadow_offset = [0.0, 0.0];
    }

    /// Fills a device-space rect with `paint` under the identity transform,
    /// at full alpha and without antialiasing: the blit an offscreen pass
    /// ends with. The scissor, composite operation and shadow state stay the
    /// caller's - that is how a layer's composite honors the outer scissor
    /// and casts the group's shadow.
    pub(crate) fn fill_device_rect(&mut self, x: f32, y: f32, width: f32, height: f32, paint: &PaintFlavor) {
        let saved_transform = self.state().transform;
        let saved_alpha = self.state().alpha;
        self.state_mut().transform = Transform2D::identity();
        self.state_mut().alpha = 1.0;
        let mut rect = Path::new();
        rect.rect(x, y, width, height);
        self.fill_path_internal(&rect, None, paint, false, FillRule::NonZero);
        self.state_mut().transform = saved_transform;
        self.state_mut().alpha = saved_alpha;
    }

    /// Acquires a mask's coverage transients for a layer store of
    /// `width` x `height`: the mask normalized into layer space, sampled
    /// upright through FLIP_Y like a capture, and for luminance masks the
    /// conversion target, whose color-matrix pass leaves storage upright.
    /// `None`, holding nothing, when the budget cannot fit them.
    pub(crate) fn reserve_mask_images(
        &mut self,
        width: usize,
        height: usize,
        kind: MaskKind,
        headroom: usize,
    ) -> Option<MaskImages> {
        let normalized = self
            .acquire_transient_image_reserving(width, height, ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y, headroom)
            .ok()?;
        let converted = match kind {
            MaskKind::Alpha => None,
            MaskKind::Luminance => {
                match self.acquire_transient_image_reserving(width, height, ImageFlags::PREMULTIPLIED, headroom) {
                    Ok(converted) => Some(converted),
                    Err(_) => {
                        self.rollback_transient_image(normalized);
                        return None;
                    }
                }
            }
        };
        Some(MaskImages { normalized, converted })
    }

    /// Acquires a filter chain's transients for a layer store of
    /// `width` x `height`: the result, which the chain stores upright and the
    /// composite samples without FLIP_Y, and the scratches its pass plan
    /// needs, sized like the capture the chain reads. `None`, holding
    /// nothing, when the budget cannot fit them.
    pub(crate) fn reserve_filter_images(
        &mut self,
        width: usize,
        height: usize,
        filters: &[ImageFilter],
        headroom: usize,
    ) -> Option<FilterImages> {
        let passes = filter_passes(filters)?;
        for pass in &passes {
            if let ImageFilter::Blend { backdrop, .. } = pass.filter {
                // A blend without its backdrop has nothing to run against.
                self.images.info(backdrop)?;
            }
        }
        // The capture is stored the way a render target is; the result is
        // read through FLIP_Y when the chain's passes leave it that way too,
        // so the chain never spends a pass on orientation.
        let flags = if plan_stores_flipped(&passes, true) {
            ImageFlags::PREMULTIPLIED | ImageFlags::FLIP_Y
        } else {
            ImageFlags::PREMULTIPLIED
        };
        let target = self
            .acquire_transient_image_reserving(width, height, flags, headroom)
            .ok()?;
        // The result and the chain scratches are plain render targets alike,
        // so a layer can alternate through its result and one scratch; the
        // result's flag matters only to the composite that reads it.
        match self.acquire_filter_scratches(width, height, &passes, 1, headroom) {
            Ok(scratch) => Some(FilterImages { target, scratch }),
            Err(_) => {
                self.rollback_transient_image(target);
                None
            }
        }
    }

    /// A blend mode's transients for a store of `width` x `height`; the
    /// result is skipped when `capture_serves` - the chain's result reads
    /// without FLIP_Y, so the capture reads the blend of it the right way
    /// up. `None` holds nothing.
    pub(crate) fn reserve_blend_images(
        &mut self,
        width: usize,
        height: usize,
        capture_serves: bool,
        headroom: usize,
    ) -> Option<BlendImages> {
        let backdrop = self
            .acquire_transient_image_reserving(width, height, ImageFlags::PREMULTIPLIED, headroom)
            .ok()?;
        let result = if capture_serves {
            None
        } else {
            match self.acquire_transient_image_reserving(width, height, ImageFlags::PREMULTIPLIED, headroom) {
                Ok(result) => Some(result),
                Err(_) => {
                    self.rollback_transient_image(backdrop);
                    return None;
                }
            }
        };
        Some(BlendImages { backdrop, result })
    }

    /// Multiplies `layer`'s alpha by the mask's coverage, in layer space,
    /// drawing through the coverage images reserved at `begin_layer`.
    ///
    /// Orientation: a draw into an image target lands in flipped storage, so
    /// draw-space row 0 writes the storage row holding the top of an image
    /// sampled through FLIP_Y (a capture, a filtered result whose chain left
    /// it that way) but the bottom of one sampled without it. Both coverage
    /// images sample upright - `normalized` through FLIP_Y like a capture,
    /// `converted` without it - so the coverage draw runs under the identity
    /// over a FLIP_Y layer and under a vertical flip over the other kind.
    pub(crate) fn apply_layer_mask(
        &mut self,
        layer: ImageId,
        record: &LayerRecord,
        mask: LayerMask,
        images: MaskImages,
    ) {
        let layer_read_flipped = self
            .images
            .info(layer)
            .is_some_and(|info| info.flags().contains(ImageFlags::FLIP_Y));
        let (width, height) = (record.width as f32, record.height as f32);
        // The mask rect is root device space and the store's (0, 0) sits at
        // the record's root origin - every enclosing capture's shift
        // included - not at the local origin the composite lands on.
        let (minx, miny) = record.root_origin;
        // Normalize the mask into layer space with an ordinary draw, so the
        // caller's storage convention (an upload or a render target, whatever
        // ImageFlags it carries) never enters the parity rule above. The
        // backdrop sets what uncovered pixels mean. A luminance mask lands
        // over opaque black: every pixel is then opaque, the color-matrix
        // pass's unpremultiply is the identity, and the luminance it writes
        // from the premultiplied color is luminance x alpha - SVG's mask
        // value - in one pass, with the uncovered rest reading black,
        // coverage 0. An alpha mask lands over transparent and its own alpha
        // is the coverage.
        let backdrop = match mask.kind {
            MaskKind::Luminance => Color::black(),
            MaskKind::Alpha => Color::rgbaf(0.0, 0.0, 0.0, 0.0),
        };
        let transform = if layer_read_flipped {
            Transform2D::identity()
        } else {
            Transform2D::new(1.0, 0.0, 0.0, -1.0, 0.0, height)
        };
        self.offscreen_pass(
            RenderTarget::Image(images.normalized),
            Transform2D::identity(),
            |canvas| {
                canvas.clear_rect(0, 0, record.width as u32, record.height as u32, backdrop);
                let mask_paint = Paint::image(
                    mask.image,
                    mask.x - minx,
                    mask.y - miny,
                    mask.width,
                    mask.height,
                    0.0,
                    1.0,
                );
                canvas.fill_device_rect(
                    mask.x - minx,
                    mask.y - miny,
                    mask.width,
                    mask.height,
                    &mask_paint.flavor,
                );
                let coverage = match images.converted {
                    Some(converted) => {
                        let _ = canvas.filter_image_with_scratch(
                            converted,
                            ImageFilter::luminance_to_alpha(),
                            images.normalized,
                            None,
                            PassDraw::default(),
                        );
                        converted
                    }
                    None => images.normalized,
                };
                // layer.alpha *= coverage.alpha over the whole store.
                canvas.set_render_target(RenderTarget::Image(layer));
                canvas.enter_offscreen_state(transform);
                canvas.state_mut().composite_operation =
                    CompositeOperationState::new(CompositeOperation::DestinationIn);
                let coverage_paint = Paint::image(coverage, 0.0, 0.0, width, height, 0.0, 1.0);
                let mut store = Path::new();
                store.rect(0.0, 0.0, width, height);
                canvas.fill_path_internal(&store, None, &coverage_paint.flavor, false, FillRule::NonZero);
            },
        );
    }
}

#[test]
fn an_open_layer_keeps_a_pending_mask_alive_through_its_composite() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let mask = canvas
        .create_image_empty(64, 64, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let effects = LayerEffects::new().with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 64.0, 64.0);
    assert!(canvas.begin_layer(&effects));
    canvas.delete_image(mask);
    canvas.flush_to_output(());
    assert!(canvas.images.info(mask).is_some());
    assert!(canvas.pending_image_deletions.contains(&mask));

    canvas.end_layer();
    assert!(canvas.images.info(mask).is_some());
    canvas.flush_to_output(());
    assert!(canvas.images.info(mask).is_none());
    assert!(!canvas.pending_image_deletions.contains(&mask));
}

/// A rounded scissor set before `begin_layer` is not applied to the draws
/// inside the layer - padded store or not - and clips the composite once,
/// where it was set: the draw inside records no scissor, and the composite
/// carries the scissor centered at the root center, extent and radius as set.
#[test]
fn a_rounded_scissor_clips_a_layer_once_at_its_composite() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let recorded_commands = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(100, 100, 1.0);

    // Sigma 2 pads the store by ceil(3 * 2) + 2 = 8 px per side; no filter
    // leaves the store at the root origin. Neither may clip the inside.
    let blur = LayerEffects::new().with_filters(&[ImageFilter::gaussian_blur(2.0)]);
    let plain = LayerEffects::new();
    for (effects, origin) in [(&blur, (-8.0, -8.0)), (&plain, (0.0, 0.0))] {
        canvas.save();
        canvas.rounded_scissor(10.0, 10.0, 40.0, 20.0, 5.0); // centered on root (30, 20)
        assert!(canvas.begin_layer(effects));
        assert_eq!(canvas.layers.last().unwrap().origin, origin);
        fill_across_current_scissor(&mut canvas);
        {
            let commands = recorded_commands.borrow();
            let params = first_draw_params(&commands);
            assert_eq!(
                params.scissor_ext,
                [1.0, 1.0],
                "no scissor inside the layer, store origin {origin:?}"
            );
        }
        canvas.end_layer();
        canvas.flush_to_output(());
        {
            let commands = recorded_commands.borrow();
            let params = first_draw_params(&commands);
            let expected = Transform2D::translation(30.0, 20.0).inverse().to_mat3x4();
            assert_eq!(
                params.scissor_mat, expected,
                "the composite is clipped where the scissor was set"
            );
            assert_eq!(params.scissor_ext, [20.0, 10.0]);
            assert_approx_eq(params.scissor_radius, 5.0);
        }
        canvas.restore();
    }
}

/// Layer backing stores are bounded by the scissor rect plus declared blur
/// reach - the memory rule that keeps nested layers from allocating
/// full-canvas images. Pass-through degradation keeps begin/end balanced.
#[test]
fn layer_bounds_follow_the_scissor() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(800, 600, 1.0);

    // Unscissored: the layer spans the canvas.
    assert!(canvas.begin_layer(&LayerEffects::new()));
    let record = canvas.layers.last().unwrap();
    // Stores round up to 64 px per axis so siblings share them (see TRANSIENT_GRANULARITY).
    assert_eq!((record.width, record.height), (832, 640));
    canvas.end_layer();

    // A rect scissor bounds the layer to its size.
    canvas.save();
    canvas.scissor(100.0, 50.0, 120.0, 80.0);
    assert!(canvas.begin_layer(&LayerEffects::new()));
    let record = canvas.layers.last().unwrap();
    assert_eq!((record.width, record.height), (128, 128));
    canvas.end_layer();

    // Declaring a blur pads the store by the kernel reach (3*sigma + 2).
    assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[ImageFilter::gaussian_blur(4.0)])));
    let record = canvas.layers.last().unwrap();
    assert_eq!((record.width, record.height), (192, 128)); // 148 x 108 before rounding
    canvas.end_layer();
    canvas.restore();

    // Two plain layers cost one transient each (their sizes differ, so no
    // reuse); the blurred layer costs its capture, the filtered target and
    // the blur scratch. All five are free again once their layers have
    // ended, and the flush deletes them.
    assert_eq!(canvas.transients.images.len(), 5);
    assert_eq!(canvas.transients.free.len(), 5);
    canvas.flush_to_output(());
    assert_eq!(canvas.transients.images.len(), 0);
    assert_eq!(canvas.transients.free.len(), 0);
    assert_eq!(canvas.transient_image_bytes(), 0);

    // Unbalanced end_layer is ignored.
    canvas.end_layer();
}

/// Sibling layers share backing stores: a layer's images return to the pool
/// at end_layer and the next layer of the same size takes them, so a frame's
/// transient memory is its deepest nesting, not its layer count. This is what
/// lets a 1080p frame of a few hundred layers - every BuseyBench portrait -
/// stay within tens of MB instead of the gigabytes their sum would be.
#[test]
fn sibling_layers_reuse_backing_stores() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(320, 200, 1.0);
    let bytes = 320 * 256 * 4; // 320 x 200 rounds up to 320 x 256

    // Three plain siblings: one allocation.
    for _ in 0..3 {
        assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
        assert!(canvas.layers.last().unwrap().image.is_some());
        canvas.end_layer();
    }
    assert_eq!(canvas.transients.images.len(), 1);
    assert_eq!(canvas.transient_image_bytes(), bytes);

    // Nesting needs one store per open level: a child cannot take its parent's.
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    canvas.end_layer();
    canvas.end_layer();
    assert_eq!(canvas.transients.images.len(), 2);

    // Blurred siblings: capture, filtered target and one horizontal blur
    // scratch, once.
    let blur = LayerEffects::new().with_filters(&[ImageFilter::gaussian_blur(2.0)]);
    for _ in 0..4 {
        assert!(canvas.begin_layer(&blur));
        canvas.end_layer();
    }
    let padded = 384 * 256 * 4; // 336 x 216 padded, rounded
    assert_eq!(canvas.transients.images.len(), 2 + 3);
    assert_eq!(canvas.transient_image_bytes(), 2 * bytes + 3 * padded);

    // Everything is free between layers, nothing after the flush.
    assert_eq!(canvas.transients.free.len(), 5);
    canvas.flush_to_output(());
    assert_eq!(canvas.transients.images.len(), 0);
    assert_eq!(canvas.transient_image_bytes(), 0);
}

/// One capture-sized slice stays available after a layer reserves its effect
/// images, so a later group can still preserve group opacity.
#[test]
fn a_budget_for_one_layer_fits_a_frame_of_them() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(256, 256, 1.0);
    let padded = 320 * 320 * 4; // 272 x 272 padded, rounded
    canvas.set_transient_image_budget(5 * padded);
    let blur = LayerEffects::new().with_filters(&[ImageFilter::gaussian_blur(2.0)]);
    for i in 0..200 {
        assert!(canvas.begin_layer(&blur));
        assert!(
            canvas.layers.last().unwrap().image.is_some(),
            "layer {i} degraded to pass-through"
        );
        canvas.end_layer();
    }
    assert_eq!(canvas.transients.images.len(), 3);
    assert_eq!(canvas.transient_image_bytes(), 3 * padded);
}

/// A layer keeps its capture and group opacity when optional filter storage
/// does not fit. With enough room plus capture headroom, it applies the full
/// chain.
#[test]
fn a_layer_short_of_its_chain_scratch_budget_keeps_its_capture() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(128, 128, 1.0);
    let padded = 192 * 192 * 4; // 144 x 144 padded, rounded
    let blur = LayerEffects::new().with_filters(&[ImageFilter::gaussian_blur(2.0)]);
    canvas.set_transient_image_budget(3 * padded);
    assert!(canvas.begin_layer(&blur));
    assert!(canvas.layers.last().unwrap().image.is_some());
    assert!(canvas.layers.last().unwrap().filter_images.is_none());
    assert_eq!(canvas.transients.images.len(), 1);
    canvas.end_layer();

    canvas.flush_to_output(());
    canvas.set_transient_image_budget(5 * padded);
    assert!(canvas.begin_layer(&blur));
    let target = canvas.layers.last().unwrap().filter_images.as_ref().unwrap().target;
    assert_eq!(canvas.transients.images.len(), 3);
    assert_eq!(
        canvas.transients.free.len(),
        0,
        "the chain's images are held from begin_layer"
    );
    canvas.end_layer();
    // The composite samples the filtered result, not the capture.
    let composite = canvas
        .commands
        .iter()
        .rev()
        .find(|c| c.image.is_some())
        .expect("a composite was recorded");
    assert_eq!(composite.image, Some(target));
    assert_eq!(canvas.transients.free.len(), 3);
}

/// A layer whose chain is one colour-space conversion runs it in its
/// composite: it holds its capture alone and no filter work, and the
/// composite's image paint carries the conversion. With a blend mode the
/// chain keeps its pass, whose result the blend reads.
#[test]
fn a_lone_conversion_runs_in_the_layer_composite() {
    use crate::{BlendMode, ImageFilter};
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    for (filter, transfer) in [
        (ImageFilter::LinearRgbToSrgb, Transfer::ToSrgb),
        (ImageFilter::SrgbToLinearRgb, Transfer::ToLinearRgb),
    ] {
        assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[filter])));
        let record = canvas.layers.last().unwrap();
        assert!(record.filter_images.is_none(), "{filter:?}");
        assert_eq!(record.reserved_filter_work, 0, "{filter:?}");
        assert_eq!(canvas.transients.images.len(), 1, "{filter:?}: the capture only");
        canvas.end_layer();
        let params = canvas
            .commands
            .iter()
            .rev()
            .find_map(|c| match (&c.cmd_type, c.image) {
                (CommandType::ConvexFill { params } | CommandType::Triangles { params }, Some(_)) => Some(*params),
                _ => None,
            })
            .expect("a composite was recorded");
        assert_eq!(params.image_transfer, Some(transfer), "{filter:?}");
        canvas.flush_to_output(());
    }

    let blended = LayerEffects::new()
        .with_filters(&[ImageFilter::LinearRgbToSrgb])
        .with_blend(BlendMode::Multiply);
    assert_eq!(blended.composite_transfer(), None);
    assert!(canvas.begin_layer(&blended));
    assert!(
        canvas.layers.last().unwrap().filter_images.is_some(),
        "the chain keeps its pass"
    );
    canvas.end_layer();
}

/// A filtered layer's reservation is sized by its pass plan: the result, one
/// chain scratch when needed, and a blur scratch when needed. The layer's
/// result has the same storage convention as its scratch, so longer chains
/// can alternate through the two.
#[test]
fn a_filtered_layer_reserves_its_chain_images_by_pass_plan() {
    use crate::ImageFilter;
    // (filters, transient images held at begin_layer, store edge, pixels held in all)
    let cases: [(&[ImageFilter], usize, usize, usize); 4] = [
        // One color pass: the result only. No blur, so the store is the canvas.
        (&[ImageFilter::brightness(0.0)], 2, 64, 2 * 64 * 64),
        // One blur pass writing the result: its horizontal scratch only.
        // Sigma 1 pads 5 px, rounding to 128.
        (&[ImageFilter::gaussian_blur(1.0)], 3, 128, 3 * 128 * 128),
        // A blur above the per-pass bound runs down the pyramid: the store
        // pads by the true reach, 50 px, to 192; the scale back up writes
        // the result, and the halving, the blur and its scratch are
        // quarter-size.
        (&[ImageFilter::gaussian_blur(16.0)], 5, 192, 2 * 192 * 192 + 3 * 96 * 96),
        // A blur never folds with a color matrix, so brightness, blur and
        // invert are three passes: one chain scratch and the blur's.
        (
            &[
                ImageFilter::brightness(2.0),
                ImageFilter::gaussian_blur(1.0),
                ImageFilter::invert(1.0),
            ],
            4,
            128,
            4 * 128 * 128,
        ),
    ];
    for (filters, images, store, pixels) in cases {
        let renderer = RecordingRenderer::default();
        let mut canvas = Canvas::new(renderer).unwrap();
        canvas.set_size(64, 64, 1.0);
        let effects = LayerEffects::new().with_filters(filters);
        let bytes = store * store * 4;

        // The filter images are reserved with the store's cost as headroom.
        canvas.set_transient_image_budget((pixels + store * store) * 4);
        assert!(canvas.begin_layer(&effects), "{filters:?}: {images} images fit");
        assert_eq!(canvas.transients.images.len(), images, "{filters:?}");
        assert_eq!(
            canvas.transients.free.len(),
            0,
            "{filters:?}: all held from begin_layer"
        );
        canvas.end_layer();
        assert_eq!(
            canvas.transients.free.len(),
            images,
            "{filters:?}: all returned at end_layer"
        );

        canvas.flush_to_output(());
        canvas.set_transient_image_budget((pixels + store * store) * 4 - 1);
        assert!(canvas.begin_layer(&effects), "{filters:?}: the capture still fits");
        assert!(canvas.layers.last().unwrap().filter_images.is_none(), "{filters:?}");
        assert!(bytes <= pixels * 4);
        canvas.end_layer();
    }
}

/// A blending layer reserves one more transient, the image its backdrop is
/// placed into before the pass, and returns it with the rest at end_layer; a
/// blend whose backdrop does not exist reserves nothing and the layer
/// composites unfiltered.
#[test]
fn a_blending_layer_reserves_its_backdrop_scratch() {
    use crate::{BlendMode, ImageFilter};
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    let backdrop = canvas
        .create_image_empty(4, 4, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let blend = ImageFilter::Blend {
        mode: BlendMode::Multiply,
        backdrop,
        x: 0.0,
        y: 0.0,
        width: 4.0,
        height: 4.0,
    };
    // One pass: capture, result and the placement scratch. Brightness then
    // the blend is two flipping passes plus the parity identity: one chain
    // scratch more.
    for (filters, images) in [(vec![blend], 3), (vec![ImageFilter::brightness(2.0), blend], 4)] {
        let effects = LayerEffects::new().with_filters(&filters);
        assert!(canvas.begin_layer(&effects));
        assert_eq!(canvas.transients.images.len(), images, "{filters:?}");
        assert_eq!(canvas.transients.free.len(), 0, "{filters:?}: all held");
        canvas.end_layer();
        assert_eq!(canvas.transients.free.len(), images, "{filters:?}: all returned");
        canvas.flush_to_output(());
    }

    canvas.delete_image(backdrop);
    canvas.flush_to_output(());
    let effects = LayerEffects::new().with_filters(&[blend]);
    assert!(canvas.begin_layer(&effects), "the capture still opens");
    assert!(
        canvas.layers.last().unwrap().filter_images.is_none(),
        "no chain images without a backdrop to blend"
    );
    assert_eq!(canvas.transients.images.len(), 1, "the capture only");
    canvas.end_layer();
}

/// A blend-mode layer reserves the backdrop copy and the result; with a
/// chain whose result reads without FLIP_Y the result reuses the capture,
/// with one whose result reads through it a plain result is reserved; on
/// the screen nothing.
#[test]
fn a_blend_mode_layer_reserves_its_backdrop_and_result() {
    use crate::BlendMode;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    let target = canvas
        .create_image_empty(64, 64, PixelFormat::Rgba8, ImageFlags::PREMULTIPLIED)
        .unwrap();
    let brightness = [ImageFilter::brightness(2.0)];
    let blur = [ImageFilter::gaussian_blur(2.0)];
    let two_matrices = [ImageFilter::brightness(2.0), ImageFilter::invert(1.0)];
    // The pool only grows: cases in order of what they hold. A brightness
    // pass leaves its result reading without FLIP_Y, so its blend lands in
    // the capture; a blur (two draws) and two matrices leave it reading
    // through FLIP_Y, so the blend takes a plain result - beside the chain's
    // own scratch, a second FLIP_Y image joins the pool for the target.
    let cases = [
        (LayerEffects::new(), 1),
        (LayerEffects::new().with_filters(&brightness), 2),
        (LayerEffects::new().with_blend(BlendMode::Multiply), 3),
        (
            LayerEffects::new()
                .with_filters(&brightness)
                .with_blend(BlendMode::Multiply),
            3,
        ),
        (
            LayerEffects::new().with_filters(&blur).with_blend(BlendMode::Multiply),
            5,
        ),
        (
            LayerEffects::new()
                .with_filters(&two_matrices)
                .with_blend(BlendMode::Multiply),
            5,
        ),
    ];
    for (effects, images) in cases {
        canvas.set_render_target(RenderTarget::Image(target));
        assert!(canvas.begin_layer(&effects));
        assert_eq!(canvas.transients.images.len(), images, "{effects:?}");
        assert_eq!(canvas.transients.free.len(), 0, "{effects:?}: all held");
        canvas.end_layer();
        assert_eq!(canvas.transients.free.len(), images, "{effects:?}: all returned");
        canvas.set_render_target(RenderTarget::Screen);
        canvas.flush_to_output(());
    }
    let record = {
        assert!(canvas.begin_layer(&LayerEffects::new().with_blend(BlendMode::Multiply)));
        canvas.layers.last().unwrap()
    };
    assert!(record.blend_images.is_none(), "no backdrop to read on the screen");
    assert!(!record.discard, "the layer still captures and composites");
    let held = canvas.transients.images.len() - canvas.transients.free.len();
    assert_eq!(held, 1, "the capture only");
    canvas.end_layer();
}

/// A layer's blend is admitted by the work budget with its backdrop copy
/// counted: on a 480x320 target, exactly the charge admits, one less omits
/// the blend and keeps the capture.
#[test]
fn a_blend_mode_layer_is_admitted_at_the_edge_of_the_work_budget() {
    use crate::BlendMode;
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(480, 320, 1.0);
    let target = canvas
        .create_image_empty(480, 320, PixelFormat::Rgba8, ImageFlags::PREMULTIPLIED)
        .unwrap();
    // The store rounds up to the layer granularity.
    let (width, height) = (
        transient::round_up(480, transient::LAYER_GRANULARITY),
        transient::round_up(320, transient::LAYER_GRANULARITY),
    );
    let work = blend_work(width, height);
    assert_eq!(work, 4 * 512 * 320);
    for (budget, admitted) in [(work, true), (work - 1, false)] {
        canvas.set_filter_work_budget(budget);
        canvas.set_render_target(RenderTarget::Image(target));
        assert!(canvas.begin_layer(&LayerEffects::new().with_blend(BlendMode::Multiply)));
        let record = canvas.layers.last().unwrap();
        assert_eq!(record.blend_images.is_some(), admitted, "budget {budget}");
        assert!(record.image.is_some() && !record.discard, "the capture stays");
        canvas.end_layer();
        canvas.set_render_target(RenderTarget::Screen);
        canvas.flush_to_output(());
    }
}

/// A side pass while a layer is open goes back to the layer's store.
#[test]
fn with_render_target_goes_back_to_the_layers_store() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let image = canvas
        .create_image_empty(8, 8, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    assert!(canvas.begin_layer(&LayerEffects::new()));
    let store = canvas.current_render_target;
    assert!(matches!(store, RenderTarget::Image(_)));
    canvas.with_render_target(RenderTarget::Image(image), |canvas| {
        assert_eq!(canvas.current_render_target, RenderTarget::Image(image));
    });
    assert_eq!(canvas.current_render_target, store);
    canvas.end_layer();
    assert_eq!(canvas.current_render_target, RenderTarget::Screen);
}

/// A layer's store is the canvas's own: deleting or reallocating it is
/// refused, and a flush in between leaves the layer whole.
#[test]
fn a_layers_store_is_not_the_callers_to_delete_or_change() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    let RenderTarget::Image(store) = canvas.current_render_target else {
        panic!("a captured layer draws on its store");
    };
    canvas.delete_image(store);
    assert!(!canvas.pending_image_deletions.contains(&store));
    assert!(matches!(
        canvas.realloc_image(store, 8, 8, PixelFormat::Rgba8, ImageFlags::empty()),
        Err(ErrorKind::ImageIdNotFound)
    ));
    canvas.flush_to_output(());
    assert!(canvas.images.info(store).is_some(), "the store outlives the flush");
    let mut rect = Path::new();
    rect.rect(0.0, 0.0, 8.0, 8.0);
    canvas.fill_path(&rect, &Paint::color(Color::black()));
    canvas.end_layer();
    canvas.flush_to_output(());
}

/// A blend backdrop deleted while its layer is open is read by the
/// composite after the flush in between, as a mask is.
#[test]
fn a_blend_backdrop_deleted_while_its_layer_is_open_outlives_the_composite() {
    let renderer = RecordingRenderer::default();
    let recorded = renderer.last_commands.clone();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    let backdrop = canvas
        .create_image_empty(4, 4, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let blend = ImageFilter::Blend {
        mode: crate::BlendMode::Multiply,
        backdrop,
        x: 0.0,
        y: 0.0,
        width: 4.0,
        height: 4.0,
    };
    assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[blend])));
    assert!(canvas.layers.last().unwrap().filter_images.is_some());
    canvas.delete_image(backdrop);
    canvas.flush_to_output(());
    assert!(canvas.images.info(backdrop).is_some(), "kept for the composite");
    canvas.end_layer();
    canvas.flush_to_output(());
    assert!(
        recorded.borrow().iter().any(|c| c.image == Some(backdrop)),
        "the backdrop was placed"
    );
    assert!(canvas.images.info(backdrop).is_none(), "released after the composite");
}

/// A frame boundary at the same size keeps an open layer capturing (WPT
/// 2d.layer.flush-on-frame-presentation): set_size re-issues the layer's
/// target and the tracked target agrees with the command stream - it used to
/// say the layer while the stream said the screen, and the next frame's
/// draws went to the screen unfaded.
#[test]
fn set_size_keeps_an_open_layer_across_a_frame() {
    use crate::renderer::CommandType;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(200, 200, 1.0);
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    let layer = canvas.layers.last().unwrap().image.unwrap();
    canvas.flush_to_output(());
    canvas.set_size(200, 200, 1.0);
    let targets: Vec<RenderTarget> = canvas
        .commands
        .iter()
        .filter_map(|c| match c.cmd_type {
            CommandType::SetRenderTarget(target) => Some(target),
            _ => None,
        })
        .collect();
    assert_eq!(targets.last(), Some(&RenderTarget::Image(layer)));
    assert_eq!(canvas.current_render_target, RenderTarget::Image(layer));
    canvas.end_layer();
    assert_eq!(canvas.current_render_target, RenderTarget::Screen);
}

/// A resize discards open layers, as a Canvas 2D reset discards pending
/// layers (WPT 2d.layer.reset): the state stack rebalances, the stores return
/// to the pool and drawing continues on the screen.
#[test]
fn set_size_resize_discards_open_layers() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(200, 200, 1.0);
    let depth = canvas.state_stack.len();
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    canvas.set_size(300, 200, 1.0);
    assert!(canvas.layers.is_empty());
    assert_eq!(canvas.state_stack.len(), depth);
    assert_eq!(canvas.current_render_target, RenderTarget::Screen);
    assert_eq!(canvas.transients.free.len(), canvas.transients.images.len());
    canvas.end_layer(); // unbalanced now, ignored
    assert_eq!(canvas.current_render_target, RenderTarget::Screen);
}

#[test]
fn filter_plans_bound_total_work_and_fail_layers_atomically() {
    // A blur of any sigma is a handful of passes; the limit bites on a list
    // of filters that do not fold, a range-unsafe matrix each.
    let past: Vec<ImageFilter> = vec![ImageFilter::brightness(2.0); MAX_FILTER_PASSES + 1];
    assert_eq!(filter_passes(&past[1..]).unwrap().len(), MAX_FILTER_PASSES);
    assert!(filter_passes(&past).is_none());
    assert_eq!(
        filter_passes(&[ImageFilter::gaussian_blur(512.0)]).unwrap().len(),
        MAX_LEVELS + 2
    );

    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let source = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let target = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    assert!(matches!(
        canvas.filter_image_chain(target, &past, source),
        Err(ErrorKind::FilterPassLimitExceeded)
    ));
    assert!(!canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::RenderFilteredImage { .. })));

    let effects = LayerEffects::new().with_filters(&past);
    assert!(canvas.begin_layer(&effects));
    assert!(canvas.layers.last().unwrap().image.is_some());
    assert!(canvas.layers.last().unwrap().filter_images.is_none());
    canvas.end_layer();

    canvas.flush_to_output(());
    let too_many = vec![ImageFilter::identity(); MAX_FILTER_PASSES + 1];
    let effects = LayerEffects::new().with_filters(&too_many);
    assert_eq!(effects.filters.len(), MAX_FILTER_PASSES + 1);
    assert!(canvas.begin_layer(&effects));
    assert!(canvas.layers.last().unwrap().filter_images.is_none());
    canvas.end_layer();
}

#[test]
fn over_budget_layers_preserve_safe_fallbacks() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    canvas.set_filter_work_budget(0);

    let color = LayerEffects::new()
        .with_opacity(0.5)
        .with_filters(&[ImageFilter::brightness(0.5)]);
    assert!(canvas.begin_layer(&color));
    assert!(canvas.layers.last().unwrap().filter_images.is_none());
    assert!(!canvas.layers.last().unwrap().discard);
    canvas.end_layer();

    let turbulence = ImageFilter::Turbulence {
        base_frequency: [0.1, 0.1],
        num_octaves: 1,
        seed: 1,
        stitch_tiles: false,
        kind: TurbulenceKind::Turbulence,
        transform: Transform2D::identity(),
    };
    assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[turbulence])));
    assert!(canvas.layers.last().unwrap().discard);
    canvas.end_layer();
}

#[test]
fn capture_failure_suppresses_masks_but_keeps_an_opacity_fallback() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let mask = canvas
        .create_image_empty(1, 1, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_transient_image_budget(0);

    let masked = LayerEffects::new().with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 64.0, 64.0);
    let commands = canvas.commands.len();
    assert!(canvas.begin_layer(&masked));
    assert!(canvas.layers.last().unwrap().image.is_none());
    assert!(canvas.layers.last().unwrap().discard);
    let mut rect = Path::new();
    rect.rect(0.0, 0.0, 64.0, 64.0);
    canvas.fill_path(&rect, &Paint::color(Color::white()));
    assert_eq!(canvas.commands.len(), commands);
    canvas.end_layer();

    let faded = LayerEffects::new().with_opacity(0.5);
    assert!(!canvas.begin_layer(&faded));
    assert_eq!(canvas.state().alpha, 0.5);
    canvas.end_layer();
    assert_eq!(canvas.state().alpha, 1.0);
}

#[test]
fn a_suppressed_layer_reissues_an_image_target_across_flush() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let target = canvas
        .create_image_empty(64, 64, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let mask = canvas
        .create_image_empty(1, 1, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_render_target(RenderTarget::Image(target));
    canvas.set_transient_image_budget(0);

    let masked = LayerEffects::new().with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 64.0, 64.0);
    assert!(canvas.begin_layer(&masked));
    assert!(canvas.layers.last().unwrap().discard);
    canvas.flush_to_output(());
    canvas.end_layer();

    let mut rect = Path::new();
    rect.rect(0.0, 0.0, 8.0, 8.0);
    canvas.fill_path(&rect, &Paint::color(Color::white()));
    assert!(matches!(
        canvas.commands.first().map(|command| &command.cmd_type),
        Some(CommandType::SetRenderTarget(RenderTarget::Image(id))) if *id == target
    ));
}

#[test]
fn explicit_image_filters_are_not_suppressed_with_layer_draws() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let mask = canvas
        .create_image_empty(1, 1, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let source = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let target = canvas
        .create_image_empty(16, 16, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_transient_image_budget(0);

    let masked = LayerEffects::new().with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 64.0, 64.0);
    assert!(canvas.begin_layer(&masked));
    canvas.filter_image(target, ImageFilter::brightness(0.5), source);
    assert!(canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::RenderFilteredImage { .. })));
    canvas.end_layer();
}

#[test]
fn open_layer_filter_work_survives_a_flush_until_recorded() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let effects = LayerEffects::new().with_filters(&[ImageFilter::gaussian_blur(2.0)]);
    assert!(canvas.begin_layer(&effects));
    let reserved = canvas.layers.last().unwrap().reserved_filter_work;
    assert!(reserved > 0);
    canvas.flush_to_output(());
    assert_eq!(canvas.filter_work, reserved);
    canvas.end_layer();
    assert_eq!(canvas.filter_work, reserved);
    canvas.flush_to_output(());
    assert_eq!(canvas.filter_work, 0);
}

#[test]
fn a_transparent_layer_skips_its_filter_plan() {
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let effects = LayerEffects::new()
        .with_opacity(0.0)
        .with_filters(&[ImageFilter::gaussian_blur(128.0)]);
    assert!(canvas.begin_layer(&effects));
    canvas.end_layer();
    assert_eq!(canvas.filter_work, 0);
    assert!(!canvas
        .commands
        .iter()
        .any(|command| matches!(command.cmd_type, CommandType::RenderFilteredImage { .. })));
}

/// A layer pads its store by the blur's true reach, 3 sigma + 2 per side,
/// since the chain now renders it: sigma 16 pads 50 px where the per-pass
/// bound padded 26, and two of them compound to sqrt(512) = 22.6, 70 px.
#[test]
fn a_layer_pads_by_the_true_blur_reach() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(800, 600, 1.0);
    canvas.save();
    canvas.scissor(100.0, 50.0, 200.0, 200.0);
    for (sigma, pad, store) in [(8.0, 26.0, 256), (16.0, 50.0, 320), (23.0, 71.0, 384)] {
        let blur = ImageFilter::gaussian_blur(sigma);
        assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[blur])));
        let record = canvas.layers.last().unwrap();
        assert_eq!(record.origin, (100.0 - pad, 50.0 - pad), "sigma {sigma}");
        // 200 + 2 * pad, rounded up to 64.
        assert_eq!((record.width, record.height), (store, store), "sigma {sigma}");
        canvas.end_layer();
    }
    let blur = ImageFilter::gaussian_blur(16.0);
    assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[blur, blur])));
    let record = canvas.layers.last().unwrap();
    assert_eq!(record.origin, (30.0, -20.0));
    assert_eq!((record.width, record.height), (384, 384));
    canvas.end_layer();
    canvas.restore();
}

/// An anisotropic blur pads each axis by its own reach: sigma 16 along x
/// alone grows the store by 50 px a side in x and only the fringe in y, and
/// a chain adds each axis's squares separately.
#[test]
fn a_layer_pads_each_axis_by_its_own_reach() {
    use crate::ImageFilter;
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(800, 600, 1.0);
    canvas.save();
    canvas.scissor(100.0, 50.0, 200.0, 200.0);
    let streak = ImageFilter::GaussianBlur {
        sigma_x: 16.0,
        sigma_y: 0.0,
    };
    assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[streak])));
    let record = canvas.layers.last().unwrap();
    assert_eq!(record.origin, (50.0, 48.0));
    // 300 and 204 px, rounded up to 64.
    assert_eq!((record.width, record.height), (320, 256));
    canvas.end_layer();
    let glow = ImageFilter::GaussianBlur {
        sigma_x: 0.0,
        sigma_y: 8.0,
    };
    assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[streak, glow])));
    let record = canvas.layers.last().unwrap();
    assert_eq!(record.origin, (50.0, 24.0));
    assert_eq!((record.width, record.height), (320, 256));
    canvas.end_layer();
    canvas.restore();
}

/// A layer pads its store by a dilation's radius and an offset's shift,
/// which carry content outward as a blur's reach does: a 10 px dilation
/// with a shift of 6 right and 3 up pads 16 + 2 in x and 13 + 2 in y.
#[test]
fn a_layer_pads_by_a_dilation_and_an_offset() {
    use crate::ImageFilter;
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(800, 600, 1.0);
    canvas.save();
    canvas.scissor(100.0, 50.0, 200.0, 200.0);
    let spread = [
        ImageFilter::Morphology {
            radius_x: 10.0,
            radius_y: 10.0,
            operator: MorphologyOperator::Dilate,
        },
        ImageFilter::Offset { dx: 6.0, dy: -3.0 },
    ];
    assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&spread)));
    let record = canvas.layers.last().unwrap();
    assert_eq!(record.origin, (82.0, 35.0));
    // 236 and 230 px, rounded up to 64.
    assert_eq!((record.width, record.height), (256, 256));
    assert!(record.filter_images.is_some());
    canvas.end_layer();
    canvas.restore();
}

/// A filtered layer reads its result through the flag its chain's passes
/// call for: a lone blur leaves the capture's flipped storage as it is,
/// so the result carries FLIP_Y like the capture; a matrix turns it over
/// once, so the result carries none; noise lands the way the capture's
/// own draws did, and a matrix after it turns that over - and none spends
/// a pass on it.
#[test]
fn a_filtered_layer_reads_its_result_through_the_flag_its_passes_need() {
    use crate::{ImageFilter, TurbulenceKind};
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(64, 64, 1.0);
    let noise = ImageFilter::Turbulence {
        base_frequency: [0.1, 0.1],
        num_octaves: 1,
        seed: 1,
        stitch_tiles: false,
        kind: TurbulenceKind::FractalNoise,
        transform: Transform2D::identity(),
    };
    for (filters, flipped) in [
        (&[ImageFilter::gaussian_blur(2.0)][..], true),
        (&[ImageFilter::brightness(0.5)][..], false),
        (
            &[ImageFilter::brightness(2.0), ImageFilter::gaussian_blur(2.0)][..],
            false,
        ),
        (&[ImageFilter::brightness(2.0), ImageFilter::invert(1.0)][..], true),
        (&[noise][..], true),
        (&[noise, ImageFilter::brightness(0.5)][..], false),
    ] {
        assert!(canvas.begin_layer(&LayerEffects::new().with_filters(filters)));
        let target = canvas.layers.last().unwrap().filter_images.as_ref().unwrap().target;
        let flags = canvas.images.info(target).unwrap().flags();
        assert_eq!(flags.contains(ImageFlags::FLIP_Y), flipped, "{filters:?}");
        canvas.end_layer();
        canvas.flush_to_output(());
    }
}

/// A wide blur over a large store fits the default work budget: sigma 77
/// over a 1080 px square (a `stdDeviation="10"` SVG blur at 1080p) is a
/// pyramid of four small passes and a copy, where 93 quadrature passes of
/// 94 taps over the padded store were fifteen giga-samples, past the four
/// the budget allows, so the layer kept its content and dropped its blur.
#[test]
fn a_wide_blur_fits_the_default_work_budget() {
    use crate::ImageFilter;
    let mut canvas = Canvas::new(RecordingRenderer::default()).unwrap();
    canvas.set_size(1920, 1080, 1.0);
    canvas.scissor(420.0, 0.0, 1080.0, 1080.0);
    let blur = ImageFilter::gaussian_blur(77.0);
    let passes = filter_passes(&[blur]).unwrap();
    assert_eq!(
        passes.len(),
        6,
        "four halvings, the blur at a sixteenth, the scale back up"
    );
    let record_size = |canvas: &Canvas<RecordingRenderer>| {
        let record = canvas.layers.last().unwrap();
        (record.width, record.height)
    };
    assert!(canvas.begin_layer(&LayerEffects::new().with_filters(&[blur])));
    let (width, height) = record_size(&canvas);
    assert!(
        canvas.layers.last().unwrap().filter_images.is_some(),
        "the blur is reserved under the default budget"
    );
    let work = filter_work(&passes, width, height);
    assert!(work < DEFAULT_FILTER_WORK_BUDGET / 16, "{work} samples");
    canvas.end_layer();
}

/// A shadowed layer whose reach would push its store past the texture limit
/// keeps the capture it had without the reach - a 2048 px store with a 1 px
/// offset used to round to 2112 and pass through, dropping the mask along
/// with the shadow - and gives the reach whatever room is left: none here,
/// 64 px a side for a 1920 px store under a 60 px blur.
#[test]
fn a_shadowed_layer_at_the_texture_limit_keeps_its_visible_capture() {
    let renderer = RecordingRenderer {
        max_texture_size: 2048,
        ..RecordingRenderer::default()
    };
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(2048, 64, 1.0);
    let mask = canvas
        .create_image_empty(64, 64, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    let masked = LayerEffects::new().with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 2048.0, 64.0);
    canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
    canvas.set_shadow_offset(1.0, 0.0);
    assert!(
        canvas.begin_layer(&masked),
        "the layer captures instead of passing through"
    );
    let record = canvas.layers.last().unwrap();
    assert!(record.image.is_some() && !record.discard, "the mask still applies");
    assert_eq!((record.width, record.height), (2048, 64));
    assert_eq!(record.origin, (0.0, 0.0), "no room: the reach is given up");
    canvas.end_layer();

    canvas.set_size(1920, 1080, 1.0);
    canvas.set_shadow_offset(0.0, 0.0);
    canvas.set_shadow_blur(60.0); // spread 90 a side: 2100 px wide, past the limit
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    let record = canvas.layers.last().unwrap();
    assert!(record.image.is_some());
    assert_eq!((record.width, record.height), (2048, 1280));
    assert_eq!(
        record.origin,
        (-64.0, -90.0),
        "the reach takes the 128 px the limit leaves, shared"
    );
    canvas.end_layer();
}

/// The same under the transient budget: when the store with its reach is
/// refused, the layer takes the visible capture it had before instead of
/// passing through.
#[test]
fn a_shadowed_layer_the_budget_cannot_reach_keeps_its_visible_capture() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(256, 256, 1.0);
    // The visible 256 x 256 store (256 KiB) fits; with 90 px of reach a side (448 x 448) it does not.
    canvas.set_transient_image_budget(300 * 1024);
    canvas.set_shadow_color(Color::rgba(0, 0, 0, 255));
    canvas.set_shadow_blur(60.0);
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    let record = canvas.layers.last().unwrap();
    assert!(record.image.is_some());
    assert_eq!((record.width, record.height, record.origin), (256, 256, (0.0, 0.0)));
    canvas.end_layer();
}

/// A scissor set under a scale still bounds the layer - every canvas drawn
/// under a device-pixel ratio has one - so the store follows the device-space
/// rect, not the whole canvas.
#[test]
fn layer_bounds_follow_a_scaled_scissor() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(800, 600, 1.0);
    canvas.save();
    canvas.scale(2.0, 2.0);
    canvas.scissor(50.0, 25.0, 60.0, 40.0); // device rect (100, 50) 120 x 80
    assert!(canvas.begin_layer(&LayerEffects::new()));
    let record = canvas.layers.last().unwrap();
    assert_eq!((record.width, record.height), (128, 128));
    assert_eq!(record.origin, (100.0, 50.0));
    canvas.end_layer();
    canvas.restore();
}

/// Past the backend's texture limit a layer passes through - drawing keeps
/// landing on the current target and begin/end stay balanced - instead of
/// failing to allocate. A VideoCore IV (Raspberry Pi Zero) reports 2048.
#[test]
fn layers_past_the_texture_limit_pass_through() {
    let renderer = RecordingRenderer {
        max_texture_size: 2048,
        ..RecordingRenderer::default()
    };
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(4096, 64, 1.0);
    assert!(!canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    assert!(canvas.layers.last().unwrap().image.is_none());
    canvas.end_layer();
    assert!(canvas.transients.images.is_empty());
    // A scissor within the limit captures again.
    canvas.save();
    canvas.scissor(0.0, 0.0, 1000.0, 64.0);
    assert!(canvas.begin_layer(&LayerEffects::new().with_opacity(0.5)));
    assert_eq!(canvas.layers.last().unwrap().width, 1024);
    canvas.end_layer();
    canvas.restore();
}

/// Masked sibling layers reuse the mask's transients too: a luminance mask
/// needs the layer capture, the normalized mask and its alpha conversion,
/// once for any number of siblings.
#[test]
fn masked_siblings_reuse_mask_transients() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(200, 120, 1.0);
    let mask = canvas
        .create_image_empty(200, 120, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    for _ in 0..4 {
        assert!(canvas.begin_layer(&LayerEffects::new().with_mask(mask, MaskKind::Luminance, 0.0, 0.0, 200.0, 120.0)));
        assert!(canvas.layers.last().unwrap().image.is_some());
        // Reserved with the store: nothing of the mask's is left to chance
        // at end_layer.
        assert_eq!(
            canvas.transients.images.len(),
            3,
            "capture, normalized mask, converted mask"
        );
        assert!(
            canvas.transients.free.is_empty(),
            "all three are held while the layer is open"
        );
        canvas.end_layer();
        assert_eq!(
            canvas.transients.free.len(),
            3,
            "and returned once its composite is recorded"
        );
    }
    assert_eq!(
        canvas.transients.images.len(),
        3,
        "four masked siblings, one set of images"
    );
    canvas.flush_to_output(());
    assert_eq!(canvas.transients.images.len(), 0);
}

/// A reset or a resize inside a masked layer returns everything the layer
/// held - the capture and the mask's two coverage images - not only the
/// capture: under a budget of exactly those three, the next masked layer
/// takes them back from the pool instead of being refused a fourth.
#[test]
fn a_discarded_masked_layer_returns_its_coverage_images() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    let mask = canvas
        .create_image_empty(64, 64, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    canvas.set_transient_image_budget(4 * 64 * 64 * 4);
    let luminance = LayerEffects::new().with_mask(mask, MaskKind::Luminance, 0.0, 0.0, 64.0, 64.0);
    for what in ["reset", "resize"] {
        assert!(canvas.begin_layer(&luminance), "{what}: the first masked layer fits");
        assert_eq!(canvas.transients.images.len(), 3);
        match what {
            "reset" => canvas.reset(),
            _ => {
                canvas.set_size(128, 64, 1.0);
                canvas.set_size(64, 64, 1.0);
            }
        }
        assert!(canvas.layers.is_empty(), "{what}: the open layer is discarded");
        assert_eq!(
            canvas.transients.free.len(),
            3,
            "{what}: capture, normalized mask and converted mask all return"
        );
        assert!(
            canvas.begin_layer(&luminance),
            "{what}: the next masked layer takes them back"
        );
        assert_eq!(canvas.transients.images.len(), 3, "{what}: no fourth image");
        assert!(canvas.transients.free.is_empty());
        canvas.end_layer();
    }
}

/// A masked layer whose coverage images do not fit captures its draws but
/// discards them rather than revealing the group unmasked.
#[test]
fn a_masked_layer_without_room_for_its_coverage_fails_closed() {
    let renderer = RecordingRenderer::default();
    let mut canvas = Canvas::new(renderer).unwrap();
    canvas.set_size(64, 64, 1.0);
    let mask = canvas
        .create_image_empty(64, 64, PixelFormat::Rgba8, ImageFlags::empty())
        .unwrap();
    // Room for the capture and the normalized mask, not the luminance conversion.
    canvas.set_transient_image_budget(2 * 64 * 64 * 4);
    let luminance = LayerEffects::new().with_mask(mask, MaskKind::Luminance, 0.0, 0.0, 64.0, 64.0);
    assert!(canvas.begin_layer(&luminance));
    assert!(canvas.layers.last().unwrap().image.is_some());
    assert!(canvas.layers.last().unwrap().discard);
    canvas.end_layer();
    // The same budget fits an alpha mask, which needs no conversion.
    let alpha = LayerEffects::new().with_mask(mask, MaskKind::Alpha, 0.0, 0.0, 64.0, 64.0);
    assert!(canvas.begin_layer(&alpha));
    canvas.end_layer();
    canvas.set_transient_image_budget(4 * 64 * 64 * 4);
    assert!(
        canvas.begin_layer(&luminance),
        "three images fit with one capture held in reserve"
    );
    assert!(!canvas.layers.last().unwrap().discard);
    canvas.end_layer();
}

/// `LayerEffects::default()` is the no-op effects `new()` describes, field by
/// field: full opacity, no filters, no mask. A derived Default would zero the
/// opacity, and `end_layer` composites nothing at alpha 0.
#[test]
fn default_layer_effects_are_the_no_op_effects_new_describes() {
    let default = LayerEffects::default();
    let new = LayerEffects::new();
    assert_eq!(default.opacity, 1.0);
    assert_eq!(default.opacity, new.opacity);
    assert!(default.filters.is_empty());
    assert!(new.filters.is_empty());
    assert!(default.mask.is_none());
    assert!(new.mask.is_none());
}
