//! Thin fills, drawn through coverage masks. A fill's fringe antialiases
//! each edge for itself, so two edges closer than a pixel each add their
//! ramp: a sliver a tenth of a pixel wide comes out nearly eight times as
//! dark as its area (#327). Such a fill is rasterized on the CPU instead,
//! by the clip masks' exact coverage, into one atlas, and drawn as
//! a quad that reads its mask as a glyph's is read: the paint, the scissor
//! and every clip apply to it as they do to text. Skia's Graphite does the
//! same for small paths (`RasterPathAtlas`, at most 256 of them a flush).

use super::mask::{MaskKey, WordMap};
use crate::{path::OutlineWidth, text::Atlas, ImageId};

/// Whether a fill is thin: its mean width - twice its area over its
/// perimeter - under two pixels, or under one if each of its contours is
/// convex. A mark at thumbnail size has hairlines under a pixel beside stems
/// over one, which its mean width hides (#327's New York Times marks at
/// 50 px, 14 and 23 % over-inked); a convex contour a pixel wide and more is
/// drawn by its fringe as before, a rectangle of it exactly.
pub(crate) fn is_thin(width: OutlineWidth) -> bool {
    width.mean < if width.convex { 1.0 } else { 2.0 }
}

/// The most pixels a thin fill's bounds may span to be drawn through a mask.
pub(crate) const THIN_FILL_SPAN: f32 = 65_536.0;

/// The atlas's sides in pixels at first, a byte each: a quarter of a megabyte.
const ATLAS_FIRST: usize = 512;

/// The most either side of the atlas grows to, doubling when a frame's masks
/// find no room: four megabytes at most, more than the thin fills of any
/// corpus frame at 1080p took (1.9 MB).
const ATLAS_MOST: usize = 2048;

/// The most masks rasterized in a frame, as Skia's 256 a flush: past it a
/// thin fill is filled as any other, so that a scene whose thin fills move
/// every frame does not rasterize and upload them all again each time.
const MASKS_A_FRAME: usize = 256;

/// The most requests remembered bit for bit.
const ASKED_KEPT: usize = 4096;

/// Start-overs in a row whose masks would have fitted a smaller atlas before
/// it is made smaller: one sliver on its way through the scene does not keep
/// it long, nor does a scene whose masks alternate make and remake it.
const SMALLER_AFTER: usize = 8;

/// The most room kept between frames for the rows of an upload: more than a
/// frame's masks in motion at 1080p take, less than an atlas's first upload.
const UPLOAD_KEPT: usize = 1 << 18;

/// What the atlas has for a new mask.
pub(crate) enum Placement {
    At([u32; 2]),
    /// No room, but the atlas can grow.
    Grow,
    Refused,
}

/// Where a mask lies in the atlas, the rect of the target it covers, and
/// whether it is kept there transposed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlacedMask {
    pub(crate) atlas: [u32; 2],
    pub(crate) rect: [i32; 4],
    pub(crate) transposed: bool,
}

/// The atlas and what it holds.
#[derive(Debug)]
pub(crate) struct FillMasks {
    enabled: bool,
    pub(crate) image: Option<ImageId>,
    packer: Atlas,
    masks: WordMap<MaskKey, [u32; 2]>,
    /// The masks of requests asked for bit for bit as before: the fill rule,
    /// the path's words, the transform, the tolerances and the target's size.
    asked: WordMap<Vec<u32>, PlacedMask>,
    /// The request being asked, built where the last one was: a still frame
    /// finds every one again, and allocates none.
    pub(crate) request: Vec<u32>,
    made: usize,
    /// Masks the frame found again, by request or by outline.
    found: usize,
    /// Masks the frame before made that the atlas still holds.
    made_before: usize,
    full: bool,
    // The atlas's image could not be made or written this frame: no mask is
    // found or placed until it starts over.
    failed: bool,
    // The atlas's width and height.
    size: [usize; 2],
    // The largest side the atlas grows to: ATLAS_MOST, or less if the backend
    // cannot allocate that.
    most: usize,
    // The images of atlases outgrown this frame, freed once it is drawn.
    retired: Vec<ImageId>,
    // How far right and down the masks placed since the atlas started reach.
    reach: [usize; 2],
    // Start-overs in a row that needed a smaller atlas, and the most of it.
    smaller: (usize, [usize; 2]),
    coverage: super::coverage::Coverage,
    // A mask's pixels on their way into the atlas, kept for the next.
    scratch: Vec<u8>,
    /// The atlas's rows down to the lowest mask, which the masks a frame
    /// makes are written into and uploaded from once, as the rectangle they
    /// span, when it is drawn: the rows below hold none, and are not kept.
    pub(crate) pixels: Vec<u8>,
    /// That rectangle, `[x0, y0, x1, y1]`, and its rows side by side.
    dirty: Option<[usize; 4]>,
    upload: Vec<u8>,
    /// Every mask rasterized, for the tests.
    #[cfg(test)]
    pub(crate) rasterized: usize,
    /// Every outline keyed to be found or made, for the tests.
    #[cfg(test)]
    pub(crate) keyed: usize,
}

