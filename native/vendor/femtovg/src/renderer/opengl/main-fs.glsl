
precision highp float;

#define UNIFORMARRAY_SIZE 14

#define TAU 6.28318530717958647692528676655900577

uniform vec4 frag[UNIFORMARRAY_SIZE];

#define scissorMat mat3(frag[0].xyz, frag[1].xyz, frag[2].xyz)
#define paintMat mat3(frag[3].xyz, frag[4].xyz, frag[5].xyz)
#define innerCol frag[6]
#define outerCol frag[7]
#define scissorExt frag[8].xy
#define scissorScale frag[8].zw
#define extent frag[9].xy
#define radius frag[9].z
#define feather frag[9].w
#define strokeMult frag[10].x
#define strokeThr frag[10].y
#define texType int(frag[10].z)
#define shaderType int(frag[10].w)
#define glyphTextureType int(frag[11].x)
#define imageBlurFilterDirection frag[11].yz
#define imageBlurFilterSigma frag[11].w
#define imageBlurFilterCoeff frag[12].xyz
#define scissorRadius frag[12].w
#define conicStartAngle frag[13].x

uniform sampler2D tex;
uniform sampler2D glyphtex;
uniform vec2 viewSize;

varying vec2 ftcoord;
varying vec2 fpos;

 #define SHADER_TYPE_FillGradient 0
 #define SHADER_TYPE_FillImage 1
 #define SHADER_TYPE_Stencil 2
 #define SHADER_TYPE_FillImageGradient 3
 #define SHADER_TYPE_FilterImage 4
 #define SHADER_TYPE_FillColor 5
 #define SHADER_TYPE_TextureCopyUnclipped 6
 #define SHADER_TYPE_FillGradientConic 8
 #define SHADER_TYPE_FillImageGradientConic 9
 #define SHADER_TYPE_FilterImageColorMatrix 10

float sdroundrect(vec2 pt, vec2 ext, float rad) {
    vec2 ext2 = ext - vec2(rad,rad);
    vec2 d = abs(pt) - ext2;
    return min(max(d.x,d.y),0.0) + length(max(d,0.0)) - rad;
}

// Scissoring
float scissorMask(vec2 p) {
    if (scissorRadius > 0.0) {
        vec2 pt = (scissorMat * vec3(p,1.0)).xy;
        float distance = sdroundrect(pt, scissorExt, scissorRadius);
        return clamp(0.5 - distance * min(scissorScale.x, scissorScale.y), 0.0, 1.0);
    }

    vec2 sc = (abs((scissorMat * vec3(p,1.0)).xy) - scissorExt);
    sc = vec2(0.5,0.5) - sc * scissorScale;
    return clamp(sc.x,0.0,1.0) * clamp(sc.y,0.0,1.0);
}

#ifdef EDGE_AA
// Stroke - from [0..1] to clipped pyramid, where the slope is 1px.
float strokeMask() {
    return min(1.0, (1.0-abs(ftcoord.x*2.0-1.0))*strokeMult) * min(1.0, ftcoord.y);
    // Using this smoothstep preduces maybe better results when combined with fringe_width of 2, but it may look blurrier
    // maybe this should be controlled via flag
    //return smoothstep(0.0, 1.0, (1.0-abs(ftcoord.x*2.0-1.0))*strokeMult) * smoothstep(0.0, 1.0, ftcoord.y);
}
#endif

// Interleaved gradient noise (Jimenez 2014): a cheap, deterministic, screen-space
// ordered dither. Offsetting the gradient colour by up to +/-0.5 of an 8-bit step
// before the framebuffer quantizes it spreads the rounding spatially, breaking up
// the banding that appears between close colours over a large gradient (issue
// femtovg/femtovg#239). The offset is sub-LSB, so solid regions are unaffected and
// the Unorm write clamps it away at the 0.0/1.0 extremes.
float ditherNoise(vec2 p) {
    return fract(52.9829189 * fract(dot(p, vec2(0.06711056, 0.00583715))));
}
vec4 ditherGradient(vec4 color) {
    float d = (ditherNoise(gl_FragCoord.xy) - 0.5) / 255.0;
    return vec4(color.rgb + d, color.a);
}

vec4 renderGradient() {
    // Calculate gradient color using box gradient
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy;

    float d = clamp((sdroundrect(pt, extent, radius) + feather*0.5) / feather, 0.0, 1.0);
    return ditherGradient(mix(innerCol,outerCol,d));
}

// Image-based Gradient; sample a texture using the gradient position.
vec4 renderImageGradient() {
    // Calculate gradient color using box gradient
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy;

    float d = clamp((sdroundrect(pt, extent, radius) + feather*0.5) / feather, 0.0, 1.0);
    return ditherGradient(texture2D(tex, vec2(d, 0.0)));
}

float conicAngleFraction() {
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy;
    // Measure the angle clockwise from the positive x axis. In the gradient's
    // local space (y points down on screen), atan(pt.y, pt.x) increases in the
    // clockwise direction, so offset 0 sits at 3 o'clock and the ramp proceeds
    // clockwise, matching Canvas 2D createConicGradient. fract() wraps the angle
    // into [0, 1) for negative or large start angles.
    return fract((atan(pt.y, pt.x) - conicStartAngle) / TAU);
}

vec4 renderGradientConic() {
    float d = conicAngleFraction();
    return ditherGradient(mix(innerCol,outerCol,d));
}

