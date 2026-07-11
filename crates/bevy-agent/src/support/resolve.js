// 由 `script_runner.rs` 经 `include_str!` 嵌入。把脚本构建的请求序列化为 JSON 字符串，
// 写入 `globalThis.__resolved_json`，供 Rust 侧 `JSON.parse` 还原为 `AgentInteractionRequest`。
globalThis.__resolved_json = JSON.stringify((function () {
  var candidate = globalThis.request || globalThis.actions || globalThis.agent;
  if (candidate && typeof candidate.build === "function") {
    candidate = candidate.build();
  }
  return candidate;
})());
