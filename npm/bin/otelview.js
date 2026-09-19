#!/usr/bin/env node
// Shim: exec the platform binary installed next to this file. bun (bunx)
// skips postinstall scripts, so when the binary is missing it is downloaded
// on demand before the first run.

const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");

const bin = path.join(__dirname, "otelview");

async function run() {
  if (!fs.existsSync(bin)) {
    await require("../install.js").main();
  }
  const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
  if (result.error) {
    console.error(String(result.error));
    process.exit(1);
  }
  process.exit(result.status ?? 0);
}

run().catch((err) => {
  console.error(String(err && err.message ? err.message : err));
  process.exit(1);
});
