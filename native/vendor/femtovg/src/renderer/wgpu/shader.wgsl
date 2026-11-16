struct Params {
    scissor_mat: mat3x4<f32>,
    paint_mat: mat3x4<f32>,
    inner_col: vec4<f32>,
    outer_col: vec4<f32>,
    scissor_ext: vec2<f32>,
    scissor_scale: vec2<f32>,
    extent: vec2<f32>,
    radius: f32,
    feather: f32,
    stroke_mult: f32,
    stroke_thr: f32,
    tex_type: f32,
    shader_type: f32,
    glyph_texture_type: f32, // 0 -> no glyph rendering, 1 -> alpha mask, 2 -> color texture
    image_blur_filter_sigma: f32,
    image_blur_filter_direction: vec2<f32>,
    image_blur_filter_coeff: vec3<f32>,
    // scissor_radius fills the padding after the vec3 (byte offset 204);
    // conic_start_angle starts the next 16-byte row (byte offset 208), which
    // is frag[13].x in the flat uniform array written from Rust.
    scissor_radius: f32,
    conic_start_angle: f32,
    // Whether the coverage mask of a draw under one takes a pixel whole or
    // not at all (see clipMaskCoverage). Where the mask's corner lies and
    // the pixels it spans are scissor_mat[0].zw and scissor_mat[1].zw, which
    // the scissor does not read; scissor_mat[2].zw is one of them as a share
    // of the mask's image, for a backend that cannot read a texel by index.
    clip_mask_hard: f32,
    // The clip shape of a draw under one (see clipMask): where its corners'
    // ellipses are centered, the rows of the map from device pixels to its
    // frame, and that map's offset with the half extents plus half a fringe.
    clip_inner: vec2<f32>,
    clip_linear: vec4<f32>,
    clip_offset_outer: vec4<f32>,
}

const SHADER_TYPE_FillGradient: i32 = 0;
const SHADER_TYPE_FillImage: i32 = 1;
const SHADER_TYPE_Stencil: i32 = 2;
const SHADER_TYPE_FillImageGradient: i32 = 3;
const SHADER_TYPE_FilterImage: i32 = 4;
const SHADER_TYPE_FillColor: i32 = 5;
const SHADER_TYPE_TextureCopyUnclipped: i32 = 6;
const SHADER_TYPE_FillColorUnclipped: i32 = 7;
const SHADER_TYPE_FillGradientConic: i32 = 8;
const SHADER_TYPE_FillImageGradientConic: i32 = 9;
const SHADER_TYPE_FilterImageColorMatrix: i32 = 10;
const SHADER_TYPE_FillGradientTwoPointRadial: i32 = 11;
const SHADER_TYPE_FillImageGradientTwoPointRadial: i32 = 12;
const SHADER_TYPE_FilterImageTurbulence: i32 = 13;
const SHADER_TYPE_FilterImageTransfer: i32 = 14;
const SHADER_TYPE_FilterImageBlend: i32 = 15;
const SHADER_TYPE_FilterImageMorphology: i32 = 16;
const SHADER_TYPE_FilterImageOffset: i32 = 17;
const SHADER_TYPE_FillBlur: i32 = 18;

const TAU: f32 = 6.28318530717958647692528676655900577;

struct ViewSize {
    x: f32,
    y: f32,
    pad: vec2<f32>,
}

@group(0)
@binding(0)
var<uniform> viewSize: ViewSize;

@group(1)
@binding(0)
var<uniform> params: Params;

struct Vertex {
    vertex: vec2<f32>,
    tcoord: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) ftcoord: vec2<f32>,
    @location(1) fpos: vec2<f32>,
};

@vertex
fn vs_main(
    @location(0) vertex: vec2<f32>,
    @location(1) tcoord: vec2<f32>,
) -> VertexOutput {
    var result: VertexOutput;
    result.ftcoord = tcoord;
    result.fpos = vertex;
    result.position = vec4<f32>(2.0 * vertex.x / viewSize.x - 1.0, 1.0 - 2.0 * vertex.y / viewSize.y, 0, 1);
    return result;
}

