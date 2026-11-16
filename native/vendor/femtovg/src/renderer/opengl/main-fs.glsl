
precision highp float;

#define UNIFORMARRAY_SIZE 16

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
#define clipInner frag[13].zw
#define clipLinear frag[14]
#define clipOffset frag[15].xy
#define clipOuter frag[15].zw
// An image paint's transfer (0 none, 1 to sRGB, 2 to linearRGB) in the paint
// matrix's first column's w, which no image paint reads.
#define imageTransfer int(frag[3].w)
// Whether a two-stop gradient's endpoints arrive premultiplied (1) or straight (0),
// see GRADIENT_PREMULTIPLIED_INDEX.
#define gradientPremultiplied frag[4].w

uniform sampler2D tex;
uniform sampler2D glyphtex;
uniform sampler2D cliptex;
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
 #define SHADER_TYPE_FillGradientTwoPointRadial 11
 #define SHADER_TYPE_FillImageGradientTwoPointRadial 12
 #define SHADER_TYPE_FilterImageTurbulence 13
 #define SHADER_TYPE_FilterImageTransfer 14
 #define SHADER_TYPE_FilterImageBlend 15
 #define SHADER_TYPE_FilterImageMorphology 16
 #define SHADER_TYPE_FilterImageOffset 17
 #define SHADER_TYPE_FillBlur 18

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

#ifdef CLIP_MASK
// The clip taken as a coverage mask, in the programs of the draws under one:
// the texel under the fragment, of a mask that holds one for each pixel of
// the clip's bounds and nothing along its border - what a fragment past the
// mask reads. Where the mask's corner lies, the pixels it spans and what one
// of them is of its image, which is no smaller, are the halves of the
// scissor matrix's columns that the scissor does not read. An
// operation that coverage cannot bound takes the pixels the clip covers by
// half or more, whole.
float clipMaskCoverage(vec2 p) {
    vec2 at = clamp(p - frag[0].zw, vec2(0.5, 0.5), frag[1].zw - vec2(0.5, 0.5));
    float coverage = texture2D(cliptex, at * frag[2].zw).r;
    if (frag[13].y != 0.0) {
        if (coverage < 0.5) {
            discard;
        }
        return 1.0;
    }
    return coverage;
}
#endif

#ifdef CLIP_SHAPE
// The clip taken as a shape, in the programs of the draws under one:
// coverage of a box with elliptical corners, in a frame where one unit is
// one fringe width across each side. clipInner is where the corners'
// ellipses are centered and clipOuter the half extents plus half a fringe.
// Past a corner's center the distance is the first-order one to its ellipse,
// exact for a circle: k1 is one on the ellipse, and its gradient in device
// pixels is g along the frame's rows - unit vectors, at a right angle only
// without a skew. A box with square corners has their centers out of reach.
float clipMask(vec2 p) {
    vec2 at = vec2(dot(clipLinear.xy, p), dot(clipLinear.zw, p)) + clipOffset;
    vec2 reach = abs(at);
    vec2 corner = reach - clipInner;
    if (corner.x > 0.0 && corner.y > 0.0) {
        vec2 radii = clipOuter - vec2(0.5, 0.5) - clipInner;
        vec2 k = corner / radii;
        float k1 = length(k);
        vec2 g = k / radii;
        float lean = dot(clipLinear.xy, clipLinear.zw) * sign(at.x) * sign(at.y);
        return clamp(0.5 - (k1 - 1.0) * k1 / sqrt(dot(g, g) + 2.0 * g.x * g.y * lean), 0.0, 1.0);
    }
    vec2 cover = clamp(clipOuter - reach, 0.0, 1.0);
    return cover.x * cover.y;
}
#endif

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

// Premultiplies a two-stop gradient's mixed color, unless its endpoints arrive
// premultiplied, and clamps the color channels to [0, alpha], which the dither
// can step outside.
vec4 premultiplyGradient(vec4 color) {
    vec3 rgb = clamp(color.rgb * max(color.a, gradientPremultiplied), vec3(0.0), vec3(color.a));
    return vec4(rgb, color.a);
}

