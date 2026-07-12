const assert = require("node:assert/strict");
const path = require("node:path");

const projectDirectory = path.resolve(process.argv[2] || "dist-douyin/emoji");

delete global.TextDecoder;
delete global.TextEncoder;
global.Window = class HeliumWindow {};
global.queueMicrotask = function heliumQueueMicrotask(callback) {
  if (this !== global) throw new TypeError("Illegal invocation");
  return Promise.resolve().then(callback);
};
global.GameGlobal = {
  tt: {
    getSystemInfoSync: () => ({
      platform: "android",
      windowWidth: 640,
      windowHeight: 360,
      screenWidth: 640,
      screenHeight: 360,
      pixelRatio: 1,
    }),
    createCanvas: () => ({ width: 0, height: 0, getContext: () => ({}) }),
    createImage: () => ({}),
    getFileSystemManager: () => ({ readFile: () => {} }),
    onTouchStart: () => {},
    onTouchMove: () => {},
    onTouchEnd: () => {},
    onTouchCancel: () => {},
    onShow: () => {},
    onHide: () => {},
  },
};

const adapter = require(path.join(projectDirectory, "adapter.js"));
const wasmBindgen = require(path.join(projectDirectory, "game_wasm.js"));
const decoder = require(path.join(projectDirectory, "brotli_decoder.js"));

assert.equal(globalThis instanceof Window, true);
assert.equal(global.BevyMiniGame, adapter);
assert.equal(global.GameGlobal.BevyMiniGame, adapter);
assert.equal(adapter.platform, "douyin");
assert.equal(adapter.available(), true);
assert.equal(typeof global.TextDecoder, "function");
assert.equal(typeof global.TextEncoder, "function");
assert.equal(new TextDecoder("utf-8", { fatal: true }).decode(new TextEncoder().encode("抖音😀")), "抖音😀");
assert.equal(typeof wasmBindgen, "function");
assert.equal(typeof decoder, "function");

console.log("Helium split-global glue load passed");
