# Changelog
All notable changes to this project will be documented in this file.

## [0.27.0] - 2026-08-31

- Added text decoration for `fill_text()` and `stroke_text()`: underline,
  strikethrough and overline. New `TextDecoration` type, set through
  `Paint::set_text_decoration()` and `with_text_decoration()`. Line position and
  thickness come from the font's own metrics.
- Added `Path::svg_arc_to()`, the SVG path data `A` command: an elliptical arc
  to an endpoint, with per-axis radii, an x-axis rotation, and the large-arc and
  sweep flags.
- Added `ImageFilter::ColorMatrix` (SVG `feColorMatrix`) with constructors for
  the CSS filter functions: `grayscale()`, `sepia()`, `saturate()`,
  `hue_rotate()`, `brightness()`, `contrast()`, `invert()` and `opacity()`.
- Added elliptical radial gradients, matching CSS `radial-gradient(ellipse ...)`.
  New `Paint` constructors `elliptical_gradient()` and
  `elliptical_gradient_stops()` take separate radii per axis.
  `radial_gradient()` and `radial_gradient_stops()` are unchanged.
- **Breaking:** `PaintFlavor::RadialGradient`'s `in_radius` and `out_radius`
  changed from `f32` to `(f32, f32)` to hold the per-axis radii. Paints
  serialized by earlier versions no longer deserialize.
- Added typesetting metrics to `FontMetrics`: `subscript_size()`,
  `subscript_offset()`, `superscript_size()`, `superscript_offset()`,
  `x_height()`, `cap_height()`, `line_gap()`, `underline_position()`,
  `underline_thickness()`, `strikeout_position()` and `strikeout_thickness()`.
  Missing or zeroed font tables fall back to conventional em fractions.
- Added `TextMetrics::baseline()`, the run's baseline in the same space as the
  glyph positions.
