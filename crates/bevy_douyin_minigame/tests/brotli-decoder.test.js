const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const projectDirectory = path.resolve(process.argv[2] || "dist-douyin/emoji");
const decoder = require(path.join(projectDirectory, "brotli_decoder.js"));

async function main() {
  const decoderWasm = fs.readFileSync(
    path.join(projectDirectory, "brotli_decoder_bg.wasm"),
  );
  await decoder({ module_or_path: decoderWasm });
  const compressed = fs.readFileSync(
    path.join(projectDirectory, "game_wasm_bg.wasm.br"),
  );
  const decompressed = decoder.decompress(compressed);

  assert.deepEqual(Array.from(decompressed.subarray(0, 4)), [0, 97, 115, 109]);
  assert.doesNotThrow(() => new WebAssembly.Module(decompressed));
  console.log(
    `Brotli main module decoded: ${compressed.length} -> ${decompressed.length} bytes`,
  );
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
