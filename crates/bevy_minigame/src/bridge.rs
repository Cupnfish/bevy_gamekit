use crate::MiniGameSystemInfo;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = BevyMiniGame, js_name = available)]
    fn js_available() -> bool;
    #[wasm_bindgen(js_namespace = BevyMiniGame, js_name = systemInfoJson)]
    fn js_system_info_json() -> String;
    #[wasm_bindgen(js_namespace = BevyMiniGame, js_name = setPreferredFramesPerSecond)]
    fn js_set_preferred_fps(fps: u32);
    #[wasm_bindgen(js_namespace = BevyMiniGame, js_name = takeLifecycleEvent)]
    fn js_take_lifecycle_event() -> u32;
    #[wasm_bindgen(js_namespace = BevyMiniGame, js_name = storageGet)]
    fn js_storage_get(key: &str) -> String;
    #[wasm_bindgen(js_namespace = BevyMiniGame, js_name = storageSet)]
    fn js_storage_set(key: &str, value: &str);
    #[wasm_bindgen(js_namespace = BevyMiniGame, js_name = storageRemove)]
    fn js_storage_remove(key: &str);
}

pub fn is_minigame() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        js_available()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

pub fn system_info() -> Option<MiniGameSystemInfo> {
    #[cfg(target_arch = "wasm32")]
    {
        let json = js_system_info_json();
        (!json.is_empty())
            .then(|| serde_json::from_str(&json).ok())
            .flatten()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

pub fn set_preferred_frames_per_second(fps: u32) {
    #[cfg(target_arch = "wasm32")]
    js_set_preferred_fps(fps.clamp(1, 60));
    #[cfg(not(target_arch = "wasm32"))]
    let _ = fps;
}

pub fn storage_get(key: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let value = js_storage_get(key);
        (!value.is_empty()).then_some(value)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = key;
        None
    }
}

pub fn storage_set(key: &str, value: &str) {
    #[cfg(target_arch = "wasm32")]
    js_storage_set(key, value);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (key, value);
}

pub fn storage_remove(key: &str) {
    #[cfg(target_arch = "wasm32")]
    js_storage_remove(key);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = key;
}

#[cfg(feature = "bevy")]
pub(crate) fn take_lifecycle_event() -> Option<crate::MiniGameLifecycleEvent> {
    #[cfg(target_arch = "wasm32")]
    {
        match js_take_lifecycle_event() {
            1 => Some(crate::MiniGameLifecycleEvent::Show),
            2 => Some(crate::MiniGameLifecycleEvent::Hide),
            _ => None,
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}