- Fixed the paragraph base direction of shaped text: it now follows the first
  strong character (UAX #9 P2/P3) instead of always being left to right, so an
  Arabic or Hebrew sentence orders its punctuation and embedded words
  correctly. The shaped word cache now also keys on the run direction, so a word
  shaped in one direction is no longer reused in the other.
- Fixed `stroke_text()` line widths under a scaled canvas transform. The width
  was left unscaled for atlas glyphs and scaled twice for path fallback glyphs,
  so it changed with the zoom, the font size and the paint.
- Fixed `Canvas::measure_font()` scaling its result by the internal glyph
  rasterization scale and the DPI factor. It now reports user space metrics,
  like `measure_text()` and `TextContext::measure_font()` do.
- Fixed `measure_text()` returning a run box shifted down by the glyph bearing
  (about 4.5px at font size 12). `TextMetrics::y` and `height()` are measured
  from the glyph ink again, not from the baseline.
- Fixed `Path::rounded_rect()` and `rounded_rect_varying()` squashing corners
  into ellipses when a radius did not fit. Radii that overlap are now reduced by
  one common factor, as CSS and the Canvas `roundRect()` algorithm specify, and
  negative or NaN radii leave the corner square. This changes rendering for
  shapes whose radii did not fit.
- Fixed `Path::arc_to()` drawing a thin seam across the fill when the corner
  could not be rounded, and `Path::arc()` producing NaN control points for a
  zero sweep (#309).
- Fixed the WGPU renderer aborting instead of returning an error when
  `update_image()` is given a copy that reaches past the destination image, or a
  source in a different pixel format, as the other renderers already did. The
  shared check is exposed as `ImageSource::check_update()` for out-of-tree
  renderers.
- Fixed Gray texture updates being dropped by strict OpenGL drivers: the
  external format passed to `glTexSubImage2D` is now `RED`, not the sized `R8`,
  which is invalid there.
- Fixed OpenGL textures being created with undefined contents, which could show
  up as specks at glyph edges on embedded drivers (#310). They are now zeroed at
  creation, as the WGPU backend already guaranteed.
- Sped up the WGPU backend, most of all on WebAssembly. The frame's uniforms go
  into one buffer, uploaded in a single call and bound with dynamic offsets; the
  vertex buffer stays resident across frames; and samplers, texture views,
  pipeline and bind group state are no longer rebuilt for every draw.

## [0.26.0] - 2026-07-20

- Added Canvas 2D drop shadows for fills, strokes and text. New `Canvas` methods
  `set_shadow_color()`, `set_shadow_blur()` and `set_shadow_offset()`. Shadows
  cost a per-draw offscreen blur; a transparent shadow color restores the
  zero-overhead path. Thanks @matthargett
- Added rounded scissor support. New `Canvas` methods `rounded_scissor()` and
  `intersect_rounded_scissor()` take a corner radius; intersections that can't
  be represented exactly fall back to rectangular scissoring. Thanks
  @matthargett
- Added a start angle to conic gradients for Canvas conformance. New `Paint`
  constructors `conic_gradient()`, `conic_gradient_with_angle()` and
  `conic_gradient_stops_with_angle()`. Previously serialized paints still
  deserialize, defaulting to the prior no-rotation behavior. Thanks
  @matthargett
- Added an `ImageSource::HtmlCanvasElement` variant, letting callers hand over a
  source already rasterized at the wanted size. This avoids a wgpu panic when
  enlarging an `HtmlImageElement` for hidpi output. Thanks @yebei199
- Gradients are dithered to reduce banding. Thanks @matthargett
- Bumped WGPU renderer to use WGPU 30.x
- Fixed text layout to preserve the fractional baseline (#281). Thanks
  @matthargett
- Fixed scaled-atlas text positioning to use the true scale rather than the
  quantized one. Thanks @matthargett

## [0.25.1] - 2026-05-29

- Added dashed stroke support. New `Paint` methods `set_line_dash()` /
  `line_dash()` / `with_line_dash()` and `set_line_dash_offset()` /
  `line_dash_offset()` / `with_line_dash_offset()`, plus `Path::dashed()` to
  produce a dashed copy of a path.
- Render text crisply under uniform-scale transforms: glyphs drawn with a
  uniform scale combined with a translation are now rasterized into the atlas
  at the on-screen size (for solid color fills) instead of falling back to
  path rendering.

## [0.25.0] - 2026-05-13

- Bumped WGPU renderer to use WGPU 29.x. Thanks @matthargett

## [0.24.0] - 2026-05-05

- WGPU: Fixed `HtmlImageElement` upload path.
- Added external texture support. New `Canvas::create_image_from_external_texture()`
  for importing platform-specific external textures (e.g. EGL/OES external
  textures in OpenGL, external texture views in WGPU).

## [0.23.2] - 2026-04-13

- Fall back to path rendering for glyphs under non-translation transforms.

## [0.23.1] - 2026-03-31

- Fix variable font glyph caching returning wrong variation instance when
  using swash. Stale normalized coordinates in swash's ScaleContext could
  cause glyphs from a previous variation (e.g. bold) to appear for
  subsequent default-instance renders.

## [0.23.0] - 2026-03-30

- Added variable font support. New methods on `Paint`: `set_font_weight()`,
  `set_font_italic()`, `set_font_slant()`, and generic `set_font_variation()`
  with corresponding getters, `with_` builders, and `clear_` methods.
- Added named font weight constants on `Paint` (`FONT_WEIGHT_THIN` through
  `FONT_WEIGHT_BLACK`).
- Added `Canvas::font_variation_axes()` to query available variation axes
  for variable fonts, and new `VariationAxisInfo` public type.
- **Breaking:** `Canvas::fill_glyph_run()` and `Canvas::stroke_glyph_run()`
  now take an additional `normalized_coords: &[i16]` parameter for variable
  font axis positions. Pass `&[]` for default behavior.
- Replaced bundled Roboto with Roboto Flex for examples and added slant
  support with italic fallback.

## [0.22.0] - 2026-03-26

- WGPU: Changed API from `flush_to_surface()` to `flush_to_output()` and
  accept a type that can be converted to a new WGPURenderOutput struct,
  making it possible to render into texture views.
- WGPU: Simplified CommandBuffer to be an Option<>, so that it can be
  passed to queue's `submit()` without additional wrapping.

## [0.21.0] - 2026-03-23

- Re-release 0.20.5 as 0.21 as the glow dependency upgrade is a public
  dependency that came with a new major version.

## [0.20.5] - 2026-03-23

- Fix text rendering with wgpu and mesa versions that have difficulties
  with pipeline override constants.
- Make the ttf-parser dependency optional.

## [0.20.4] - 2026-02-23

- Fix occasional fringes around swash rendered glyphs caused by uninitialized
  padding.
- Ported examples to latest winit/glutin/parley/cosmic-text versions.

## [0.20.3] - 2026-02-18

- Upgraded swash to the latest version.

## [0.20.2] - 2026-02-18

- Added `swash` feature to enable rasterization of glyphs with swash instead of
  the built-in path renderer.

## [0.20.1] - 2026-01-15

- Lowered MSRV requirement as it's only opt-in via wgpu feature.

## [0.20.0] - 2026-01-14

- Bumped wgpu dependency to wgpu-28
- Bumped MSRV to 1.92

## [0.19.3] - 2025-10-13

- Fix regression in text rendering performance.

## [0.19.2] - 2025-10-10

- Fix erroneous glyph position regression of commit a1e215782a60df7e4fde9271fe7f95c134ac832f
  that accidentally subtracted bearing.

## [0.19.1] - 2025-10-08

- Fix docs.rs build

## [0.19.0] - 2025-10-07

- Bump MSRV to 1.88.
- breaking: Upgraded WGPU renderer to use WGPU 27.x.
- breaking: Make text layout an optional feature

## [0.18.1] - 2025-09-25

- Fix regression causing panic when rendering text.

## [0.18.0] - 2025-09-25

- Added API to drawing runs of glyphs from the same font.

## [0.17.0] - 2025-09-05

- Bump MSRV to 1.85.
- Added support for conical gradients.

## [0.16.0] - 2025-08-03

 - Bumped WGPU renderer to use WGPU 26.x.

## [0.15.0] - 2025-07-03

 - Bumped WGPU renderer to use WGPU 25.x.

## [0.14.1] - 2025-06-14

 - Fixed accidental rendering of newline (and other control characters) when using the Inter font. (#236) (thanks @peterprototypes)
 - WGPU renderer: Fixed panic when rendering empty scenes.

## [0.14.0] - 2025-03-24

- Bump MSRV to 1.84.
- Fixed WGPU web rendering (thanks @JoshBurbidge)

## [0.13.0] - 2025-01-29

 - Bump MSRV to 1.81.
 - Bump wgpu to 0.24.
 - **breaking**: The WGPU renderer is now constructed with a wgpu Device/Queue
   ithat's not wrapped in an Arc anymore. These types implement clone themselves.

## [0.12.0] - 2025-01-14

 - WGPU renderer: Changed `flush_to_surface()` API to return a command buffer,
   to let the application decide when to submit.
 - Bumped glow dependency.

## [0.11.3] - 2024-12-26

 - WGPU renderer: Fix crash when rendering without always calling `set_size()`. (#226)

## [0.11.1] - 2024-11-17

 - No code changes, just a release for docs.rs.

## [0.11.0] - 2024-11-17

 - Added WGPU renderer, behind `wgpu` feature flag.
 - Fixed rendering of glyphs with overlaps (#183). Thanks to Richard Hozák.
 - Bumped MSRV to 1.76.

## [0.10.1] - 2024-10-24

 - Fix accidental breakage with scissor clipping.

## [0.10.0] - 2024-10-23

- **breaking**: Removed the mutable reference of self in `new` constructor for Transform2D
 - Implemented arithmetic operations for `Transform2D`
 - Completed and improved documentation
- **breaking**: Removed methods `multipy` from `Transform2D`, since arithmetic operations are defined for `Transform2D` now.
- **breaking**: Renamed `Transform2D` methods `inversed` to `inverse` and `inverse` to `invert`.
- **breaking**: Renamed `Transform2D` constructor `new_translation` to `translation` and added new constructors `rotation` and `scaling`.
- Reimplemented `Transform2D` transformation functions (`translate`, `rotate`, `scale`, `skew_x`, `skew_y`) to do what they are supposed to.
- **breaking**: glow dependency bumped.

## [0.9.2] - 2024-06-27

 - Fix path rendering where the default path solidity would interfere with the path's
   own winding direction (https://github.com/femtovg/femtovg/issues/124)
 - Fix blurry text rendering when drawing on non-integer coordinates
 - Bumped MSRV to 1.68.

## [0.9.1] - 2024-04-12

 - Fixed inability to introspect `Path` verbs by making `PathIter` and `Verb` public.
 - Fixed rendering of text strokes with large font sizes.

## [0.9.0] - 2024-02-27

 - **breaking**: Removed pub key field in ImageId. This accidentally
   exposed the implementation detail of the image store (generational-arena),
   which has been replaced with slotmap.
 - For WASM builds, require WebGL 2. This is supported by all major browsers
   and needed to make `ImageFlags::REPEAT_X/Y` work.
 - Bumped MSRV to 1.66.

## [0.8.2] - 2024-01-20

 - Improved performance when rendering large texts.
 - Replace error logging to stderr with use of log crate.

## [0.8.1] - 2023-12-18

 - Fix documentation build on docs.rs.

## [0.8.0] - 2023-11-02

 - Re-release 0.7.2 with major version bump. 0.7.2 was yanked because
   glow is a re-exported public dependency, that was bumped.

## [0.7.2] - 2023-11-02

 - Bump internal dependencies.

## [0.7.1] - 2023-06-14

- Fix performance regression when drawing unclipped image path fills.

## [0.7.0] - 2023-05-26

### Changed

 - Path drawing functions now take a `&Path` instead of a `&mut Path` and use interior mutability
   for caching.

## [0.6.0] - 2023-02-06

### Changed

 - Changed `linear_gradient_stops` and `radial_gradient_stops` to take an `IntoIterator`
   instead of a slice slice for the color stops.

## [0.5.0] - 2023-02-06

### Added

 - added a new `Size` struct, having a `width` and a `height`.
 - added `size` function to `Image` type, which returns both, `width` and `height` as a `Size`

### Changed

 - Renamed `draw_glyph_cmds` to `draw_glyph_commands`.
 - Renamed `DrawCmd` to `DrawCommand`.
 - `set_transform` takes a value of type `Transform2D` now instead of a parameter list.
 - `dimensions` of `ImageSource` returns a new `Size` type now.

## [0.4.0] - 2023-01-27

### Added

 - `OpenGl::new_from_function_cstr` to create the renderer from a GL loading function that
   takes an `&std::ffi::CStr`.

### Fixed

 - Fixed erroneously multiply applied global alpha when mixing color glyphs with regular glyphs.

### Changed

 - MRSV was bumped to Rust 1.63, the crate now uses Rust Edition 2021.
 - `new_from_glutin_context` can now be used with headless contexts.
 - All const-safe `Color` constructors are now const.
 - `Canvas`'s text layout methods no longer require a mutable reference.
 - Removed the copy trait from `Paint` to avoid accidental copies.
 - `Paint` is always supplied by reference now.
 - `TextContext`'s `resize_shaping_run_cache` and `resize_shaped_words_cache` functions now take a
   `std::num::NonZeroUsize` for the capacity value.
 - As part of the glutin update, `OpenGL::new_from_glutin_context` was renamed to `new_from_glutin_display` and takes a glutin display now.
 - Removed `glutin` from the default features.

## [0.3.7] - 2022-10-24

### Fixed

 - Fix build with latest rustybuzz release after 0.5.2 breakage. 0.5.3 doesn't
   re-export the ttf_parser module anymore.

## [0.3.6] - 2022-10-23

### Fixed

 - Fix build with latest rustybuzz release.

## [0.3.5] - 2022-05-23

### Changed

 - Optimized the OpenGL renderer to perform better on older GPUs by splitting the large fragment shader
   into smaller programs.

## [0.3.4] - 2022-04-07

### Added

 - Added support for importing backend-specific textures into the rendering of a scene with `Canvas::create_image_from_native_texture`.
 - Added functions to `TextContext` to configure the text shaping caches: `resize_shaping_run_cache` and `resize_shaped_words_cache`.

### Changed

 - Added optimized rendering code path for the common case of filling a rectangular path with an image and anti-aliasing
   on the paint disabled.

### Fixed

 - Fixed line breaking to permit a break in the middle of a word if it is the first word in the paragraph
   and it doesn't fit otherwise.

## [0.3.3] - 2022-02-21

### Changed

 - Bumped rustybuzz and ttf-parser dependencies.

## [0.3.2] - 2022-02-09

### Fixed

 - Correctly detect when WebGL is disabled in a web browser in the `renderer::OpenGL::new_from_html_canvas` function.

## [0.3.1] - 2022-02-08

### Fixed

 - Don't require default features of glutin. We don't need any and this way other users of glutin
   have the ability to opt out.

## [0.3.0] - 2022-02-04

### Changed

 - **Breaking:** The dependency to the `image` crate was bumped from `0.23` to `0.24`.
   Since the types of this crate are used in public femtovg API, users need to upgrade
   their dependency to the `image` crate as well.
 - **Breaking**: Removed deprecated `renderer::OpenGL::new` function. Use `renderer::OpenGl::new_from_function`
   or `renderer::OpenGl::new_from_glutin_context`.

### Added

 - Use `Paint::image_tint` to create an image paint that not only applies an alpha but an entire color (tint).

### Fixed

 - Improved performance of `fill_path` and `stroke_path`

[0.3.0]: https://github.com/femtovg/femtovg/releases/tag/v0.3.0
[0.3.1]: https://github.com/femtovg/femtovg/releases/tag/v0.3.1
[0.3.2]: https://github.com/femtovg/femtovg/releases/tag/v0.3.2
[0.3.3]: https://github.com/femtovg/femtovg/releases/tag/v0.3.3
[0.3.4]: https://github.com/femtovg/femtovg/releases/tag/v0.3.4
[0.3.5]: https://github.com/femtovg/femtovg/releases/tag/v0.3.5
[0.3.6]: https://github.com/femtovg/femtovg/releases/tag/v0.3.6
[0.3.7]: https://github.com/femtovg/femtovg/releases/tag/v0.3.7
[0.4.0]: https://github.com/femtovg/femtovg/releases/tag/v0.4.0
[0.5.0]: https://github.com/femtovg/femtovg/releases/tag/v0.5.0
[0.6.0]: https://github.com/femtovg/femtovg/releases/tag/v0.6.0
[0.7.0]: https://github.com/femtovg/femtovg/releases/tag/v0.7.0
[0.7.1]: https://github.com/femtovg/femtovg/releases/tag/v0.7.1
[0.7.2]: https://github.com/femtovg/femtovg/releases/tag/v0.7.2
[0.8.0]: https://github.com/femtovg/femtovg/releases/tag/v0.8.0
[0.8.1]: https://github.com/femtovg/femtovg/releases/tag/v0.8.1
[0.8.2]: https://github.com/femtovg/femtovg/releases/tag/v0.8.2
[0.9.0]: https://github.com/femtovg/femtovg/releases/tag/v0.9.0
[0.9.1]: https://github.com/femtovg/femtovg/releases/tag/v0.9.1
[0.9.2]: https://github.com/femtovg/femtovg/releases/tag/v0.9.2
[0.10.0]: https://github.com/femtovg/femtovg/releases/tag/v0.10.0
[0.10.1]: https://github.com/femtovg/femtovg/releases/tag/v0.10.1
[0.11.0]: https://github.com/femtovg/femtovg/releases/tag/v0.11.0
[0.11.1]: https://github.com/femtovg/femtovg/releases/tag/v0.11.1
[0.11.3]: https://github.com/femtovg/femtovg/releases/tag/v0.11.3
[0.12.0]: https://github.com/femtovg/femtovg/releases/tag/v0.12.0
[0.13.0]: https://github.com/femtovg/femtovg/releases/tag/v0.13.0
[0.14.0]: https://github.com/femtovg/femtovg/releases/tag/v0.14.0
[0.14.1]: https://github.com/femtovg/femtovg/releases/tag/v0.14.1
[0.15.0]: https://github.com/femtovg/femtovg/releases/tag/v0.15.0
[0.16.0]: https://github.com/femtovg/femtovg/releases/tag/v0.16.0
[0.17.0]: https://github.com/femtovg/femtovg/releases/tag/v0.17.0
[0.18.0]: https://github.com/femtovg/femtovg/releases/tag/v0.18.0
[0.18.1]: https://github.com/femtovg/femtovg/releases/tag/v0.18.1
[0.19.0]: https://github.com/femtovg/femtovg/releases/tag/v0.19.0
[0.19.1]: https://github.com/femtovg/femtovg/releases/tag/v0.19.1
[0.19.2]: https://github.com/femtovg/femtovg/releases/tag/v0.19.2
[0.19.3]: https://github.com/femtovg/femtovg/releases/tag/v0.19.3
[0.20.0]: https://github.com/femtovg/femtovg/releases/tag/v0.20.0
[0.20.1]: https://github.com/femtovg/femtovg/releases/tag/v0.20.1
[0.20.2]: https://github.com/femtovg/femtovg/releases/tag/v0.20.2
[0.20.3]: https://github.com/femtovg/femtovg/releases/tag/v0.20.3
[0.20.4]: https://github.com/femtovg/femtovg/releases/tag/v0.20.4
[0.20.5]: https://github.com/femtovg/femtovg/releases/tag/v0.20.5
[0.21.0]: https://github.com/femtovg/femtovg/releases/tag/v0.21.0
[0.22.0]: https://github.com/femtovg/femtovg/releases/tag/v0.22.0
[0.23.0]: https://github.com/femtovg/femtovg/releases/tag/v0.23.0
[0.23.1]: https://github.com/femtovg/femtovg/releases/tag/v0.23.1
[0.23.2]: https://github.com/femtovg/femtovg/releases/tag/v0.23.2
[0.24.0]: https://github.com/femtovg/femtovg/releases/tag/v0.24.0
[0.25.0]: https://github.com/femtovg/femtovg/releases/tag/v0.25.0
[0.25.1]: https://github.com/femtovg/femtovg/releases/tag/v0.25.1
[0.26.0]: https://github.com/femtovg/femtovg/releases/tag/v0.26.0
[0.27.0]: https://github.com/femtovg/femtovg/releases/tag/v0.27.0
