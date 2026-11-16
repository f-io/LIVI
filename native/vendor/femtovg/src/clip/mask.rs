//! Clips that are no box, as coverage masks: one byte a pixel over the
//! clip's bounds, rasterized on the CPU ([`coverage`](super::coverage)),
//! kept as an image and read by the fragment shader of each draw under the
//! clip. A mask is found again by its outline - the children's points to a
//! 256th of a pixel, counted from the mask's corner - so a clip that has
//! moved by whole pixels costs its flattening and a lookup, and one that is
//! asked for bit for bit as before ([`ClipMasks::asked`]) not even that.

use std::{
    cell::Cell,
    collections::HashMap,
    hash::{BuildHasherDefault, Hasher},
    rc::{Rc, Weak},
};

use super::coverage::{self, Coverage, SUBPIXELS};
use crate::{FillRule, ImageId};

/// A map keyed by a mask's outline or a request for one: words, a few
/// kilobytes of them, hashed eight bytes at a time as rustc's FxHasher does.
/// std's SipHash took a twentieth of the cycles of a frame whose thin fills
/// move; these keys come from the scene's paths, which nothing picks to
/// collide.
pub(crate) type WordMap<K, V> = HashMap<K, V, BuildHasherDefault<WordHasher>>;

#[derive(Default)]
pub(crate) struct WordHasher(u64);

impl WordHasher {
    fn add(&mut self, word: u64) {
        self.0 = self.0.wrapping_add(word).wrapping_mul(0xf135_7aea_2e62_a9c5);
    }
}

impl Hasher for WordHasher {
    fn write(&mut self, bytes: &[u8]) {
        let mut words = bytes.chunks_exact(8);
        for word in &mut words {
            self.add(u64::from_le_bytes(word.try_into().unwrap_or_default()));
        }
        let rest = words.remainder();
        if !rest.is_empty() {
            let mut word = [0; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word));
        }
    }

    fn write_u32(&mut self, value: u32) {
        self.add(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.add(value);
    }

    fn write_usize(&mut self, value: usize) {
        self.add(value as u64);
    }

    fn finish(&self) -> u64 {
        self.0.rotate_left(26)
    }
}

/// What identifies a mask: its size, the mask around it that it was cut by
/// and where in it, and each child's fill rule and contours.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct MaskKey {
    size: [u32; 2],
    parent: Option<(u64, [i32; 2])>,
    outline: Vec<i32>,
}

/// The outlines of a clip in the making: every child's contours in device
/// pixels, and the box around them all.
#[derive(Debug)]
pub(crate) struct MaskOutline {
    children: Vec<(FillRule, Vec<Vec<[f32; 2]>>)>,
    bounds: [f32; 4],
}

impl Default for MaskOutline {
    fn default() -> Self {
        Self {
            children: Vec::new(),
            bounds: [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY],
        }
    }
}

impl MaskOutline {
    /// Adds a child: the contours of one path, filled by its own rule.
    pub(crate) fn child(&mut self, rule: FillRule, contours: impl Iterator<Item = Vec<[f32; 2]>>) {
        let contours: Vec<Vec<[f32; 2]>> = contours.filter(|contour| contour.len() > 2).collect();
        for &[x, y] in contours.iter().flatten() {
            if x.is_finite() && y.is_finite() {
                let [minx, miny, maxx, maxy] = self.bounds;
                self.bounds = [minx.min(x), miny.min(y), maxx.max(x), maxy.max(y)];
            }
        }
        self.children.push((rule, contours));
    }

    /// Swaps the outlines' x and y.
    pub(crate) fn transpose(&mut self) {
        for point in self
            .children
            .iter_mut()
            .flat_map(|(_, contours)| contours.iter_mut().flatten())
        {
            point.swap(0, 1);
        }
        let [minx, miny, maxx, maxy] = self.bounds;
        self.bounds = [miny, minx, maxy, maxx];
    }

    /// The pixels the mask spans: the outlines' bounds and a pixel around
    /// them - the empty border that a draw past the mask reads - cut to
    /// `within`, a rect `[x0, y0, x1, y1]`. An outline that misses it is
    /// left a single pixel there, which nothing covers.
    pub(crate) fn rect(&self, within: [i32; 4]) -> [i32; 4] {
        pixel_rect(self.bounds, within)
    }

