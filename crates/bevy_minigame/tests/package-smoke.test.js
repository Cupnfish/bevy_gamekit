const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");

const projectDirectory = path.resolve(process.argv[2] || "dist-douyin");
const platform = process.argv[3] || (projectDirectory.includes("wechat") ? "wechat" : "douyin");
const apiGlobal = platform === "wechat" ? "wx" : "tt";
const runtimeRoot = platform === "wechat" ? "bevy" : "";
const runtimeDirectory = path.join(projectDirectory, runtimeRoot);
const splitPackage = fs.existsSync(path.join(runtimeDirectory, "__wasm_split.js"));
const requestedFiles = [];
const loadedSubpackages = [];
let resolveStarted;
const started = new Promise((resolve) => { resolveStarted = resolve; });

global.Window = class HeliumWindow {};
const platformApi = {
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
  getFileSystemManager: () => ({
    readFile({ filePath, success }) {
      requestedFiles.push(filePath);
      success({ data: Uint8Array.from([0, 97, 115, 109]).buffer });
    },
  }),
  onTouchStart: () => {},
  onTouchMove: () => {},
  onTouchEnd: () => {},
  onTouchCancel: () => {},
  onShow: () => {},
  onHide: () => {},
  loadSubpackage({ name, success }) {
    loadedSubpackages.push(name);
    success({});
  },
};
global[apiGlobal] = platformApi;
global.GameGlobal = { [apiGlobal]: platformApi };

const originalLoad = Module._load;
Module._load = function load(request, parent, isMain) {
  if ((request === "./game_wasm.js" || request === "./bevy/game_wasm.js")
      && parent && parent.filename.endsWith("game.js")) {
    const initialize = async ({ module_or_path }) => {
      if (platform === "wechat") {
        assert.equal(
          module_or_path,
          runtimeRoot + "/" + (splitPackage ? "game_wasm_bg.wasm.br" : "game_wasm_bg.wasm"),
        );
      } else {
        assert.deepEqual(Array.from(module_or_path), [0, 97, 115, 109]);
      }
      resolveStarted();
    };
    initialize.initSync = () => ({});
    return initialize;
  }
  if (request === "./brotli_decoder.js" && parent && parent.filename.endsWith("game.js")) {
    assert.notEqual(platform, "wechat", "WeChat must use native .wasm.br loading");
    const initializeDecoder = async ({ module_or_path: bytes }) => {
      assert.deepEqual(Array.from(bytes), [0, 97, 115, 109]);
    };
    initializeDecoder.decompress = (bytes) => bytes;
    return initializeDecoder;
  }
  return originalLoad.call(this, request, parent, isMain);
};

require(path.join(projectDirectory, "game.js"));

Promise.race([
  started,
  new Promise((_, reject) => setTimeout(() => reject(new Error("startup timed out")), 2000)),
]).then(() => {
  assert.deepEqual(
    requestedFiles,
    platform === "wechat"
      ? []
      : splitPackage
        ? ["brotli_decoder_bg.wasm", "game_wasm_bg.wasm.br"]
        : ["game_wasm_bg.wasm"],
  );
  assert.equal(global.BevyMiniGame.installed, true);
  assert.equal(global.BevyMiniGame.platform, platform);
  assert.equal(global.BevyMiniGame.webAssemblyPathOnly, platform === "wechat");
  assert.deepEqual(loadedSubpackages, platform === "wechat" ? ["bevy"] : []);
  assert.equal(global.GameGlobal.BevyMiniGame, global.BevyMiniGame);
  assert.equal(globalThis instanceof Window, true);
  console.log("generated project startup chain passed");
}).catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
