const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const callbacks = {};
const files = new Map([["assets/test.bin", Uint8Array.from([1, 2, 3]).buffer]]);
const storage = new Map();
let preferredFps = 0;
let canvasCount = 0;

delete global.TextDecoder;
delete global.TextEncoder;

const readonlyScreen = {};
Object.defineProperties(readonlyScreen, {
  width: { get: () => 1024 },
  height: { get: () => 576 },
});
Object.preventExtensions(readonlyScreen);
Object.defineProperty(global, "screen", {
  configurable: true,
  get: () => readonlyScreen,
});
Object.defineProperty(global, "location", {
  configurable: true,
  get: () => Object.freeze({ href: "game.js", protocol: "file:" }),
});

// Helium's developer sandbox and GameGlobal are distinct objects. It may also
// expose a Window constructor that does not recognize the sandbox global.
global.Window = class HeliumWindow {};
global.queueMicrotask = function heliumQueueMicrotask(callback) {
  if (this !== global) throw new TypeError("Illegal invocation");
  return Promise.resolve().then(callback);
};

global.tt = {
  getSystemInfoSync: () => ({
    platform: "android",
    system: "Android 15",
    appName: "Douyin",
    SDKVersion: "4.0",
    screenWidth: 800,
    screenHeight: 360,
    windowWidth: 800,
    windowHeight: 360,
    pixelRatio: 2,
  }),
  createCanvas: () => {
    const canvas = {
      width: 0,
      height: 0,
      getContext(type) { return { type }; },
    };
    Object.defineProperty(canvas, "style", {
      value: {},
      configurable: false,
      writable: false,
    });
    return canvas;
  },
  createImage: () => ({}),
  getFileSystemManager: () => ({
    readFile({ filePath, success, fail }) {
      if (files.has(filePath)) success({ data: files.get(filePath) });
      else fail({ errMsg: "not found" });
    },
  }),
  onTouchStart: (callback) => { callbacks.touchStart = callback; },
  onTouchMove: (callback) => { callbacks.touchMove = callback; },
  onTouchEnd: (callback) => { callbacks.touchEnd = callback; },
  onTouchCancel: (callback) => { callbacks.touchCancel = callback; },
  offTouchStart: (callback) => { if (!callback || callbacks.touchStart === callback) delete callbacks.touchStart; },
  offTouchMove: (callback) => { if (!callback || callbacks.touchMove === callback) delete callbacks.touchMove; },
  offTouchEnd: (callback) => { if (!callback || callbacks.touchEnd === callback) delete callbacks.touchEnd; },
  offTouchCancel: (callback) => { if (!callback || callbacks.touchCancel === callback) delete callbacks.touchCancel; },
  onShow: (callback) => { callbacks.show = callback; },
  onHide: (callback) => { callbacks.hide = callback; },
  offShow: (callback) => { if (!callback || callbacks.show === callback) delete callbacks.show; },
  offHide: (callback) => { if (!callback || callbacks.hide === callback) delete callbacks.hide; },
  setPreferredFramesPerSecond: (fps) => { preferredFps = fps; },
  getStorageSync: (key) => storage.get(key),
  setStorageSync: (key, value) => storage.set(key, value),
  removeStorageSync: (key) => storage.delete(key),
};
global.GameGlobal = { tt: global.tt };

const originalCreateCanvas = global.tt.createCanvas;
global.tt.createCanvas = () => {
  canvasCount += 1;
  return originalCreateCanvas();
};

const adapterPath = path.join(__dirname, "../runtime/adapter.js");
const adapterSource = fs.readFileSync(adapterPath, "utf8");
assert.equal(adapterSource.includes("?."), false, "TMA parser does not support optional chaining");
assert.equal(adapterSource.includes("??"), false, "TMA parser does not support nullish coalescing");
const adapter = require(adapterPath);