impl Default for FillMasks {
    fn default() -> Self {
        Self {
            enabled: true,
            image: None,
            packer: Atlas::new(ATLAS_FIRST, ATLAS_FIRST),
            masks: WordMap::default(),
            asked: WordMap::default(),
            request: Vec::new(),
            made: 0,
            found: 0,
            made_before: 0,
            full: false,
            failed: false,
            size: [ATLAS_FIRST; 2],
            most: ATLAS_MOST,
            retired: Vec::new(),
            reach: [0; 2],
            smaller: (0, [0; 2]),
            coverage: Default::default(),
            scratch: Vec::new(),
            pixels: Vec::new(),
            dirty: None,
            upload: Vec::new(),
            #[cfg(test)]
            rasterized: 0,
            #[cfg(test)]
            keyed: 0,
        }
    }
}

impl FillMasks {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub(crate) fn size(&self) -> [usize; 2] {
        self.size
    }

    /// Holds the atlas within the backend's largest image, before the first
    /// atlas is made: 2048 a side on a VideoCore IV (Raspberry Pi Zero to 3)
    /// is ATLAS_MOST already, but a backend may allow less.
    pub(crate) fn limit(&mut self, max_texture_size: usize) {
        self.most = ATLAS_MOST.min(max_texture_size).max(1);
        if self.size.iter().any(|&side| side > self.most) {
            self.size = self.size.map(|side| side.min(self.most));
            self.packer.reset(self.size[0], self.size[1]);
        }
    }

    pub(crate) fn asked(&mut self, request: &[u32]) -> Option<PlacedMask> {
        let mask = self.asked.get(request).copied().filter(|_| !self.failed);
        self.found += usize::from(mask.is_some());
        mask
    }

    pub(crate) fn remember(&mut self, request: Vec<u32>, mask: PlacedMask) {
        // A scene moved by whole pixels finds its masks by their outlines,
        // and asks anew every frame.
        if self.asked.len() >= ASKED_KEPT {
            self.asked.clear();
        }
        self.asked.insert(request, mask);
    }

    pub(crate) fn get(&mut self, key: &MaskKey) -> Option<[u32; 2]> {
        let at = self.masks.get(key).copied().filter(|_| !self.failed);
        self.found += usize::from(at.is_some());
        at
    }

    /// Whether a new mask of `size` could be placed this frame: the frame has
    /// not made its share, nor failed to make the atlas, and the atlas at its
    /// largest holds it. Asked before a fill's outline is built, so that the
    /// fills past the frame's share cost no more than a fringe.
    pub(crate) fn may_make(&self, size: [usize; 2]) -> bool {
        !self.failed && self.made < MASKS_A_FRAME && size[0] <= self.most && size[1] <= self.most
    }

    /// The atlas's image could not be made or written: the frame finds and
    /// places no more masks, leaving its thin fills to their fringe, and the
    /// atlas starts over once the frame is drawn.
    pub(crate) fn fail(&mut self) {
        self.failed = true;
        self.full = true;
    }

    /// A place in the atlas for a new mask of `size`, if the frame has not
    /// made its share of masks and the atlas has room or can grow; a mask
    /// that does not fit an atlas at its largest marks it full, to start
    /// over once the frame is drawn, and the smaller masks after it still
    /// take the room there is.
    pub(crate) fn place(&mut self, size: [usize; 2]) -> Placement {
        if !self.may_make([size[0], size[1]]) {
            return Placement::Refused;
        }
        let fits = size[0] <= self.size[0] && size[1] <= self.size[1];
        let Some((x, y)) = fits.then(|| self.packer.add_rect(size[0], size[1])).flatten() else {
            if self.size.iter().any(|&side| side < self.most) {
                return Placement::Grow;
            }
            self.full = true;
            return Placement::Refused;
        };
        self.made += 1;
        self.reach = [self.reach[0].max(x + size[0]), self.reach[1].max(y + size[1])];
        #[cfg(test)]
        {
            self.rasterized += 1;
        }
        Placement::At([x as u32, y as u32])
    }

