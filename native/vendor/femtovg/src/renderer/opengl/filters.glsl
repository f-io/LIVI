// The image filter passes: color matrix, feTurbulence, the sRGB transfer
// curves and feBlend. Appended after main-fs.glsl; main() calls them
// through the prototypes declared there.

// The 4x5 color matrix packed row-major into frag[0..4] (the scissor/paint
// matrix slots, unused during a filter pass) applied to a premultiplied
// color: in unpremultiplied sRGB space, clamped to [0,1], re-premultiplied
// - unpremultiplying avoids edge halos and the clamp keeps overflowing
// matrices from producing out-of-range or NaN pixels. The color-matrix pass
// and the second draw of a two-draw filter with a matrix fused into it
// share it.
vec4 colorMatrixOn(vec4 premultiplied) {
    vec4 c = premultiplied;
    if (c.a > 0.0) {
        c.rgb /= c.a;
    }
    float r = frag[0].x * c.r + frag[0].y * c.g + frag[0].z * c.b + frag[0].w * c.a + frag[1].x;
    float g = frag[1].y * c.r + frag[1].z * c.g + frag[1].w * c.b + frag[2].x * c.a + frag[2].y;
    float b = frag[2].z * c.r + frag[2].w * c.g + frag[3].x * c.b + frag[3].y * c.a + frag[3].z;
    float a = frag[3].w * c.r + frag[4].x * c.g + frag[4].y * c.b + frag[4].z * c.a + frag[4].w;
    vec4 outc = clamp(vec4(r, g, b, a), 0.0, 1.0);
    outc.rgb *= outc.a;
    return outc;
}

vec4 renderColorMatrix() {
    return colorMatrixOn(texture2D(tex, fpos.xy / extent));
}

// SVG feTurbulence (SVG 1.1 section 15.19), all four channels at once. `tex`
// is the lattice texture built by src/turbulence.rs: texel (bx, by) of the
// left 256 columns holds the unit gradient vectors of channels 0 and 1 at
// lattice corner (bx, by), the right 256 columns those of channels 2 and 3,
// each component stored as (g + 1) / 2. The reference code's integer
// truncation and 0xff mask become floor() and mod() here - GLSL ES 1.00 has
// no bitwise operators - and the PerlinN offset that kept its integers
// positive drops out, so the stitch thresholds arrive from Rust without it.
#define TURBULENCE_MAX_OCTAVES 10.0

vec2 turbulenceWrap(vec2 corner) {
    return corner - 256.0 * floor(corner / 256.0);
}

vec4 turbulenceGradients(vec2 corner, float pair) {
    return texture2D(tex, vec2(corner.x + pair * 256.0 + 0.5, corner.y + 0.5) / vec2(512.0, 256.0)) * 2.0 - 1.0;
}

// One octave of classic Perlin noise at v: the spec's noise2() for the four
// channels. stitch is (tile width, tile height, wrap x, wrap y) in lattice units.
vec4 turbulenceOctave(vec2 v, vec4 stitch, bool stitching) {
    vec2 b0 = floor(v);
    vec2 r0 = v - b0;
    vec2 b1 = b0 + 1.0;
    vec2 r1 = r0 - 1.0;
    if (stitching) {
        if (b0.x >= stitch.z) b0.x -= stitch.x;
        if (b1.x >= stitch.z) b1.x -= stitch.x;
        if (b0.y >= stitch.w) b0.y -= stitch.y;
        if (b1.y >= stitch.w) b1.y -= stitch.y;
    }
    b0 = turbulenceWrap(b0);
    b1 = turbulenceWrap(b1);
    vec2 c10 = vec2(b1.x, b0.y);
    vec2 c01 = vec2(b0.x, b1.y);
    vec4 gA00 = turbulenceGradients(b0, 0.0);
    vec4 gB00 = turbulenceGradients(b0, 1.0);
    vec4 gA10 = turbulenceGradients(c10, 0.0);
    vec4 gB10 = turbulenceGradients(c10, 1.0);
    vec4 gA01 = turbulenceGradients(c01, 0.0);
    vec4 gB01 = turbulenceGradients(c01, 1.0);
    vec4 gA11 = turbulenceGradients(b1, 0.0);
    vec4 gB11 = turbulenceGradients(b1, 1.0);
    vec2 r10 = vec2(r1.x, r0.y);
    vec2 r01 = vec2(r0.x, r1.y);
    vec4 u00 = vec4(dot(r0, gA00.xy), dot(r0, gA00.zw), dot(r0, gB00.xy), dot(r0, gB00.zw));
    vec4 u10 = vec4(dot(r10, gA10.xy), dot(r10, gA10.zw), dot(r10, gB10.xy), dot(r10, gB10.zw));
    vec4 u01 = vec4(dot(r01, gA01.xy), dot(r01, gA01.zw), dot(r01, gB01.xy), dot(r01, gB01.zw));
    vec4 u11 = vec4(dot(r1, gA11.xy), dot(r1, gA11.zw), dot(r1, gB11.xy), dot(r1, gB11.zw));
    vec2 s = r0 * r0 * (3.0 - 2.0 * r0);
    return mix(mix(u00, u10, s.x), mix(u01, u11, s.x), s.y);
}

