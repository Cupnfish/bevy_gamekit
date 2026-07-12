# Vendored `wasm-split-cli` — temporary, pending an upstream fix

> **Status — temporary.** This is a locally-patched copy of Dioxus's
> `wasm-split-cli`, kept only to work around an upstream bug. **Delete it once
> Dioxus ships a fixed release** — see [Removal](#removal).

## What this is

A copy of the `wasm-split-cli` `0.8.0-alpha.0` library from the
[DioxusLabs/dioxus](https://github.com/DioxusLabs/dioxus) monorepo, taken at
commit `a82361ef1ad2ffe0219a82992fe4642b59ba88f9`. The parent crate
([`bevy_minigame_wasm_split`](../..)) consumes it as a Cargo **path** dependency,
behind the `cli` feature:

```toml
wasm-split-cli = { path = "vendor/wasm-split-cli", optional = true }
```

This is **vendored, not forked** — there is no separate repository. The files
live inside this repo and are committed to our own history. (The runtime half of
the split API, `wasm-splitter`, is used from crates.io unchanged; only this
host-side splitting library is vendored.)

## Why it's here (the upstream bug)

The published `0.8.0-alpha.0` can **delete Wasm functions that are still
referenced** by the surviving module IR. Splitting a large Bevy module then makes
[Walrus](https://github.com/rustwasm/walrus) **panic** mid-split, which blocks
the Douyin/WeChat packaging builds that rely on splitting to fit the package
size limits.

## The local patch

Only part of `src/lib.rs` is modified; `src/__wasm_split.js` and the rest of
`src/lib.rs` are unchanged from upstream. The patch:

- adds `ref.func` globals to the **main module's roots**, and
- **conservatively preserves** any split candidate still referenced by a
  surviving function, export, the start function, a global, an element segment,
  or a shared jump-table symbol,

so nothing still reachable gets deleted. Upstream's own `todo(jon):` comments are
left intact. The patch has no in-code markers; to see the exact diff, compare
`src/lib.rs` against the upstream file at the commit above.

## Licensing

Upstream is dual-licensed **MIT OR Apache-2.0**. [`LICENSE-MIT`](LICENSE-MIT) and
[`LICENSE-APACHE`](LICENSE-APACHE) are included, and `Cargo.toml` records the
original author (`Jonathan Kelley`) and repository, covering redistribution
here. When re-syncing from upstream, copy `LICENSE-MIT` verbatim so its
`Copyright (c) …` header is retained.

## Removal

When Dioxus releases a `wasm-split-cli` that no longer panics on large Bevy
modules:

1. In [`../../Cargo.toml`](../../Cargo.toml), swap
   `wasm-split-cli = { path = "vendor/wasm-split-cli", … }` for a crates.io
   dependency on the fixed version.
2. Delete this `vendor/wasm-split-cli/` directory.
3. Re-run `scripts/build-douyin.ps1` and `scripts/build-wechat.ps1`; confirm the
   split step no longer panics and the sizes in `wasm-split-report.json` are
   acceptable.
4. Drop the workaround mention from the parent crate's README.

### How to tell whether upstream has fixed it

- Watch `DioxusLabs/dioxus` for changes to the `wasm-split-cli` splitting logic
  past commit `a82361ef…`, or
- bump the dependency to the latest alpha and run a Douyin/WeChat split build —
  the Walrus panic is the signal that the bug is still present.
