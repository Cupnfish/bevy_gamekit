const assert = require("node:assert/strict");
const path = require("node:path");

const calls = [];
const gl = {
  LINK_STATUS: 0x8b82,
  getExtension: () => null,
  attachShader(program, shader) {
    calls.push(["attach", program, shader]);
  },
  linkProgram(program) {
    calls.push(["link", program]);
  },
  deleteShader(shader) {
    calls.push(["delete", shader]);
  },
  deleteProgram(program) {
    calls.push(["deleteProgram", program]);
  },
  // WeChat real devices return numeric 1/0 here. glow requires a primitive
  // boolean, otherwise JsValue::as_bool() turns the successful link into false.
  getProgramParameter(program, parameter) {
    calls.push(["status", program, parameter]);
    return 1;
  },
};

let touchEndCallback = null;
let preferredFps = 0;
let intervalCallback = null;
let intervalDelay = 0;
let clearedInterval = null;
let nativeAnimationFrameCalls = 0;
const audioContexts = [];
function createInnerAudioContext() {
  const context = {
    autoplay: null,
    loop: null,
    src: "",
    volume: 1,
    playbackRate: 1,
    playCalls: 0,
    pauseCalls: 0,
    stopCalls: 0,
    destroyCalls: 0,
    endedCallback: null,
    errorCallback: null,
    play() { this.playCalls += 1; },
    pause() { this.pauseCalls += 1; },
    stop() { this.stopCalls += 1; },
    destroy() { this.destroyCalls += 1; },
    onEnded(callback) { this.endedCallback = callback; },
    offEnded(callback) {
      if (this.endedCallback === callback) this.endedCallback = null;
    },
    onError(callback) { this.errorCallback = callback; },
    offError(callback) {
      if (this.errorCallback === callback) this.errorCallback = null;
    },
    emitEnded() {
      const callback = this.endedCallback;
      if (callback) callback();
    },
  };
  audioContexts.push(context);
  return context;
}
global.requestAnimationFrame = () => {
  nativeAnimationFrameCalls += 1;
  return 999;
};
global.cancelAnimationFrame = () => {};
global.setInterval = (callback, delay) => {
  intervalCallback = callback;
  intervalDelay = delay;
  return 77;
};
global.clearInterval = (handle) => { clearedInterval = handle; };
const canvas = {
  width: 0,
  height: 0,
  getContext: () => gl,
  addEventListener() {
    throw new Error("native addEventListener must be replaced as a family");
  },
  removeEventListener() {
    throw new Error("native removeEventListener must be replaced as a family");
  },
  dispatchEvent() {
    throw new TypeError("parameter 1 is not of type Event");
  },
};
// WeChat real-device canvases already expose DOM-like properties as
// non-configurable native fields. Decorating them must preserve those fields
// instead of throwing `Cannot redefine property: tagName` before Wasm starts.
Object.defineProperties(canvas, {
  tagName: { value: "CANVAS", configurable: false },
  ownerDocument: { value: null, configurable: false },
  parentElement: { get: () => null, configurable: false },
  clientWidth: { get: () => 844, configurable: false },
  clientHeight: { get: () => 390, configurable: false },
});
const wx = {
  getSystemInfoSync: () => ({
    platform: "android",
    windowWidth: 844,
    windowHeight: 390,
    screenWidth: 2532,
    screenHeight: 1170,
    pixelRatio: 3,
  }),
  createCanvas: () => canvas,
  createImage: () => ({}),
  createInnerAudioContext,
  getFileSystemManager: () => ({ readFile: () => {} }),
  onTouchStart: () => {},
  onTouchMove: () => {},
  onTouchEnd: (callback) => { touchEndCallback = callback; },
  onTouchCancel: () => {},
  onShow: () => {},
  onHide: () => {},
  setPreferredFramesPerSecond: (fps) => { preferredFps = fps; },
};

global.Window = class MiniGameWindow {};
global.wx = wx;
global.GameGlobal = { wx };
global.__bevyMiniGamePlatform = {
  platform: "wechat",
  displayName: "WeChat",
  apiGlobal: "wx",
  assetRoot: "bevy/assets",
  webAssemblyPathOnly: true,
  profileClockRetry: true,
  maxCanvasWidth: 1280,
  maxCanvasHeight: 720,
};

const adapter = require(path.join(__dirname, "../runtime/adapter.js"));
assert.equal(adapter.adapterVersion, 43);
const adaptedGl = adapter.canvas.getContext("webgl2");
const program = {};
const vertex = {};
const fragment = {};

adaptedGl.attachShader(program, vertex);
adaptedGl.attachShader(program, fragment);
adaptedGl.linkProgram(program);
adaptedGl.deleteShader(vertex);
adaptedGl.deleteShader(fragment);

