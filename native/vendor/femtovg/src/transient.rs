//! The pool of transient offscreen images: layer backing stores, filtered
//! results, filter-chain scratches and shadow coverage.
//!
//! A transient lives until the next flush, when every one not held by an open
//! layer is deleted; a layer's images stay across the flush and return to the
//! pool when the layer ends or is discarded.
//! Between a release and that flush an image is free for the next acquire of
//! the same size and flags, so a frame's peak transient memory is what is
//! live at once - nesting depth times layer size - not the sum over every
//! layer, chain and shadow it draws. Reuse needs no synchronization: commands
//! execute in order, so a later layer drawing into an image an earlier
//! layer's composite reads is well-defined, and each consumer clears or fully
//! overwrites the image it takes.

use crate::image::ImageStore;
use crate::renderer::Renderer;
use crate::{ErrorKind, ImageFlags, ImageId, ImageInfo, PixelFormat};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Default)]
pub(crate) struct FreeImages {
    by_info: HashMap<ImageInfo, Vec<ImageId>>,
    ids: HashSet<ImageId>,
}

impl FreeImages {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.ids.len()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    fn contains(&self, id: &ImageId) -> bool {
        self.ids.contains(id)
    }

    fn insert(&mut self, info: ImageInfo, id: ImageId) {
        let inserted = self.ids.insert(id);
        debug_assert!(inserted, "transient released twice");
        self.by_info.entry(info).or_default().push(id);
    }

    fn take_exact(&mut self, info: ImageInfo) -> Option<ImageId> {
        let bucket = self.by_info.get_mut(&info)?;
        let id = bucket.pop()?;
        let empty = bucket.is_empty();
        self.ids.remove(&id);
        if empty {
            self.by_info.remove(&info);
        }
        Some(id)
    }

    fn clear(&mut self) {
        self.by_info.clear();
        self.ids.clear();
    }
}

/// Default cap on live transient memory. A Raspberry Pi Zero's GPU share is
/// 64-128 MB in total, so integrations targeting it set a smaller budget; see
/// [`crate::Canvas::set_transient_image_budget`].
pub(crate) const DEFAULT_BUDGET: usize = 128 * 1024 * 1024;

/// Layer stores round up to this many pixels per axis. Sibling layers whose
/// scissors or blur reaches differ by a few pixels then request the same
/// size and share one pooled store instead of each holding its own; the cost
/// is at most 63 px per axis (a tenth of a 1080 px layer) and the extra area
/// is clipped away at the composite.
pub(crate) const LAYER_GRANULARITY: usize = 64;

/// Shadow coverage rounds more finely. Shadows are many, small and
/// differently sized (text, icons), and their coverage images are cleared and
/// blurred over their whole area: modelled on a frame of 600 glyph-sized
/// shadows, exact sizes allocate 650 images per frame, 8 px rounding 60 (for
/// 18 % more fill), 64 px rounding 8 (for 235 % more fill).
pub(crate) const SHADOW_GRANULARITY: usize = 8;

pub(crate) fn round_up(n: usize, granularity: usize) -> usize {
    debug_assert_ne!(granularity, 0);
    n.checked_add(granularity - 1)
        .and_then(|n| n.checked_div(granularity))
        .and_then(|n| n.checked_mul(granularity))
        .unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::round_up;

    #[test]
    fn round_up_saturates_instead_of_wrapping() {
        assert_eq!(round_up(65, 64), 128);
        assert_eq!(round_up(usize::MAX, 64), usize::MAX);
    }
}

#[derive(Debug)]
pub(crate) struct TransientPool {
    /// Every live transient, deleted at the flush.
    pub(crate) images: Vec<ImageId>,
    /// Transients whose last consumer command has been recorded; a subset of
    /// `images`, taken by the next acquire of the same size and flags.
    pub(crate) free: FreeImages,
    /// Images already referenced by a recorded command in this frame. A
    /// failed reservation may delete only images absent from this set.
    recorded: HashSet<ImageId>,
    /// Backend-estimated allocation charged for `images`.
    bytes: usize,
    budget: usize,
}

impl TransientPool {
    pub(crate) fn new(budget: usize) -> Self {
        Self {
            images: Vec::new(),
            free: FreeImages::default(),
            recorded: HashSet::new(),
            bytes: 0,
            budget,
        }
    }

    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Whether `id` is a live transient: a layer's store or scratch.
    pub(crate) fn owns(&self, id: ImageId) -> bool {
        self.images.contains(&id)
    }

