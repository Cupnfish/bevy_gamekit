// 由 `script_runner.rs` 经 `include_str!` 嵌入。定义 `AgentQueue` 构建器与全局 `agent`。
// 形状与 `AgentAction` 的 serde（`#[serde(tag="type", rename_all="snake_case")]`）一致。
// 不含 `agentArgs`（由调用方注入）与 resolve（见 resolve.js）。无任何运行时依赖。
function __button(value) {
  const normalized = String(value == null ? "left" : value).toLowerCase();
  const names = { left: "Left", right: "Right", middle: "Middle", back: "Back", forward: "Forward" };
  if (names[normalized]) return names[normalized];
  if (normalized.startsWith("other:")) return { Other: Number(normalized.slice("other:".length)) };
  throw new Error("Unknown mouse button: " + value);
}
function __point(target, options) {
  options = options || {};
  return { target: target, anchor: options.anchor || [0, 0], offset: options.offset || [0, 0] };
}
class AgentQueue {
  constructor() { this.actions = []; this.requestId = ""; }
  id(requestId) { this.requestId = requestId; return this; }
  hover(target, frames, options) { this.actions.push({ type: "hover", point: __point(target, options), frames: frames == null ? 12 : frames }); return this; }
  moveTo(target, frames, options) { this.actions.push({ type: "move_to", point: __point(target, options), frames: frames == null ? 1 : frames }); return this; }
  moveToPoint(x, y, frames) { this.actions.push({ type: "move_to_point", position: [x, y], frames: frames == null ? 1 : frames }); return this; }
  click(target, options) { options = options || {}; this.actions.push({ type: "click", point: __point(target, options), button: __button(options.button) }); return this; }
  clickAt(x, y, options) { options = options || {}; this.actions.push({ type: "click_at", position: [x, y], button: __button(options.button) }); return this; }
  doubleClick(target, options) { options = options || {}; this.actions.push({ type: "double_click", point: __point(target, options), button: __button(options.button), gap_frames: options.gapFrames == null ? 2 : options.gapFrames }); return this; }
  press(target, options) { options = options || {}; this.actions.push({ type: "press", point: __point(target, options), button: __button(options.button) }); return this; }
  pressAt(x, y, options) { options = options || {}; this.actions.push({ type: "press_at", position: [x, y], button: __button(options.button) }); return this; }
  release(options) { options = options || {}; this.actions.push({ type: "release", point: options.target ? __point(options.target, options) : null, button: __button(options.button) }); return this; }
  drag(from, to, options) { options = options || {}; this.actions.push({ type: "drag_to", from: __point(from, options.from), to: __point(to, options.to), button: __button(options.button), frames: options.frames == null ? 12 : options.frames }); return this; }
  dragBy(target, dx, dy, options) { options = options || {}; this.actions.push({ type: "drag_by", from: __point(target, options), delta: [dx, dy], button: __button(options.button), frames: options.frames == null ? 12 : options.frames }); return this; }
  scroll(target, dx, dy, options) { options = options || {}; this.actions.push({ type: "scroll", point: __point(target, options), delta: [dx, dy], unit: options.unit || "line" }); return this; }
  scrollAt(x, y, dx, dy, options) { options = options || {}; this.actions.push({ type: "scroll_at", position: [x, y], delta: [dx, dy], unit: options.unit || "line" }); return this; }
  key(k) { this.actions.push({ type: "key_tap", key: k }); return this; }
  keyDown(k) { this.actions.push({ type: "key_press", key: k }); return this; }
  keyUp(k) { this.actions.push({ type: "key_release", key: k }); return this; }
  text(value, options) { options = options || {}; this.actions.push({ type: "text_input", point: options.target ? __point(options.target, options) : null, text: String(value), mode: options.mode || "keyboard" }); return this; }
  waitFrames(frames) { this.actions.push({ type: "wait_frames", frames: frames }); return this; }
  cursorLeave() { this.actions.push({ type: "cursor_leave" }); return this; }
  build(requestId) { return { request_id: requestId || this.requestId || "", actions: this.actions }; }
  // snake_case 别名
  move_to(a, b, c) { return this.moveTo(a, b, c); }
  move_to_point(a, b, c) { return this.moveToPoint(a, b, c); }
  click_at(a, b, c) { return this.clickAt(a, b, c); }
  double_click(a, b) { return this.doubleClick(a, b); }
  press_at(a, b, c) { return this.pressAt(a, b, c); }
  drag_by(a, b, c, d) { return this.dragBy(a, b, c, d); }
  scroll_at(a, b, c, d, e) { return this.scrollAt(a, b, c, d, e); }
  key_down(k) { return this.keyDown(k); }
  key_up(k) { return this.keyUp(k); }
  wait_frames(f) { return this.waitFrames(f); }
  cursor_leave() { return this.cursorLeave(); }
}
globalThis.AgentQueue = AgentQueue;
globalThis.agent = new AgentQueue();
