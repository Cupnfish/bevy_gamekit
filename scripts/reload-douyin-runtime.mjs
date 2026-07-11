const port = Number(process.argv[2] || 8307);
const waitMs = Number(process.argv[3] || 15000);
const targets = await fetch(`http://127.0.0.1:${port}/json/list`).then((response) => response.json());
const target = targets.find((entry) => entry.title === "MiniApp Webview");

if (!target) throw new Error(`MiniApp Webview target was not found on CDP port ${port}`);

const socket = new WebSocket(target.webSocketDebuggerUrl);
const pending = new Map();
let nextId = 1;

function remoteValue(value) {
  if (Object.hasOwn(value, "value")) return value.value;
  return value.description || value.type;
}

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

  if (message.method === "Runtime.consoleAPICalled") {
    console.log(`[console.${message.params.type}]`, ...message.params.args.map(remoteValue));
  } else if (message.method === "Runtime.exceptionThrown") {
    const details = message.params.exceptionDetails;
    console.log("[exception]", details.exception?.description || details.text);
  } else if (message.method === "Log.entryAdded") {
    const entry = message.params.entry;
    console.log(`[log.${entry.level}]`, entry.text);
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

await Promise.all([call("Runtime.enable"), call("Log.enable"), call("Page.enable")]);
console.log(`[reload] ${target.url}`);
await call("Page.reload", { ignoreCache: true });
await new Promise((resolve) => setTimeout(resolve, waitMs));
socket.close();
