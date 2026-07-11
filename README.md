# bevy_emoji

Bevy 0.19 recreation of `emoji-merge`, with native desktop and WebAssembly builds.

## Run locally

Native:

```sh
cargo run
```

Web:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14
trunk serve --release
```

Open <http://127.0.0.1:8080/>. Mouse and touch input are detected from the
active pointer, so hybrid devices can switch between them while the game is
running.

## GitHub Pages

The workflow in `.github/workflows/pages.yml` builds and deploys the game on
pushes to `main` or `master`. In the repository settings, select **GitHub
Actions** as the Pages source. The workflow fetches the pinned upstream
`emoji-merge` assets before running the Trunk release build.

The in-game screen button requests browser fullscreen and landscape orientation
when the browser allows it. Older iOS Safari versions do not expose the
Fullscreen API for arbitrary canvas elements; on those versions, use **Add to
Home Screen** for the closest full-screen experience.

## Douyin Mini Game

The independent crate in `crates/bevy_douyin_minigame` provides the Bevy plugin,
Douyin runtime adapter, touch/lifecycle/storage bridge, and packaging CLI.

Install the matching wasm-bindgen CLI once:

```powershell
cargo install -f wasm-bindgen-cli --version 0.2.126 --locked
```

Build the importable project:

```powershell
.\scripts\build-douyin.ps1 -AppId "your-douyin-app-id"
```

The result is written to `dist-douyin/emoji`. Import that directory in the
official Douyin Developer Tool as a Mini Game. The script downloads the pinned
Binaryen `wasm-opt` release on first use, verifies its SHA-256 checksum, splits
the Bevy call graph, runs `wasm-opt -Oz`, and Brotli-compresses the main and
deferred Wasm modules. Packaging fails if the generated project exceeds 30 MiB.

The current checked build is about 7.93 MiB on disk, down from the rejected
50.62 MiB package. Inspect `package-report.json` for the upload size and
`wasm-split-report.json` for uncompressed, optimized, and compressed module
sizes. For faster local iterations, lower the Brotli quality while retaining
the same package format:

```powershell
.\scripts\build-douyin.ps1 -CompressionQuality 5
```

This target is currently silent because browser WebAudio is not available in
the Mini Game runtime; the build script also excludes `.ogg` files. The main
module is decompressed before Bevy starts, so startup latency and peak memory
must be measured with the Developer Tool's real-device debugger before release.

See `crates/bevy_douyin_minigame/README.md` for platform integration and
`crates/bevy_douyin_wasm_split/README.md` for the reusable splitter crate.
