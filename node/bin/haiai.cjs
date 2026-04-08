#!/usr/bin/env node
"use strict";

/**
 * Binary wrapper for the haiai Rust CLI.
 *
 * Resolution order:
 * 1. HAIAI_BINARY_PATH environment variable
 * 2. Platform-specific @haiai/cli-{os}-{arch} optional dependency
 * 3. Project-local binary: walk up from cwd looking for .cargo-local/bin/haiai
 * 4. System PATH (only if version matches the SDK)
 */

const { execFileSync } = require("child_process");
const { existsSync } = require("fs");
const path = require("path");

// Must match the version in package.json.
const SDK_VERSION = "0.2.2";
const SDK_MAJOR_MINOR = SDK_VERSION.split(".").slice(0, 2).join(".");

const PLATFORMS = {
  "darwin-arm64": "@haiai/cli-darwin-arm64",
  "darwin-x64": "@haiai/cli-darwin-x64",
  "linux-x64": "@haiai/cli-linux-x64",
  "linux-arm64": "@haiai/cli-linux-arm64",
  "win32-x64": "@haiai/cli-win32-x64",
};

function getBinaryName() {
  return process.platform === "win32" ? "haiai.exe" : "haiai";
}

function getBinaryVersion(binPath) {
  try {
    const out = execFileSync(binPath, ["--version"], {
      encoding: "utf8",
      timeout: 5000,
      stdio: ["ignore", "pipe", "ignore"],
    });
    const m = out.match(/haiai (\d+\.\d+\.\d+)/);
    return m ? m[1] : null;
  } catch {
    return null;
  }
}

function walkUpForCargoLocal(binName) {
  let cur = path.resolve(process.cwd());
  for (let i = 0; i < 20; i++) {
    const candidate = path.join(cur, ".cargo-local", "bin", binName);
    if (existsSync(candidate)) return candidate;
    const parent = path.dirname(cur);
    if (parent === cur) break;
    cur = parent;
  }
  return null;
}

function whichSync(name) {
  try {
    const out = execFileSync(
      process.platform === "win32" ? "where" : "which",
      [name],
      { encoding: "utf8", timeout: 3000, stdio: ["ignore", "pipe", "ignore"] },
    );
    const first = out.trim().split(/\r?\n/)[0];
    return first && existsSync(first) ? first : null;
  } catch {
    return null;
  }
}

function findBinary() {
  const binName = getBinaryName();

  // 1. Check env override (trusted — skip version check)
  if (process.env.HAIAI_BINARY_PATH) {
    const envPath = process.env.HAIAI_BINARY_PATH;
    if (existsSync(envPath)) return envPath;
  }

  // 2. Check platform-specific optional dependency
  const platformKey = `${process.platform}-${process.arch}`;
  const pkgName = PLATFORMS[platformKey];
  if (pkgName) {
    try {
      const pkgDir = path.dirname(require.resolve(`${pkgName}/package.json`));
      const binPath = path.join(pkgDir, "bin", binName);
      if (existsSync(binPath)) return binPath;
    } catch {
      // Package not installed — fall through
    }
  }

  // 3. Project-local .cargo-local/bin/haiai
  const local = walkUpForCargoLocal(binName);
  if (local) return local;

  // 4. System PATH — only accept if major.minor matches the SDK
  const systemBin = whichSync("haiai");
  if (systemBin) {
    const ver = getBinaryVersion(systemBin);
    if (ver && ver.startsWith(SDK_MAJOR_MINOR)) return systemBin;
    if (ver) {
      console.error(
        `haiai: skipping PATH binary ${systemBin} (v${ver}) -- ` +
        `SDK requires v${SDK_MAJOR_MINOR}.x`,
      );
    }
  }

  return null;
}

const binary = findBinary();

if (binary) {
  // Exec the native binary — replaces this process
  try {
    execFileSync(binary, process.argv.slice(2), {
      stdio: "inherit",
      env: process.env,
    });
  } catch (err) {
    process.exit(err.status ?? 1);
  }
} else {
  console.error(
    "haiai: native binary not found for this platform.\n" +
    `Platform: ${process.platform}-${process.arch}\n` +
    `SDK requires haiai v${SDK_MAJOR_MINOR}.x\n` +
    "Install the correct @haiai/cli-* package, build with cargo, or set HAIAI_BINARY_PATH.",
  );
  process.exit(1);
}