    /// Grows the atlas for a mask of `size`: a side the mask is longer than
    /// doubles until it is not, so that a long sliver makes the atlas wide,
    /// not wide and tall; a mask that fits but finds no room doubles the
    /// shorter side. Each mask is kept where it lies: the image is let go,
    /// freed once the frame is drawn, and the next one made of the pixels kept
    /// here. Masks made this frame must be in it before. An atlas that holds
    /// no masks - not made yet, or started over - grows empty.
    pub(crate) fn grow(&mut self, size: [usize; 2]) {
        let old = self.size;
        let mut new = old;
        if size[0] > old[0] || size[1] > old[1] {
            for (side, needed) in new.iter_mut().zip(size) {
                while *side < needed {
                    *side = (2 * *side).min(self.most);
                }
            }
        } else {
            let taller = old[1] < self.most && (old[1] <= old[0] || old[0] == self.most);
            let side = &mut new[usize::from(taller)];
            *side = (2 * *side).min(self.most);
        }
        if self.masks.is_empty() {
            self.packer = Atlas::new(new[0], new[1]);
            self.pixels.clear();
        } else {
            if new[0] != old[0] {
                let mut pixels = vec![0; new[0] * self.pixels.len() / old[0]];
                for (row, line) in self.pixels.chunks_exact(old[0]).enumerate() {
                    pixels[row * new[0]..][..old[0]].copy_from_slice(line);
                }
                self.pixels = pixels;
            }
            // The masks held where they lie, the new room beside and below them.
            self.packer.expand(new[0], new[1]);
        }
        self.dirty = None;
        self.size = new;
        self.retired.extend(self.image.take());
    }

    /// Rasterizes the mask of `key` and keeps it at `at`.
    pub(crate) fn make(&mut self, key: MaskKey, at: [u32; 2]) {
        let mut pixels = std::mem::take(&mut self.scratch);
        key.rasterize_into(&mut self.coverage, &mut pixels);
        self.keep(key, at, &pixels);
        self.scratch = pixels;
    }

    /// Keeps the mask of `key`, its rows `pixels`, at `at`.
    fn keep(&mut self, key: MaskKey, at: [u32; 2], pixels: &[u8]) {
        let [width, height] = key.size();
        let [x, y] = at.map(|side| side as usize);
        if self.pixels.len() < (y + height) * self.size[0] {
            self.pixels.resize((y + height) * self.size[0], 0);
        }
        for (row, line) in pixels.chunks_exact(width).enumerate() {
            let start = (y + row) * self.size[0] + x;
            self.pixels[start..start + width].copy_from_slice(line);
        }
        let [x0, y0, x1, y1] = self.dirty.unwrap_or([x, y, x + width, y + height]);
        self.dirty = Some([x0.min(x), y0.min(y), x1.max(x + width), y1.max(y + height)]);
        self.masks.insert(key, at);
    }

    /// The masks the atlas holds, for the tests.
    #[cfg(test)]
    pub(crate) fn held(&self) -> usize {
        self.masks.len()
    }

    /// The rectangle written since the last upload: its corner, its size
    /// and its rows side by side. Not the atlas's whole rows: a tall mask
    /// remade each frame of a moving scene would upload two megabytes.
    pub(crate) fn take_upload(&mut self) -> Option<([usize; 2], [usize; 2], &[u8])> {
        let [x0, y0, x1, y1] = self.dirty.take()?;
        self.upload.clear();
        for row in y0..y1 {
            let start = row * self.size[0];
            self.upload.extend_from_slice(&self.pixels[start + x0..start + x1]);
        }
        Some(([x0, y0], [x1 - x0, y1 - y0], &self.upload))
    }

    /// The frame is drawn: an atlas that filled starts over, its masks made
    /// again as the frames that follow ask for them. So does one whose frame
    /// made at least as many masks as it found, after a frame that made some
    /// too - a scene in motion, whose masks no frame asks for again, if one
    /// still thin fill among moving ones finds its own - so that the atlas
    /// does not grow to pack them frame after frame. Returns the images of
    /// the atlases the frame outgrew, to be freed.
    pub(crate) fn end_frame(&mut self) -> Vec<ImageId> {
        if self.made > 0 && self.made_before > 0 && self.made >= self.found {
            self.full = true;
        }
        self.made_before = self.made;
        self.made = 0;
        self.found = 0;
        if self.upload.capacity() > UPLOAD_KEPT {
            self.upload = Vec::new();
        }
        if self.full {
            self.full = false;
            self.failed = false;
            self.made_before = 0;
            let needed = self
                .reach
                .map(|side| side.next_power_of_two().max(ATLAS_FIRST).min(self.most));
            let (count, most) = self.smaller;
            self.smaller = if needed[0] < self.size[0] || needed[1] < self.size[1] {
                (count + 1, [most[0].max(needed[0]), most[1].max(needed[1])])
            } else {
                (0, [0; 2])
            };
            if self.smaller.0 >= SMALLER_AFTER {
                let size = [self.smaller.1[0].min(self.size[0]), self.smaller.1[1].min(self.size[1])];
                self.smaller = (0, [0; 2]);
                self.size = size;
                self.retired.extend(self.image.take());
                // The rows' room goes with the larger atlas.
                self.pixels = Vec::new();
            } else {
                self.pixels.clear();
            }
            self.packer.reset(self.size[0], self.size[1]);
            self.masks.clear();
            self.asked.clear();
            self.reach = [0; 2];
            self.dirty = None;
        }
        std::mem::take(&mut self.retired)
    }
}
