const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");

const projectDirectory = path.resolve(process.argv[2] || "dist-douyin");
const splitPackage = fs.existsSync(path.join(projectDirectory, "__wasm_split.js"));
const requestedFiles = [];
let resolveStarted;
const started = new Promise((resolve) => { resolveStarted = resolve; });

global.Window = class HeliumWindow {};
global.tt = {
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
};
global.GameGlobal = { tt: global.tt };

const originalLoad = Module._load;
Module._load = function load(request, parent, isMain) {
  if (request === "./game_wasm.js" && parent && parent.filename.endsWith("game.js")) {
    const initialize = async ({ module_or_path: bytes }) => {
      assert.deepEqual(Array.from(bytes), [0, 97, 115, 109]);
      resolveStarted();
    };
    initialize.initSync = () => ({});
    return initialize;
  }
  if (request === "./brotli_decoder.js" && parent && parent.filename.endsWith("game.js")) {
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
    splitPackage
      ? ["brotli_decoder_bg.wasm", "game_wasm_bg.wasm.br"]
      : ["game_wasm_bg.wasm"],
  );
  assert.equal(global.DouyinBevy.installed, true);
  assert.equal(global.GameGlobal.DouyinBevy, global.DouyinBevy);
  assert.equal(globalThis instanceof Window, true);
  console.log("generated project startup chain passed");
}).catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
