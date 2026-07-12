//! Cross-platform Bevy integration and packaging support for Mini Games.
//!
//! Mini Game hosts do not provide a browser DOM. The generated JavaScript
//! runtime installs the compatibility surface required by Bevy's web window,
//! renderer, asset I/O, and touch input before starting the Wasm module.

mod bridge;
#[cfg(feature = "cli")]
pub mod cli;
#[cfg(feature = "cli")]
pub mod package;

use serde::{Deserialize, Serialize};

pub use bridge::{
    is_minigame, set_preferred_frames_per_second, storage_get, storage_remove, storage_set,
    system_info,
};

/// Runtime settings applied when the Bevy plugin starts.
#[derive(Clone, Debug)]
pub struct MiniGameConfig {
    /// Preferred callback frequency for `requestAnimationFrame`.
    pub preferred_fps: u32,
}

impl Default for MiniGameConfig {
    fn default() -> Self {
        Self { preferred_fps: 60 }
    }
}

/// Device and host information returned by the platform system-info API.
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MiniGameSystemInfo {
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
    pub safe_area: MiniGameSafeArea,
}

/// Insets-safe rectangle expressed in platform logical pixels.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MiniGameSafeArea {
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

    use super::{MiniGameConfig, bridge};

    /// Indicates whether this app is currently hosted by a Mini Game runtime.
    #[derive(Resource, Clone, Debug)]
    pub struct MiniGameRuntime {
        pub available: bool,
    }

    /// Bevy message emitted for platform show and hide transitions.
    #[derive(Message, Clone, Copy, Debug, Eq, PartialEq)]
    pub enum MiniGameLifecycleEvent {
        Show,
        Hide,
    }

    /// Installs access to Mini Game runtime services in a Bevy app.
    pub struct MiniGamePlugin {
        config: MiniGameConfig,
    }

    impl MiniGamePlugin {
        pub fn new(config: MiniGameConfig) -> Self {
            Self { config }
        }
    }

    impl Default for MiniGamePlugin {
        fn default() -> Self {
            Self::new(MiniGameConfig::default())
        }
    }

    impl Plugin for MiniGamePlugin {
        fn build(&self, app: &mut App) {
            let available = bridge::is_minigame();
            if available {
                bridge::set_preferred_frames_per_second(self.config.preferred_fps);
            }

            app.insert_resource(MiniGameRuntime { available })
                .insert_resource(bridge::system_info().unwrap_or_default())
                .add_message::<MiniGameLifecycleEvent>()
                .add_systems(PreUpdate, forward_lifecycle_events);
        }
    }

    fn forward_lifecycle_events(mut events: MessageWriter<MiniGameLifecycleEvent>) {
        while let Some(event) = bridge::take_lifecycle_event() {
            events.write(event);
        }
    }
}

#[cfg(feature = "bevy")]
pub use bevy_plugin::{MiniGameLifecycleEvent, MiniGamePlugin, MiniGameRuntime};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_info_accepts_official_camel_case_shape() {
        let info: MiniGameSystemInfo = serde_json::from_str(
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
        assert!(!is_minigame());
        assert_eq!(system_info(), None);
    }
}