vec4 renderTurbulence() {
    // Slots (see ImageFilter::single_pass): frag[0] = inverse transform a b c d,
    // frag[1] = e f and the base frequency, frag[2] = octaves, fractal flag and
    // the stitch tile size, frag[3] = the stitch wrap thresholds and flag.
    // Each output pixel is evaluated at its integer index mapped into noise space.
    vec2 px = floor(fpos.xy);
    vec2 p = vec2(frag[0].x * px.x + frag[0].z * px.y + frag[1].x,
                  frag[0].y * px.x + frag[0].w * px.y + frag[1].y);
    vec2 v = p * frag[1].zw;
    float octaves = frag[2].x;
    bool fractal = frag[2].y > 0.5;
    vec4 stitch = vec4(frag[2].zw, frag[3].xy);
    bool stitching = frag[3].z > 0.5;
    vec4 sum = vec4(0.0);
    float ratio = 1.0;
    for (float o = 0.0; o < TURBULENCE_MAX_OCTAVES; o += 1.0) {
        // GLES 2.0 loops need a constant bound; the octave count breaks out.
        if (o >= octaves) break;
        vec4 n = turbulenceOctave(v, stitch, stitching);
        sum += (fractal ? n : abs(n)) / ratio;
        v *= 2.0;
        ratio *= 2.0;
        stitch *= 2.0;
    }
    vec4 c = clamp(fractal ? (sum + 1.0) / 2.0 : sum, 0.0, 1.0);
    return vec4(c.rgb * c.a, c.a);
}

// The sRGB transfer curve as a pass (see transferColor): frag[0].x > 0.5
// converts linearRGB to sRGB, otherwise the reverse.
vec4 renderTransfer() {
    return transferColor(texture2D(tex, fpos.xy / extent), frag[0].x > 0.5);
}


// SVG feBlend: the image over the backdrop in `glyphtex`. frag[0].x is the
// BlendMode index, frag[0].y whether the backdrop is stored the other way up
// from the image at this pass. Both textures are premultiplied; the blend
// function B(Cb, Cs) of the Compositing and Blending spec runs on the
// unpremultiplied colors and the result is composited as
// cs * (1 - ab) + cb * (1 - as) + as * ab * B, alpha as = as + ab - as * ab.
float blendLum(vec3 c) {
    return 0.3 * c.r + 0.59 * c.g + 0.11 * c.b;
}

vec3 blendClipColor(vec3 c) {
    float l = blendLum(c);
    float n = min(c.r, min(c.g, c.b));
    float x = max(c.r, max(c.g, c.b));
    vec3 o = c;
    if (n < 0.0) {
        o = l + (c - l) * l / (l - n);
    }
    if (x > 1.0) {
        o = l + (o - l) * (1.0 - l) / (x - l);
    }
    return o;
}

vec3 blendSetLum(vec3 c, float l) {
    return blendClipColor(c + (l - blendLum(c)));
}

float blendSat(vec3 c) {
    return max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b));
}

vec3 blendSetSat(vec3 c, float s) {
    float mn = min(c.r, min(c.g, c.b));
    float mx = max(c.r, max(c.g, c.b));
    if (mx > mn) {
        return (c - mn) * s / (mx - mn);
    }
    return vec3(0.0);
}

float blendHardLight1(float cb, float cs) {
    if (cs <= 0.5) {
        return cb * 2.0 * cs;
    }
    float cs2 = 2.0 * cs - 1.0;
    return cb + cs2 - cb * cs2;
}

float blendColorDodge1(float cb, float cs) {
    if (cb <= 0.0) {
        return 0.0;
    }
    if (cs >= 1.0) {
        return 1.0;
    }
    return min(1.0, cb / (1.0 - cs));
}

float blendColorBurn1(float cb, float cs) {
    if (cb >= 1.0) {
        return 1.0;
    }
    if (cs <= 0.0) {
        return 0.0;
    }
    return 1.0 - min(1.0, (1.0 - cb) / cs);
}

float blendSoftLight1(float cb, float cs) {
    if (cs <= 0.5) {
        return cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb);
    }
    float d = cb <= 0.25 ? ((16.0 * cb - 12.0) * cb + 4.0) * cb : sqrt(cb);
    return cb + (2.0 * cs - 1.0) * (d - cb);
}

