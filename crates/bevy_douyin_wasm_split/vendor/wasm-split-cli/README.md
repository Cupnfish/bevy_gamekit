# Vendored wasm-split-cli

This is the `wasm-split-cli` 0.8.0-alpha.0 library from Dioxus commit
`a82361ef1ad2ffe0219a82992fe4642b59ba88f9`, licensed under MIT or Apache-2.0.
It is vendored because the published alpha can delete functions that remain
referenced by the surviving Wasm IR. That causes Walrus to panic while
splitting large Bevy modules.

The local compatibility patch adds `ref.func` globals to the main roots and
conservatively preserves split candidates still referenced by surviving
functions, exports, the start function, globals, element segments, or shared
jump-table symbols. The remainder of `src/lib.rs` and `src/__wasm_split.js` is
unchanged. `LICENSE-MIT` and `LICENSE-APACHE` are copied from that upstream
commit.
