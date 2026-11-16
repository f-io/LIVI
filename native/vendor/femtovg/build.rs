fn main() {
    cfg_aliases::cfg_aliases! {
        // wasm32 in the browser through web-sys and wasm-bindgen, as opposed to Emscripten.
        wasm_unknown: { all(target_arch = "wasm32", target_os = "unknown") },
    }
}
