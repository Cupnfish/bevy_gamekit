# bevy-agent

Command-line client for [`bevy_agent_harness`](../bevy_agent_harness): drive a **running** Bevy game over BRP — semantic-target input, state reads & assertions, screenshot capture, and JS scripting on an **embedded `boa_engine`** (no Node or Python on the host).

It talks to a game that has the `bevy_agent_harness` plugin (which runs the BRP server on port `15702` by default). Launch the game, then point the CLI at it (`--url`, default `http://127.0.0.1:15702/`):

```text
cargo run -p bevy-agent -- targets
cargo run -p bevy-agent -- click game.drop
cargo run -p bevy-agent -- key Space
cargo run -p bevy-agent -- test path/to/test.js
```

## Commands

```text
bevy-agent targets [filter]
bevy-agent click <target> [button]            # button: left|right|middle|back|forward|other:N
bevy-agent drag-by <target> <dx> <dy> [frames] [button]
bevy-agent key <KeyCode>                       # e.g. KeyR, Space, ArrowUp
bevy-agent sequence <label> <frames> [interval_ms] [delay_frames]
bevy-agent capture [label]
bevy-agent get <resource_type_path>
bevy-agent wait-for <resource> <field> <expected> [--op eq|ne|ge|le|gt|lt|contains] [--timeout SECONDS]
bevy-agent run <script.js>                     # queued input
bevy-agent plan <script.js>                    # print the request, no game needed
bevy-agent test <script.js>                    # live: drive + assert
bevy-agent sheet <capture_dir> [columns]
```

Run `bevy-agent --help` and `bevy-agent <command> --help` for the full, typed synopsis. Mouse buttons, scroll units (`line|pixel`), `--op`, and `KeyCode` are parsed as typed values, so invalid input is rejected at parse time.

`targets` returns the logical-pixel center and bounds for every stable ID the game marked; dynamic physics targets refresh every frame.

## State Introspection & Assertions

`get` reads any reflected resource; `wait-for` polls until a field satisfies a comparison (exit `0` on match, non-zero on timeout) — usable directly as a test assertion:

```text
bevy-agent get my_game::Phase
bevy-agent wait-for my_game::Score value 100 --op ge --timeout 30
```

Field paths are dotted (`a.b.c`); values compare as strings (raw for strings, JSON form otherwise, e.g. booleans `true`). `wait-for` retries through BRP connection errors during boot.

The game must expose the resource for BRP to read it — see the plugin's [“Expose state for testing”](../bevy_agent_harness/README.md#expose-state-for-testing) section.

## Queued Scripts

`run` / `plan` use **JavaScript only** on an embedded `boa_engine` (no Node/Python). The script receives globals `agent` (an `AgentQueue` builder), `AgentQueue`, and `agentArgs`; build a queue and the CLI submits it as one request:

```text
bevy-agent run examples/semantic-flow.js
bevy-agent plan examples/semantic-flow.js
```

```js
agent
  .hover("game.drop", 8)
  .click("game.drop")
  .waitFrames(45)
  .dragBy("board.score", 120, 0, { frames: 12 })
  .key("Space");
```

`run` submits against a running game; `plan` only prints the normalized request. Targets resolve when each action begins, so moving targets use their current bounds. Methods have camelCase (`dragBy`, `waitFrames`) and snake_case (`drag_by`, `wait_frames`) aliases. `run` is *fire-and-forget* (input only) — to read or assert mid-flow, use `test`.

## Live Test Scripts

`bevy-agent test <script.js>` runs a JS script with a **live** `agent`: input methods submit one action and block until the game completes it; read methods return parsed values. One script can both drive and assert:

```js
agent.wait_for("my_game::Phase", "value", "GameOver", "eq", 20); // poll until GameOver
agent.key("KeyR");                                                // restart
agent.wait_for("my_game::Phase", "value", "Playing", "eq", 10);
print("PASS");
```

- Input methods mirror `AgentQueue` (`key`, `click`, `hover`, `dragBy`, `scroll`, `text`, …).
- Read/assert: `get(resource)`, `wait_for(resource, field, expected, op, timeout)` (`op` ∈ `eq|ne|ge|le|gt|lt|contains`), `targets(filter)`.
- `print(...)` writes to stdout (boa ships no `console`). Any host error throws and `bevy-agent test` exits non-zero, so the exit code is the assertion result.

Like `run`, `test` drives a **running** game — launch it separately.

## Input Semantics

Actions include hover, move, click, double-click, press/release, drag, scroll, key press/release, text input, cursor leave, and frame waits. The runtime emits both the concrete Bevy input messages and aggregate `WindowEvent` messages, and updates `Window::cursor_position()`. `key` also derives `logical_key` / `text` for common keys (letters, digits, space, arrows) so games reading `Key::Character` work too. Coordinate commands (`click-at`, `move-at`, `scroll-at`) are available as low-level fallbacks.