vec4 renderImageGradientConic() {
    float d = conicAngleFraction();
    return ditherGradient(texture2D(tex, vec2(d, 0.0)));
}

vec4 renderImage() {
    // Calculate color from texture
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy / extent;

    vec4 color = texture2D(tex, pt);

    if (texType == 1) color = vec4(color.xyz * color.w, color.w);
    if (texType == 2) color = vec4(color.x);

    // Apply color tint and alpha.
    color *= innerCol;
    return color;
}

vec4 renderPlainTextureCopy() {
    vec4 color = texture2D(tex, ftcoord);
    if (texType == 1) color = vec4(color.xyz * color.w, color.w);
    if (texType == 2) color = vec4(color.x);
    // Apply color tint and alpha.
    color *= innerCol;
    return color;
}

vec4 renderFilteredImage() {
    float sampleCount = ceil(3.0 * imageBlurFilterSigma);

    vec3 gaussian_coeff = imageBlurFilterCoeff;

    vec4 color_sum = texture2D(tex, fpos.xy / extent) * gaussian_coeff.x;
    float coefficient_sum = gaussian_coeff.x;
    gaussian_coeff.xy *= gaussian_coeff.yz;

    for (float i = 1.0; i <= 24.0; i += 1.) {
        // Work around GLES 2.0 limitation of only allowing constant loop indices by
        // breaking here. Sigma is clamped to 8 on the Rust side and the kernel reaches
        // +/-3*sigma, so the tap count never exceeds this 24-iteration bound.
        if (i >= sampleCount) {
            break;
        }
        color_sum += texture2D(tex, (fpos.xy - i * imageBlurFilterDirection) / extent) * gaussian_coeff.x;
        color_sum += texture2D(tex, (fpos.xy + i * imageBlurFilterDirection) / extent) * gaussian_coeff.x;
        coefficient_sum += 2.0 * gaussian_coeff.x;

        // Compute the coefficients incrementally:
        // https://developer.nvidia.com/gpugems/gpugems3/part-vi-gpu-computing/chapter-40-incremental-computation-gaussian
        gaussian_coeff.xy *= gaussian_coeff.yz;
    }

    vec4 color = color_sum / coefficient_sum;

    if (texType == 1) color = vec4(color.xyz * color.w, color.w);
    if (texType == 2) color = vec4(color.x);

    return color;
}

vec4 renderColorMatrix() {
    // The 4x5 color matrix is packed row-major into frag[0..4] (the scissor/paint
    // matrix slots, unused during a filter pass). Apply it in unpremultiplied
    // sRGB space, clamp to [0,1], then re-premultiply: unpremultiplying avoids
    // edge halos and the clamp keeps overflowing matrices from producing
    // out-of-range or NaN pixels.
    vec4 c = texture2D(tex, fpos.xy / extent);
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

void main(void) {
    vec4 result;

#ifdef EDGE_AA
    float strokeAlpha = 1.0;
#if SELECT_SHADER != 6
    strokeAlpha = strokeMask();
    if (strokeAlpha < strokeThr) discard;
#endif
#else
    float strokeAlpha = 1.0;
#endif

#if SELECT_SHADER == SHADER_TYPE_FillGradient
    // Gradient
    result = renderGradient();
#elif SELECT_SHADER == SHADER_TYPE_FillImageGradient
    // Image-based Gradient; sample a texture using the gradient position.
    result = renderImageGradient();
#elif SELECT_SHADER == SHADER_TYPE_FillImage
    // Image
    result = renderImage();
#elif SELECT_SHADER == SHADER_TYPE_FillColor
    // Plain color fill
    result = innerCol;
#elif SELECT_SHADER == SHADER_TYPE_TextureCopyUnclipped
    // Plain texture copy, unclipped
    gl_FragColor = renderPlainTextureCopy();
    return;
#elif SELECT_SHADER == SHADER_TYPE_Stencil
    // Stencil fill
    result = vec4(1,1,1,1);
#elif SELECT_SHADER == SHADER_TYPE_FilterImage
    // Filter Image
    result = renderFilteredImage();
#elif SELECT_SHADER == SHADER_TYPE_FillGradientConic
    result = renderGradientConic();
#elif SELECT_SHADER == SHADER_TYPE_FillImageGradientConic
    result = renderImageGradientConic();
#elif SELECT_SHADER == SHADER_TYPE_FilterImageColorMatrix
    result = renderColorMatrix();
#else
#error A shader variant must be selected with the SELECT_SHADER pre-processor variable
#endif

    float scissor = scissorMask(fpos);

#ifdef ENABLE_GLYPH_TEXTURE
    // Textured tris
    vec4 mask = texture2D(glyphtex, ftcoord);

    if (glyphTextureType == 1) {
        mask = vec4(mask.x);
    } else {
        result = vec4(1, 1, 1, 1);
        mask = vec4(mask.xyz * mask.w, mask.w);
    }

    mask *= scissor;
    result *= mask;
#else
#if SELECT_SHADER != SHADER_TYPE_Stencil && SELECT_SHADER != SHADER_TYPE_FilterImage && SELECT_SHADER != SHADER_TYPE_FilterImageColorMatrix
        // Not stencil fill
        // Combine alpha
        result *= strokeAlpha * scissor;
#endif
#endif

    gl_FragColor = result;
}
