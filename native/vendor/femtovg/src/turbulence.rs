//! Perlin turbulence lattice for [`ImageFilter::Turbulence`], the SVG
//! `feTurbulence` primitive.
//!
//! The SVG 1.1 specification (Filter Effects, section 15.19) defines
//! `feTurbulence` by reference code: a Park-Miller pseudo-random generator
//! seeds a 256-entry permutation and four tables of 256 unit gradient
//! vectors (one per color channel), and each pixel sums octaves of classic
//! Perlin noise looked up through them. Chromium (Skia's `SkPerlinNoiseShader`)
//! and Firefox both implement that code as written, which is why the two
//! browsers agree on every noise pixel and why this module transcribes it
//! rather than reaching for a different noise.
//!
//! The lattice is built once per seed on the CPU and uploaded as a texture
//! the fragment shader samples. The spec reaches a gradient through two
//! dependent permutation reads (`selector[selector[bx] + by]`); the texture
//! stores that composition already resolved, so every lattice fetch in the
//! shader is addressed straight from the pixel position. That matters on
//! the tile-based mobile GPUs femtovg targets (a Raspberry Pi Zero's
//! VideoCore IV pipelines independent fetches but stalls on dependent ones),
//! and it is what keeps a four-octave, four-channel pixel at 32 fetches
//! instead of 160.

use rgb::RGBA8;

use crate::{ImageFilter, ImageFlags, ImageSource, TurbulenceKind};

/// The composed lattice texture is `LATTICE_WIDTH` x `LATTICE_HEIGHT` texels:
/// two 256x256 tiles side by side, one per pair of color channels.
pub(crate) const LATTICE_WIDTH: usize = 512;
pub(crate) const LATTICE_HEIGHT: usize = 256;

/// Lattices are cached per seed; a seed animated every frame cycles through
/// this many uploads before evicting, so a handful of static seeds stay put.
pub(crate) const LATTICE_CACHE_CAPACITY: usize = 4;

/// Octaves past this one are not summed. Octave `n` contributes at most
/// `1 / 2^n`, so everything skipped changes the sum by less than half of an
/// 8-bit step, and GLES 2.0 needs a constant loop bound to unroll against.
pub(crate) const MAX_OCTAVES: u32 = 10;

// The spec's PRNG constants: a minimal standard generator, m = 2^31 - 1.
const RAND_M: i64 = 2147483647;
const RAND_A: i64 = 16807;
const RAND_Q: i64 = 127773; // m / a
const RAND_R: i64 = 2836; // m % a
const B_SIZE: usize = 0x100;

fn setup_seed(mut seed: i64) -> i64 {
    if seed <= 0 {
        seed = -(seed % (RAND_M - 1)) + 1;
    }
    if seed > RAND_M - 1 {
        seed = RAND_M - 1;
    }
    seed
}

fn random(seed: i64) -> i64 {
    let mut result = RAND_A * (seed % RAND_Q) - RAND_R * (seed / RAND_Q);
    if result <= 0 {
        result += RAND_M;
    }
    result
}

/// The spec's `init(lSeed)`: the permutation and the four gradient tables,
/// before the mirror copy into the upper half, which the composed texture
/// makes unnecessary (see [`lattice_texels`]).
fn build_lattice(seed: i32) -> ([u8; B_SIZE], [[[f64; 2]; B_SIZE]; 4]) {
    let mut seed = setup_seed(seed as i64);
    let mut selector = [0u8; B_SIZE];
    let mut gradient = [[[0.0f64; 2]; B_SIZE]; 4];
    for table in gradient.iter_mut() {
        for (i, g) in table.iter_mut().enumerate() {
            selector[i] = i as u8;
            for component in g.iter_mut() {
                seed = random(seed);
                *component = ((seed % (2 * B_SIZE as i64)) - B_SIZE as i64) as f64 / B_SIZE as f64;
            }
            let len = (g[0] * g[0] + g[1] * g[1]).sqrt();
            g[0] /= len;
            g[1] /= len;
        }
    }
    // `while (--i)`: swap entries 255 down to 1 with a random lower slot.
    for i in (1..B_SIZE).rev() {
        let k = selector[i];
        seed = random(seed);
        let j = (seed % B_SIZE as i64) as usize;
        selector[i] = selector[j];
        selector[j] = k;
    }
    (selector, gradient)
}

