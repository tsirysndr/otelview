#!/usr/bin/env node
// Downloads the pre-built otelview binary for this platform from GitHub
// releases (same version as this package) into ./bin/.

const fs = require("fs");
const path = require("path");
const { execFileSync } = require("child_process");

const pkg = require("./package.json");
const REPO = "tsirysndr/otelview";
const VERSION = process.env.OTELVIEW_VERSION || `v${pkg.version}`;

function target() {
  const { platform, arch } = process;
  if (platform === "darwin" && arch === "arm64") return "aarch64-apple-darwin";
  if (platform === "darwin" && arch === "x64") return "x86_64-apple-darwin";
  if (platform === "linux" && arch === "x64") return "x86_64-unknown-linux-gnu";
  if (platform === "linux" && arch === "arm64") return "aarch64-unknown-linux-gnu";
  throw new Error(`otelview: unsupported platform ${platform}-${arch}`);
}

async function download(url, dest) {
  const res = await fetch(url, { redirect: "follow" });
  if (!res.ok) {
    throw new Error(`download failed: ${res.status} ${res.statusText} (${url})`);
  }
  const buf = Buffer.from(await res.arrayBuffer());
  fs.writeFileSync(dest, buf);
}

async function latestVersion() {
  const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
    redirect: "follow",
  });
  if (!res.ok) throw new Error(`could not resolve the latest release (${res.status})`);
  return (await res.json()).tag_name;
}

async function fetchArchive(version, triple, dest) {
  const asset = `otelview-${version}-${triple}.tar.gz`;
  const url = `https://github.com/${REPO}/releases/download/${version}/${asset}`;
  console.log(`otelview: downloading ${url}`);
  await download(url, dest);
}

async function main() {
  const triple = target();
  const binDir = path.join(__dirname, "bin");
  fs.mkdirSync(binDir, { recursive: true });
  const archive = path.join(binDir, "otelview.tar.gz");

  try {
    await fetchArchive(VERSION, triple, archive);
  } catch (err) {
    // No asset for this package version (yet): fall back to the newest release.
    const latest = await latestVersion();
    if (latest === VERSION) throw err;
    console.log(`otelview: ${VERSION} not found, falling back to ${latest}`);
    await fetchArchive(latest, triple, archive);
  }
  execFileSync("tar", ["-xzf", archive, "-C", binDir]);
  fs.rmSync(archive);
  const bin = path.join(binDir, "otelview");
  fs.chmodSync(bin, 0o755);
  console.log(`otelview: installed ${bin}`);
}

module.exports = { main };

if (require.main === module) {
  main().catch((err) => {
    console.error(String(err && err.message ? err.message : err));
    process.exit(1);
  });
}
