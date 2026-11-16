// The image filter passes: color matrix, feTurbulence, the sRGB transfer
// curves, feBlend, feMorphology and feOffset. Included after shader.wgsl.

// The 4x5 color matrix in the scissor/paint matrix slots (dead during a
// filter pass: scissor_mat columns 0..2 hold the first 12 values, paint_mat
// columns 0..1 the last 8) applied to a premultiplied color: in
// unpremultiplied sRGB space, clamped to [0,1], re-premultiplied -
// unpremultiplying avoids edge halos and the clamp keeps overflowing
// matrices from producing out-of-range/NaN. The color-matrix pass and the
// second draw of a two-draw filter with a matrix fused into it share it.
fn colorMatrixOn(premultiplied: vec4<f32>, params: Params) -> vec4<f32> {
    var c: vec4<f32> = premultiplied;
    if (c.a > 0.0) {
        c = vec4<f32>(c.rgb / c.a, c.a);
    }
    let m0 = params.scissor_mat[0];
    let m1 = params.scissor_mat[1];
    let m2 = params.scissor_mat[2];
    let m3 = params.paint_mat[0];
    let m4 = params.paint_mat[1];
    let r = m0.x * c.r + m0.y * c.g + m0.z * c.b + m0.w * c.a + m1.x;
    let g = m1.y * c.r + m1.z * c.g + m1.w * c.b + m2.x * c.a + m2.y;
    let b = m2.z * c.r + m2.w * c.g + m3.x * c.b + m3.y * c.a + m3.z;
    let a = m3.w * c.r + m4.x * c.g + m4.y * c.b + m4.z * c.a + m4.w;
    let outc = clamp(vec4<f32>(r, g, b, a), vec4<f32>(0.0), vec4<f32>(1.0));
    return vec4<f32>(outc.rgb * outc.a, outc.a);
}

fn renderColorMatrix(vertex: VertexOutput, params: Params) -> vec4<f32> {
    return colorMatrixOn(textureSample(image_texture, image_sampler, vertex.fpos.xy / params.extent), params);
}

// SVG feTurbulence (SVG 1.1 section 15.19), all four channels at once. The
// image texture is the lattice built by src/turbulence.rs: texel (bx, by) of
// the left 256 columns holds the unit gradient vectors of channels 0 and 1 at
// lattice corner (bx, by), the right 256 columns those of channels 2 and 3,
// each component stored as (g + 1) / 2. The reference code's integer
// truncation and 0xff mask become floor() and a floored modulo, the same
// arithmetic as the GLSL ES 1.00 shader so both backends agree, and the
// PerlinN offset that kept its integers positive drops out, so the stitch
// thresholds arrive from Rust without it.
const TURBULENCE_MAX_OCTAVES: f32 = 10.0;

fn turbulenceWrap(corner: vec2<f32>) -> vec2<f32> {
    return corner - 256.0 * floor(corner / 256.0);
}

fn turbulenceGradients(corner: vec2<f32>, pair: f32) -> vec4<f32> {
    let uv = vec2<f32>(corner.x + pair * 256.0 + 0.5, corner.y + 0.5) / vec2<f32>(512.0, 256.0);
    return textureSample(image_texture, image_sampler, uv) * 2.0 - 1.0;
}