    /// The key of this outline as a mask over `rect`, cut by `parent` - a
    /// mask's id and where `rect`'s corner lies in it.
    pub(crate) fn key(&self, rect: [i32; 4], parent: Option<(u64, [i32; 2])>) -> MaskKey {
        let points: usize = self
            .children
            .iter()
            .flat_map(|(_, contours)| contours)
            .map(Vec::len)
            .sum();
        let mut outline = Vec::with_capacity(2 * points + 2 * self.children.len() + 8);
        for (rule, contours) in &self.children {
            outline.push(match rule {
                FillRule::NonZero => 0,
                FillRule::EvenOdd => 1,
            });
            outline.push(contours.len() as i32);
            for contour in contours {
                outline.push(contour.len() as i32);
                for &[x, y] in contour {
                    outline.push(subpixel(x - rect[0] as f32));
                    outline.push(subpixel(y - rect[1] as f32));
                }
            }
        }
        MaskKey {
            size: [(rect[2] - rect[0]) as u32, (rect[3] - rect[1]) as u32],
            parent,
            outline,
        }
    }
}

/// The pixels a mask over `bounds`, `[minx, miny, maxx, maxy]`, spans (see
/// [`MaskOutline::rect`]).
pub(crate) fn pixel_rect(bounds: [f32; 4], within: [i32; 4]) -> [i32; 4] {
    let [minx, miny, maxx, maxy] = bounds;
    // Past a range any target has, and what a float holds exactly.
    let pixel = |v: f32| v.clamp(-16_000_000.0, 16_000_000.0) as i32;
    let rect = [
        (pixel(minx.floor()) - 1).max(within[0]),
        (pixel(miny.floor()) - 1).max(within[1]),
        (pixel(maxx.ceil()) + 1).min(within[2]),
        (pixel(maxy.ceil()) + 1).min(within[3]),
    ];
    if rect[0] < rect[2] && rect[1] < rect[3] {
        rect
    } else {
        [within[0], within[1], within[0] + 1, within[1] + 1]
    }
}

fn subpixel(v: f32) -> i32 {
    // Far outside the mask the point only has to stay on its side of it.
    (v * SUBPIXELS).round().clamp(-1e9, 1e9) as i32
}

impl MaskKey {
    pub(crate) fn size(&self) -> [usize; 2] {
        [self.size[0] as usize, self.size[1] as usize]
    }

    /// Whether `other` has this key's outline and size: the same clip, cut
    /// by another mask or by none.
    pub(crate) fn same_outline(&self, other: &Self) -> bool {
        self.size == other.size && self.outline == other.outline
    }

    /// Rasterizes the outline the key was made of: each child's coverage
    /// under its rule, added up to the children's union.
    pub(crate) fn rasterize(&self, coverage: &mut Coverage) -> Vec<u8> {
        let [width, height] = self.size();
        let mut mask = vec![0; width * height];
        self.rasterize_onto(coverage, &mut mask);
        mask
    }

    /// Rasterizes the outline into `mask`, which it resizes to the key's.
    pub(crate) fn rasterize_into(&self, coverage: &mut Coverage, mask: &mut Vec<u8>) {
        let [width, height] = self.size();
        // Zeroed as a slice, which is a memset, and grown only past its length.
        mask.truncate(width * height);
        mask.fill(0);
        mask.resize(width * height, 0);
        self.rasterize_onto(coverage, mask);
    }

    fn rasterize_onto(&self, coverage: &mut Coverage, mask: &mut [u8]) {
        let [width, height] = self.size();
        coverage.reset(width, height);
        let mut contour = std::mem::take(&mut coverage.points);
        let mut words = self.outline.iter().copied();
        while let (Some(rule), Some(contours)) = (words.next(), words.next()) {
            for _ in 0..contours {
                let points = words.next().unwrap_or(0);
                contour.clear();
                for _ in 0..points {
                    if let (Some(x), Some(y)) = (words.next(), words.next()) {
                        contour.push([x as f32 / SUBPIXELS, y as f32 / SUBPIXELS]);
                    }
                }
                coverage.contour(&contour);
            }
            let rule = if rule == 0 {
                FillRule::NonZero
            } else {
                FillRule::EvenOdd
            };
            coverage.resolve(rule, mask);
        }
        coverage.points = contour;
    }
}

/// A mask as an image, and the pixels it was uploaded from - what a mask
/// taken inside it is cut by.
#[derive(Debug)]
pub(crate) struct MaskImage {
    pub(crate) id: u64,
    pub(crate) image: ImageId,
    pub(crate) key: MaskKey,
    pub(crate) pixels: Vec<u8>,
    used: Cell<u64>,
}

