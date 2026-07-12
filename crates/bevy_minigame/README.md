# bevy_douyin_minigame

Runtime bridge and packaging tools for running a Bevy WebGL build as a Douyin
Mini Game.

Douyin Mini Games expose `tt.createCanvas()` and WebGL, but do not expose a
browser DOM. This crate supplies:

- `DouyinMiniGamePlugin`, system information, lifecycle messages, storage, and
  preferred-FPS APIs.
- `runtime/adapter.js`, a compatibility surface for Bevy 0.19's winit, wgpu,
  touch input, and HTTP-style asset reader.
- `bevy-douyin`, a CLI that converts raw Cargo Wasm with wasm-bindgen's
  `no-modules` target and creates an importable Mini Game project.
- Optional integration with `bevy_douyin_wasm_split` for optimized Brotli
  modules and a package-local asynchronous split loader.

## Bevy setup

```toml
[dependencies]
bevy_douyin_minigame = { path = "crates/bevy_douyin_minigame" }
```

```rust
#[cfg(all(target_arch = "wasm32", feature = "douyin"))]
app.add_plugins(bevy_douyin_minigame::DouyinMiniGamePlugin::default());
```

Use `#bevy-canvas` as the primary Bevy canvas selector and `assets` as the
default Wasm asset root. The generated adapter creates the visible Douyin canvas
before Bevy starts and exposes it through that selector. Platforms that place
assets in a runtime subpackage publish the effective root in `assetRoot`.

## Package

Build a raw Wasm binary, then run:

```sh
cargo run -p bevy_douyin_minigame --features cli --bin bevy-douyin -- \
  package \
  --wasm target/wasm32-unknown-unknown/release/my_game.wasm \
  --assets assets \
  --output dist-douyin \
  --project-name my-game
```

The installed `wasm-bindgen` CLI must exactly match the version in
`Cargo.lock`. The output contains `game.js`, `game.json`,
`project.config.json`, the adapter, Wasm glue, assets, and a size report.
The generated configuration uses `douyinProjectType: "native"`, matching the
JavaScript Mini Game projects created by current Douyin Developer Tool builds.

Use repeatable `--exclude-extension` arguments for files not needed by the
platform build:

```sh
--exclude-extension ogg --exclude-extension wav
```

For a split build, compile the game with relocation metadata, pass a decoder
Wasm and `wasm-opt` to the package command, and mark at least one async split
point in the game. The repository-level `scripts/build-douyin.ps1` demonstrates
the complete flow. It also sets `--max-total-mib 30`, which makes packaging fail
before replacing a valid output directory when the generated package is too
large. See `../bevy_douyin_wasm_split/README.md` for the reusable API and file
format.

## Current boundaries

- wasm-bindgen `inline_js` and imported JavaScript modules are incompatible
  with the required `no-modules` output. Gate browser-only bindings out of the
  Douyin feature and bind platform methods through the global `DouyinBevy`
  namespace instead.
- Browser WebAudio is not available in Mini Game hosts. Browser builds can keep
  Bevy's normal `AudioPlayer` backend; Mini Game game-side WASM bindings should
  use the adapter's `audioCreate`, playback controls, and managed one-shot
  helpers backed by `createInnerAudioContext()` instead.
- The adapter is tested with a Node mock, but WebGL behavior and package limits
  must be verified in the official Douyin Developer Tool and on a device.
- Split builds decompress the optimized main Wasm synchronously before Bevy
  starts. The compressed upload is small, but peak memory still includes the
  decompressed module. Measure cold-start time and memory on low-end devices.
- Call-graph splitting cannot move functions that are reachable from exported
  WGPU and browser callbacks. For a Bevy game, whole-module `wasm-opt` and
  Brotli compression are therefore still needed in addition to split points.
