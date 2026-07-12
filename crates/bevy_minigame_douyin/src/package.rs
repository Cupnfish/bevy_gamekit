use bevy_minigame::package::{MiniGamePlatform, PackageOptions};

#[derive(Clone, Copy, Debug, Default)]
pub struct DouyinPlatform;

impl MiniGamePlatform for DouyinPlatform {
    fn id(&self) -> &'static str {
        "douyin"
    }

    fn display_name(&self) -> &'static str {
        "Douyin"
    }

    fn api_global(&self) -> &'static str {
        "tt"
    }

    fn douyin_workarounds(&self) -> bool {
        true
    }

    fn project_config(&self, options: &PackageOptions) -> serde_json::Value {
        serde_json::json!({
            "description": "Bevy Douyin Mini Game",
            "setting": {
                "urlCheck": false,
                "es6": true,
                "postcss": true,
                "minified": false,
                "newFeature": true,
                "autoCompile": true,
                "compileHotReLoad": true,
                "nativeCompile": true
            },
            "appid": options.app_id,
            "projectname": options.project_name,
            "douyinProjectType": "native"
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bevy_minigame::package::{MiniGamePlatform, Orientation, PackageOptions};

    use super::DouyinPlatform;

    fn options() -> PackageOptions {
        PackageOptions {
            wasm: "game.wasm".into(),
            assets: None,
            output: "dist".into(),
            app_id: "testAppId".into(),
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
    fn config_is_recognized_as_a_native_minigame() {
        let config = DouyinPlatform.project_config(&options());

        assert_eq!(config["douyinProjectType"], "native");
        assert!(config.get("compileType").is_none());
        assert!(DouyinPlatform.douyin_workarounds());
    }
}