impl MaskImage {
    pub(crate) fn size(&self) -> [usize; 2] {
        self.key.size()
    }

    /// What the mask holds: its pixels, and its image.
    fn bytes(&self) -> usize {
        bytes_of(self.size())
    }

    /// Cuts `pixels`, a mask of `size` whose corner lies at `offset` in
    /// this one, by this one's coverage.
    pub(crate) fn cut(&self, pixels: &mut [u8], size: [usize; 2], offset: [i32; 2]) {
        coverage::intersect(pixels, size, &self.pixels, self.size(), offset);
    }
}

/// The masks in use and in reach: found by key, counted against a budget,
/// and let go once two frames have gone by without a clip that took them -
/// a clip that moves leaves a mask a frame behind it. The images of masks
/// let go wait, a few of them, for a mask of their size to come.
#[derive(Debug)]
pub(crate) struct ClipMasks {
    masks: WordMap<MaskKey, Rc<MaskImage>>,
    asked: WordMap<Vec<u32>, Asked>,
    /// The request being asked, built where the last one was.
    pub(crate) request: Vec<u32>,
    bytes: usize,
    budget: usize,
    frame: u64,
    next_id: u64,
    spare: Vec<(ImageId, [usize; 2])>,
    pub(crate) coverage: Coverage,
}

/// What a clip came to when it was asked for: its mask, and where the
/// mask's corner lies on the target.
#[derive(Debug)]
struct Asked {
    mask: Weak<MaskImage>,
    origin: [i32; 2],
    used: Cell<u64>,
}

/// How many frames a mask is kept after the last clip that took it.
const IDLE_FRAMES: u64 = 2;

/// How many images of masks let go are kept for the masks to come.
const SPARE_IMAGES: usize = 8;

/// A mask's image is this many pixels larger than the mask at most, each
/// way: its sides are rounded up to a multiple of it, so that a clip which
/// changes by a pixel from frame to frame finds an image of its size.
pub(crate) const IMAGE_GRANULARITY: usize = 64;

/// The size of the image a mask of `size` is kept in.
pub(crate) fn image_size(size: [usize; 2]) -> [usize; 2] {
    size.map(|side| side.max(1).div_ceil(IMAGE_GRANULARITY) * IMAGE_GRANULARITY)
}

/// The masks' default budget: sixteen megapixels of them - about eight
/// 1080p frames - each a byte a pixel as an image and another as the pixels
/// it was made from.
pub(crate) const DEFAULT_MASK_BUDGET: usize = 32 << 20;

/// What a mask of `size` holds: its pixels, and its image.
fn bytes_of(size: [usize; 2]) -> usize {
    let [width, height] = image_size(size);
    size[0] * size[1] + width * height
}

impl Default for ClipMasks {
    fn default() -> Self {
        Self {
            masks: WordMap::default(),
            asked: WordMap::default(),
            request: Vec::new(),
            bytes: 0,
            budget: DEFAULT_MASK_BUDGET,
            frame: 0,
            next_id: 1,
            spare: Vec::new(),
            coverage: Coverage::default(),
        }
    }
}

impl ClipMasks {
    pub(crate) fn budget(&self) -> usize {
        self.budget
    }

    /// What the masks and the spare images hold, in bytes.
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Sets the budget, in bytes. Masks over it go once nothing draws
    /// under them; none at all clips every path on the stencil.
    pub(crate) fn set_budget(&mut self, bytes: usize) {
        self.budget = bytes;
    }

    /// The mask of `key`, if it is kept, marked as used in this frame.
    pub(crate) fn get(&self, key: &MaskKey) -> Option<Rc<MaskImage>> {
        let mask = self.masks.get(key)?;
        mask.used.set(self.frame);
        Some(mask.clone())
    }

    /// The mask that `request` came to when a clip last asked with it, and
    /// where the mask's corner lies, if the mask is still kept; both are
    /// marked as used in this frame. A request is the clip bit for bit -
    /// its paths, the transform, the tolerances, the rect the mask may span
    /// and the mask around it - so the same request is the same mask, and
    /// no path has to be flattened to find it.
    pub(crate) fn asked(&self, request: &[u32]) -> Option<(Rc<MaskImage>, [i32; 2])> {
        let asked = self.asked.get(request)?;
        let mask = asked.mask.upgrade()?;
        asked.used.set(self.frame);
        mask.used.set(self.frame);
        Some((mask, asked.origin))
    }