    pub(crate) fn set_budget(&mut self, bytes: usize) {
        self.budget = bytes;
    }

    /// A pooled image of exactly `width` x `height` with `flags` if one is
    /// free, otherwise a fresh RGBA8 image counted against the budget.
    pub(crate) fn acquire<T: Renderer>(
        &mut self,
        images: &mut ImageStore<T::Image>,
        renderer: &mut T,
        width: usize,
        height: usize,
        flags: ImageFlags,
    ) -> Result<ImageId, ErrorKind> {
        self.acquire_reserving(images, renderer, width, height, flags, 0)
    }

    pub(crate) fn acquire_reserving<T: Renderer>(
        &mut self,
        images: &mut ImageStore<T::Image>,
        renderer: &mut T,
        width: usize,
        height: usize,
        flags: ImageFlags,
        headroom: usize,
    ) -> Result<ImageId, ErrorKind> {
        let info = ImageInfo::new(flags, width, height, PixelFormat::Rgba8);
        if self.bytes.saturating_add(headroom) > self.budget {
            return Err(ErrorKind::TransientImageBudgetExceeded);
        }
        if let Some(id) = self.free.take_exact(info) {
            return Ok(id);
        }
        self.allocate(images, renderer, info, headroom)
    }

    fn allocate<T: Renderer>(
        &mut self,
        images: &mut ImageStore<T::Image>,
        renderer: &mut T,
        info: ImageInfo,
        headroom: usize,
    ) -> Result<ImageId, ErrorKind> {
        let bytes = renderer.transient_image_cost(info);
        if self.bytes.saturating_add(bytes).saturating_add(headroom) > self.budget {
            return Err(ErrorKind::TransientImageBudgetExceeded);
        }
        let id = images.alloc(renderer, info)?;
        self.bytes = self.bytes.saturating_add(bytes);
        self.images.push(id);
        Ok(id)
    }

    /// Returns an image to the pool once every command that reads it has
    /// been recorded. Whoever takes it next must clear or overwrite it.
    pub(crate) fn release<I>(&mut self, images: &ImageStore<I>, id: ImageId) {
        debug_assert!(self.images.contains(&id), "released image is not a transient");
        debug_assert!(!self.free.contains(&id), "transient released twice");
        self.recorded.insert(id);
        self.free
            .insert(images.info(id).expect("a transient has image info"), id);
    }

    /// Rolls back an admission that recorded no commands. A fresh image can
    /// be deleted immediately; a reused one may still back an earlier command.
    pub(crate) fn rollback<T: Renderer>(&mut self, images: &mut ImageStore<T::Image>, renderer: &mut T, id: ImageId) {
        debug_assert!(self.images.contains(&id), "rolled back image is not a transient");
        debug_assert!(!self.free.contains(&id), "transient rolled back twice");
        if self.recorded.contains(&id) {
            self.free
                .insert(images.info(id).expect("a transient has image info"), id);
            return;
        }
        if let Some(at) = self.images.iter().position(|&image| image == id) {
            self.images.swap_remove(at);
            self.bytes = self.bytes.saturating_sub(image_cost(images, renderer, id));
            images.remove(renderer, id);
        }
    }

    /// Deletes every transient except those in `held` (the images of layers
    /// still open across the flush), which stay live and in use.
    pub(crate) fn release_all<T: Renderer>(
        &mut self,
        images: &mut ImageStore<T::Image>,
        renderer: &mut T,
        held: &[ImageId],
    ) {
        self.free.clear();
        let held: HashSet<ImageId> = held.iter().copied().collect();
        let mut kept = Vec::new();
        for id in std::mem::take(&mut self.images) {
            if held.contains(&id) {
                kept.push(id);
            } else {
                self.bytes = self.bytes.saturating_sub(image_cost(images, renderer, id));
                self.recorded.remove(&id);
                images.remove(renderer, id);
            }
        }
        self.images = kept;
    }
}

/// Backend-estimated allocation charged for an image.
fn image_cost<I, R: Renderer<Image = I>>(images: &ImageStore<I>, renderer: &R, id: ImageId) -> usize {
    images
        .info(id)
        .map(|info| renderer.transient_image_cost(info))
        .unwrap_or(0)
}
