[![Crates.io](https://img.shields.io/crates/v/femtovg?color=4d76ae)](https://crates.io/crates/femtovg)
[![API](https://docs.rs/femtovg/badge.svg)](https://docs.rs/femtovg)
[![dependency status](https://deps.rs/repo/github/femtovg/femtovg/status.svg)](https://deps.rs/repo/github/femtovg/femtovg)
[![rust: 1.70](https://img.shields.io/badge/MSRV-1.70-success.svg)](#msrv)
[![Discord](https://img.shields.io/discord/691716054560407592)](https://discord.gg/V69VdVu)

<h2 align="center">femtovg</h2>
<p align="center">GPU Antialiased 2D vector drawing library written in Rust.</p>

- [Cargo crate](https://crates.io/crates/femtovg)
- [Book](https://femtovg.github.io)
- [Join the femtovg Discord channel](https://discord.gg/V69VdVu)

---

Ported from https://github.com/memononen/nanovg

Most of the implementation is the same as the original C code with some bug fixes, some features added and several parts have been made more Rust-y. Rendering is done via one OpenGl (ES) 3.0+ backend.

## Screenshots
### Demo
![demo](assets/demo.png)
Run with `cargo run --example demo`

### Breakout
![breakout](assets/breakout.png)
Run with `cargo run --example breakout`

### SVG
![svg](assets/svg.png)
Run with `cargo run --example svg`

### Text
![text](assets/text.png)
Run with `cargo run --example text`

## Features
* [x] Anti-aliasing
* [x] Bézier paths filling and stroking
* [x] Solid color and image pattern fills and strokes
* [x] Gradients - box, linear and radial
* [x] Stroke width and miterlimit
* [x] Stroke caps: butt, round and square
* [x] Stroke joins: miter, round and bevel
* [x] Fill rules - EvenOdd/NonZero
* [x] Rectangle scissoring
* [x] Composition modes (SourceOver, SourceIn, SourceOut, Atop, etc..)
* [x] Global alpha
* [x] Text filling and stroking
* [x] Text shaping
* [x] Text alignment: (left center right), (top, middle, alphabetic, bottom)
* [x] Nearest font matching
* [x] Path hit testing
* [x] OpenGl ES2 backend
* [x] WGPU backend
* [x] Color fonts

## Not supported
* [ ] Stroke dashing
* [ ] Path scissoring
* [ ] Custom shaders
* [ ] 3D transforms

## Ecosystem

FemtoVG is used by these projects:

 - pn-editor <https://gitlab.com/porky11/pn-editor>:
   view, edit, and simulate petri nets
 - Slint <https://slint.dev>:
   A declarative GUI toolkit to build native user interfaces for embedded, desktop, and mobile application
 - inlottie <https://github.com/mhfan/inlottie>:
   lib/tool in Rust for Lottie (Bodymovin) Animation
 - Mapvas <https://github.com/UdHo/mapvas>:
   A map canvas showing OSM tiles with drawing functionality

## License
Licensed under either of
 * Apache License, Version 2.0 ([LICENSE-APACHE](docs/LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](docs/LICENSE-MIT) or http://opensource.org/licenses/MIT)
at your option.

Fonts used in examples:
- Roboto licensed under [Apache license](http://www.apache.org/licenses/LICENSE-2.0)
- Entypo licensed under CC BY-SA 4.0.
- Amiri licensed under [SIL Open Font License, Version 1.1](http://scripts.sil.org/cms/scripts/page.php?site_id=nrsi&id=OFL)
