const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const projectDirectory = path.resolve(process.argv[2] || "dist-douyin/emoji");
const manifest = JSON.parse(
  fs.readFileSync(path.join(projectDirectory, "wasm-split-report.json"), "utf8"),
);
const runtime = require(path.join(projectDirectory, "__wasm_split.js"));
const loaderName = manifest.loaderImports[0];
const modulePath = manifest.modules.find((entry) => entry.kind === "splitPoint").path
  .replaceAll("\\", "/");

const callbackCalls = [];
const table = {
  get(index) {
    assert.equal(index, 7);
    return (data, success) => callbackCalls.push({ data, success });
  },
};
const main = {
  memory: { kind: "memory" },
  __indirect_function_table: table,
  shared_export: () => 1,
};
let requestedPath = null;
let instantiateImports = null;
const originalInstantiate = WebAssembly.instantiate;
WebAssembly.instantiate = async (bytes, imports) => {
  assert.deepEqual(Array.from(bytes), [0, 97, 115, 109]);
  instantiateImports = imports;
  return { instance: { exports: { split_export: () => 2 } } };
};

runtime.configure({
  readFile: async (filePath) => {
    requestedPath = filePath;
    return Uint8Array.from([1, 2, 3]);
  },
  decompress: () => Uint8Array.from([0, 97, 115, 109]),
  mainExports: () => main,
});

runtime[loaderName](7, 99)
  .then(() => {
    assert.equal(requestedPath, modulePath);
    assert.equal(instantiateImports.env.memory, main.memory);
    assert.equal(instantiateImports.__wasm_split.memory, main.memory);
    assert.equal(
      instantiateImports.__wasm_split.__indirect_function_table,
      table,
    );
    assert.equal(instantiateImports.__wasm_split.shared_export, main.shared_export);
    assert.deepEqual(callbackCalls, [{ data: 99, success: true }]);
    console.log("compressed Wasm split runtime passed");
  })
  .catch((error) => {
    console.error(error);
    process.exitCode = 1;
  })
  .finally(() => {
    WebAssembly.instantiate = originalInstantiate;
  });
