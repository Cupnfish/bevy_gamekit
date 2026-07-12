import { writeFile } from "node:fs/promises";

const port = Number(process.argv[2] || 8307);
const screenshotPath = process.argv[3] || "agent-captures/douyin-presentation-test.png";
const mode = process.argv[4] || "color";
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
  } else if (message.method === "Runtime.executionContextCreated") {
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

await Promise.all([call("Runtime.enable"), call("Page.enable")]);
await new Promise((resolve) => setTimeout(resolve, 200));

const installExpression = `(() => {
  const mode = ${JSON.stringify(mode)};
  const sandbox = globalThis.developerSandbox && globalThis.developerSandbox.global;
  const game = sandbox && sandbox.BevyMiniGame ? sandbox : globalThis;
  const adapter = game.BevyMiniGame;
  if (!adapter || !adapter.canvas || typeof game.requestAnimationFrame !== "function") return null;

  if (typeof game.__restoreDouyinPresentationTest === "function") {
    game.__restoreDouyinPresentationTest();
  }

  const context = adapter.canvas.getContext("webgl2");
  if (!context) return null;
  const original = game.requestAnimationFrame;
  const test = { mode, frames: 0, pixels: null, sourceFramebufferBound: null };

  game.requestAnimationFrame = (callback) => original((time) => {
    callback(time);
    const sourceFramebuffer = context.getParameter(context.DRAW_FRAMEBUFFER_BINDING);
    test.sourceFramebufferBound = !!sourceFramebuffer;
    if (mode === "blit" && sourceFramebuffer) {
      context.bindFramebuffer(context.READ_FRAMEBUFFER, sourceFramebuffer);
      context.bindFramebuffer(context.DRAW_FRAMEBUFFER, null);
      context.blitFramebuffer(
        0,
        0,
        context.drawingBufferWidth,
        context.drawingBufferHeight,
        0,
        0,
        context.drawingBufferWidth,
        context.drawingBufferHeight,
        context.COLOR_BUFFER_BIT,
        context.NEAREST,
      );
      context.bindFramebuffer(context.READ_FRAMEBUFFER, null);
    } else if (mode !== "inspect") {
      context.bindFramebuffer(context.FRAMEBUFFER, null);
    }
    if (mode === "dirty-pixel") {
      context.bindFramebuffer(context.FRAMEBUFFER, null);
      context.enable(context.SCISSOR_TEST);
      context.scissor(0, 0, 1, 1);
      context.colorMask(true, true, true, true);
      context.clearColor(0, 0, 0, 1);
      context.clear(context.COLOR_BUFFER_BIT);
      context.disable(context.SCISSOR_TEST);
    } else if (mode === "masked-clear") {
      context.bindFramebuffer(context.FRAMEBUFFER, null);
      context.disable(context.SCISSOR_TEST);
      context.colorMask(false, false, false, false);
      context.clear(context.COLOR_BUFFER_BIT);
      context.colorMask(true, true, true, true);
    } else if (mode === "color") {
      context.disable(context.SCISSOR_TEST);
      context.colorMask(true, true, true, true);
      context.clearColor(1, 0, 1, 1);
      context.clear(context.COLOR_BUFFER_BIT);
    }
    context.flush();
    const pixel = new Uint8Array(4);
    context.readPixels(
      Math.floor(context.drawingBufferWidth / 2),
      Math.floor(context.drawingBufferHeight / 2),
      1,
      1,
      context.RGBA,
      context.UNSIGNED_BYTE,
      pixel,
    );
    test.frames += 1;
    test.pixels = Array.from(pixel);
  });

  game.__douyinPresentationTest = test;
  game.__restoreDouyinPresentationTest = () => {
    game.requestAnimationFrame = original;
    delete game.__restoreDouyinPresentationTest;
  };
  game.setTimeout(game.__restoreDouyinPresentationTest, 3000);
  return { installed: true, width: context.drawingBufferWidth, height: context.drawingBufferHeight };
})()`;

let installedContext = null;
for (const context of contexts) {
  const result = await call("Runtime.evaluate", {
    expression: installExpression,
    contextId: context.id,
    returnByValue: true,
  });
  if (result.result.value?.installed) {
    installedContext = context;
    console.log(JSON.stringify({ context: context.id, ...result.result.value }));
    break;
  }
}

if (!installedContext) throw new Error("BevyMiniGame runtime context was not found");

await new Promise((resolve) => setTimeout(resolve, 750));
const status = await call("Runtime.evaluate", {
  expression: `(() => {
    const sandbox = globalThis.developerSandbox && globalThis.developerSandbox.global;
    const game = sandbox && sandbox.BevyMiniGame ? sandbox : globalThis;
    return game.__douyinPresentationTest;
  })()`,
  contextId: installedContext.id,
  returnByValue: true,
});
console.log(JSON.stringify(status.result.value));

const screenshot = await call("Page.captureScreenshot", { format: "png" });
await writeFile(screenshotPath, Buffer.from(screenshot.data, "base64"));
console.log(`Screenshot written to ${screenshotPath}`);

socket.close();