@vertex
fn vs_main_texture(
    @location(0) vertex: vec2<f32>,
    @location(1) tcoord: vec2<f32>,
) -> VertexOutput {
    var result: VertexOutput;
    result.ftcoord = tcoord;
    result.fpos = vertex;
    result.position = vec4<f32>(2.0 * vertex.x / viewSize.x - 1.0, 2.0 * vertex.y / viewSize.y - 1.0, 0, 1);
    return result;
}

@group(1)
@binding(1)
var image_texture: texture_2d<f32>;
@group(1)
@binding(2)
var image_sampler: sampler;

@group(1)
@binding(3)
var glyph_texture: texture_2d<f32>;
@group(1)
@binding(4)
var glyph_sampler: sampler;

@group(1)
@binding(5)
var clip_mask_texture: texture_2d<f32>;


@fragment
fn fs_main(vertex: VertexOutput) -> @location(0) vec4<f32> {
    return shade(vertex, 1.0);
}

// The entry point of a draw under a clip shape: no other draw evaluates one.
@fragment
fn fs_main_clip(vertex: VertexOutput) -> @location(0) vec4<f32> {
    return shade(vertex, clipMask(vertex.fpos));
}

// The entry points of a draw under a coverage mask, and under a mask and a
// shape: no other draw reads one.
@fragment
fn fs_main_mask(vertex: VertexOutput) -> @location(0) vec4<f32> {
    return shade(vertex, clipMaskCoverage(vertex.fpos));
}

@fragment
fn fs_main_clip_mask(vertex: VertexOutput) -> @location(0) vec4<f32> {
    return shade(vertex, clipMask(vertex.fpos) * clipMaskCoverage(vertex.fpos));
}

fn shade(vertex: VertexOutput, clip: f32) -> vec4<f32> {
    var result: vec4<f32>;
    let shader_type_int: i32 = i32(params.shader_type);

    var strokeAlpha: f32 = 1.0;
    if (shader_type_int != SHADER_TYPE_TextureCopyUnclipped && shader_type_int != SHADER_TYPE_FillColorUnclipped && shader_type_int != SHADER_TYPE_FilterImage) {
        strokeAlpha = strokeMask(vertex, params);
        if (strokeAlpha < params.stroke_thr) {
            discard;
        }
    }

    switch (shader_type_int) {
        case SHADER_TYPE_FillGradient: {
            // Gradient
            result = renderGradient(vertex, params);
        }
        case SHADER_TYPE_FillImageGradient: {
            // Image-based Gradient; sample a texture using the gradient position.
            result = renderImageGradient(vertex, params);
        }
        case SHADER_TYPE_FillImage: {
            // Image
            result = renderImage(vertex, params);
        }
        case SHADER_TYPE_FillColor: {
            // Plain color fill
            result = params.inner_col;
        }
        case SHADER_TYPE_TextureCopyUnclipped: {
            // Plain texture copy, unclipped
            return renderPlainTextureCopy(vertex, params);
        }
        case SHADER_TYPE_Stencil: {
            // Stencil fill
            result = vec4<f32>(1,1,1,1);
        }
        case SHADER_TYPE_FilterImage: {
            // Filter Image
            return renderFilteredImage(vertex, params);
        }
        case SHADER_TYPE_FillColorUnclipped: {
            // Plain color fill
            return params.inner_col;
        }
        case SHADER_TYPE_FillGradientConic: {
            // Set `result` and fall through to the scissor + stroke-AA multiply
            // below (like the other gradient cases); returning here would skip
            // the clip and antialiasing, so conic fills would ignore scissors.
            let d = conicAngleFraction(vertex, params);
            let mixed = ditherGradient(mix(params.inner_col,params.outer_col,d), vertex.position.xy);
            result = premultiplyGradient(mixed, params);
        }
        case SHADER_TYPE_FillImageGradientConic: {
            let d = conicAngleFraction(vertex, params);
            result = ditherGradient(textureSample(image_texture, image_sampler, lutCoord(d)), vertex.position.xy);
        }
        case SHADER_TYPE_FilterImageColorMatrix: {
            return renderColorMatrix(vertex, params);
        }
        case SHADER_TYPE_FillGradientTwoPointRadial: {
            // Set `result` and fall through to the scissor + stroke-AA multiply.
            result = renderGradientTwoPointRadial(vertex, params);
        }
        case SHADER_TYPE_FillImageGradientTwoPointRadial: {
            result = renderImageGradientTwoPointRadial(vertex, params);
        }
        case SHADER_TYPE_FilterImageTurbulence: {
            return renderTurbulence(vertex, params);
        }
        case SHADER_TYPE_FilterImageTransfer: {
            return renderTransfer(vertex, params);
        }
        case SHADER_TYPE_FilterImageBlend: {
            return renderBlend(vertex, params);
        }
        case SHADER_TYPE_FilterImageMorphology: {
            return renderMorphology(vertex, params);
        }
        case SHADER_TYPE_FilterImageOffset: {
            return renderOffset(vertex, params);
        }
        case SHADER_TYPE_FillBlur: {
            result = renderFillBlur(vertex, params);
        }
        default: {
            result = vec4<f32>(0.0, 0.0, 1.0, 1.0);
        }
    }

    var scissor: f32 = scissorMask(vertex.fpos, params) * clip;

    if (params.glyph_texture_type != 0.0) {
        // Textured tris
        var mask: vec4<f32> = textureSample(glyph_texture, glyph_sampler, vertex.ftcoord);

        if (params.glyph_texture_type == 1) {
            mask = vec4<f32>(mask.x);
        } else {
            result = vec4<f32>(1, 1, 1, 1);
            mask = vec4<f32>(mask.xyz * mask.w, mask.w);
        }

        mask *= scissor;
        result *= mask;
    } else if (shader_type_int != SHADER_TYPE_Stencil && shader_type_int != SHADER_TYPE_FilterImage) {
        // Not stencil fill
        // Combine alpha
        result *= strokeAlpha * scissor;
    }

    return result;
}

