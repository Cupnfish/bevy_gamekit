use bevy_minigame::package::{MiniGamePlatform, PackageOptions};

#[derive(Clone, Copy, Debug, Default)]
pub struct WeChatPlatform;

impl MiniGamePlatform for WeChatPlatform {
    fn id(&self) -> &'static str {
        "wechat"
    }

    fn display_name(&self) -> &'static str {
        "WeChat"
    }

    fn api_global(&self) -> &'static str {
        "wx"
    }

    fn webassembly_path_only(&self) -> bool {
        true
    }

    fn profile_clock_retry(&self) -> bool {
        true
    }

    fn max_render_resolution(&self) -> Option<(u32, u32)> {
        // The game itself is pixel-art rendered at 640x360. A 1280x720
        // presentation target preserves a crisp 2x image without paying the
        // fill-rate cost of high-DPR phone screens through WeChat's GL bridge.
        Some((1280, 720))
    }

    fn runtime_subpackage(&self) -> Option<&'static str> {
        Some("bevy")
    }

    fn asset_subpackage(&self) -> Option<&'static str> {
        Some("bevy")
    }

    fn max_main_package_bytes(&self) -> Option<u64> {
        Some(4 * 1024 * 1024)
    }

    fn game_config(&self, options: &PackageOptions) -> serde_json::Value {
        serde_json::json!({
            "deviceOrientation": options.orientation.as_str(),
            "showStatusBar": false,
            "subpackages": [{
                "name": "bevy",
                "root": "bevy"
            }]
        })
    }

    fn project_config(&self, options: &PackageOptions) -> serde_json::Value {
        serde_json::json!({
            "description": "Bevy WeChat Mini Game",
            "compileType": "game",
            "appid": options.app_id,
            "projectname": options.project_name,
            "setting": {
                "urlCheck": false,
                "es6": true,
                "postcss": true,
                "minified": false,
                "newFeature": true,
                "compileHotReLoad": true,
                "uploadWithSourceMap": true,
                "useCompilerPlugins": false,
                "disableUseStrict": false,
                "disableSWC": true,
                "babelSetting": {
                    "ignore": [],
                    "disablePlugins": [],
                    "outputPath": ""
                }
            },
            "packOptions": {
                "ignore": [],
                "include": []
            },
            "simulatorPluginLibVersion": {},
            "isGameTourist": false
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bevy_minigame::package::{MiniGamePlatform, Orientation, PackageOptions};

    use super::WeChatPlatform;

    fn options() -> PackageOptions {
        PackageOptions {
            wasm: "game.wasm".into(),
            assets: None,
            output: "dist".into(),
            app_id: "wx-test".into(),
            project_name: "emoji".into(),
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
    fn config_is_recognized_as_a_wechat_game() {
        let config = WeChatPlatform.project_config(&options());

        assert_eq!(config["compileType"], "game");
        assert_eq!(config["appid"], "wx-test");
        assert!(!WeChatPlatform.douyin_workarounds());
        assert!(WeChatPlatform.webassembly_path_only());
        assert!(WeChatPlatform.profile_clock_retry());
        assert_eq!(WeChatPlatform.max_render_resolution(), Some((1280, 720)));
        assert_eq!(WeChatPlatform.runtime_subpackage(), Some("bevy"));
        assert_eq!(WeChatPlatform.asset_subpackage(), Some("bevy"));
        assert_eq!(
            WeChatPlatform.max_main_package_bytes(),
            Some(4 * 1024 * 1024)
        );
        assert_eq!(
            WeChatPlatform.game_config(&options())["subpackages"][0]["root"],
            "bevy"
        );
    }
}