vec4 renderGradient() {
    // Calculate gradient color using box gradient
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy;

    float d = clamp((sdroundrect(pt, extent, radius) + feather*0.5) / feather, 0.0, 1.0);
    vec4 color = ditherGradient(mix(innerCol,outerCol,d));
    return premultiplyGradient(color);
}

// Where a gradient LUT is sampled for position `d`. Texel i holds the color
// at i / 255, so map d = 0 and d = 1 onto the end texels' centres: the pads
// then get the exact stop colors instead of a blend with the next texel.
vec2 lutCoord(float d) {
    return vec2(d * (255.0 / 256.0) + 0.5 / 256.0, 0.0);
}

// Image-based Gradient; sample a texture using the gradient position.
vec4 renderImageGradient() {
    // Calculate gradient color using box gradient
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy;

    float d = clamp((sdroundrect(pt, extent, radius) + feather*0.5) / feather, 0.0, 1.0);
    return ditherGradient(texture2D(tex, lutCoord(d)));
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
    vec4 color = ditherGradient(mix(innerCol,outerCol,d));
    return premultiplyGradient(color);
}

vec4 renderImageGradientConic() {
    float d = conicAngleFraction();
    return ditherGradient(texture2D(tex, lutCoord(d)));
}

// Two-point (independently centered) radial gradient: the general Canvas
// createRadialGradient(x0,y0,r0, x1,y1,r1). paintMat places the start circle at
// the origin, so in local space the start circle is (0,0) radius r0 and the end
// circle is `extent` away with radius r0 + dr (dr in feather). For a fragment pt
// we solve for the interpolation offset t where |pt - t*cd| = r0 + t*dr, i.e.
// a*t^2 - 2b*t + c = 0. `covered` reports whether any interpolated circle with a
// non-negative radius reaches the fragment; uncovered fragments are transparent.
float radialTwoPointT(out bool covered) {
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy;
    vec2 cd = extent;    // end circle center relative to start
    float r0 = radius;
    float dr = feather;  // r1 - r0
    float a = dot(cd, cd) - dr * dr;
    float b = dot(pt, cd) + r0 * dr;
    float c = dot(pt, pt) - r0 * r0;
    covered = false;
    float t = 0.0;
    // `a` is a squared length, so how close to zero it counts as depends on the
    // size of the shape: the same geometry scaled up scales `a` with the square
    // of the scale factor. Comparing it against a fixed constant would treat a
    // focal point sitting on the end circle as a cone in one scene and as a
    // degenerate in another. Measure it against the terms it came from instead,
    // which also folds in the case where both are zero and the circles coincide.
    float a_scale = max(dot(cd, cd), dr * dr);
    if (a_scale == 0.0) {
        // The two circles are the same circle. There is no sweep to walk, so
        // the whole plane takes the far end of the ramp. The Canvas algorithm
        // says to paint nothing here, but SVG resolved the opposite and its
        // test suite asserts the gradient is drawn, so follow SVG.
        t = 1.0;
        covered = true;
    } else if (abs(a) <= 1e-4 * a_scale) {
        // Degenerate cone (|cd| == |dr|): the quadratic collapses to the linear
        // equation -2b*t + c = 0. This is exactly the case where the end center
        // sits on the start circle, common in focal-style gradients.
        if (abs(b) > 1e-6) {
            t = c / (2.0 * b);
            covered = (r0 + t * dr >= 0.0);
        }
    } else {
        float disc = b * b - a * c;
        // When one circle strictly contains the other (a < 0), every fragment
        // is on some interpolated circle and the discriminant is never really
        // negative: at the focal point it is mathematically zero and rounds
        // just below, which would punch a hole exactly where the first stop
        // belongs. So a negative discriminant only counts as a miss for a > 0.
        if (a < 0.0 || disc >= 0.0) {
            float s = sqrt(max(disc, 0.0));
            // One reciprocal, two roots (a is guaranteed away from zero here).
            float inv_a = 1.0 / a;
            float root_a = (b - s) * inv_a;
            float root_b = (b + s) * inv_a;
            float t_lo = min(root_a, root_b);
            float t_hi = max(root_a, root_b);
            // Canvas draws circles from the largest offset toward the smallest and
            // does not overpaint, so the largest offset whose interpolated circle
            // has a non-negative radius is the one that claims the fragment.
            if (r0 + t_hi * dr >= 0.0) {
                t = t_hi;
                covered = true;
            } else if (r0 + t_lo * dr >= 0.0) {
                t = t_lo;
                covered = true;
            }
        }
    }
    return clamp(t, 0.0, 1.0);
}