    /// Remembers that `request` came to `mask`, its corner at `origin`.
    pub(crate) fn remember(&mut self, request: Vec<u32>, mask: &Rc<MaskImage>, origin: [i32; 2]) {
        let asked = Asked {
            mask: Rc::downgrade(mask),
            origin,
            used: Cell::new(self.frame),
        };
        self.asked.insert(request, asked);
    }

    /// Whether a mask of `size` fits the budget beside the masks this frame
    /// has used and those a clip still holds: spare images of other sizes
    /// make room for it first, then the other masks, least recently used
    /// first. The images let go for it come back, to be deleted; one of the
    /// mask's own size stays spare, for the mask.
    pub(crate) fn make_room(&mut self, size: [usize; 2]) -> Option<Vec<ImageId>> {
        let fits = image_size(size);
        let image_bytes = fits[0] * fits[1];
        // What the mask adds: its pixels, and its image unless a spare one serves.
        let needs = |spare: &[(ImageId, [usize; 2])]| {
            let served = spare.iter().any(|(_, size)| *size == fits);
            size[0] * size[1] + if served { 0 } else { image_bytes }
        };
        let mut dropped = Vec::new();
        let mut at = 0;
        while self.bytes + needs(&self.spare) > self.budget && at < self.spare.len() {
            if self.spare[at].1 == fits {
                at += 1;
            } else {
                let (image, spare) = self.spare.swap_remove(at);
                self.bytes -= spare[0] * spare[1];
                dropped.push(image);
            }
        }
        if self.bytes + needs(&self.spare) > self.budget {
            let mut idle: Vec<(u64, MaskKey)> = self
                .masks
                .iter()
                .filter(|(_, mask)| mask.used.get() < self.frame && Rc::strong_count(mask) == 1)
                .map(|(key, mask)| (mask.used.get(), key.clone()))
                .collect();
            idle.sort_unstable_by_key(|(used, _)| *used);
            for (_, key) in idle {
                if self.bytes + needs(&self.spare) <= self.budget {
                    break;
                }
                let Some(mask) = self.masks.remove(&key) else {
                    continue;
                };
                self.bytes -= mask.bytes();
                let served = self.spare.iter().any(|(_, size)| *size == fits);
                if image_size(mask.size()) == fits && !served {
                    self.bytes += image_bytes;
                    self.spare.push((mask.image, fits));
                } else {
                    dropped.push(mask.image);
                }
            }
        }
        (self.bytes + needs(&self.spare) <= self.budget).then_some(dropped)
    }

    /// An image let go by another mask that one of `size` fits exactly.
    pub(crate) fn spare_image(&mut self, size: [usize; 2]) -> Option<ImageId> {
        let fits = image_size(size);
        let at = self.spare.iter().position(|(_, spare)| *spare == fits)?;
        let (image, spare) = self.spare.swap_remove(at);
        self.bytes -= spare[0] * spare[1];
        Some(image)
    }

    /// Keeps a mask: `pixels` as `image`, under `key`.
    pub(crate) fn keep(&mut self, key: MaskKey, image: ImageId, pixels: Vec<u8>) -> Rc<MaskImage> {
        let mask = Rc::new(MaskImage {
            id: self.next_id,
            image,
            key: key.clone(),
            pixels,
            used: Cell::new(self.frame),
        });
        self.next_id += 1;
        self.bytes += mask.bytes();
        self.masks.insert(key, mask.clone());
        mask
    }

    /// A frame has been flushed: the masks it drew under are no longer held
    /// by it, and those no clip has taken for two frames are let go. Their
    /// images are spare, but for the ones that come back, to be deleted.
    pub(crate) fn end_frame(&mut self) -> Vec<ImageId> {
        self.frame += 1;
        let frame = self.frame;
        let mut dropped = Vec::new();
        let (spare, bytes) = (&mut self.spare, &mut self.bytes);
        // What was spare for a frame found no mask of its size.
        for (image, size) in spare.drain(..) {
            *bytes -= size[0] * size[1];
            dropped.push(image);
        }
        self.masks.retain(|_, mask| {
            let keep = mask.used.get() + IDLE_FRAMES > frame || Rc::strong_count(mask) > 1;
            if !keep {
                *bytes -= mask.bytes();
                let size = image_size(mask.size());
                if spare.len() < SPARE_IMAGES {
                    *bytes += size[0] * size[1];
                    spare.push((mask.image, size));
                } else {
                    dropped.push(mask.image);
                }
            }
            keep
        });
        // A request goes with its mask, or after as long unasked: a clip
        // that moves asks anew every frame.
        self.asked
            .retain(|_, asked| asked.used.get() + IDLE_FRAMES > frame && asked.mask.strong_count() > 0);
        dropped
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.masks.len()
    }

