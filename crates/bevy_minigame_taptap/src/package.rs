use bevy_minigame::package::{MiniGamePlatform, PackageOptions, PackagingKind};

const GLUE_JS: &str = include_str!("../runtime/taptap-glue.js");

/// TapTap H5 packaging backend.
///
/// Games are delivered as a zip of web files (`index.html`, Wasm module,
/// assets) that TapTap deploys under its own domain. The host is a standard
/// browser, so no DOM shim is required; `glue.js` provides the
/// `BevyMiniGame` bridge surface over browser APIs and hosts the TapTap H5
/// SDK integration points.
#[derive(Clone, Copy, Debug, Default)]
pub struct TapTapPlatform;

impl MiniGamePlatform for TapTapPlatform {
    fn id(&self) -> &'static str {
        "taptap"
    }

    fn display_name(&self) -> &'static str {
        "TapTap H5"
    }

    /// The host SDK is loaded through `glue.js` rather than a single global
    /// object, so no host API global is required here.
    fn api_global(&self) -> &'static str {
        ""
    }

    fn packaging_kind(&self) -> PackagingKind {
        PackagingKind::BrowserWeb
    }

    fn project_config(&self, _options: &PackageOptions) -> serde_json::Value {
        // TapTap H5 packages have no developer-tool project file; the store
        // upload is the zip produced by the packaging pipeline.
        serde_json::json!({
            "platform": "taptap-h5",
        })
    }

    fn browser_index_html(&self, options: &PackageOptions) -> String {
        format!(
            r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta http-equiv="Cache-Control" content="no-store">
<meta name="viewport" content="width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no">
<title>{title}</title>
<style>
  html, body {{ margin: 0; padding: 0; height: 100%; background: #faf8ef;
                overflow: hidden; touch-action: none;
                user-select: none; -webkit-user-select: none; }}
  canvas {{ display: block; margin: 0 auto; outline: none; }}
  #boot {{ position: fixed; inset: 0; display: flex; align-items: center;
           justify-content: center; color: #776e65;
           font-family: system-ui, sans-serif; font-size: 18px; }}
</style>
</head>
<body>
<div id="boot">{title} 加载中…</div>
<script src="./glue.js"></script>
<script>window.TAP_AD_UNIT_ID = "";</script>
<script>
  // TapTap H5 SDK script is injected by the platform shell here when live.
  window.__gameErrors = [];
  window.addEventListener("error", (e) => window.__gameErrors.push(String(e.message)));
</script>
<script type="module">
import init from './game_wasm.js';

// main() auto-runs via __wbindgen_start during init(); never call an
// exported entry again — a second App panics with RecreationAttempt.
async function start() {{
  try {{
    await init();
    const boot = document.getElementById('boot');
    if (boot) boot.remove();
  }} catch (e) {{
    const boot = document.getElementById('boot');
    if (boot) boot.textContent = '加载失败: ' + (e && e.message ? e.message : e);
    console.error(e);
  }}
}}

start();
</script>
</body>
</html>
"#,
            title = options.project_name,
        )
    }

    fn browser_glue_js(&self, _options: &PackageOptions) -> String {
        GLUE_JS.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bevy_minigame::package::{
        MiniGamePlatform, Orientation, PackagingKind, PackageOptions,
    };

    use super::TapTapPlatform;

    fn options() -> PackageOptions {
        PackageOptions {
            wasm: "game.wasm".into(),
            assets: None,
            output: "dist".into(),
            app_id: "testAppId".into(),
            project_name: "tapdemo".into(),
            orientation: Orientation::Landscape,
            wasm_bindgen: PathBuf::from("wasm-bindgen"),
            exclude_extensions: Vec::new(),
            wasm_split_decoder: None,
            wasm_opt: None,
            compression_quality: 11,
            max_total_bytes: None,
        }
    }

    #[test]
    fn uses_browser_web_packaging() {
        assert_eq!(TapTapPlatform.packaging_kind(), PackagingKind::BrowserWeb);
        assert_eq!(TapTapPlatform.id(), "taptap");
    }

    #[test]
    fn index_html_loads_wasm_module_exactly_once() {
        let html = TapTapPlatform.browser_index_html(&options());
        assert!(html.contains("<title>tapdemo</title>"));
        assert!(html.contains("./game_wasm.js"));
        assert!(html.contains("./glue.js"));
        // main() auto-runs via __wbindgen_start; a second entry invocation
        // would build a second App and panic with RecreationAttempt.
        assert!(!html.contains("run()"));
        assert_eq!(html.matches("await init()").count(), 1);
    }

    #[test]
    fn glue_installs_bevy_minigame_surface() {
        let glue = TapTapPlatform.browser_glue_js(&options());
        assert!(glue.contains("window.BevyMiniGame"));
        assert!(glue.contains("systemInfoJson"));
        assert!(glue.contains("storageGet"));
        assert!(glue.contains("takeLifecycleEvent"));
    }

    #[test]
    fn glue_implements_tap_bridge_over_the_h5_sdk() {
        let glue = TapTapPlatform.browser_glue_js(&options());
        // Real rewarded-video path over the runtime-injected `tap` global…
        assert!(glue.contains("createRewardedVideoAd"));
        assert!(glue.contains("TAP_AD_UNIT_ID"));
        // …with build info and audio bridge members games import.
        assert!(glue.contains("buildNumber"));
        assert!(glue.contains("bgmStart"));
        // Input diagnostics must stay opt-in, never ship enabled by default.
        assert!(glue.contains("__TAPTAP_GLUE_DEBUG"));
    }
}
