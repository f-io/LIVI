# Vendored crates

## femtovg 0.27.0

- Text up to 384 px goes through the glyph atlas, upstream switches to path filling above 92 px.
- Path filling leaves a step on flat top edges of large glyphs (P, 7 in the dashboard).
- Drop the copy once upstream renders large text cleanly.
- Its Cargo.toml allows `unused_imports` and `dead_code`: with the default features off, as Slint builds it, upstream leaves `FilePath` and `font_scale` unused. A path dependency shows those warnings, the registry copy would hide them.

## i-slint-backend-android-activity 1.18.1

- Adds `set_touch_hook`: every pointer goes to LIVI with its id and position first, and only reaches Slint when the hook declines it.
- The winit backend hands LIVI the raw touches through its window event hook, the Android backend has no such hook.
- Mouse wheel scrolls become `PointerScrolled` (60 px per notch, like winit).
- Drop the copy once upstream exposes the raw pointers and handles scrolling.
