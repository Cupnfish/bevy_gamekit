import { writeFile } from "node:fs/promises";

const port = Number(process.argv[2] || 8307);
const screenshotPath = process.argv[3];
const targets = await fetch(`http://127.0.0.1:${port}/json/list`).then((response) => response.json());
const target = targets.find((entry) => entry.title === "MiniApp Webview");

if (!target) throw new Error(`MiniApp Webview target was not found on CDP port ${port}`);

const socket = new WebSocket(target.webSocketDebuggerUrl);
const contexts = [];
const pending = new Map();
let nextId = 1;

socket.addEventListener("message", (event) => {
  const message = JSON.parse(event.data);
  if (message.id) {
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    if (message.error) request.reject(new Error(message.error.message));
    else request.resolve(message.result);
    return;
  }
  if (message.method === "Runtime.executionContextCreated") {
    contexts.push(message.params.context);
  }
});

await new Promise((resolve, reject) => {
  socket.addEventListener("open", resolve, { once: true });
  socket.addEventListener("error", reject, { once: true });
});

function call(method, params = {}) {
  const id = nextId++;
  socket.send(JSON.stringify({ id, method, params }));
  return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
}

await call("Runtime.enable");
await new Promise((resolve) => setTimeout(resolve, 200));

const expression = `(() => {
  const sandbox = globalThis.developerSandbox && globalThis.developerSandbox.global;
  const host = typeof GameGlobal !== "undefined"
    ? GameGlobal
    : (sandbox && sandbox.GameGlobal ? sandbox.GameGlobal : (sandbox || globalThis));
  const platform = typeof tt !== "undefined" ? tt : host.tt;
  let canvas = null;
  let webgl2 = null;
  let webgl = null;
  let mainWebgl2 = null;
  let adapter = null;
  let mainPixels = null;
  let mainPixelError = null;
  let probeError = null;
  const prototypeKeys = (value) => {
    const layers = [];
    let prototype = value && Object.getPrototypeOf(value);
    for (let depth = 0; prototype && depth < 5; depth += 1) {
      layers.push({
        constructor: prototype.constructor && prototype.constructor.name,
        keys: Reflect.ownKeys(prototype).map(String),
      });
      prototype = Object.getPrototypeOf(prototype);
    }
    return layers;
  };
  const tryValue = (read) => {
    try {
      return read();
    } catch (error) {
      return { error: String(error && (error.stack || error.message || error)) };
    }
  };
  try {
    adapter = (sandbox && sandbox.BevyMiniGame) || host.BevyMiniGame;
    canvas = adapter && adapter.canvas;
    webgl2 = canvas && canvas.getContext
      ? canvas.getContext("webgl2", { antialias: false })
      : null;
    if (!webgl2) webgl = canvas && canvas.getContext ? canvas.getContext("webgl") : null;
    mainWebgl2 = webgl2;
    if (mainWebgl2) {
      const points = [
        [0, 0],
        [Math.floor(mainWebgl2.drawingBufferWidth / 2), Math.floor(mainWebgl2.drawingBufferHeight / 2)],
        [Math.max(0, mainWebgl2.drawingBufferWidth - 1), Math.max(0, mainWebgl2.drawingBufferHeight - 1)],
      ];
      mainPixels = points.map(([x, y]) => {
        const pixel = new Uint8Array(4);
        mainWebgl2.readPixels(x, y, 1, 1, mainWebgl2.RGBA, mainWebgl2.UNSIGNED_BYTE, pixel);
        return { x, y, rgba: Array.from(pixel), error: mainWebgl2.getError() };
      });
    }
  } catch (error) {
    mainPixelError = String(error && (error.stack || error.message || error));
  }
  const canvasKeys = sandbox
    ? Object.keys(sandbox).filter((key) => /canvas/i.test(key))
    : [];
  const exposedCanvases = {
    sandboxCanvas: sandbox && sandbox.canvas,
    hostCanvas: host && host.canvas,
    gameGlobalCanvas: sandbox && sandbox.GameGlobal && sandbox.GameGlobal.canvas,
    globalCanvas: globalThis.canvas,
  };
  return {
    href: typeof location !== "undefined" ? location.href : null,
    hasDeveloperSandbox: !!sandbox,
    hasTt: !!platform,
    splitGlobal: host !== globalThis,
    adapterVersion: host.BevyMiniGame && host.BevyMiniGame.adapterVersion,
    availableType: host.BevyMiniGame && typeof host.BevyMiniGame.available,
    sandboxAdapterVersion: sandbox && sandbox.BevyMiniGame && sandbox.BevyMiniGame.adapterVersion,
    sandboxAvailableType: sandbox && sandbox.BevyMiniGame && typeof sandbox.BevyMiniGame.available,
    sandboxWindowInstance: sandbox && typeof sandbox.Window === "function"
      ? sandbox.Window[Symbol.hasInstance](sandbox)
      : null,
    sandboxWebGl2Instance: sandbox && typeof sandbox.WebGL2RenderingContext === "function" && webgl2
      ? sandbox.WebGL2RenderingContext[Symbol.hasInstance](webgl2)
      : null,
    mainCanvasConstructor: sandbox && sandbox.BevyMiniGame && sandbox.BevyMiniGame.canvas
      && sandbox.BevyMiniGame.canvas.constructor && sandbox.BevyMiniGame.canvas.constructor.name,
    mainCanvasWidth: sandbox && sandbox.BevyMiniGame && sandbox.BevyMiniGame.canvas
      && sandbox.BevyMiniGame.canvas.width,
    mainCanvasHeight: sandbox && sandbox.BevyMiniGame && sandbox.BevyMiniGame.canvas
      && sandbox.BevyMiniGame.canvas.height,
    mainCanvasWebgl2: !!mainWebgl2,
    mainCanvasWebgl2Instance: sandbox && typeof sandbox.WebGL2RenderingContext === "function" && mainWebgl2
      ? sandbox.WebGL2RenderingContext[Symbol.hasInstance](mainWebgl2)
      : null,
    mainCanvasVersion: mainWebgl2 && mainWebgl2.getParameter
      ? mainWebgl2.getParameter(mainWebgl2.VERSION)
      : null,
    mainContextAttributes: mainWebgl2 && mainWebgl2.getContextAttributes
      ? mainWebgl2.getContextAttributes()
      : null,
    mainDrawingBufferWidth: mainWebgl2 && mainWebgl2.drawingBufferWidth,
    mainDrawingBufferHeight: mainWebgl2 && mainWebgl2.drawingBufferHeight,
    mainContextLost: mainWebgl2 && mainWebgl2.isContextLost && mainWebgl2.isContextLost(),
    mainPixels,
    mainPixelError,
    canvasKeys,
    exposedCanvasMatches: Object.fromEntries(Object.entries(exposedCanvases).map(([key, value]) => [
      key,
      !!value && value === (adapter && adapter.canvas),
    ])),
    exposedCanvasSizes: Object.fromEntries(Object.entries(exposedCanvases).map(([key, value]) => [
      key,
      value ? [value.width, value.height] : null,
    ])),
    gameReady: sandbox && sandbox.document && sandbox.BevyMiniGame
      ? sandbox.document.activeElement === sandbox.BevyMiniGame.canvas
      : null,
    sandboxQueueMicrotaskType: sandbox && typeof sandbox.queueMicrotask,
    runtimeStats: adapter && adapter.runtimeStats,
    createCanvasSource: platform && platform.createCanvas
      ? Function.prototype.toString.call(platform.createCanvas)
      : null,
    createCanvasName: platform && platform.createCanvas && platform.createCanvas.name,
    createCanvasLength: platform && platform.createCanvas && platform.createCanvas.length,
    platformCanvasKeys: platform
      ? Reflect.ownKeys(platform).map(String).filter((key) => /canvas|screen|surface|frame/i.test(key))
      : [],
    canvasOwnKeys: canvas ? Reflect.ownKeys(canvas).map(String) : [],
    canvasPrototypeLayers: prototypeKeys(canvas),
    canvasInternals: canvas ? {
      isOnScreen: canvas.isOnScreen,
      currentCtx: canvas.currentCtx,
      cachedContext2D: !!canvas._cachedContext2D,
      cachedContextWebGL: !!canvas._cachedContextWebGL,
      underlyingConstructor: canvas._canvas && canvas._canvas.constructor
        && canvas._canvas.constructor.name,
      underlyingWidth: canvas._canvas && canvas._canvas.width,
      underlyingHeight: canvas._canvas && canvas._canvas.height,
      underlyingIsConnected: canvas._canvas && canvas._canvas.isConnected,
      underlyingOuterHtml: canvas._canvas && canvas._canvas.outerHTML,
      underlyingRect: canvas._canvas && canvas._canvas.getBoundingClientRect
        ? tryValue(() => {
          const rect = canvas._canvas.getBoundingClientRect();
          return {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
            left: rect.left,
          };
        })
        : null,
      underlyingStyle: canvas._canvas && canvas._canvas.ownerDocument
        && canvas._canvas.ownerDocument.defaultView
        ? tryValue(() => {
          const style = canvas._canvas.ownerDocument.defaultView.getComputedStyle(canvas._canvas);
          return {
            display: style.display,
            visibility: style.visibility,
            opacity: style.opacity,
            position: style.position,
            zIndex: style.zIndex,
            width: style.width,
            height: style.height,
          };
        })
        : null,
      ownerBodyChildren: canvas._canvas && canvas._canvas.ownerDocument
        ? tryValue(() => Array.from(canvas._canvas.ownerDocument.body.children).map((child) => {
          const style = canvas._canvas.ownerDocument.defaultView.getComputedStyle(child);
          return {
            tag: child.tagName,
            id: child.id,
            className: child.className,
            display: style.display,
            visibility: style.visibility,
            opacity: style.opacity,
            position: style.position,
            zIndex: style.zIndex,
            outerHtml: child.outerHTML.slice(0, 500),
          };
        }))
        : [],
      appendCanvasOnScreenSource: typeof canvas._appendCanvasOnScreen === "function"
        ? Function.prototype.toString.call(canvas._appendCanvasOnScreen)
        : null,
      createOffscreenCanvasSource: typeof canvas._createOffscreenCanvas === "function"
        ? Function.prototype.toString.call(canvas._createOffscreenCanvas)
        : null,
      hookCanvasDrawApiSource: typeof canvas._hookCanvasDrawApi === "function"
        ? Function.prototype.toString.call(canvas._hookCanvasDrawApi)
        : null,
      getContextSource: typeof Object.getPrototypeOf(canvas).getContext === "function"
        ? Function.prototype.toString.call(Object.getPrototypeOf(canvas).getContext)
        : null,
      proxyContextFunctionsSource: typeof canvas._proxyContextFunctions === "function"
        ? Function.prototype.toString.call(canvas._proxyContextFunctions)
        : null,
    } : null,
    webglOwnKeys: mainWebgl2 ? Reflect.ownKeys(mainWebgl2).map(String) : [],
    windowInstance: typeof Window === "function" ? globalThis instanceof Window : null,
    canvasConstructor: canvas && canvas.constructor && canvas.constructor.name,
    webgl2: !!webgl2,
    webgl2Constructor: webgl2 && webgl2.constructor && webgl2.constructor.name,
    webgl2Version: webgl2 && webgl2.getParameter ? webgl2.getParameter(webgl2.VERSION) : null,
    webgl: !!webgl,
    webglConstructor: webgl && webgl.constructor && webgl.constructor.name,
    probeError,
  };
})()`;

for (const context of contexts) {
  try {
    const result = await call("Runtime.evaluate", {
      expression,
      contextId: context.id,
      returnByValue: true,
      awaitPromise: true,
    });
    console.log(JSON.stringify({
      context: { id: context.id, name: context.name, origin: context.origin },
      result: result.result.value,
      exception: result.exceptionDetails
        && (result.exceptionDetails.exception?.description || result.exceptionDetails.text),
    }, null, 2));
  } catch (error) {
    console.log(JSON.stringify({ context: context.id, error: error.message }, null, 2));
  }
}

if (screenshotPath) {
  const screenshot = await call("Page.captureScreenshot", { format: "png" });
  await writeFile(screenshotPath, Buffer.from(screenshot.data, "base64"));
  console.log(`Screenshot written to ${screenshotPath}`);
}

socket.close();