// One octave of classic Perlin noise at v: the spec's noise2() for the four
// channels. stitch is (tile width, tile height, wrap x, wrap y) in lattice units.
fn turbulenceOctave(v: vec2<f32>, stitch: vec4<f32>, stitching: bool) -> vec4<f32> {
    var b0 = floor(v);
    let r0 = v - b0;
    var b1 = b0 + 1.0;
    let r1 = r0 - 1.0;
    if (stitching) {
        if (b0.x >= stitch.z) { b0.x -= stitch.x; }
        if (b1.x >= stitch.z) { b1.x -= stitch.x; }
        if (b0.y >= stitch.w) { b0.y -= stitch.y; }
        if (b1.y >= stitch.w) { b1.y -= stitch.y; }
    }
    b0 = turbulenceWrap(b0);
    b1 = turbulenceWrap(b1);
    let c10 = vec2<f32>(b1.x, b0.y);
    let c01 = vec2<f32>(b0.x, b1.y);
    let gA00 = turbulenceGradients(b0, 0.0);
    let gB00 = turbulenceGradients(b0, 1.0);
    let gA10 = turbulenceGradients(c10, 0.0);
    let gB10 = turbulenceGradients(c10, 1.0);
    let gA01 = turbulenceGradients(c01, 0.0);
    let gB01 = turbulenceGradients(c01, 1.0);
    let gA11 = turbulenceGradients(b1, 0.0);
    let gB11 = turbulenceGradients(b1, 1.0);
    let r10 = vec2<f32>(r1.x, r0.y);
    let r01 = vec2<f32>(r0.x, r1.y);
    let u00 = vec4<f32>(dot(r0, gA00.xy), dot(r0, gA00.zw), dot(r0, gB00.xy), dot(r0, gB00.zw));
    let u10 = vec4<f32>(dot(r10, gA10.xy), dot(r10, gA10.zw), dot(r10, gB10.xy), dot(r10, gB10.zw));
    let u01 = vec4<f32>(dot(r01, gA01.xy), dot(r01, gA01.zw), dot(r01, gB01.xy), dot(r01, gB01.zw));
    let u11 = vec4<f32>(dot(r1, gA11.xy), dot(r1, gA11.zw), dot(r1, gB11.xy), dot(r1, gB11.zw));
    let s = r0 * r0 * (3.0 - 2.0 * r0);
    return mix(mix(u00, u10, s.x), mix(u01, u11, s.x), s.y);
}

fn renderTurbulence(vertex: VertexOutput, params: Params) -> vec4<f32> {
    // Slots (see ImageFilter::single_pass): scissor_mat[0] = inverse transform
    // a b c d, scissor_mat[1] = e f and the base frequency, scissor_mat[2] =
    // octaves, fractal flag and the stitch tile size, paint_mat[0] = the stitch
    // wrap thresholds and flag. Each output pixel is evaluated at its integer
    // index mapped into noise space.
    let m0 = params.scissor_mat[0];
    let m1 = params.scissor_mat[1];
    let m2 = params.scissor_mat[2];
    let m3 = params.paint_mat[0];
    let px = floor(vertex.fpos.xy);
    let p = vec2<f32>(m0.x * px.x + m0.z * px.y + m1.x, m0.y * px.x + m0.w * px.y + m1.y);
    var v = p * m1.zw;
    let octaves = m2.x;
    let fractal = m2.y > 0.5;
    var stitch = vec4<f32>(m2.zw, m3.xy);
    let stitching = m3.z > 0.5;
    var sum = vec4<f32>(0.0);
    var ratio: f32 = 1.0;
    for (var o: f32 = 0.0; o < TURBULENCE_MAX_OCTAVES; o += 1.0) {
        // Constant loop bound with the octave count breaking out, as in the
        // GLES 2.0 shader, so both backends sum the same octaves.
        if (o >= octaves) {
            break;
        }
        let n = turbulenceOctave(v, stitch, stitching);
        sum += select(abs(n), n, fractal) / ratio;
        v *= 2.0;
        ratio *= 2.0;
        stitch *= 2.0;
    }
    let c = clamp(select(sum, (sum + 1.0) / 2.0, fractal), vec4<f32>(0.0), vec4<f32>(1.0));
    return vec4<f32>(c.rgb * c.a, c.a);
}