vec4 renderGradientTwoPointRadial() {
    bool covered;
    float d = radialTwoPointT(covered);
    if (!covered) return vec4(0.0);
    vec4 color = ditherGradient(mix(innerCol, outerCol, d));
    return premultiplyGradient(color);
}

vec4 renderImageGradientTwoPointRadial() {
    bool covered;
    float d = radialTwoPointT(covered);
    if (!covered) return vec4(0.0);
    return ditherGradient(texture2D(tex, lutCoord(d)));
}

// The sRGB transfer curve (IEC 61966-2-1) on a premultiplied color, run on
// the unpremultiplied color, alpha untouched: to sRGB when toSrgb, else to
// linearRGB.
vec4 transferColor(vec4 c, bool toSrgb) {
    if (c.a > 0.0) {
        c.rgb /= c.a;
    }
    vec3 x = clamp(c.rgb, 0.0, 1.0);
    vec3 y;
    if (toSrgb) {
        // mix evaluates both sides: keep pow off 0, where an exp2(log2) under fast math can give a NaN that
        // mix carries even at weight 0. The clamp only touches the side step leaves out.
        y = mix(x * 12.92, 1.055 * pow(max(x, vec3(0.0031308)), vec3(1.0 / 2.4)) - 0.055, step(0.0031308, x));
    } else {
        y = mix(x / 12.92, pow((x + 0.055) / 1.055, vec3(2.4)), step(0.04045, x));
    }
    return vec4(y * c.a, c.a);
}

// A sampled image texel as the paint uses it: premultiplied, converted by
// the paint's transfer if it has one - rounded to 8 bits, as the transfer's
// pass stores its result - then tinted.
vec4 paintTexel(vec4 color) {
    if (texType == 1) color = vec4(color.xyz * color.w, color.w);
    if (texType == 2) color = vec4(color.x);
    if (imageTransfer != 0) color = floor(transferColor(color, imageTransfer == 1) * 255.0 + 0.5) / 255.0;
    // Apply color tint and alpha.
    return color * innerCol;
}

vec4 renderImage() {
    // Calculate color from texture
    vec2 pt = (paintMat * vec3(fpos, 1.0)).xy / extent;

    // Half a level toward the larger mip: browsers minify sharper than plain
    // trilinear (Chromium samples the larger level, Firefox downsamples
    // directly); no effect without mipmaps or under magnification.
    return paintTexel(texture2D(tex, pt, -0.5));
}