/// RGBA8 texels of the composed lattice for `seed`, row-major,
/// [`LATTICE_WIDTH`] x [`LATTICE_HEIGHT`].
///
/// Texel `(bx + 256 * pair, by)` holds the gradients the spec's `noise2`
/// reaches at lattice corner `(bx, by)` for channels `2 * pair` (in `r, g`)
/// and `2 * pair + 1` (in `b, a`): `gradient[channel][selector[selector[bx] +
/// by]]`. Indexing the mirrored upper half of the spec's tables, where
/// `selector[i + by]` runs up to index 510, is the same as indexing
/// `selector[(i + by) & 255]` in the lower half, which is what is stored.
///
/// Each gradient component is a unit-vector coordinate in `[-1, 1]` stored
/// as `round((g + 1) / 2 * 255)`; the shader decodes `texel * 2 - 1`. That
/// quantizes to 1/255 per component, which bounds the noise sum's deviation
/// from the double-precision reference below 2/255 for any octave count and
/// keeps the lattice at one fetch per corner per channel pair on GLES 2.0,
/// which has no float textures to lean on.
pub(crate) fn lattice_texels(seed: i32) -> Vec<RGBA8> {
    let (selector, gradient) = build_lattice(seed);
    let encode = |g: f64| ((g + 1.0) / 2.0 * 255.0).round() as u8;
    let mut texels = vec![RGBA8::default(); LATTICE_WIDTH * LATTICE_HEIGHT];
    for by in 0..B_SIZE {
        for bx in 0..B_SIZE {
            let corner = selector[(selector[bx] as usize + by) & 0xff] as usize;
            for pair in 0..2 {
                let g0 = gradient[2 * pair][corner];
                let g1 = gradient[2 * pair + 1][corner];
                texels[by * LATTICE_WIDTH + bx + pair * B_SIZE] =
                    RGBA8::new(encode(g0[0]), encode(g0[1]), encode(g1[0]), encode(g1[1]));
            }
        }
    }
    texels
}

/// The image flags a lattice texture is created with: it is sampled by texel
/// index, so nearest filtering, and it holds table data rather than color,
/// so no premultiplication convention applies.
pub(crate) fn lattice_flags() -> ImageFlags {
    ImageFlags::NEAREST
}

/// Wraps lattice texels as an image source for upload.
pub(crate) fn lattice_source(texels: &[RGBA8]) -> ImageSource<'_> {
    ImageSource::Rgba(imgref::ImgRef::new(texels, LATTICE_WIDTH, LATTICE_HEIGHT))
}

/// Per-pass turbulence parameters in the form the shader consumes: the
/// frequencies after the stitching adjustment, and the stitch wrap
/// thresholds without the spec's `PerlinN` (4096) offset, which the shader
/// does not need because it uses `floor` and `mod` where the reference code
/// truncates and masks integers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PassParams {
    pub base_frequency: [f32; 2],
    pub stitching: bool,
    /// `[width, height, wrap_x, wrap_y]` of the first octave's stitch tile in
    /// lattice units; all zero when not stitching.
    pub stitch: [f32; 4],
}

