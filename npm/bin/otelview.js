#!/usr/bin/env node
// Shim: exec the platform binary installed next to this file by install.js.

const path = require("path");
const { spawnSync } = require("child_process");

const bin = path.join(__dirname, "otelview");
const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(
    result.error.code === "ENOENT"
      ? "otelview binary not found — reinstall the otel-viewer package (postinstall downloads it)"
      : String(result.error),
  );
  process.exit(1);
}
process.exit(result.status ?? 0);