fn conicAngleFraction(vertex: VertexOutput, params: Params) -> f32 {
    let pt: vec2<f32> = (params.paint_mat * vec3<f32>(vertex.fpos, 1.0)).xy;
    // Measure the angle clockwise from the positive x axis. In the gradient's
    // local space (y points down on screen), atan2(pt.y, pt.x) increases in the
    // clockwise direction, so offset 0 sits at 3 o'clock and the ramp proceeds
    // clockwise, matching Canvas 2D createConicGradient. fract() wraps the angle
    // into [0, 1) for negative or large start angles.
    return fract((atan2(pt.y, pt.x) - params.conic_start_angle) / TAU);
}

// Two-point (independently centered) radial gradient: the general Canvas
// createRadialGradient(x0,y0,r0, x1,y1,r1). paint_mat places the start circle at
// the origin, so in local space the start circle is (0,0) radius r0 and the end
// circle is `extent` away with radius r0 + dr (dr in feather). For a fragment pt
// we solve for the interpolation offset t where |pt - t*cd| = r0 + t*dr, i.e.
// a*t^2 - 2b*t + c = 0. `covered` reports whether any interpolated circle with a
// non-negative radius reaches the fragment; uncovered fragments are transparent.
struct RadialT {
    t: f32,
    covered: bool,
}