vec4 renderPlainTextureCopy() {
    return paintTexel(texture2D(tex, ftcoord, -0.5));
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
vec4 blurTap(vec2 pos) {
    vec2 uv = pos / extent;
#if SELECT_SHADER == SHADER_TYPE_FillBlur
    vec2 inside = vec2(1.0);
#else
    vec2 inside = step(vec2(0.0), uv) * step(uv, vec2(1.0));
#endif
    vec4 color = texture2D(tex, uv);
    if (texType == 1) color = vec4(color.xyz * color.w, color.w);
    if (radius > 0.5) color = vec4(0.0, 0.0, 0.0, color.w);
    return color * inside.x * inside.y;
}

// Defined in filters.glsl: the color matrix in the parameter slots applied
// to a premultiplied color.
vec4 colorMatrixOn(vec4 premultiplied);

// The Gaussian along imageBlurFilterDirection, centered on `center` in
// source pixels.
vec4 blurAt(vec2 center) {
    float sampleCount = ceil(3.0 * imageBlurFilterSigma);

    vec3 gaussian_coeff = imageBlurFilterCoeff;

    vec4 color_sum = blurTap(center) * gaussian_coeff.x;
    float coefficient_sum = gaussian_coeff.x;
    gaussian_coeff.xy *= gaussian_coeff.yz;

    for (float i = 1.0; i <= 24.0; i += 1.) {
        // Work around GLES 2.0 limitation of only allowing constant loop indices by
        // breaking here. Each pass's sigma is bounded to 8 on the Rust side (a larger
        // blur is split into passes that compose to it) and the kernel reaches
        // +/-3*sigma, so the tap count never exceeds this 24-iteration bound.
        if (i >= sampleCount) {
            break;
        }
        color_sum += blurTap(center - i * imageBlurFilterDirection) * gaussian_coeff.x;
        color_sum += blurTap(center + i * imageBlurFilterDirection) * gaussian_coeff.x;
        coefficient_sum += 2.0 * gaussian_coeff.x;

        // Compute the coefficients incrementally:
        // https://developer.nvidia.com/gpugems/gpugems3/part-vi-gpu-computing/chapter-40-incremental-computation-gaussian
        gaussian_coeff.xy *= gaussian_coeff.yz;
    }

    return color_sum / coefficient_sum;
}

vec4 renderFilteredImage() {
    // The quad's texture coordinates say where in the source it samples.
    vec4 color = blurAt(ftcoord);

    // A color matrix fused into this second draw.
    if (feather > 0.5) color = colorMatrixOn(color);
    if (texType == 2) color = vec4(color.x);

    return color;
}

// A blur's draw as a fill: its texture coordinates carry the fill's fringe,
// so paintMat maps the fragment to where it samples.
vec4 renderFillBlur() {
    return blurAt((paintMat * vec3(fpos, 1.0)).xy) * innerCol.a;
}

// The filter passes, defined in filters.glsl.
vec4 renderColorMatrix();
vec4 renderTurbulence();
vec4 renderTransfer();
vec4 renderBlend();
vec4 renderMorphology();
vec4 renderOffset();

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
#elif SELECT_SHADER == SHADER_TYPE_FillGradientTwoPointRadial
    result = renderGradientTwoPointRadial();
#elif SELECT_SHADER == SHADER_TYPE_FillImageGradientTwoPointRadial
    result = renderImageGradientTwoPointRadial();
#elif SELECT_SHADER == SHADER_TYPE_FilterImageTurbulence
    result = renderTurbulence();
#elif SELECT_SHADER == SHADER_TYPE_FilterImageTransfer
    result = renderTransfer();
#elif SELECT_SHADER == SHADER_TYPE_FilterImageBlend
    result = renderBlend();
#elif SELECT_SHADER == SHADER_TYPE_FilterImageMorphology
    result = renderMorphology();
#elif SELECT_SHADER == SHADER_TYPE_FilterImageOffset
    result = renderOffset();
#elif SELECT_SHADER == SHADER_TYPE_FillBlur
    result = renderFillBlur();
#else
#error A shader variant must be selected with the SELECT_SHADER pre-processor variable
#endif

    float scissor = scissorMask(fpos);
#ifdef CLIP_SHAPE
    scissor *= clipMask(fpos);
#endif
#ifdef CLIP_MASK
    scissor *= clipMaskCoverage(fpos);
#endif

#ifdef ENABLE_GLYPH_TEXTURE
#if SELECT_SHADER != SHADER_TYPE_FilterImageBlend
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
#endif
#else
#if SELECT_SHADER != SHADER_TYPE_Stencil && SELECT_SHADER != SHADER_TYPE_FilterImage && SELECT_SHADER != SHADER_TYPE_FilterImageColorMatrix && SELECT_SHADER != SHADER_TYPE_FilterImageTurbulence && SELECT_SHADER != SHADER_TYPE_FilterImageTransfer && SELECT_SHADER != SHADER_TYPE_FilterImageMorphology && SELECT_SHADER != SHADER_TYPE_FilterImageOffset
        // Not stencil fill
        // Combine alpha
        result *= strokeAlpha * scissor;
#endif
#endif

    gl_FragColor = result;
}
