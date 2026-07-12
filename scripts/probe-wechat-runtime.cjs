const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const projectPath = path.resolve(process.argv[2] || "dist-wechat/emoji");
const screenshotPath = path.resolve(
  process.argv[3] || "agent-captures/wechat-runtime.png",
);
const cliPath = process.env.WECHAT_DEVTOOLS_CLI
  || "C:\\Program Files (x86)\\Tencent\\微信web开发者工具\\cli.bat";
const automatorPath = process.env.WECHAT_AUTOMATOR_PATH
  || path.resolve("target/wechat-automator/node_modules/miniprogram-automator");
const port = Number(process.env.WECHAT_AUTOMATION_PORT || 9420);
const initialDelayMs = Number(process.env.WECHAT_PROBE_INITIAL_DELAY_MS || 12000);
const sampleIntervalMs = Number(process.env.WECHAT_PROBE_INTERVAL_MS || 15000);
const sampleDurationMs = Number(process.env.WECHAT_PROBE_DURATION_MS || 120000);
const launchProject = process.env.WECHAT_PROBE_SKIP_LAUNCH !== "1";
const pwshPath = process.env.PWSH_PATH
  || "C:\\Program Files\\PowerShell\\7\\pwsh.exe";

const automator = require(automatorPath);

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function quotePowerShell(value) {
  return `'${String(value).replace(/'/g, "''")}'`;
}

async function settle(label, operation, timeout = 10000) {
  let handle;
  try {
    const value = await Promise.race([
      operation,
      new Promise((_, reject) => {
        handle = setTimeout(() => reject(new Error(`${label} timed out`)), timeout);
      }),
    ]);
    return { ok: true, value };
  } catch (error) {
    return { ok: false, error: String(error && error.message || error) };
  } finally {
    clearTimeout(handle);
  }
}

function processMemory() {
  const script = [
    "Get-Process -Name wechatdevtools -ErrorAction SilentlyContinue",
    "| Sort-Object PrivateMemorySize64 -Descending",
    "| Select-Object -First 8 Id,ProcessName,@{Name='privateMiB';Expression={[math]::Round($_.PrivateMemorySize64 / 1MB, 1)}},@{Name='workingSetMiB';Expression={[math]::Round($_.WorkingSet64 / 1MB, 1)}},StartTime",
    "| ConvertTo-Json -Compress",
  ].join(" ");
  const result = spawnSync(pwshPath, [
    "-NoProfile",
    "-NonInteractive",
    "-Command",
    script,
  ], { encoding: "utf8" });
  if (result.error) return { error: String(result.error.message || result.error) };
  if (result.status !== 0) {
    return { error: String(result.stderr || `PowerShell exited ${result.status}`) };
  }
  const output = String(result.stdout || "").trim();
  if (!output) return [];
  try {
    const parsed = JSON.parse(output);
    return Array.isArray(parsed) ? parsed : [parsed];
  } catch (error) {
    return { error: String(error.message || error), output };
  }
}

function runtimeEvaluation(miniProgram) {
  return miniProgram.evaluate(() => {
    const host = typeof GameGlobal !== "undefined" ? GameGlobal : globalThis;
    const adapter = host.BevyMiniGame || globalThis.BevyMiniGame;
    return adapter
      ? {
          installed: adapter.installed === true,
          adapterVersion: adapter.adapterVersion,
          platform: adapter.platform,
          apiGlobal: adapter.apiGlobal,
          canvasWidth: adapter.canvas && adapter.canvas.width,
          canvasHeight: adapter.canvas && adapter.canvas.height,
          clientWidth: adapter.canvas && adapter.canvas.clientWidth,
          clientHeight: adapter.canvas && adapter.canvas.clientHeight,
          gameReady: !!(
            host.document
            && adapter.canvas
            && host.document.activeElement === adapter.canvas
          ),
          debugStats: typeof adapter.debugStats === "function" ? adapter.debugStats() : null,
        }
      : null;
  });
}

async function main() {
  const messages = [];
  const exceptions = [];
  if (launchProject) {
    const cliCommand = [
      "&",
      quotePowerShell(cliPath),
      "auto --project",
      quotePowerShell(projectPath),
      "--auto-port",
      String(port),
      "--trust-project",
    ].join(" ");
    const cli = spawnSync(pwshPath, [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      cliCommand,
    ], {
      stdio: "inherit",
    });
    if (cli.error) throw cli.error;
    if (cli.status !== 0) throw new Error("WeChat CLI failed to enable automation");
  }

  let miniProgram = null;
  let connectError = null;
  for (let attempt = 0; attempt < 30 && !miniProgram; attempt += 1) {
    try {
      miniProgram = await automator.connect({
        wsEndpoint: `ws://127.0.0.1:${port}`,
      });
    } catch (error) {
      connectError = error;
      await delay(500);
    }
  }
  if (!miniProgram) throw connectError;

  miniProgram.on("console", (message) => messages.push(message));
  miniProgram.on("exception", (error) => exceptions.push(error));

  try {
    await delay(initialDelayMs);
    fs.mkdirSync(path.dirname(screenshotPath), { recursive: true });
    const systemInfo = await settle("systemInfo", miniProgram.systemInfo(), 30000);
    const samples = [];
    const startedAt = Date.now();
    while (Date.now() - startedAt <= sampleDurationMs) {
      const runtime = await settle(
        "runtime evaluation",
        runtimeEvaluation(miniProgram),
        30000,
      );
      const sample = {
        elapsedMs: Date.now() - startedAt,
        runtime,
        processes: processMemory(),
      };
      samples.push(sample);
      console.log("WECHAT_PROBE_SAMPLE " + JSON.stringify(sample));
      if (!runtime.ok) break;
      const remaining = sampleDurationMs - (Date.now() - startedAt);
      if (remaining <= 0) break;
      await delay(Math.min(sampleIntervalMs, remaining));
    }
    const screenshot = await settle(
      "screenshot",
      miniProgram.screenshot({ path: screenshotPath }),
      30000,
    );

    console.log(JSON.stringify({
      projectPath,
      screenshotPath,
      systemInfo,
      samples,
      screenshot,
      messages,
      exceptions,
    }, null, 2));
  } finally {
    miniProgram.disconnect();
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