fn radialTwoPointT(vertex: VertexOutput, params: Params) -> RadialT {
    let pt: vec2<f32> = (params.paint_mat * vec3<f32>(vertex.fpos, 1.0)).xy;
    let cd: vec2<f32> = params.extent;  // end circle center relative to start
    let r0: f32 = params.radius;
    let dr: f32 = params.feather;       // r1 - r0
    let a: f32 = dot(cd, cd) - dr * dr;
    let b: f32 = dot(pt, cd) + r0 * dr;
    let c: f32 = dot(pt, pt) - r0 * r0;
    var result: RadialT;
    result.covered = false;
    result.t = 0.0;
    // `a` is a squared length, so how close to zero it counts as depends on the
    // size of the shape: the same geometry scaled up scales `a` with the square
    // of the scale factor. Comparing it against a fixed constant would treat a
    // focal point sitting on the end circle as a cone in one scene and as a
    // degenerate in another. Measure it against the terms it came from instead,
    // which also folds in the case where both are zero and the circles coincide.
    let a_scale = max(dot(cd, cd), dr * dr);
    if (a_scale == 0.0) {
        // The two circles are the same circle. There is no sweep to walk, so
        // the whole plane takes the far end of the ramp. The Canvas algorithm
        // says to paint nothing here, but SVG resolved the opposite and its
        // test suite asserts the gradient is drawn, so follow SVG.
        result.t = 1.0;
        result.covered = true;
    } else if (abs(a) <= 1e-4 * a_scale) {
        // Degenerate cone (|cd| == |dr|): the quadratic collapses to the linear
        // equation -2b*t + c = 0. This is exactly the case where the end center
        // sits on the start circle, common in focal-style gradients.
        if (abs(b) > 1e-6) {
            result.t = c / (2.0 * b);
            result.covered = (r0 + result.t * dr >= 0.0);
        }
    } else {
        let disc: f32 = b * b - a * c;
        // When one circle strictly contains the other (a < 0), every fragment
        // is on some interpolated circle and the discriminant is never really
        // negative: at the focal point it is mathematically zero and rounds
        // just below, which would punch a hole exactly where the first stop
        // belongs. So a negative discriminant only counts as a miss for a > 0.
        if (a < 0.0 || disc >= 0.0) {
            let s: f32 = sqrt(max(disc, 0.0));
            // One reciprocal, two roots (a is guaranteed away from zero here).
            let inv_a: f32 = 1.0 / a;
            let root_a: f32 = (b - s) * inv_a;
            let root_b: f32 = (b + s) * inv_a;
            let t_lo: f32 = min(root_a, root_b);
            let t_hi: f32 = max(root_a, root_b);
            // Canvas draws circles from the largest offset toward the smallest and
            // does not overpaint, so the largest offset whose interpolated circle
            // has a non-negative radius is the one that claims the fragment.
            if (r0 + t_hi * dr >= 0.0) {
                result.t = t_hi;
                result.covered = true;
            } else if (r0 + t_lo * dr >= 0.0) {
                result.t = t_lo;
                result.covered = true;
            }
        }
    }
    result.t = clamp(result.t, 0.0, 1.0);
    return result;
}

fn renderGradientTwoPointRadial(vertex: VertexOutput, params: Params) -> vec4<f32> {
    let r: RadialT = radialTwoPointT(vertex, params);
    if (!r.covered) {
        return vec4<f32>(0.0);
    }
    let color = ditherGradient(mix(params.inner_col, params.outer_col, r.t), vertex.position.xy);
    return premultiplyGradient(color, params);
}

fn renderImageGradientTwoPointRadial(vertex: VertexOutput, params: Params) -> vec4<f32> {
    let r: RadialT = radialTwoPointT(vertex, params);
    // textureSample() has to come before the early return: WGSL allows it
    // only in uniform control flow, and r.covered differs between fragments.
    // Chrome rejects the shader module otherwise; naga accepts it either way.
    let color: vec4<f32> = textureSample(image_texture, image_sampler, lutCoord(r.t));
    if (!r.covered) {
        return vec4<f32>(0.0);
    }
    return ditherGradient(color, vertex.position.xy);
}

fn sdroundrect(pt: vec2<f32>, ext: vec2<f32>, rad: f32) -> f32 {
    let ext2: vec2<f32> = ext - vec2<f32>(rad,rad);
    let d: vec2<f32> = abs(pt) - ext2;
    return min(max(d.x,d.y),0.0) + length(max(d, vec2<f32>(0.0, 0.0))) - rad;
}

