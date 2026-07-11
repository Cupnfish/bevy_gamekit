//! 脚本加载：仅支持 `.js`，用**内建 `boa_engine`** 执行（无需 node/python）。
//!
//! 运行模型：注入 `AgentQueue` 类与全局 `agent`（构建器）、`agentArgs`，执行用户脚本，
//! 再把解析出的请求（`globalThis.request` / `globalThis.actions` / `agent`）经 `JSON.stringify`
//! 取回，转成 `AgentInteractionRequest`。预置 JS 见同目录 [`preamble.js`] / [`resolve.js`]。

use std::{error::Error, path::Path};

use bevy_agent_harness::{AgentAction, AgentInteractionRequest};
use boa_engine::{Context, Source, js_string};
use serde_json::Value;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// JS 预置：`AgentQueue` 构建器 + 全局 `agent`（见 `preamble.js`）。
const PREAMBLE: &str = include_str!("preamble.js");
/// 取回脚本构建的请求并序列化为 JSON（见 `resolve.js`）。
const RESOLVE: &str = include_str!("resolve.js");

/// 加载并执行 `.js` 脚本，返回它构建的交互请求。
pub fn load_request(path: &Path, args: &[String]) -> Result<AgentInteractionRequest> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension != "js" {
        return Err(format!(
            "unsupported script extension for {}; only .js is supported (executed by the embedded boa_engine)",
            path.display()
        )
        .into());
    }

    let source = std::fs::read_to_string(path)?;
    let args_json = serde_json::to_string(args)?;
    let program = format!("{PREAMBLE}\nvar agentArgs = {args_json};\n{source}\n{RESOLVE}");

    let mut context = Context::default();
    context
        .eval(Source::from_bytes(&program))
        .map_err(|error| format!("JS error: {error}"))?;

    let resolved = context
        .global_object()
        .get(js_string!("__resolved_json"), &mut context)
        .map_err(|error| format!("JS resolve error: {error}"))?;
    let json = resolved
        .as_string()
        .map(|string| string.to_std_string_escaped())
        .ok_or_else(|| "script did not resolve to a JSON string".to_string())?;

    parse_request_value(serde_json::from_str(&json)?)
}

fn parse_request_value(value: Value) -> Result<AgentInteractionRequest> {
    if value.is_array() {
        let actions = serde_json::from_value::<Vec<AgentAction>>(value)?;
        return Ok(AgentInteractionRequest::new("", actions));
    }
    Ok(serde_json::from_value(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn raw_action_array_becomes_a_request() {
        let value = json!([
            {"type": "wait_frames", "frames": 3},
            {"type": "key_tap", "key": "Space"}
        ]);
        let request = parse_request_value(value).unwrap();
        assert_eq!(request.actions.len(), 2);
        assert!(request.request_id.is_empty());
    }
}
