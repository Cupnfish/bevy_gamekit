#[cfg(all(target_arch = "wasm32", not(feature = "minigame")))]
use wasm_bindgen::prelude::*;

#[cfg(all(target_arch = "wasm32", not(feature = "minigame")))]
#[wasm_bindgen(inline_js = r##"
const SAVE_KEY = "bevy_emoji_save";

export function touchCapable() {
  return navigator.maxTouchPoints > 0 || window.matchMedia("(pointer: coarse)").matches;
}

export function devicePixelRatio() {
  return window.devicePixelRatio || 1;
}

export function gameCanvasClientWidth() {
  const canvas = document.querySelector("#bevy-canvas");
  return canvas ? canvas.clientWidth : 0;
}

function fullscreenElement() {
  return document.fullscreenElement || document.webkitFullscreenElement;
}

function lockLandscape() {
  try {
    const result = screen.orientation && screen.orientation.lock
      ? screen.orientation.lock("landscape")
      : null;
    if (result && result.catch) result.catch(() => {});
  } catch (_) {}
}

let suppressBevyFullscreenUntil = 0;

function requestGameFullscreen(canvas) {
  const request = canvas.requestFullscreen || canvas.webkitRequestFullscreen;
  if (!request) return;
  try {
    const result = canvas.requestFullscreen
      ? request.call(canvas, { navigationUI: "hide" })
      : request.call(canvas);
    if (result && result.then) result.then(lockLandscape).catch(() => {});
    else lockLandscape();
  } catch (_) {}
}

function toggleGameFullscreenFromGesture(canvas) {
  suppressBevyFullscreenUntil = performance.now() + 1000;
  if (!fullscreenElement()) requestGameFullscreen(canvas);
  else exitGameFullscreen();
}

function installFullscreenGesture(canvas) {
  if (canvas.dataset.fullscreenGesture === "installed") return;
  canvas.dataset.fullscreenGesture = "installed";
  canvas.addEventListener("pointerdown", event => {
    if (!event.isPrimary) return;
    const rect = canvas.getBoundingClientRect();
    const fit = Math.min(rect.width / 640, rect.height / 360);
    const continuous = event.pointerType === "touch" || touchCapable();
    const scale = fit >= 1 && !continuous ? Math.floor(fit) : fit;
    const left = (rect.width - 640 * scale) * 0.5;
    const top = (rect.height - 360 * scale) * 0.5;
    const x = (event.clientX - rect.left - left) / scale;
    const y = (event.clientY - rect.top - top) / scale;
    const half = event.pointerType === "touch" ? 22 / scale : 9;
    if (Math.abs(x - 78) <= half && Math.abs(y - 340) <= half) {
      toggleGameFullscreenFromGesture(canvas);
    }
  }, { passive: true });
}

export function toggleGameFullscreen() {
  const canvas = document.querySelector("#bevy-canvas");
  if (!canvas) return;

  if (performance.now() < suppressBevyFullscreenUntil) return;

  if (!fullscreenElement()) {
    requestGameFullscreen(canvas);
  } else {
    exitGameFullscreen();
  }
}

export function exitGameFullscreen() {
  try {
    if (screen.orientation && screen.orientation.unlock) screen.orientation.unlock();
  } catch (_) {}
  const exit = document.exitFullscreen || document.webkitExitFullscreen;
  if (fullscreenElement() && exit) {
    try {
      const result = exit.call(document);
      if (result && result.catch) result.catch(() => {});
    } catch (_) {}
  }
}

export function loadGameSave() {
  try { return localStorage.getItem(SAVE_KEY) || ""; }
  catch (_) { return ""; }
}

export function saveGameSave(value) {
  try { localStorage.setItem(SAVE_KEY, value); }
  catch (_) {}
}

export function markGameReady() {
  const loading = document.querySelector("#loading");
  if (loading) loading.remove();
  document.body.classList.add("game-ready");
  const canvas = document.querySelector("#bevy-canvas");
  if (canvas) {
    installFullscreenGesture(canvas);
    canvas.focus();
  }
}
"##)]
extern "C" {
    #[wasm_bindgen(js_name = touchCapable)]
    fn js_touch_capable() -> bool;
    #[wasm_bindgen(js_name = devicePixelRatio)]
    fn js_device_pixel_ratio() -> f32;
    #[wasm_bindgen(js_name = gameCanvasClientWidth)]
    fn js_canvas_client_width() -> f32;
    #[wasm_bindgen(js_name = toggleGameFullscreen)]
    fn js_toggle_fullscreen();
    #[wasm_bindgen(js_name = exitGameFullscreen)]
    fn js_exit_fullscreen();
    #[wasm_bindgen(js_name = loadGameSave)]
    fn js_load_save() -> String;
    #[wasm_bindgen(js_name = saveGameSave)]
    fn js_save_save(value: &str);
    #[wasm_bindgen(js_name = markGameReady)]
    fn js_mark_ready();
}

#[cfg(all(target_arch = "wasm32", feature = "minigame"))]
use wasm_bindgen::prelude::*;

#[cfg(all(target_arch = "wasm32", feature = "minigame"))]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = BevyMiniGame, js_name = touchCapable)]
    fn js_touch_capable() -> bool;
    #[wasm_bindgen(js_namespace = BevyMiniGame, js_name = devicePixelRatio)]
    fn js_device_pixel_ratio() -> f32;
    #[wasm_bindgen(js_namespace = BevyMiniGame, js_name = gameCanvasClientWidth)]
    fn js_canvas_client_width() -> f32;
    #[wasm_bindgen(js_namespace = BevyMiniGame, js_name = toggleFullscreen)]
    fn js_toggle_fullscreen();
    #[wasm_bindgen(js_namespace = BevyMiniGame, js_name = exitFullscreen)]
    fn js_exit_fullscreen();
    #[wasm_bindgen(js_namespace = BevyMiniGame, js_name = storageGet)]
    fn js_load_save(key: &str) -> String;
    #[wasm_bindgen(js_namespace = BevyMiniGame, js_name = storageSet)]
    fn js_save_save(key: &str, value: &str);
    #[wasm_bindgen(js_namespace = BevyMiniGame, js_name = markGameReady)]
    fn js_mark_ready();
}

pub fn touch_capable() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        js_touch_capable()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

#[cfg(target_arch = "wasm32")]
pub fn device_pixel_ratio() -> f32 {
    js_device_pixel_ratio()
}

#[cfg(target_arch = "wasm32")]
pub fn canvas_client_width() -> f32 {
    js_canvas_client_width()
}

#[cfg(target_arch = "wasm32")]
pub fn toggle_fullscreen() {
    js_toggle_fullscreen();
}

#[cfg(target_arch = "wasm32")]
pub fn exit_fullscreen() {
    js_exit_fullscreen();
}

#[cfg(target_arch = "wasm32")]
pub fn load_save() -> String {
    #[cfg(feature = "minigame")]
    {
        js_load_save("bevy_emoji_save")
    }
    #[cfg(not(feature = "minigame"))]
    {
        js_load_save()
    }
}

#[cfg(target_arch = "wasm32")]
pub fn save_save(value: &str) {
    #[cfg(feature = "minigame")]
    js_save_save("bevy_emoji_save", value);
    #[cfg(not(feature = "minigame"))]
    js_save_save(value);
}

pub fn mark_ready() {
    #[cfg(target_arch = "wasm32")]
    js_mark_ready();
}