// Scissoring
fn scissorMask(p: vec2<f32>, params: Params) -> f32 {
    if (params.scissor_radius > 0.0) {
        let pt = (params.scissor_mat * vec3<f32>(p, 1.0)).xy;
        let distance = sdroundrect(pt, params.scissor_ext, params.scissor_radius);
        return clamp(0.5 - distance * min(params.scissor_scale.x, params.scissor_scale.y), 0.0, 1.0);
    }

    var sc: vec2<f32> = (abs((params.scissor_mat * vec3<f32>(p,1.0)).xy) - params.scissor_ext);
    sc = vec2(0.5,0.5) - sc * params.scissor_scale;
    return clamp(sc.x,0.0,1.0) * clamp(sc.y,0.0,1.0);
}

// The clip taken as a shape: coverage of a box with elliptical corners, in
// a frame where one unit is one fringe width across each side. Past a
// corner's center the distance is the first-order one to its ellipse, exact
// for a circle: k1 is one on the ellipse, and its gradient in device pixels
// is g along the frame's rows - unit vectors, at a right angle only without
// a skew. A box with square corners has their centers out of reach.
fn clipMask(p: vec2<f32>) -> f32 {
    let at = vec2<f32>(dot(params.clip_linear.xy, p), dot(params.clip_linear.zw, p)) + params.clip_offset_outer.xy;
    let reach = abs(at);
    let corner = reach - params.clip_inner;
    if (corner.x > 0.0 && corner.y > 0.0) {
        let radii = params.clip_offset_outer.zw - vec2<f32>(0.5, 0.5) - params.clip_inner;
        let k = corner / radii;
        let k1 = length(k);
        let g = k / radii;
        let lean = dot(params.clip_linear.xy, params.clip_linear.zw) * sign(at.x) * sign(at.y);
        return clamp(0.5 - (k1 - 1.0) * k1 / sqrt(dot(g, g) + 2.0 * g.x * g.y * lean), 0.0, 1.0);
    }
    let cover = clamp(params.clip_offset_outer.zw - reach, vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));
    return cover.x * cover.y;
}

// The clip taken as a coverage mask: the texel under the fragment, of a mask
// that holds one for each pixel of the clip's bounds and nothing along its
// border - what a fragment past the mask reads. An operation that coverage
// cannot bound takes the pixels the clip covers by half or more, whole.
fn clipMaskCoverage(p: vec2<f32>) -> f32 {
    let size = params.scissor_mat[1].zw;
    let at = clamp(p - params.scissor_mat[0].zw, vec2<f32>(0.5, 0.5), size - vec2<f32>(0.5, 0.5));
    let coverage = textureLoad(clip_mask_texture, vec2<i32>(at), 0).r;
    if (params.clip_mask_hard != 0.0) {
        if (coverage < 0.5) {
            discard;
        }
        return 1.0;
    }
    return coverage;
}

// Stroke - from [0..1] to clipped pyramid, where the slope is 1px.
fn strokeMask(vertex: VertexOutput, params: Params) -> f32 {
    return min(1.0, (1.0-abs(vertex.ftcoord.x*2.0-1.0))*params.stroke_mult) * min(1.0, vertex.ftcoord.y);
    // Using this smoothstep preduces maybe better results when combined with fringe_width of 2, but it may look blurrier
    // maybe this should be controlled via flag
    //return smoothstep(0.0, 1.0, (1.0-abs(vertex.ftcoord.x*2.0-1.0))*params.stroke_mult) * smoothstep(0.0, 1.0, vertex.ftcoord.y);
}

// Interleaved gradient noise dither (see the GLSL shader / issue femtovg/femtovg#239):
// a cheap, deterministic screen-space ordered dither. A sub-LSB colour offset
// before the 8-bit write breaks up gradient banding between close colours.
fn ditherNoise(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}
fn ditherGradient(color: vec4<f32>, fragcoord: vec2<f32>) -> vec4<f32> {
    let d = (ditherNoise(fragcoord) - 0.5) / 255.0;
    return vec4<f32>(color.rgb + d, color.a);
}

// Premultiplies a two-stop gradient's mixed color, unless its endpoints arrive
// premultiplied (see GRADIENT_PREMULTIPLIED_INDEX), and clamps the color channels
// to [0, alpha], which the dither can step outside.
fn premultiplyGradient(color: vec4<f32>, params: Params) -> vec4<f32> {
    let premultiplied = params.paint_mat[1].w;
    let rgb = clamp(color.rgb * max(color.a, premultiplied), vec3<f32>(0.0), vec3<f32>(color.a));
    return vec4<f32>(rgb, color.a);
}

