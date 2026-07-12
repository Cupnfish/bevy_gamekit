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
  webAssemblyPathOnly: true,
  profileClockRetry: true,
  maxCanvasWidth: 1280,
  maxCanvasHeight: 720,
};

const adapter = require(path.join(__dirname, "../runtime/adapter.js"));
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
adapter.disposeAdapter();
assert.equal(clearedInterval, 77);

console.log("WeChat adapter compatibility and bounded RAF scheduler passed");
