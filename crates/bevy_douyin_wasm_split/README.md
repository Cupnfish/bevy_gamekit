# bevy_douyin_wasm_split

Reusable Wasm call-graph splitting and Brotli packaging for Bevy Mini Game
targets. It builds on Dioxus's `wasm-splitter`, adds a CommonJS loader suitable
for the Douyin runtime, and can compress both the optimized main module and
deferred modules.

## Features

- `runtime` (default): re-exports `wasm_split`, `lazy_loader`, and the runtime
  types from `wasm-splitter` for code compiled into the game.
- `decoder`: builds a small `cdylib` that exposes Brotli decompression through
  wasm-bindgen. Build this feature alone for the bootstrap decoder.
- `cli`: exposes the host-side `package` API and the `bevy-douyin-split` binary.

## Mark a Bevy split point

Alias the dependency as `wasm-split` so the attribute path remains concise:

```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-split = { package = "bevy_douyin_wasm_split", path = "../bevy_douyin_wasm_split" }
wasm-bindgen-futures = "0.4"
```

Defer startup through an async split point on the Mini Game target:

```rust
fn main() {
    #[cfg(all(target_arch = "wasm32", feature = "douyin"))]
    wasm_bindgen_futures::spawn_local(run_split_game());

    #[cfg(not(all(target_arch = "wasm32", feature = "douyin")))]
    run_game();
}

#[cfg(all(target_arch = "wasm32", feature = "douyin"))]
#[wasm_split::wasm_split(bevy_game)]
async fn run_split_game() {
    run_game();
}
```

The raw Cargo Wasm must retain relocation metadata:

```powershell
cargo rustc --release --target wasm32-unknown-unknown --features douyin -- `
  -C link-arg=--emit-relocs -C strip=none
```

Run wasm-bindgen with `--target no-modules`, build this crate separately with
`--no-default-features --features decoder`, then invoke the host CLI:

```powershell
cargo run -p bevy_douyin_wasm_split --features cli --bin bevy-douyin-split -- `
  --original target/wasm32-unknown-unknown/release/game.wasm `
  --bindgen-wasm dist/game_wasm_bg.wasm `
  --bindgen-js dist/game_wasm.js `
  --output dist `
  --decoder-wasm target/wasm32-unknown-unknown/release/bevy_douyin_wasm_split.wasm `
  --wasm-opt target/tools/binaryen/bin/wasm-opt.exe `
  --compression-quality 11
```

`SplitOptions` and `split()` provide the same operation as a Rust API. The
`bevy_douyin_minigame` packaging CLI uses this API directly.

## Output

- `game_wasm_bg.wasm.br`: optimized and compressed main module.
- `brotli_decoder_bg.wasm` and `brotli_decoder.js`: bootstrap decoder.
- `__wasm_split.js`: CommonJS loader and import wiring.
- `wasm-split/*.wasm.br`: compressed shared chunks and split points.
- `wasm-split-report.json`: deterministic byte counts and loader imports.

The host must initialize the decoder, call `configure()` on the split runtime,
decompress and instantiate the main module, and provide package-file reads for
deferred modules. `bevy_douyin_minigame/runtime/game.js.template` is the
reference integration.

## Constraints

- Splitting only moves code exclusively reachable from a split point. Bevy and
  WGPU export many callbacks, so substantial code can remain in the main module.
- Brotli reduces the uploaded package, not the runtime Wasm footprint. The main
  module is fully decompressed before startup; test cold-start latency and peak
  memory on physical devices.
- `wasm-bindgen` CLI and library versions must match exactly.
- `wasm-opt` is optional in the API but strongly recommended for Bevy. Binaryen
  feature flags used by the crate match the Wasm features emitted by this game.
- The vendored `wasm-split-cli` alpha includes a conservative reachability fix
  needed for large Bevy modules; see `vendor/wasm-split-cli/README.md`.