fn renderGradient(vertex: VertexOutput, params: Params) -> vec4<f32> {
    // Calculate gradient color using box gradient
    let pt: vec2<f32> = (params.paint_mat * vec3<f32>(vertex.fpos, 1.0)).xy;

    let d: f32 = clamp((sdroundrect(pt, params.extent, params.radius) + params.feather*0.5) / params.feather, 0.0, 1.0);
    let color: vec4<f32> = ditherGradient(mix(params.inner_col,params.outer_col,d), vertex.position.xy);
    return premultiplyGradient(color, params);
}

// Where a gradient LUT is sampled for position `d`. Texel i holds the color
// at i / 255, so map d = 0 and d = 1 onto the end texels' centres: the pads
// then get the exact stop colors instead of a blend with the next texel.
fn lutCoord(d: f32) -> vec2<f32> {
    return vec2<f32>(d * (255.0 / 256.0) + 0.5 / 256.0, 0.0);
}

// Image-based Gradient; sample a texture using the gradient position.
fn renderImageGradient(vertex: VertexOutput, params: Params) -> vec4<f32> {
    // Calculate gradient color using box gradient
    let pt: vec2<f32> = (params.paint_mat * vec3<f32>(vertex.fpos, 1.0)).xy;

    let d: f32 = clamp((sdroundrect(pt, params.extent, params.radius) + params.feather*0.5) / params.feather, 0.0, 1.0);
    return ditherGradient(textureSample(image_texture, image_sampler, lutCoord(d)), vertex.position.xy);
}

// The sRGB transfer curve (IEC 61966-2-1) on a premultiplied color, run on
// the unpremultiplied color, alpha untouched: to sRGB when to_srgb, else to
// linearRGB.
fn transferColor(color: vec4<f32>, to_srgb: bool) -> vec4<f32> {
    var c: vec4<f32> = color;
    if (c.a > 0.0) {
        c = vec4<f32>(c.rgb / c.a, c.a);
    }
    let x = clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    var y: vec3<f32>;
    if (to_srgb) {
        // mix evaluates both sides: keep pow off 0, where an exp2(log2) under fast math can give a NaN that
        // mix carries even at weight 0. The clamp only touches the side step leaves out.
        y = mix(x * 12.92, 1.055 * pow(max(x, vec3<f32>(0.0031308)), vec3<f32>(1.0 / 2.4)) - 0.055, step(vec3<f32>(0.0031308), x));
    } else {
        y = mix(x / 12.92, pow((x + 0.055) / 1.055, vec3<f32>(2.4)), step(vec3<f32>(0.04045), x));
    }
    return vec4<f32>(y * c.a, c.a);
}

// A sampled image texel as the paint uses it: premultiplied, converted by
// the paint's transfer if it has one - rounded to 8 bits, as the transfer's
// pass stores its result - then tinted.
fn paintTexel(texel: vec4<f32>, params: Params) -> vec4<f32> {
    var color: vec4<f32> = texel;
    if (params.tex_type == 1) { color = vec4(color.xyz * color.w, color.w); }
    if (params.tex_type == 2) { color = vec4(color.x); }
    // The transfer is the paint matrix's first column's w (0 none, 1 to
    // sRGB, 2 to linearRGB), which no image paint reads.
    let transfer = params.paint_mat[0].w;
    if (transfer > 0.5) {
        color = floor(transferColor(color, transfer < 1.5) * 255.0 + 0.5) / 255.0;
    }
    // Apply color tint and alpha.
    return color * params.inner_col;
}

fn renderImage(vertex: VertexOutput, params: Params) -> vec4<f32> {
    // Calculate color from texture
    let pt: vec2<f32> = (params.paint_mat * vec3<f32>(vertex.fpos, 1.0)).xy / params.extent;

    // Half a level toward the larger mip: browsers minify sharper than plain
    // trilinear (Chromium samples the larger level, Firefox downsamples
    // directly); no effect without mipmaps or under magnification.
    return paintTexel(textureSampleBias(image_texture, image_sampler, pt, -0.5), params);
}