    #[cfg(test)]
    pub(crate) fn asked_len(&self) -> usize {
        self.asked.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f32, y: f32, size: f32) -> Vec<[f32; 2]> {
        vec![[x, y], [x + size, y], [x + size, y + size], [x, y + size]]
    }

    /// An outline is the same mask wherever it lies, by whole pixels: its
    /// key counts from the mask's corner. A fraction of a pixel away, under
    /// another rule or cut by another mask it is another.
    #[test]
    fn a_key_is_the_outline_from_its_masks_corner() {
        let within = [0, 0, 1000, 1000];
        let key_of = |x: f32, y: f32, rule: FillRule, parent| {
            let mut outline = MaskOutline::default();
            outline.child(rule, std::iter::once(square(x, y, 20.5)));
            let rect = outline.rect(within);
            (rect, outline.key(rect, parent))
        };
        let (rect, key) = key_of(10.25, 30.5, FillRule::NonZero, None);
        assert_eq!(rect, [9, 29, 32, 52], "the bounds and a pixel around them");
        let (moved_rect, moved) = key_of(410.25, 630.5, FillRule::NonZero, None);
        assert_eq!(moved_rect, [409, 629, 432, 652]);
        assert_eq!(key, moved, "whole pixels away");
        assert_ne!(
            key,
            key_of(10.5, 30.5, FillRule::NonZero, None).1,
            "a quarter pixel away"
        );
        assert_ne!(key, key_of(10.25, 30.5, FillRule::EvenOdd, None).1);
        let cut = key_of(10.25, 30.5, FillRule::NonZero, Some((7, [3, 4]))).1;
        assert_ne!(key, cut);
        assert!(key.same_outline(&cut));

        // The mask is cut to its target, and an outline outside it is one
        // empty pixel.
        let mut outline = MaskOutline::default();
        outline.child(FillRule::NonZero, std::iter::once(square(-30.0, 990.0, 50.0)));
        assert_eq!(outline.rect(within), [0, 989, 21, 1000]);
        let mut outside = MaskOutline::default();
        outside.child(FillRule::NonZero, std::iter::once(square(-90.0, 10.0, 50.0)));
        let rect = outside.rect(within);
        assert_eq!(rect, [0, 0, 1, 1]);
        assert_eq!(outside.key(rect, None).rasterize(&mut Coverage::default()), [0]);
        assert_eq!(MaskOutline::default().rect(within), [0, 0, 1, 1], "no outline at all");
    }

    /// A key rasterizes to its children's union, each under its own rule.
    #[test]
    fn a_key_rasterizes_to_the_union_of_its_children() {
        let mut outline = MaskOutline::default();
        // A ring under even-odd, and its hole's left half as a second child.
        outline.child(
            FillRule::EvenOdd,
            [square(2.0, 2.0, 8.0), square(4.0, 4.0, 4.0)].into_iter(),
        );
        outline.child(
            FillRule::NonZero,
            std::iter::once(vec![[4.0, 4.0], [6.0, 4.0], [6.0, 8.0], [4.0, 8.0]]),
        );
        let rect = outline.rect([0, 0, 100, 100]);
        assert_eq!(rect, [1, 1, 11, 11]);
        let mask = outline.key(rect, None).rasterize(&mut Coverage::default());
        let row = |y: usize| &mask[(y - 1) * 10..][..10];
        assert_eq!(row(1), [0; 10], "the border");
        assert_eq!(row(3), [0, 255, 255, 255, 255, 255, 255, 255, 255, 0]);
        assert_eq!(
            row(5),
            [0, 255, 255, 255, 255, 0, 0, 255, 255, 0],
            "the hole's right half"
        );
    }

