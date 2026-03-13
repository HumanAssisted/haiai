#!/usr/bin/env node
"use strict";

/**
 * Populates platform-specific @haiai/cli-* packages with binaries.
 *
 * Usage:
 *   node build-platform-packages.js <artifacts-dir>
 *
 * Expects artifacts-dir to contain:
 *   haiai-cli-<version>-darwin-arm64.tar.gz
 *   haiai-cli-<version>-darwin-x64.tar.gz
 *   haiai-cli-<version>-linux-x64.tar.gz
 *   haiai-cli-<version>-linux-arm64.tar.gz
 *   haiai-cli-<version>-windows-x64.zip
 *
 * Or, for local development with a single binary:
 *   node build-platform-packages.js --local <path-to-binary>
 */

const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const PLATFORM_MAP = {
  "darwin-arm64": { pkg: "cli-darwin-arm64", bin: "haiai" },
  "darwin-x64": { pkg: "cli-darwin-x64", bin: "haiai" },
  "linux-x64": { pkg: "cli-linux-x64", bin: "haiai" },
  "linux-arm64": { pkg: "cli-linux-arm64", bin: "haiai" },
  "windows-x64": { pkg: "cli-win32-x64", bin: "haiai.exe" },
};

const npmDir = path.resolve(__dirname, "..", "npm", "@haiai");

function extractAndCopy(artifactsDir, version) {
  for (const [suffix, { pkg, bin }] of Object.entries(PLATFORM_MAP)) {
    const pkgBinDir = path.join(npmDir, pkg, "bin");
    fs.mkdirSync(pkgBinDir, { recursive: true });

    const isWindows = suffix.startsWith("windows");
    const ext = isWindows ? "zip" : "tar.gz";
    const archiveName = `haiai-cli-${version}-${suffix}.${ext}`;
    const archivePath = path.join(artifactsDir, archiveName);

    if (!fs.existsSync(archivePath)) {
      // Try glob pattern (CI downloads into subdirectories)
      const glob = execSync(`find "${artifactsDir}" -name "${archiveName}" -type f 2>/dev/null`)
        .toString()
        .trim();
      if (!glob) {
        console.warn(`Warning: ${archiveName} not found in ${artifactsDir}, skipping`);
        continue;
      }
      // Use first match
      const found = glob.split("\n")[0];
      extractArchive(found, pkgBinDir, bin, isWindows);
    } else {
      extractArchive(archivePath, pkgBinDir, bin, isWindows);
    }

    console.log(`Populated @haiai/${pkg}/bin/${bin}`);
  }
}

function extractArchive(archivePath, destDir, binName, isWindows) {
  const tmpDir = fs.mkdtempSync(path.join(destDir, ".tmp-"));
  try {
    if (isWindows) {
      execSync(`unzip -o "${archivePath}" -d "${tmpDir}"`, { stdio: "pipe" });
    } else {
      execSync(`tar xzf "${archivePath}" -C "${tmpDir}"`, { stdio: "pipe" });
    }
    // The archive contains haiai-cli (or haiai-cli.exe) — rename to haiai
    const extractedName = isWindows ? "haiai-cli.exe" : "haiai-cli";
    const srcPath = path.join(tmpDir, extractedName);
    const destPath = path.join(destDir, binName);

    if (fs.existsSync(srcPath)) {
      fs.copyFileSync(srcPath, destPath);
    } else {
      // Might be named just "haiai" in newer builds
      const altName = isWindows ? "haiai.exe" : "haiai";
      const altPath = path.join(tmpDir, altName);
      if (fs.existsSync(altPath)) {
        fs.copyFileSync(altPath, destPath);
      } else {
        throw new Error(`Binary not found in archive: tried ${extractedName} and ${altName}`);
      }
    }
    fs.chmodSync(destPath, 0o755);
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
}

function copyLocal(binaryPath) {
  const platform = process.platform;
  const arch = process.arch;
  const key = `${platform === "win32" ? "windows" : platform}-${arch}`;
  const entry = PLATFORM_MAP[key];
  if (!entry) {
    console.error(`Unsupported platform: ${key}`);
    process.exit(1);
  }

  const pkgBinDir = path.join(npmDir, entry.pkg, "bin");
  fs.mkdirSync(pkgBinDir, { recursive: true });
  fs.copyFileSync(binaryPath, path.join(pkgBinDir, entry.bin));
  fs.chmodSync(path.join(pkgBinDir, entry.bin), 0o755);
  console.log(`Copied ${binaryPath} -> @haiai/${entry.pkg}/bin/${entry.bin}`);
}

// --- Main ---
const args = process.argv.slice(2);

if (args[0] === "--local" && args[1]) {
  copyLocal(args[1]);
} else if (args[0] && args[1]) {
  // build-platform-packages.js <artifacts-dir> <version>
  extractAndCopy(args[0], args[1]);
} else if (args[0]) {
  // Read version from root package.json
  const rootPkg = JSON.parse(
    fs.readFileSync(path.resolve(__dirname, "..", "package.json"), "utf-8")
  );
  extractAndCopy(args[0], rootPkg.version);
} else {
  console.error("Usage:");
  console.error("  node build-platform-packages.js <artifacts-dir> [version]");
  console.error("  node build-platform-packages.js --local <path-to-binary>");
  process.exit(1);
}