vec3 blendMode(int mode, vec3 cb, vec3 cs) {
    if (mode == 0) {
        return cs;
    } else if (mode == 1) {
        return cb * cs;
    } else if (mode == 2) {
        return cb + cs - cb * cs;
    } else if (mode == 3) {
        return vec3(blendHardLight1(cs.r, cb.r), blendHardLight1(cs.g, cb.g), blendHardLight1(cs.b, cb.b));
    } else if (mode == 4) {
        return min(cb, cs);
    } else if (mode == 5) {
        return max(cb, cs);
    } else if (mode == 6) {
        return vec3(blendColorDodge1(cb.r, cs.r), blendColorDodge1(cb.g, cs.g), blendColorDodge1(cb.b, cs.b));
    } else if (mode == 7) {
        return vec3(blendColorBurn1(cb.r, cs.r), blendColorBurn1(cb.g, cs.g), blendColorBurn1(cb.b, cs.b));
    } else if (mode == 8) {
        return vec3(blendHardLight1(cb.r, cs.r), blendHardLight1(cb.g, cs.g), blendHardLight1(cb.b, cs.b));
    } else if (mode == 9) {
        return vec3(blendSoftLight1(cb.r, cs.r), blendSoftLight1(cb.g, cs.g), blendSoftLight1(cb.b, cs.b));
    } else if (mode == 10) {
        return abs(cb - cs);
    } else if (mode == 11) {
        return cb + cs - 2.0 * cb * cs;
    } else if (mode == 12) {
        return blendSetLum(blendSetSat(cs, blendSat(cb)), blendLum(cb));
    } else if (mode == 13) {
        return blendSetLum(blendSetSat(cb, blendSat(cs)), blendLum(cb));
    } else if (mode == 14) {
        return blendSetLum(cs, blendLum(cb));
    }
    return blendSetLum(cb, blendLum(cs));
}

// Slots as in shader.wgsl: mode, backdrop flip, image alpha, contribution.
vec4 renderBlend() {
    vec2 uv = fpos.xy / extent;
    vec2 buv = frag[0].y > 0.5 ? vec2(uv.x, 1.0 - uv.y) : uv;
    vec4 src = texture2D(tex, uv) * frag[0].z;
    vec4 bd = texture2D(glyphtex, buv);
    // Mode 16, plus-lighter, adds premultiplied color and alpha; it has no
    // blend function. Layers composite it with CompositeOperation::Lighter
    // instead of this pass, so it never asks for a contribution.
    if (int(frag[0].x) == 16) {
        return min(src + bd, vec4(1.0));
    }
    vec3 cs = src.rgb;
    if (src.a > 0.0) {
        cs = src.rgb / src.a;
    }
    vec3 cb = bd.rgb;
    if (bd.a > 0.0) {
        cb = bd.rgb / bd.a;
    }
    vec3 b = clamp(blendMode(int(frag[0].x), clamp(cb, 0.0, 1.0), clamp(cs, 0.0, 1.0)), 0.0, 1.0);
    vec3 contribution = src.rgb * (1.0 - bd.a) + src.a * bd.a * b;
    if (frag[0].w > 0.5) {
        return vec4(contribution, src.a);
    }
    return vec4(contribution + bd.rgb * (1.0 - src.a), src.a + bd.a - src.a * bd.a);
}

// SVG feMorphology, one axis per draw like the blur: each pixel takes the
// per-channel maximum (dilate) or minimum (erode) of the premultiplied
// pixels within the radius along imageBlurFilterDirection, every tap
// through blurTap, so beyond the image reads transparent and an erosion
// eats into the border. The radius rides imageBlurFilterSigma (whole
// pixels, at most 24, the loop's constant bound) and the operator
// imageBlurFilterCoeff.x (1 dilates).
vec4 renderMorphology() {
    // `radius` is the box gradient's macro in main-fs.glsl.
    float reach = imageBlurFilterSigma;
    bool dilate = imageBlurFilterCoeff.x > 0.5;
    vec4 acc = blurTap(fpos.xy);
    for (float i = 1.0; i <= 24.0; i += 1.) {
        if (i > reach) {
            break;
        }
        vec4 before = blurTap(fpos.xy - i * imageBlurFilterDirection);
        vec4 after = blurTap(fpos.xy + i * imageBlurFilterDirection);
        if (dilate) {
            acc = max(acc, max(before, after));
        } else {
            acc = min(acc, min(before, after));
        }
    }
    // A color matrix fused into this second draw.
    if (feather > 0.5) acc = colorMatrixOn(acc);
    if (texType == 2) acc = vec4(acc.x);
    return acc;
}

// SVG feOffset: the image shifted by the (dx, dy) in the first two
// parameter slots; what shifts in from beyond the image is transparent.
vec4 renderOffset() {
    return blurTap(fpos.xy - frag[0].xy);
}