    /// A request finds the mask it came to while that mask is kept and the
    /// request has not gone a frame unasked; a mask found by a request
    /// counts as used.
    #[test]
    fn a_request_finds_its_mask_while_both_are_kept() {
        let mut canvas = crate::Canvas::new(crate::RecordingRenderer::default()).unwrap();
        let image = canvas
            .create_image_empty(64, 64, crate::PixelFormat::Gray8, crate::ImageFlags::empty())
            .unwrap();
        let mut masks = ClipMasks::default();
        let mut outline = MaskOutline::default();
        outline.child(FillRule::NonZero, std::iter::once(square(1.25, 1.0, 8.0)));
        let key = outline.key(outline.rect([0, 0, 100, 100]), None);
        let pixels = key.rasterize(&mut Coverage::default());
        let mask = masks.keep(key, image, pixels);
        masks.remember(vec![1, 2, 3], &mask, [4, 5]);
        drop(mask);
        assert!(masks.asked(&[1, 2, 4]).is_none(), "another request");
        let (found, origin) = masks.asked(&[1, 2, 3]).unwrap();
        assert_eq!((found.image, origin), (image, [4, 5]));
        drop(found);
        // Asked in every frame, the request keeps its mask in use.
        for _ in 0..4 {
            masks.end_frame();
            assert!(masks.asked(&[1, 2, 3]).is_some());
        }
        masks.end_frame();
        masks.end_frame();
        assert_eq!((masks.len(), masks.asked_len()), (0, 0), "a frame unasked");
        assert!(masks.asked(&[1, 2, 3]).is_none());
    }

    /// Masks are kept up to the budget: one that no clip holds and this
    /// frame has not used makes room, the least recently used first, and its
    /// image serves the mask that comes in its place. A mask no clip has
    /// taken for two frames is let go, its image spare for a frame.
    #[test]
    fn masks_are_kept_up_to_the_budget_and_let_go_when_idle() {
        let mut canvas = crate::Canvas::new(crate::RecordingRenderer::default()).unwrap();
        let mut new_image = || {
            canvas
                .create_image_empty(64, 64, crate::PixelFormat::Gray8, crate::ImageFlags::empty())
                .unwrap()
        };
        let mut masks = ClipMasks::default();
        // An 11 x 10 mask holds its pixels and a 64 x 64 image.
        masks.set_budget(3 * (110 + 64 * 64));
        let key_of = |x: f32| {
            let mut outline = MaskOutline::default();
            outline.child(FillRule::NonZero, std::iter::once(square(x, 1.0, 8.0)));
            outline.key(outline.rect([0, 0, 100, 100]), None)
        };
        let mut keep = |masks: &mut ClipMasks, x: f32| {
            let key = key_of(x);
            let dropped = masks.make_room(key.size())?;
            let pixels = key.rasterize(&mut Coverage::default());
            let image = masks.spare_image(key.size()).unwrap_or_else(&mut new_image);
            Some((masks.keep(key, image, pixels), dropped))
        };
        let (first, _) = keep(&mut masks, 1.25).unwrap();
        let first_image = first.image;
        drop(first);
        let (second, _) = keep(&mut masks, 1.5).unwrap();
        let (third, _) = keep(&mut masks, 1.75).unwrap();
        assert!(keep(&mut masks, 2.25).is_none(), "all three were used in this frame");
        assert_eq!(masks.end_frame(), [], "a frame later all three are still within reach");
        assert!(masks.get(&key_of(1.75)).is_some(), "used again");
        let (fourth, dropped) = keep(&mut masks, 2.25).unwrap();
        assert_eq!(dropped, [], "the first mask's image is not let go:");
        assert_eq!(fourth.image, first_image, "it serves the mask that took its place");
        assert_eq!(masks.len(), 3);
        assert!(keep(&mut masks, 2.5).is_none(), "the second is held by a clip");
        drop(second);
        assert!(keep(&mut masks, 2.5).is_some());

        // Two frames without a clip: let go, the image spare for one frame more.
        let third_image = third.image;
        drop((third, fourth));
        assert_eq!(masks.end_frame(), []);
        assert_eq!(masks.len(), 3, "used a frame ago");
        assert_eq!(masks.end_frame(), []);
        assert_eq!(masks.len(), 0);
        let (again, _) = keep(&mut masks, 1.75).unwrap();
        assert!(
            [third_image, first_image].contains(&again.image) || masks.len() == 1,
            "a spare image serves"
        );
        drop(again);
        let let_go = masks.end_frame();
        assert_eq!(let_go.len(), 2, "the spare images no mask came for");
    }
}
