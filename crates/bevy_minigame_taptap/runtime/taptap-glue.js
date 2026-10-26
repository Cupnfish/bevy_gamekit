// TapTap H5 glue for bevy_minigame.
//
// Installs `window.BevyMiniGame` — the surface bevy_minigame's Rust bridge
// imports — over standard browser APIs, and `window.TapBridge`, the game-side
// integration surface for ads / audio / build info.
//
// The TapTap H5 host is a real browser, so most capabilities are native:
//   storage    -> localStorage
//   lifecycle  -> visibilitychange (1 = show, 2 = hide)
//   systemInfo -> navigator + screen + visualViewport
// SDK-backed capabilities read the `tap` global injected by the TapTap H5
// runtime and degrade to local-development fallbacks when it is absent, so
// the exact same glue runs on device and in a plain browser.
//
// NOTE: do NOT touch the game canvas from this file. Sizing is owned by
// winit (`fit_canvas_to_parent`); CSS manipulation here (transform/fixed)
// has been verified to break pointer event delivery inside the TapTap shell.

(function installTapTapGlue(global) {
  "use strict";

  if (global.BevyMiniGame && typeof global.BevyMiniGame.disposeGlue === "function") {
    global.BevyMiniGame.disposeGlue();
  }

  const lifecycleEvents = [];
  let lastVisibility = document.visibilityState;
  function onVisibilityChange() {
    const state = document.visibilityState;
    if (state !== lastVisibility) {
      lastVisibility = state;
      lifecycleEvents.push(state === "visible" ? 1 : 2);
    }
  }
  document.addEventListener("visibilitychange", onVisibilityChange);

  function storageKey(key) {
    return "bevy_minigame:" + key;
  }

  const safeArea = (function readSafeArea() {
    const inset = global.visualViewport || {};
    return {
      left: 0,
      top: 0,
      right: 0,
      bottom: 0,
      width: inset.width || global.screen.width,
      height: inset.height || global.screen.height,
    };
  })();

  function systemInfo() {
    return {
      platform: "taptap-h5",
      system: navigator.userAgent || "",
      brand: "",
      model: "",
      appName: "TapTap",
      SDKVersion: "",
      screenWidth: global.screen.width,
      screenHeight: global.screen.height,
      windowWidth: global.innerWidth,
      windowHeight: global.innerHeight,
      pixelRatio: global.devicePixelRatio || 1,
      deviceOrientation:
        global.innerWidth >= global.innerHeight ? "landscape" : "portrait",
      safeArea,
    };
  }

  global.BevyMiniGame = {
    version: 1,
    platform: "taptap",

    available() {
      return true;
    },

    systemInfoJson() {
      return JSON.stringify(systemInfo());
    },

    setPreferredFramesPerSecond(_fps) {
      // Browser requestAnimationFrame cadence is not adjustable.
    },

    takeLifecycleEvent() {
      return lifecycleEvents.length ? lifecycleEvents.shift() : 0;
    },

    storageGet(key) {
      try {
        return global.localStorage.getItem(storageKey(key)) || "";
      } catch (_) {
        return "";
      }
    },

    storageSet(key, value) {
      try {
        global.localStorage.setItem(storageKey(key), value);
      } catch (_) {}
    },

    storageRemove(key) {
      try {
        global.localStorage.removeItem(storageKey(key));
      } catch (_) {}
    },

    disposeGlue() {
      document.removeEventListener("visibilitychange", onVisibilityChange);
      delete global.BevyMiniGame;
    },
  };

  // ---- TapTap H5 SDK integration points (window.TapBridge) ----
  // Implemented over the `tap` global injected by the TapTap H5 runtime,
  // with local-development fallbacks so every member is safe to call from a
  // plain browser. If the host shell already installed its own TapBridge,
  // respect it and only enrich it with the audio fallback below.
  if (!global.TapBridge) {
    const tap = global.tap;
    global.TapBridge = {
      sdkAvailable: !!tap && typeof tap.createRewardedVideoAd === "function",

      // showRewarded(callback) -> callback(granted: boolean)
      // Granted only when the video was watched to the end (res.isEnded);
      // closing early, errors and a hard 30s timeout all resolve to false so
      // the reward flow can never soft-lock. The ad unit id comes from
      // window.TAP_AD_UNIT_ID (inlined into index.html by the packager);
      // without it we simulate the grant so flows stay testable.
      showRewarded(callback) {
        const granted = (ok) => callback(!!ok);
        const t = global.tap;
        if (t && typeof t.createRewardedVideoAd === "function") {
          try {
            const adUnitId = global.TAP_AD_UNIT_ID || "";
            if (!adUnitId) {
              console.warn("[TapBridge] no TAP_AD_UNIT_ID configured; simulating reward");
              setTimeout(() => granted(true), 800);
              return;
            }
            const ad = t.createRewardedVideoAd({ adUnitId });
            let settled = false;
            const finish = (ok) => {
              if (settled) return;
              settled = true;
              granted(ok);
            };
            ad.onClose((res) => finish(!!(res && res.isEnded)));
            ad.onError((err) => {
              console.error("[TapBridge] rewarded error", err);
              finish(false);
            });
            setTimeout(() => finish(false), 30000);
            ad.show().catch(() =>
              ad.load()
                .then(() => ad.show())
                .catch((e) => {
                  console.error("[TapBridge] rewarded show failed", e);
                  finish(false);
                })
            );
          } catch (e) {
            console.error("[TapBridge] rewarded exception", e);
            granted(false);
          }
          return;
        }
        console.warn("[TapBridge] SDK not loaded; auto-granting reward (dev mode)");
        setTimeout(() => granted(true), 800);
      },

      // login(callback) -> callback({ success, openId?, ... })
      // Not wired to the SDK yet; reserved so games can probe it.
      login(callback) {
        console.warn("[TapBridge] login not implemented; reporting unavailable");
        setTimeout(() => callback({ success: false }), 0);
      },

      // Build number inlined into index.html by the packager
      // (window.__GAME_BUILD). 0 in local preview.
      buildNumber() {
        return (global.__GAME_BUILD || 0) | 0;
      },
    };
  }

  // ---- Input diagnostics (opt-in) ----
  // Set window.__TAPTAP_GLUE_DEBUG = true before this script loads to get a
  // move/cancel counter badge; used to locate host-side touch problems.
  if (global.__TAPTAP_GLUE_DEBUG) {
    const diag = { move: 0, cancel: 0 };
    let diagBadge = null;
    function updateDiagBadge() {
      if (!diagBadge && global.document && global.document.body) {
        diagBadge = global.document.createElement("div");
        diagBadge.style.cssText =
          "position:fixed;top:4px;left:4px;z-index:99999;pointer-events:none;" +
          "background:rgba(0,0,0,0.6);color:#7fff7f;font:12px monospace;" +
          "padding:2px 6px;border-radius:4px;";
        global.document.body.appendChild(diagBadge);
      }
      if (diagBadge) {
        diagBadge.textContent = "mv:" + diag.move + " cx:" + diag.cancel;
      }
    }
    global.addEventListener("touchmove", function () { diag.move++; updateDiagBadge(); }, { capture: true, passive: true });
    global.addEventListener("pointercancel", function () { diag.cancel++; updateDiagBadge(); }, { capture: true, passive: true });
    global.addEventListener("touchcancel", function () { diag.cancel++; updateDiagBadge(); }, { capture: true, passive: true });
    updateDiagBadge();
  }

  // ---- BGM (HTMLAudioElement; deferred to first gesture by autoplay policy) ----
  let bgmAudio = null;
  global.TapBridge.bgmStart = function () {
    try {
      if (bgmAudio) {
        bgmAudio.play().catch(function () {});
        return;
      }
      bgmAudio = new Audio("assets/audio/bgm.wav");
      bgmAudio.loop = true;
      bgmAudio.volume = 0.22;
      bgmAudio.play().catch(function () {});
    } catch (e) {
      console.warn("[TapBridge] bgmStart failed", e);
    }
  };
})(typeof window !== "undefined" ? window : globalThis);
