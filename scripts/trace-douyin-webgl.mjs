const port = Number(process.argv[2] || 8307);
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

await call("Runtime.enable");
await new Promise((resolve) => setTimeout(resolve, 200));

const expression = `(() => {
  const sandbox = globalThis.developerSandbox && globalThis.developerSandbox.global;
  const adapter = sandbox && sandbox.BevyMiniGame;
  const canvas = adapter && adapter.canvas;
  const gl = canvas && canvas.getContext && canvas.getContext("webgl2", { antialias: false });
  if (!gl) return { installed: false, reason: "main WebGL2 context not found" };
  if (gl.__douyinBevyTraceStats) return { installed: true, reused: true };

  const stats = { installedAt: performance.now() };
  const methods = [
    "clear",
    "drawArrays",
    "drawElements",
    "drawArraysInstanced",
    "drawElementsInstanced",
    "blitFramebuffer",
    "bindFramebuffer",
    "texSubImage2D",
  ];
  try {
    Object.defineProperty(gl, "__douyinBevyTraceStats", { value: stats });
    for (const method of methods) {
      const original = gl[method];
      if (typeof original !== "function") continue;
      stats[method] = 0;
      gl[method] = function tracedWebGlCall(...args) {
        stats[method] += 1;
        stats[method + "LastArgs"] = args.slice(0, 8).map((value) =>
          typeof value === "number" ? value : value && value.constructor && value.constructor.name
        );
        return Reflect.apply(original, this, args);
      };
    }
  } catch (error) {
    return { installed: false, reason: String(error && (error.stack || error)) };
  }
  return {
    installed: true,
    methods: Object.keys(stats),
  };
})()`;

const installedContexts = [];
for (const context of contexts) {
  const result = await call("Runtime.evaluate", {
    expression,
    contextId: context.id,
    returnByValue: true,
  });
  console.log(JSON.stringify({
    context: context.id,
    result: result.result.value,
    exception: result.exceptionDetails && result.exceptionDetails.text,
  }, null, 2));
  if (result.result.value && result.result.value.installed) installedContexts.push(context.id);
}

await new Promise((resolve) => setTimeout(resolve, 2000));
for (const contextId of installedContexts) {
  const result = await call("Runtime.evaluate", {
    expression: `(() => {
      const sandbox = globalThis.developerSandbox && globalThis.developerSandbox.global;
      const adapter = sandbox && sandbox.BevyMiniGame;
      const gl = adapter && adapter.canvas && adapter.canvas.getContext("webgl2");
      return gl && gl.__douyinBevyTraceStats;
    })()`,
    contextId,
    returnByValue: true,
  });
  console.log(JSON.stringify({ context: contextId, stats: result.result.value }, null, 2));

  const animationFrame = await call("Runtime.evaluate", {
    expression: `(() => {
      const sandbox = globalThis.developerSandbox && globalThis.developerSandbox.global;
      if (!sandbox || typeof sandbox.requestAnimationFrame !== "function") {
        return Promise.resolve({ fired: false, reason: "requestAnimationFrame unavailable" });
      }
      return new Promise((resolve) => {
        let settled = false;
        const timeout = sandbox.setTimeout(() => {
          if (!settled) resolve({ fired: false, elapsed: sandbox.performance.now() - start });
        }, 500);
        const start = sandbox.performance.now();
        const handle = sandbox.requestAnimationFrame((timestamp) => {
          settled = true;
          sandbox.clearTimeout(timeout);
          resolve({ fired: true, handle, timestamp, elapsed: sandbox.performance.now() - start });
        });
      });
    })()`,
    contextId,
    returnByValue: true,
    awaitPromise: true,
  });
  console.log(JSON.stringify({ context: contextId, animationFrame: animationFrame.result.value }, null, 2));

  const scheduling = await call("Runtime.evaluate", {
    expression: `(() => {
      const sandbox = globalThis.developerSandbox && globalThis.developerSandbox.global;
      if (!sandbox) return Promise.resolve({ reason: "sandbox unavailable" });
      const probe = (install) => new Promise((resolve) => {
        let settled = false;
        const start = sandbox.performance.now();
        const timeout = sandbox.setTimeout(() => {
          if (!settled) resolve({ fired: false, elapsed: sandbox.performance.now() - start });
        }, 500);
        install(() => {
          settled = true;
          sandbox.clearTimeout(timeout);
          resolve({ fired: true, elapsed: sandbox.performance.now() - start });
        });
      });
      return Promise.all([
        probe((done) => sandbox.setTimeout(done, 0)),
        probe((done) => {
          const channel = new sandbox.MessageChannel();
          channel.port1.onmessage = done;
          channel.port1.start();
          channel.port2.postMessage(undefined);
        }),
        sandbox.scheduler && typeof sandbox.scheduler.postTask === "function"
          ? probe((done) => sandbox.scheduler.postTask(done))
          : Promise.resolve({ fired: false, unavailable: true }),
      ]).then(([timeout, messageChannel, scheduler]) => ({
        timeout,
        messageChannel,
        scheduler,
        messageChannelName: sandbox.MessageChannel && sandbox.MessageChannel.name,
        schedulerType: typeof sandbox.scheduler,
      }));
    })()`,
    contextId,
    returnByValue: true,
    awaitPromise: true,
  });
  console.log(JSON.stringify({ context: contextId, scheduling: scheduling.result.value }, null, 2));
}

socket.close();
