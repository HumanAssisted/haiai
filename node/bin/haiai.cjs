#!/usr/bin/env node
"use strict";

/**
 * Binary wrapper for the haiai Rust CLI.
 *
 * Resolution order:
 * 1. HAIAI_BINARY_PATH environment variable
 * 2. Platform-specific @haiai/cli-{os}-{arch} optional dependency
 * 3. Fall back to the TypeScript CLI (./dist/esm/cli.js)
 */

const { execFileSync } = require("child_process");
const { existsSync } = require("fs");
const path = require("path");

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

function findBinary() {
  // 1. Check env override
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
      const binPath = path.join(pkgDir, "bin", getBinaryName());
      if (existsSync(binPath)) return binPath;
    } catch {
      // Package not installed — fall through
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
    "Install the correct @haiai/cli-* package or set HAIAI_BINARY_PATH."
  );
  process.exit(1);
}