/// Derives the shader parameters for a pass whose stitch tile is
/// `tile_size` noise units at `tile_origin`.
///
/// With stitching, the base frequencies are adjusted to an integral number
/// of periods across the tile exactly as the spec's `turbulence()` does
/// before it computes the wrap positions. Without stitching the frequencies
/// are used as given, negative ones as zero.
pub(crate) fn pass_params(
    base_frequency: [f32; 2],
    stitch_tiles: bool,
    tile_origin: [f32; 2],
    tile_size: [f32; 2],
) -> PassParams {
    let mut freq = [base_frequency[0].max(0.0) as f64, base_frequency[1].max(0.0) as f64];
    if !stitch_tiles || !tile_size.iter().all(|size| size.is_finite() && *size > 0.0) {
        return PassParams {
            base_frequency: [freq[0] as f32, freq[1] as f32],
            stitching: false,
            stitch: [0.0; 4],
        };
    }
    let tile = [tile_size[0] as f64, tile_size[1] as f64];
    for axis in 0..2 {
        if freq[axis] != 0.0 {
            let lo = (tile[axis] * freq[axis]).floor() / tile[axis];
            let hi = (tile[axis] * freq[axis]).ceil() / tile[axis];
            freq[axis] = if freq[axis] / lo < hi / freq[axis] { lo } else { hi };
        }
    }
    let width = (tile[0] * freq[0] + 0.5).floor();
    let height = (tile[1] * freq[1] + 0.5).floor();
    // The spec's `wrapX = (fTileX * fBaseFreqX + PerlinN) + nWidth`, compared
    // against the integer-truncated `x * freq + PerlinN`; both sides carry the
    // same 4096, so the shader compares `floor(x * freq)` against this instead.
    let wrap_x = (tile_origin[0] as f64 * freq[0]).floor() + width;
    let wrap_y = (tile_origin[1] as f64 * freq[1]).floor() + height;
    PassParams {
        base_frequency: [freq[0] as f32, freq[1] as f32],
        stitching: true,
        stitch: [width as f32, height as f32, wrap_x as f32, wrap_y as f32],
    }
}

