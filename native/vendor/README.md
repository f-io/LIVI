# Vendored crates

## femtovg 0.27.0

- Text up to 384 px goes through the glyph atlas, upstream switches to path filling above 92 px.
- Path filling leaves a step on flat top edges of large glyphs (P, 7 in the dashboard).
- Drop the copy once upstream renders large text cleanly.
- Only `src/` and the manifest are kept, the book, tests and examples are left out.
- Its Cargo.toml allows `unused_imports` and `dead_code`: with the default features off, as Slint builds it, upstream leaves `FilePath` and `font_scale` unused. A path dependency shows those warnings, the registry copy would hide them.

## i-slint-backend-android-activity 1.19.0

- Adds `set_touch_hook`: every pointer goes to LIVI with its id and position first, and only reaches Slint when the hook declines it.
- The winit backend hands LIVI the raw touches through its window event hook, the Android backend has no such hook.
- Mouse wheel scrolls become `PointerScrolled` (60 px per notch, like winit).
- Drop the copy once upstream exposes the raw pointers and handles scrolling.

## i-slint-backend-linuxkms 1.19.0

- RGB565 comes first, the F133 has no GPU and half the bytes to blend and to scan out.
- The dumb buffers rotate through three, so a back buffer is three frames old. Upstream redraws it whole, LIVI redraws what changed in the two frames since (see the software renderer below).
- Its Cargo.toml allows `dead_code`: upstream leaves `is_write_combined_memory` unused, the registry copy would hide that.
- Drop the copy once upstream renders three buffers partially.

## i-slint-renderer-software 1.19.0

- Rasterised SVGs are cached by image and size. Upstream runs resvg for every icon in every frame, a quarter of the frame time when scrolling the settings on the F133.
- The cache keeps up to 512 KiB, the icons of several screens. SVGs over 256×256 are not cached.
- `set_swapped_buffer_count`: `SwappedBuffers` for more than two buffers, each redrawn wherever a frame since it was last drawn into changed.
- Drop the copy once upstream caches SVGs and knows more than two buffers.

## parley_engine 0.12.0

- On musl (LIVI OS) words and lines break without ICU's dictionaries, which are 3 MB of the UI binary. Slint asks for `complex-scripts` on every native target.
- Only Thai, Lao, Khmer, Burmese and CJK text loses dictionary word breaks; line breaks follow UAX #14 as before.
- Drop the copy once Slint lets the dictionaries go.