async function main() {
  assert.equal(adapter.available(), true);
  const encoded = new TextEncoder().encode("Bevy 抖音 😀");
  assert.equal(new TextDecoder("utf-8", { fatal: true }).decode(encoded), "Bevy 抖音 😀");
  assert.throws(
    () => new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.from([0xc0, 0xaf])),
    /valid UTF-8/,
  );
  const partial = new Uint8Array(4);
  assert.deepEqual(new TextEncoder().encodeInto("A😀B", partial), { read: 1, written: 1 });
  assert.deepEqual(Array.from(partial), [65, 0, 0, 0]);
  assert.equal(globalThis instanceof Window, true);
  assert.equal(global.DouyinBevy, adapter);
  assert.equal(global.GameGlobal.DouyinBevy, adapter);
  await new Promise((resolve) => {
    const detachedQueueMicrotask = global.queueMicrotask;
    detachedQueueMicrotask(resolve);
  });
  assert.equal(canvasCount, 1);
  assert.equal(document.querySelector("#bevy-canvas"), adapter.canvas);
  assert.equal(adapter.canvas.clientWidth, 800);
  assert.equal(typeof adapter.canvas.style.getPropertyValue, "function");
  assert.equal(typeof adapter.canvas.style.setProperty, "function");
  adapter.canvas.style.setProperty("width", "321px");
  assert.equal(adapter.canvas.clientWidth, 321);
  adapter.canvas.style.setProperty("width", "800px");
  assert.equal(adapter.canvas.width, 1600);
  assert.equal(adapter.canvas instanceof HTMLCanvasElement, true);
  assert.equal(adapter.canvas.getContext("webgl2") instanceof WebGL2RenderingContext, true);
  assert.equal(adapter.canvas.contains(adapter.canvas), true);
  assert.equal(adapter.canvas.contains({}), false);
  assert.equal(document.currentScript, null);
  assert.equal(screen.width, 1024);

  const channel = new MessageChannel();
  const message = new Promise((resolve) => { channel.port1.onmessage = resolve; });
  channel.port2.postMessage("tick");
  assert.equal((await message).data, "tick");
  if (channel.port1.close) channel.port1.close();
  if (channel.port2.close) channel.port2.close();

  const received = [];
  adapter.canvas.addEventListener("pointerdown", (event) => received.push(event));
  callbacks.touchStart({
    touches: [{ identifier: 7, screenX: 125, screenY: 80, force: 0.75 }],
    changedTouches: [{ identifier: 7, screenX: 125, screenY: 80, force: 0.75 }],
  });
  assert.equal(received.length, 1);
  assert.equal(received[0].pointerId, 8);
  assert.equal(received[0].pointerType, "touch");
  assert.equal(received[0].offsetX, 125);
  assert.equal(received[0].pressure, 0.75);

  const transitions = [];
  global.addEventListener("pagehide", (event) => transitions.push([event.type, event.persisted]));
  global.addEventListener("pageshow", (event) => transitions.push([event.type, event.persisted]));
  callbacks.hide();
  callbacks.show();
  assert.equal(adapter.takeLifecycleEvent(), 2);
  assert.equal(adapter.takeLifecycleEvent(), 1);
  assert.equal(document.visibilityState, "visible");
  assert.deepEqual(transitions, [["pagehide", true], ["pageshow", true]]);

  adapter.setPreferredFramesPerSecond(144);
  assert.equal(preferredFps, 60);
  adapter.storageSet("save", "value");
  assert.equal(adapter.storageGet("save"), "value");
  adapter.storageRemove("save");
  assert.equal(adapter.storageGet("save"), "");

  assert.deepEqual(Array.from(await adapter.readPackageFile("./assets/test.bin")), [1, 2, 3]);
  const response = await fetch("assets/test.bin");
  assert.equal(response instanceof Response, true);
  assert.equal(response.status, 200);
  assert.deepEqual(Array.from(new Uint8Array(await response.arrayBuffer())), [1, 2, 3]);
  const missing = await fetch("assets/missing.bin");
  assert.equal(missing.status, 404);

  adapter.prepareReload();
  adapter.adapterVersion = 0;
  delete require.cache[require.resolve(adapterPath)];
  const upgradedAdapter = require(adapterPath);
  assert.notEqual(upgradedAdapter.canvas, adapter.canvas);
  assert.equal(canvasCount, 2);

  console.log("adapter mock test passed");
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