/// The 20 shader parameter slots for a turbulence pass over a `width` x
/// `height` output (see [`ImageFilter::single_pass`]).
///
/// Layout: `[0..6]` the inverse of the noise-to-pixel transform, so the
/// shader can map each pixel back into noise space; `[6..8]` the (stitch-
/// adjusted) base frequency; `[8]` the octave count; `[9]` 1 for
/// `fractalNoise`, 0 for `turbulence`; `[10..14]` the stitch tile's width,
/// height and wrap thresholds; `[14]` 1 when stitching. The stitch tile is
/// the output image mapped into noise space - the SVG default of the
/// primitive subregion being the whole filter region.
pub(crate) fn shader_slots(filter: &ImageFilter, width: f32, height: f32) -> [f32; 20] {
    let ImageFilter::Turbulence {
        base_frequency,
        num_octaves,
        stitch_tiles,
        kind,
        transform,
        ..
    } = *filter
    else {
        debug_assert!(false, "shader_slots is only meaningful for ImageFilter::Turbulence");
        return [0.0; 20];
    };
    let inverse = transform.inverse();
    let corners =
        [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)].map(|(x, y)| inverse.transform_point(x, y));
    let min_x = corners.iter().map(|c| c.0).fold(f32::INFINITY, f32::min);
    let min_y = corners.iter().map(|c| c.1).fold(f32::INFINITY, f32::min);
    let max_x = corners.iter().map(|c| c.0).fold(f32::NEG_INFINITY, f32::max);
    let max_y = corners.iter().map(|c| c.1).fold(f32::NEG_INFINITY, f32::max);
    let pass = pass_params(
        base_frequency,
        stitch_tiles,
        [min_x, min_y],
        [max_x - min_x, max_y - min_y],
    );

    let mut slots = [0.0f32; 20];
    slots[..6].copy_from_slice(&inverse.0);
    slots[6..8].copy_from_slice(&pass.base_frequency);
    slots[8] = num_octaves.min(MAX_OCTAVES) as f32;
    slots[9] = match kind {
        TurbulenceKind::FractalNoise => 1.0,
        TurbulenceKind::Turbulence => 0.0,
    };
    slots[10..14].copy_from_slice(&pass.stitch);
    slots[14] = if pass.stitching { 1.0 } else { 0.0 };
    slots
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Transform2D;

    /// The spec's own check value for its generator: from seed 1, the
    /// 10,000th number is 1043618065.
    #[test]
    fn prng_matches_the_specifications_check_value() {
        let mut seed = setup_seed(1);
        for _ in 0..10_000 {
            seed = random(seed);
        }
        assert_eq!(seed, 1043618065);
    }

    #[test]
    fn lattice_is_a_permutation_of_unit_gradients() {
        let (selector, gradient) = build_lattice(7);
        let mut seen = [false; B_SIZE];
        for &s in &selector {
            assert!(!seen[s as usize], "selector repeats {s}");
            seen[s as usize] = true;
        }
        for table in &gradient {
            for g in table {
                let len = (g[0] * g[0] + g[1] * g[1]).sqrt();
                assert!((len - 1.0).abs() < 1e-12, "gradient not unit length: {len}");
            }
        }
        // Non-positive seeds are remapped by setup_seed, not rejected.
        let _ = build_lattice(0);
        let _ = build_lattice(-5);
        assert_ne!(build_lattice(7).0, build_lattice(11).0);
    }

    #[test]
    fn texels_resolve_the_composed_permutation() {
        let seed = 3;
        let (selector, gradient) = build_lattice(seed);
        let texels = lattice_texels(seed);
        assert_eq!(texels.len(), LATTICE_WIDTH * LATTICE_HEIGHT);
        let decode = |b: u8| b as f64 / 255.0 * 2.0 - 1.0;
        for (bx, by) in [(0usize, 0usize), (17, 200), (255, 255), (128, 1)] {
            let corner = selector[(selector[bx] as usize + by) & 0xff] as usize;
            for pair in 0..2 {
                let t = texels[by * LATTICE_WIDTH + bx + pair * B_SIZE];
                assert!((decode(t.r) - gradient[2 * pair][corner][0]).abs() <= 1.0 / 255.0);
                assert!((decode(t.g) - gradient[2 * pair][corner][1]).abs() <= 1.0 / 255.0);
                assert!((decode(t.b) - gradient[2 * pair + 1][corner][0]).abs() <= 1.0 / 255.0);
                assert!((decode(t.a) - gradient[2 * pair + 1][corner][1]).abs() <= 1.0 / 255.0);
            }
        }
    }

    #[test]
    fn stitching_snaps_frequency_to_whole_periods_across_the_tile() {
        // 0.05 over a 64-wide tile is 3.2 periods; the nearer ratio is 3/64.
        let p = pass_params([0.05, 0.05], true, [0.0, 0.0], [64.0, 64.0]);
        assert!(p.stitching);
        assert!((p.base_frequency[0] - 3.0 / 64.0).abs() < 1e-6);
        assert_eq!(p.stitch, [3.0, 3.0, 3.0, 3.0]);
        // The tile origin shifts the wrap thresholds by its lattice position.
        let q = pass_params([0.05, 0.05], true, [100.0, 0.0], [64.0, 64.0]);
        assert_eq!(q.stitch[2], (100.0f64 * 3.0 / 64.0).floor() as f32 + 3.0);
        // No stitching leaves the frequencies alone and zeroes the tile.
        let n = pass_params([0.05, 0.07], false, [0.0, 0.0], [64.0, 64.0]);
        assert!(!n.stitching);
        assert_eq!(n.base_frequency, [0.05, 0.07]);
        assert_eq!(n.stitch, [0.0; 4]);
        // Negative frequencies are treated as zero, as browsers do.
        assert_eq!(
            pass_params([-1.0, 0.1], false, [0.0; 2], [1.0; 2]).base_frequency[0],
            0.0
        );
    }

    #[test]
    fn slots_carry_the_inverse_transform_and_the_image_as_tile() {
        let filter = ImageFilter::Turbulence {
            base_frequency: [0.05, 0.05],
            num_octaves: 99,
            seed: 1,
            stitch_tiles: true,
            kind: TurbulenceKind::FractalNoise,
            transform: Transform2D::scaling(2.0, 2.0),
        };
        let s = shader_slots(&filter, 128.0, 128.0);
        assert_eq!(&s[..6], &[0.5, 0.0, 0.0, 0.5, 0.0, 0.0]);
        // The 128px image is a 64-unit tile in noise space: 3 periods of 3/64.
        assert!((s[6] - 3.0 / 64.0).abs() < 1e-6);
        assert_eq!(s[8], MAX_OCTAVES as f32);
        assert_eq!(s[9], 1.0);
        assert_eq!(&s[10..15], &[3.0, 3.0, 3.0, 3.0, 1.0]);
    }
}
