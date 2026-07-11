# bevy_agent_harness

In-game Bevy **plugin** for agent/automation diagnostics: native screenshot capture, BRP target discovery, and queued semantic input. Lightweight — depends only on `bevy` + `serde`.

> The [`bevy-agent`](../bevy-agent) **CLI** that drives a running game (input, state reads/assertions, and JS scripts on an embedded `boa_engine`) is a separate crate; it depends on `boa_engine`/`clap`/`ureq`/`image`, which this plugin never pulls in.

## Install

```rust
use bevy_agent_harness::{AgentCanvasConfig, AgentHarnessConfig, AgentHarnessPlugin};

app.add_plugins(AgentHarnessPlugin::new(AgentHarnessConfig {
    canvas: Some(AgentCanvasConfig::centered(Vec2::new(640.0, 360.0))),
    ..default()
}));
```

The plugin starts a BRP (Bevy Remote Protocol) HTTP server (`enable_remote` defaults on, port `15702`) that the CLI talks to. Disable it (`enable_remote: false`) if you don't want the control port open.

## Mark semantic targets

Mark an interactive world entity with a stable ID so the CLI can address it by name:

```rust
use bevy_agent_harness::{AgentTarget, AgentTargetCapability, AgentTargetRole};

commands.spawn((
    sprite,
    transform,
    AgentTarget::world("button.retry", Vec2::splat(64.0))
        .with_role(AgentTargetRole::Button)
        .with_capabilities([
            AgentTargetCapability::Hover,
            AgentTargetCapability::Click,
        ]),
));
```

Use `AgentTarget::ui` for Bevy UI nodes — their bounds come from `ComputedNode` and `UiGlobalTransform`. `AgentTarget::window` addresses fixed window-space regions. Add `AgentTargetDisabled` to temporarily hide a target without despawning it.

## Expose state for testing

For the CLI's `get` / `wait-for` (or JS `agent.get` / `agent.wait_for`) to assert on internal game state, expose that state as a reflected resource so BRP can read it:

```rust
#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
struct AudioProbe { last_action: String /* … */ }

app.register_type::<AudioProbe>().init_resource::<AudioProbe>();
```

Without `#[reflect(Resource)]` (and `register_type`), BRP returns `Unknown resource type`. This is how non-visual state — audio playback, game phase, scores — becomes testable.

## Capture

Screenshots are produced in-game: press the capture hotkey (default `F12`), set `BEVY_AGENT_CAPTURE` to a `CaptureSequenceRequest` JSON, or trigger it from the CLI (`bevy-agent capture` / `sequence`). Frame-sequence capture steps `Time<Virtual>` per frame for deterministic, framerate-independent output, and writes a `manifest.json` per session (see the `capture` module docs for the virtual-time coupling). Disable the hotkey with `capture_hotkey: None`.

See the [`bevy-agent` CLI](../bevy-agent/README.md) for how to drive targets, capture, scripts, and assertions.