assert.equal(adaptedGl.getProgramParameter(program, adaptedGl.LINK_STATUS), true);
assert.equal(typeof adaptedGl.getProgramParameter(program, adaptedGl.LINK_STATUS), "boolean");
assert.deepEqual(
  calls.filter(([name]) => name === "delete"),
  [["delete", vertex], ["delete", fragment]],
);
assert.ok(calls.findIndex(([name]) => name === "delete") > calls.findIndex(([name]) => name === "status"));
adaptedGl.deleteProgram(program);
assert.deepEqual(calls.filter(([name]) => name === "deleteProgram"), [["deleteProgram", program]]);
assert.equal(adapter.canvas.width, 1280);
assert.equal(adapter.canvas.height, 591);
assert.equal(adapter.devicePixelRatio(), 1280 / 844);
assert.equal(adapter.canvas.tagName, "CANVAS");
assert.equal(adapter.canvas.ownerDocument, null);
assert.equal(adapter.canvas.clientWidth, 844);
assert.equal(adapter.canvas.clientHeight, 390);
assert.equal(adapter.canvas instanceof global.HTMLCanvasElement, true);
let releasedPointer = null;
adapter.canvas.addEventListener("pointerup", (event) => { releasedPointer = event; });
touchEndCallback({
  changedTouches: [{ identifier: 2, screenX: 123, screenY: 45 }],
  touches: [],
});
assert.equal(releasedPointer.type, "pointerup");
assert.equal(releasedPointer.pointerId, 3);
assert.equal(releasedPointer.clientX, 123);
assert.equal(releasedPointer.clientY, 45);

assert.equal(typeof intervalCallback, "function");
assert.ok(Math.abs(intervalDelay - 1000 / 60) < 0.001);
let animationFrameTimestamp = null;
const cancelledFrame = global.requestAnimationFrame(() => {
  throw new Error("cancelled animation frame must not run");
});
global.cancelAnimationFrame(cancelledFrame);
global.requestAnimationFrame((timestamp) => { animationFrameTimestamp = timestamp; });
intervalCallback();
assert.equal(typeof animationFrameTimestamp, "number");
assert.equal(nativeAnimationFrameCalls, 0);
assert.equal(adapter.debugStats().animationFrameRequests, 2);
assert.equal(adapter.debugStats().animationFrameCallbacks, 1);
assert.equal(adapter.debugStats().animationFrameCancellations, 1);
assert.equal(adapter.debugStats().maxQueuedAnimationFrames, 1);
adapter.setPreferredFramesPerSecond(30);
assert.equal(preferredFps, 30);

const bgmId = adapter.audioCreate("Recettear OST - Closed Shop.ogg", true);
const bgm = audioContexts[0];
assert.ok(bgmId > 0);
assert.equal(bgm.autoplay, false);
assert.equal(bgm.loop, true);
assert.equal(bgm.src, "bevy/assets/Recettear OST - Closed Shop.ogg");
adapter.audioSetVolume(bgmId, 1.5);
adapter.audioSetPlaybackRate(bgmId, 0.25);
assert.equal(bgm.volume, 1);
assert.equal(bgm.playbackRate, 0.5);
adapter.audioPlay(bgmId);
adapter.audioPause(bgmId);
adapter.audioStop(bgmId);
assert.equal(bgm.playCalls, 1);
assert.equal(bgm.pauseCalls, 1);
assert.equal(bgm.stopCalls, 1);
adapter.audioDestroy(bgmId);
assert.equal(bgm.stopCalls, 2);
assert.equal(bgm.destroyCalls, 1);

adapter.audioPlayOneShot("パパッ.ogg", 0.3, 1.05);
const oneShot = audioContexts[1];
assert.equal(oneShot.loop, false);
assert.equal(oneShot.src, "bevy/assets/パパッ.ogg");
assert.equal(oneShot.volume, 0.3);
assert.equal(oneShot.playbackRate, 1.05);
assert.equal(oneShot.playCalls, 1);
assert.equal(typeof oneShot.endedCallback, "function");
assert.equal(typeof oneShot.errorCallback, "function");
oneShot.emitEnded();
assert.equal(oneShot.stopCalls, 1);
assert.equal(oneShot.destroyCalls, 1);
assert.equal(oneShot.endedCallback, null);
assert.equal(oneShot.errorCallback, null);

const reloadedBgmId = adapter.audioCreate("Recettear OST - Closed Shop.ogg", true);
assert.ok(reloadedBgmId > bgmId);
const audioStats = adapter.debugStats();
assert.equal(audioStats.adapterVersion, 43);
assert.equal(audioStats.audioContextsCreated, 3);
assert.equal(audioStats.audioContextsDestroyed, 2);
assert.equal(audioStats.audioOneShots, 1);
adapter.disposeAdapter();
assert.equal(clearedInterval, 77);
assert.equal(audioContexts[2].destroyCalls, 1);
assert.equal(adapter.debugStats().audioContextsDestroyed, 3);

console.log("WeChat adapter compatibility, audio bridge, and bounded RAF scheduler passed");
