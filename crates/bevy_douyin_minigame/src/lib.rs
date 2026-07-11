//! Bevy integration and packaging support for Douyin Mini Games.
//!
//! Douyin Mini Games do not provide a browser DOM. The generated JavaScript
//! runtime installs the compatibility surface required by Bevy's web window,
//! renderer, asset I/O, and touch input before starting the Wasm module.

mod bridge;
#[cfg(feature = "cli")]
pub mod package;

use serde::{Deserialize, Serialize};

pub use bridge::{
    is_douyin_minigame, set_preferred_frames_per_second, storage_get, storage_remove, storage_set,
    system_info,
};

/// Runtime settings applied when the Bevy plugin starts.
#[derive(Clone, Debug)]
pub struct DouyinConfig {
    /// Preferred callback frequency for `requestAnimationFrame`.
    pub preferred_fps: u32,
}

impl Default for DouyinConfig {
    fn default() -> Self {
        Self { preferred_fps: 60 }
    }
}

/// Device and host information returned by `tt.getSystemInfoSync`.
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DouyinSystemInfo {
    pub platform: String,
    pub system: String,
    pub brand: String,
    pub model: String,
    pub app_name: String,
    #[serde(rename = "SDKVersion")]
    pub sdk_version: String,
    pub screen_width: f32,
    pub screen_height: f32,
    pub window_width: f32,
    pub window_height: f32,
    pub pixel_ratio: f32,
    #[serde(default)]
    pub device_orientation: String,
    #[serde(default)]
    pub safe_area: DouyinSafeArea,
}

/// Insets-safe rectangle expressed in Douyin logical pixels.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DouyinSafeArea {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub width: f32,
    pub height: f32,
}

#[cfg(feature = "bevy")]
mod bevy_plugin {
    use bevy::prelude::*;

    use super::{DouyinConfig, bridge};

    /// Indicates whether this app is currently hosted by Douyin Mini Game.
    #[derive(Resource, Clone, Debug)]
    pub struct DouyinRuntime {
        pub available: bool,
    }

    /// Bevy message emitted for `tt.onShow` and `tt.onHide` transitions.
    #[derive(Message, Clone, Copy, Debug, Eq, PartialEq)]
    pub enum DouyinLifecycleEvent {
        Show,
        Hide,
    }

    /// Installs access to Douyin runtime services in a Bevy app.
    pub struct DouyinMiniGamePlugin {
        config: DouyinConfig,
    }

    impl DouyinMiniGamePlugin {
        pub fn new(config: DouyinConfig) -> Self {
            Self { config }
        }
    }

    impl Default for DouyinMiniGamePlugin {
        fn default() -> Self {
            Self::new(DouyinConfig::default())
        }
    }

    impl Plugin for DouyinMiniGamePlugin {
        fn build(&self, app: &mut App) {
            let available = bridge::is_douyin_minigame();
            if available {
                bridge::set_preferred_frames_per_second(self.config.preferred_fps);
            }

            app.insert_resource(DouyinRuntime { available })
                .insert_resource(bridge::system_info().unwrap_or_default())
                .add_message::<DouyinLifecycleEvent>()
                .add_systems(PreUpdate, forward_lifecycle_events);
        }
    }

    fn forward_lifecycle_events(mut events: MessageWriter<DouyinLifecycleEvent>) {
        while let Some(event) = bridge::take_lifecycle_event() {
            events.write(event);
        }
    }
}

#[cfg(feature = "bevy")]
pub use bevy_plugin::{DouyinLifecycleEvent, DouyinMiniGamePlugin, DouyinRuntime};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_info_accepts_official_camel_case_shape() {
        let info: DouyinSystemInfo = serde_json::from_str(
            r#"{
                "platform":"android","system":"Android 15","brand":"Example",
                "model":"Phone","appName":"Douyin","SDKVersion":"4.0.0",
                "screenWidth":1080,"screenHeight":2400,"windowWidth":360,
                "windowHeight":800,"pixelRatio":3,"deviceOrientation":"portrait",
                "safeArea":{"left":0,"right":360,"top":24,"bottom":780,"width":360,"height":756}
            }"#,
        )
        .unwrap();

        assert_eq!(info.app_name, "Douyin");
        assert_eq!(info.safe_area.top, 24.0);
        assert_eq!(info.pixel_ratio, 3.0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_runtime_is_an_explicit_noop() {
        assert!(!is_douyin_minigame());
        assert_eq!(system_info(), None);
    }
}
