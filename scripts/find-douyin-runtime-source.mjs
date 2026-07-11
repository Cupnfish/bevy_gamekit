const port = Number(process.argv[2] || 8307);
const needle = process.argv[3] || "_hookCanvasDrawApi";
const targets = await fetch(`http://127.0.0.1:${port}/json/list`).then((response) => response.json());
const target = targets.find((entry) => entry.title === "MiniApp Webview");

if (!target) throw new Error(`MiniApp Webview target was not found on CDP port ${port}`);

const socket = new WebSocket(target.webSocketDebuggerUrl);
const scripts = new Map();
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
  } else if (message.method === "Debugger.scriptParsed") {
    scripts.set(message.params.scriptId, message.params);
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

await call("Debugger.enable");
await new Promise((resolve) => setTimeout(resolve, 500));

let matches = 0;
for (const script of scripts.values()) {
  let source;
  try {
    source = await Promise.race([
      call("Debugger.getScriptSource", { scriptId: script.scriptId }),
      new Promise((_, reject) => setTimeout(
        () => reject(new Error("Debugger.getScriptSource timed out")),
        2000,
      )),
    ]);
  } catch (_) {
    continue;
  }
  const text = source.scriptSource || "";
  let start = 0;
  while (true) {
    const index = text.indexOf(needle, start);
    if (index < 0) break;
    const from = Math.max(0, index - 1500);
    const to = Math.min(text.length, index + needle.length + 3000);
    console.log(JSON.stringify({
      scriptId: script.scriptId,
      url: script.url,
      sourceMapUrl: script.sourceMapURL,
      snippet: text.slice(from, to),
    }, null, 2));
    matches += 1;
    start = index + needle.length;
  }
}

console.log(`Found ${matches} match(es) for ${JSON.stringify(needle)} in ${scripts.size} scripts`);
socket.close();
