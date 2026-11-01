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


@fragment
fn fs_main(vertex: VertexOutput) -> @location(0) vec4<f32> {
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
            result = ditherGradient(mix(params.inner_col,params.outer_col,d), vertex.position.xy);
        }
        case SHADER_TYPE_FillImageGradientConic: {
            let d = conicAngleFraction(vertex, params);
            result = ditherGradient(textureSample(image_texture, image_sampler, vec2<f32>(d, 0.0)), vertex.position.xy);
        }
        case SHADER_TYPE_FilterImageColorMatrix: {
            return renderColorMatrix(vertex, params);
        }
        default: {
            result = vec4<f32>(0.0, 0.0, 1.0, 1.0);
        }
    }

    var scissor: f32 = scissorMask(vertex.fpos, params);

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

fn renderColorMatrix(vertex: VertexOutput, params: Params) -> vec4<f32> {
    // The 4x5 color matrix is packed into the scissor/paint matrix slots (dead
    // during a filter pass): scissor_mat columns 0..2 hold the first 12 values,
    // paint_mat columns 0..1 the last 8. Apply in unpremultiplied sRGB space,
    // clamp to [0,1], then re-premultiply — unpremultiplying avoids edge halos
    // and the clamp keeps overflowing matrices from producing out-of-range/NaN.
    var c: vec4<f32> = textureSample(image_texture, image_sampler, vertex.fpos.xy / params.extent);
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

fn conicAngleFraction(vertex: VertexOutput, params: Params) -> f32 {
    let pt: vec2<f32> = (params.paint_mat * vec3<f32>(vertex.fpos, 1.0)).xy;
    // Measure the angle clockwise from the positive x axis. In the gradient's
    // local space (y points down on screen), atan2(pt.y, pt.x) increases in the
    // clockwise direction, so offset 0 sits at 3 o'clock and the ramp proceeds
    // clockwise, matching Canvas 2D createConicGradient. fract() wraps the angle
    // into [0, 1) for negative or large start angles.
    return fract((atan2(pt.y, pt.x) - params.conic_start_angle) / TAU);
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

fn renderGradient(vertex: VertexOutput, params: Params) -> vec4<f32> {
    // Calculate gradient color using box gradient
    let pt: vec2<f32> = (params.paint_mat * vec3<f32>(vertex.fpos, 1.0)).xy;

    let d: f32 = clamp((sdroundrect(pt, params.extent, params.radius) + params.feather*0.5) / params.feather, 0.0, 1.0);
    return ditherGradient(mix(params.inner_col,params.outer_col,d), vertex.position.xy);
}

// Image-based Gradient; sample a texture using the gradient position.
fn renderImageGradient(vertex: VertexOutput, params: Params) -> vec4<f32> {
    // Calculate gradient color using box gradient
    let pt: vec2<f32> = (params.paint_mat * vec3<f32>(vertex.fpos, 1.0)).xy;

    let d: f32 = clamp((sdroundrect(pt, params.extent, params.radius) + params.feather*0.5) / params.feather, 0.0, 1.0);
    return ditherGradient(textureSample(image_texture, image_sampler, vec2<f32>(d, 0.0)), vertex.position.xy);
}

fn renderImage(vertex: VertexOutput, params: Params) -> vec4<f32> {
    // Calculate color from texture
    let pt: vec2<f32> = (params.paint_mat * vec3<f32>(vertex.fpos, 1.0)).xy / params.extent;

    var color: vec4<f32> = textureSample(image_texture, image_sampler, pt);

    if (params.tex_type == 1) { color = vec4(color.xyz * color.w, color.w); }
    if (params.tex_type == 2) { color = vec4(color.x); }

    // Apply color tint and alpha.
    color *= params.inner_col;
    return color;
}

fn renderPlainTextureCopy(vertex: VertexOutput, params: Params) -> vec4<f32> {
    var color: vec4<f32> = textureSample(image_texture, image_sampler, vertex.ftcoord);

    if (params.tex_type == 1) { color = vec4(color.xyz * color.w, color.w); }
    if (params.tex_type == 2) { color = vec4(color.x); }
    // Apply color tint and alpha.
    color *= params.inner_col;
    return color;
}

fn renderFilteredImage(vertex: VertexOutput, params: Params) -> vec4<f32> {
    let sampleCount: f32 = ceil(3.0 * params.image_blur_filter_sigma);

    var gaussian_coeff: vec3<f32> = params.image_blur_filter_coeff;

    var color_sum: vec4<f32> = textureSample(image_texture, image_sampler, vertex.fpos.xy / params.extent) * gaussian_coeff.x;
    var coefficient_sum: f32 = gaussian_coeff.x;
    gaussian_coeff.x *= gaussian_coeff.y;
    gaussian_coeff.y *= gaussian_coeff.z;

    for (var i: f32 = 1.0; i <= 24.0; i += 1.) {
        // Work around GLES 2.0 limitation of only allowing constant loop indices by
        // breaking here. Sigma is clamped to 8 on the Rust side and the kernel reaches
        // +/-3*sigma, so the tap count never exceeds this 24-iteration bound.
        if (i >= sampleCount) {
            break;
        }
        color_sum += textureSample(image_texture, image_sampler, (vertex.fpos.xy - i * params.image_blur_filter_direction) / params.extent) * gaussian_coeff.x;
        color_sum += textureSample(image_texture, image_sampler, (vertex.fpos.xy + i * params.image_blur_filter_direction) / params.extent) * gaussian_coeff.x;
        coefficient_sum += 2.0 * gaussian_coeff.x;

        // Compute the coefficients incrementally:
        // https://developer.nvidia.com/gpugems/gpugems3/part-vi-gpu-computing/chapter-40-incremental-computation-gaussian
        gaussian_coeff.x *= gaussian_coeff.y;
        gaussian_coeff.y *= gaussian_coeff.z;
    }

    var color: vec4<f32> = color_sum / coefficient_sum;

    if (params.tex_type == 1) { color = vec4<f32>(color.xyz * color.w, color.w); }
    if (params.tex_type == 2) { color = vec4<f32>(color.x); }

    return color;
}