// The sRGB transfer curve as a pass (see transferColor): scissor_mat[0].x >
// 0.5 converts linearRGB to sRGB, otherwise the reverse.
fn renderTransfer(vertex: VertexOutput, params: Params) -> vec4<f32> {
    let c = textureSample(image_texture, image_sampler, vertex.fpos.xy / params.extent);
    return transferColor(c, params.scissor_mat[0].x > 0.5);
}


// SVG feBlend: the image over the backdrop bound in the glyph-texture slot.
// Slot 0 is the BlendMode index, slot 1 whether the backdrop is stored the
// other way up from the image at this pass, slot 2 the alpha the image is
// scaled by first, slot 3 whether to write the image's contribution over
// the backdrop (what source-over onto it adds) instead of the result. Both
// textures are premultiplied;
// the blend function B(Cb, Cs) of the Compositing and Blending spec runs on
// the unpremultiplied colors and the result is composited as
// cs * (1 - ab) + cb * (1 - as) + as * ab * B, alpha as = as + ab - as * ab.
fn blendLum(c: vec3<f32>) -> f32 {
    return 0.3 * c.r + 0.59 * c.g + 0.11 * c.b;
}

fn blendClipColor(c: vec3<f32>) -> vec3<f32> {
    let l = blendLum(c);
    let n = min(c.r, min(c.g, c.b));
    let x = max(c.r, max(c.g, c.b));
    var o = c;
    if (n < 0.0) {
        o = l + (c - l) * l / (l - n);
    }
    if (x > 1.0) {
        o = l + (o - l) * (1.0 - l) / (x - l);
    }
    return o;
}

fn blendSetLum(c: vec3<f32>, l: f32) -> vec3<f32> {
    return blendClipColor(c + (l - blendLum(c)));
}

fn blendSat(c: vec3<f32>) -> f32 {
    return max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b));
}

fn blendSetSat(c: vec3<f32>, s: f32) -> vec3<f32> {
    let mn = min(c.r, min(c.g, c.b));
    let mx = max(c.r, max(c.g, c.b));
    if (mx > mn) {
        return (c - mn) * s / (mx - mn);
    }
    return vec3<f32>(0.0);
}

fn blendHardLight(cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    let multiply = cb * (2.0 * cs);
    let cs2 = 2.0 * cs - 1.0;
    let screen = cb + cs2 - cb * cs2;
    return select(screen, multiply, cs <= vec3<f32>(0.5));
}

fn blendColorDodge(cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    let dodge = min(vec3<f32>(1.0), cb / max(1.0 - cs, vec3<f32>(1e-6)));
    let lit = select(dodge, vec3<f32>(1.0), cs >= vec3<f32>(1.0));
    return select(lit, vec3<f32>(0.0), cb <= vec3<f32>(0.0));
}

fn blendColorBurn(cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    let burn = 1.0 - min(vec3<f32>(1.0), (1.0 - cb) / max(cs, vec3<f32>(1e-6)));
    let dark = select(burn, vec3<f32>(0.0), cs <= vec3<f32>(0.0));
    return select(dark, vec3<f32>(1.0), cb >= vec3<f32>(1.0));
}

fn blendSoftLight(cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    let d = select(sqrt(cb), ((16.0 * cb - 12.0) * cb + 4.0) * cb, cb <= vec3<f32>(0.25));
    let lo = cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb);
    let hi = cb + (2.0 * cs - 1.0) * (d - cb);
    return select(hi, lo, cs <= vec3<f32>(0.5));
}

fn blendMode(mode: i32, cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    switch (mode) {
        case 0: { return cs; }
        case 1: { return cb * cs; }
        case 2: { return cb + cs - cb * cs; }
        case 3: { return blendHardLight(cs, cb); }
        case 4: { return min(cb, cs); }
        case 5: { return max(cb, cs); }
        case 6: { return blendColorDodge(cb, cs); }
        case 7: { return blendColorBurn(cb, cs); }
        case 8: { return blendHardLight(cb, cs); }
        case 9: { return blendSoftLight(cb, cs); }
        case 10: { return abs(cb - cs); }
        case 11: { return cb + cs - 2.0 * cb * cs; }
        case 12: { return blendSetLum(blendSetSat(cs, blendSat(cb)), blendLum(cb)); }
        case 13: { return blendSetLum(blendSetSat(cb, blendSat(cs)), blendLum(cb)); }
        case 14: { return blendSetLum(cs, blendLum(cb)); }
        default: { return blendSetLum(cb, blendLum(cs)); }
    }
}

