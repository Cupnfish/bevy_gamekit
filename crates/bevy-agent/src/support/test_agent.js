// 由 `bevy-agent test` 经 boa 嵌入。定义实时 `agent`：输入方法构建动作 dict 并调用
// 宿主函数 `__act(json)`（阻塞到该动作在游戏里完成）；读取方法调用 `__get/__wait_for/__targets`。
// 形状与 `AgentAction` serde 一致。无任何运行时依赖。
function __button(value) {
  var normalized = String(value == null ? "left" : value).toLowerCase();
  var names = { left: "Left", right: "Right", middle: "Middle", back: "Back", forward: "Forward" };
  if (names[normalized]) return names[normalized];
  if (normalized.indexOf("other:") === 0) return { Other: Number(normalized.slice("other:".length)) };
  throw new Error("Unknown mouse button: " + value);
}
function __point(target, options) {
  options = options || {};
  return { target: target, anchor: options.anchor || [0, 0], offset: options.offset || [0, 0] };
}
function __actv(action) { __act(JSON.stringify(action)); return agent; }

var agent = {
  // 输入（每个动作立刻提交并在游戏侧执行完成）
  key: function (k) { return __actv({ type: "key_tap", key: k }); },
  keyDown: function (k) { return __actv({ type: "key_press", key: k }); },
  keyUp: function (k) { return __actv({ type: "key_release", key: k }); },
  click: function (target, options) { options = options || {}; return __actv({ type: "click", point: __point(target, options), button: __button(options.button) }); },
  clickAt: function (x, y, options) { options = options || {}; return __actv({ type: "click_at", position: [x, y], button: __button(options.button) }); },
  hover: function (target, frames, options) { options = options || {}; return __actv({ type: "hover", point: __point(target, options), frames: frames == null ? 12 : frames }); },
  moveTo: function (target, frames, options) { options = options || {}; return __actv({ type: "move_to", point: __point(target, options), frames: frames == null ? 1 : frames }); },
  moveToPoint: function (x, y, frames) { return __actv({ type: "move_to_point", position: [x, y], frames: frames == null ? 1 : frames }); },
  press: function (target, options) { options = options || {}; return __actv({ type: "press", point: __point(target, options), button: __button(options.button) }); },
  pressAt: function (x, y, options) { options = options || {}; return __actv({ type: "press_at", position: [x, y], button: __button(options.button) }); },
  release: function (options) { options = options || {}; return __actv({ type: "release", point: options.target ? __point(options.target, options) : null, button: __button(options.button) }); },
  drag: function (from, to, options) { options = options || {}; return __actv({ type: "drag_to", from: __point(from, options.from), to: __point(to, options.to), button: __button(options.button), frames: options.frames == null ? 12 : options.frames }); },
  dragBy: function (target, dx, dy, options) { options = options || {}; return __actv({ type: "drag_by", from: __point(target, options), delta: [dx, dy], button: __button(options.button), frames: options.frames == null ? 12 : options.frames }); },
  scroll: function (target, dx, dy, options) { options = options || {}; return __actv({ type: "scroll", point: __point(target, options), delta: [dx, dy], unit: options.unit || "line" }); },
  scrollAt: function (x, y, dx, dy, options) { options = options || {}; return __actv({ type: "scroll_at", position: [x, y], delta: [dx, dy], unit: options.unit || "line" }); },
  text: function (value, options) { options = options || {}; return __actv({ type: "text_input", point: options.target ? __point(options.target, options) : null, text: String(value), mode: options.mode || "keyboard" }); },
  waitFrames: function (frames) { return __actv({ type: "wait_frames", frames: frames }); },
  cursorLeave: function () { return __actv({ type: "cursor_leave" }); },

  // 读取 / 断言（宿主函数返回 JSON 字符串，这里 parse 回 JS 值）
  get: function (resource) { var v = __get(resource); return v == null ? null : JSON.parse(v); },
  wait_for: function (resource, field, expected, op, timeout) {
    var v = __wait_for(resource, field, expected, op || "eq", timeout == null ? 20 : timeout);
    return v == null ? null : JSON.parse(v);
  },
  targets: function (filter) { var v = __targets(filter || ""); return v == null ? [] : JSON.parse(v); },

  // snake_case 别名
  key_down: function (k) { return this.keyDown(k); },
  key_up: function (k) { return this.keyUp(k); },
  click_at: function (a, b, c) { return this.clickAt(a, b, c); },
  move_to: function (a, b, c) { return this.moveTo(a, b, c); },
  move_to_point: function (a, b, c) { return this.moveToPoint(a, b, c); },
  press_at: function (a, b, c) { return this.pressAt(a, b, c); },
  drag_by: function (a, b, c, d) { return this.dragBy(a, b, c, d); },
  scroll_at: function (a, b, c, d, e) { return this.scrollAt(a, b, c, d, e); },
  wait_frames: function (f) { return this.waitFrames(f); },
  cursor_leave: function () { return this.cursorLeave(); }
};