fn renderPlainTextureCopy(vertex: VertexOutput, params: Params) -> vec4<f32> {
    return paintTexel(textureSampleBias(image_texture, image_sampler, vertex.ftcoord, -0.5), params);
}

// A blur tap, premultiplied: a straight-alpha source is converted per tap,
// so the sum is a sum of premultiplied colors as the blur's definition
// wants (converting the sum afterwards squared any alpha the taps lost). A
// tap beyond the image reads transparent, as a filter's input is beyond its
// edge in SVG and Canvas 2D, instead of the edge texel clamped outward; its
// weight stays in the sum, so an edge fades. A blur's own draws clamp to the
// edge instead: their reach and a fill's fringe pass the source's edges.
// With `radius` set, the tap reads the source's alpha alone, (0, 0, 0, a):
// an alpha-only color matrix fused into the filter's first draw.
fn blurTap(pos: vec2<f32>, params: Params) -> vec4<f32> {
    let uv = pos / params.extent;
    let inside = (all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0))) || i32(params.shader_type) == SHADER_TYPE_FillBlur;
    var color: vec4<f32> = textureSample(image_texture, image_sampler, uv);
    if (params.tex_type == 1) { color = vec4<f32>(color.xyz * color.w, color.w); }
    if (params.radius > 0.5) { color = vec4<f32>(0.0, 0.0, 0.0, color.w); }
    return color * select(0.0, 1.0, inside);
}

// The Gaussian along image_blur_filter_direction, centered on `center` in
// source pixels.
fn blurAt(center: vec2<f32>, params: Params) -> vec4<f32> {
    let sampleCount: f32 = ceil(3.0 * params.image_blur_filter_sigma);

    var gaussian_coeff: vec3<f32> = params.image_blur_filter_coeff;

    var color_sum: vec4<f32> = blurTap(center, params) * gaussian_coeff.x;
    var coefficient_sum: f32 = gaussian_coeff.x;
    gaussian_coeff.x *= gaussian_coeff.y;
    gaussian_coeff.y *= gaussian_coeff.z;

    for (var i: f32 = 1.0; i <= 24.0; i += 1.) {
        // Work around GLES 2.0 limitation of only allowing constant loop indices by
        // breaking here. Each pass's sigma is bounded to 8 on the Rust side (a larger
        // blur is split into passes that compose to it) and the kernel reaches
        // +/-3*sigma, so the tap count never exceeds this 24-iteration bound.
        if (i >= sampleCount) {
            break;
        }
        color_sum += blurTap(center - i * params.image_blur_filter_direction, params) * gaussian_coeff.x;
        color_sum += blurTap(center + i * params.image_blur_filter_direction, params) * gaussian_coeff.x;
        coefficient_sum += 2.0 * gaussian_coeff.x;

        // Compute the coefficients incrementally:
        // https://developer.nvidia.com/gpugems/gpugems3/part-vi-gpu-computing/chapter-40-incremental-computation-gaussian
        gaussian_coeff.x *= gaussian_coeff.y;
        gaussian_coeff.y *= gaussian_coeff.z;
    }

    return color_sum / coefficient_sum;
}

fn renderFilteredImage(vertex: VertexOutput, params: Params) -> vec4<f32> {
    // The quad's texture coordinates say where in the source it samples.
    var color: vec4<f32> = blurAt(vertex.ftcoord, params);

    // A color matrix fused into this second draw.
    if (params.feather > 0.5) { color = colorMatrixOn(color, params); }
    if (params.tex_type == 2) { color = vec4<f32>(color.x); }

    return color;
}

// A blur's draw as a fill: its texture coordinates carry the fill's fringe,
// so paint_mat maps the fragment to where it samples.
fn renderFillBlur(vertex: VertexOutput, params: Params) -> vec4<f32> {
    return blurAt((params.paint_mat * vec3<f32>(vertex.fpos, 1.0)).xy, params) * params.inner_col.a;
}