fn renderBlend(vertex: VertexOutput, params: Params) -> vec4<f32> {
    let uv = vertex.fpos.xy / params.extent;
    let buv = select(uv, vec2<f32>(uv.x, 1.0 - uv.y), params.scissor_mat[0].y > 0.5);
    let src = textureSample(image_texture, image_sampler, uv) * params.scissor_mat[0].z;
    let bd = textureSample(glyph_texture, glyph_sampler, buv);
    // Mode 16, plus-lighter, adds premultiplied color and alpha; it has no
    // blend function. Layers composite it with CompositeOperation::Lighter
    // instead of this pass, so it never asks for a contribution.
    if (i32(params.scissor_mat[0].x) == 16) {
        return min(src + bd, vec4<f32>(1.0));
    }
    var cs = src.rgb;
    if (src.a > 0.0) {
        cs = src.rgb / src.a;
    }
    var cb = bd.rgb;
    if (bd.a > 0.0) {
        cb = bd.rgb / bd.a;
    }
    let mode = i32(params.scissor_mat[0].x);
    let b = clamp(blendMode(mode, clamp(cb, vec3<f32>(0.0), vec3<f32>(1.0)), clamp(cs, vec3<f32>(0.0), vec3<f32>(1.0))), vec3<f32>(0.0), vec3<f32>(1.0));
    let contribution = src.rgb * (1.0 - bd.a) + src.a * bd.a * b;
    if (params.scissor_mat[0].w > 0.5) {
        return vec4<f32>(contribution, src.a);
    }
    return vec4<f32>(contribution + bd.rgb * (1.0 - src.a), src.a + bd.a - src.a * bd.a);
}

// SVG feMorphology, one axis per draw like the blur: each pixel takes the
// per-channel maximum (dilate) or minimum (erode) of the premultiplied
// pixels within the radius along image_blur_filter_direction, every tap
// through blurTap, so beyond the image reads transparent and an erosion
// eats into the border. The radius rides image_blur_filter_sigma (whole
// pixels, at most 24, the loop's constant bound) and the operator
// image_blur_filter_coeff.x (1 dilates).
fn renderMorphology(vertex: VertexOutput, params: Params) -> vec4<f32> {
    let radius: f32 = params.image_blur_filter_sigma;
    let dilate: bool = params.image_blur_filter_coeff.x > 0.5;
    var acc: vec4<f32> = blurTap(vertex.fpos.xy, params);
    for (var i: f32 = 1.0; i <= 24.0; i += 1.0) {
        if (i > radius) {
            break;
        }
        let before = blurTap(vertex.fpos.xy - i * params.image_blur_filter_direction, params);
        let after = blurTap(vertex.fpos.xy + i * params.image_blur_filter_direction, params);
        if (dilate) {
            acc = max(acc, max(before, after));
        } else {
            acc = min(acc, min(before, after));
        }
    }
    // A color matrix fused into this second draw.
    if (params.feather > 0.5) { acc = colorMatrixOn(acc, params); }
    if (params.tex_type == 2) { acc = vec4<f32>(acc.x); }
    return acc;
}

// SVG feOffset: the image shifted by the (dx, dy) in the first two
// parameter slots; what shifts in from beyond the image is transparent.
fn renderOffset(vertex: VertexOutput, params: Params) -> vec4<f32> {
    return blurTap(vertex.fpos.xy - params.scissor_mat[0].xy, params);
}
