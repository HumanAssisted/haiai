import { describe, it, expect } from "vitest";
import { spawnSync } from "child_process";
import { existsSync } from "fs";
import path from "path";

const PLATFORMS: Record<string, string> = {
  "darwin-arm64": "@haiai/cli-darwin-arm64",
  "darwin-x64": "@haiai/cli-darwin-x64",
  "linux-x64": "@haiai/cli-linux-x64",
  "linux-arm64": "@haiai/cli-linux-arm64",
  "win32-x64": "@haiai/cli-win32-x64",
};

const binWrapper = path.resolve(__dirname, "..", "bin", "haiai.cjs");

describe("binary wrapper", () => {
  it("wrapper script exists and is valid JS", () => {
    expect(existsSync(binWrapper)).toBe(true);
    const content = require("fs").readFileSync(binWrapper, "utf-8");
    expect(content).toContain("findBinary");
    expect(content).toContain("PLATFORMS");
  });

  it("wrapper script can be parsed by Node.js without errors", () => {
    const result = spawnSync(process.execPath, ["--check", binWrapper]);
    expect(result.status).toBe(0);
  });

  it("wrapper exits gracefully when no binary is available", () => {
    // Run with empty PATH and no platform packages to trigger fallback
    const result = spawnSync(process.execPath, [binWrapper], {
      env: { ...process.env, HAIAI_BINARY_PATH: "", PATH: "" },
      timeout: 5000,
    });
    const stderr = result.stderr?.toString() ?? "";
    // Should not crash with a ReferenceError (CJS/ESM mismatch)
    expect(stderr).not.toContain("ReferenceError");
    // Should not crash with a SyntaxError
    expect(stderr).not.toContain("SyntaxError");
  });

  it("platform key matches a known package", () => {
    const key = `${process.platform}-${process.arch}`;
    expect(PLATFORMS[key]).toBeDefined();
  });

  it("platform package.json exists for current platform", () => {
    const key = `${process.platform}-${process.arch}`;
    const pkgName = PLATFORMS[key];
    const pkgJsonPath = path.resolve(
      __dirname,
      "..",
      "npm",
      ...pkgName.split("/"),
      "package.json"
    );
    expect(existsSync(pkgJsonPath)).toBe(true);

    const pkg = JSON.parse(require("fs").readFileSync(pkgJsonPath, "utf-8"));
    expect(pkg.name).toBe(pkgName);
    expect(pkg.os).toContain(process.platform);
    expect(pkg.cpu).toContain(process.arch);
  });

  it("all 5 platform packages have valid package.json", () => {
    for (const [key, pkgName] of Object.entries(PLATFORMS)) {
      const pkgJsonPath = path.resolve(
        __dirname,
        "..",
        "npm",
        ...pkgName.split("/"),
        "package.json"
      );
      expect(existsSync(pkgJsonPath)).toBe(true);

      const pkg = JSON.parse(require("fs").readFileSync(pkgJsonPath, "utf-8"));
      expect(pkg.name).toBe(pkgName);
      expect(pkg.os).toBeDefined();
      expect(pkg.cpu).toBeDefined();
      expect(pkg.bin).toBeDefined();
      expect(pkg.bin.haiai).toBeDefined();
    }
  });

  it("wrapper falls back gracefully when binary not present", () => {
    const content = require("fs").readFileSync(binWrapper, "utf-8");
    expect(content).toContain("native binary not found");
    expect(content).toContain("process.exit(1)");
  });
});

// ---------------------------------------------------------------------------
// Resolution logic tests (subprocess-based, real filesystem)
// ---------------------------------------------------------------------------

import { mkdtempSync, writeFileSync, mkdirSync, readFileSync, rmSync } from "fs";
import { tmpdir } from "os";

const isWindows = process.platform === "win32";

/** Read SDK version from package.json so tests stay in sync after bumps. */
const SDK_VERSION = JSON.parse(
  readFileSync(path.resolve(__dirname, "..", "package.json"), "utf-8"),
).version as string;
const SDK_MAJOR_MINOR = SDK_VERSION.split(".").slice(0, 2).join(".");

/** Create a stub haiai binary that prints the given version. */
function makeStub(filePath: string, version: string): void {
  mkdirSync(path.dirname(filePath), { recursive: true });
  writeFileSync(
    filePath,
    `#!/bin/sh\necho "haiai ${version}"\n`,
  );
  require("fs").chmodSync(filePath, 0o755);
}

describe.skipIf(isWindows)("resolution logic", () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = mkdtempSync(path.join(tmpdir(), "haiai-resolve-"));
  });

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true });
  });

  it("finds .cargo-local/bin/haiai from cwd", () => {
    makeStub(path.join(tmpDir, ".cargo-local", "bin", "haiai"), SDK_VERSION);

    const result = spawnSync(process.execPath, [binWrapper, "--version"], {
      cwd: tmpDir,
      env: { HAIAI_BINARY_PATH: "", PATH: "" },
      timeout: 5000,
    });

    expect(result.stdout?.toString()).toContain(`haiai ${SDK_VERSION}`);
    expect(result.status).toBe(0);
  });

  it("finds .cargo-local/bin/haiai in ancestor directory", () => {
    makeStub(path.join(tmpDir, ".cargo-local", "bin", "haiai"), SDK_VERSION);
    const nested = path.join(tmpDir, "a", "b", "c");
    mkdirSync(nested, { recursive: true });

    const result = spawnSync(process.execPath, [binWrapper, "--version"], {
      cwd: nested,
      env: { HAIAI_BINARY_PATH: "", PATH: "" },
      timeout: 5000,
    });

    expect(result.stdout?.toString()).toContain(`haiai ${SDK_VERSION}`);
    expect(result.status).toBe(0);
  });

  it("HAIAI_BINARY_PATH takes priority over .cargo-local", () => {
    // cargo-local has current SDK version
    makeStub(path.join(tmpDir, ".cargo-local", "bin", "haiai"), SDK_VERSION);
    // env override points to a different stub with a distinctive version
    const envBin = path.join(tmpDir, "override", "haiai");
    makeStub(envBin, "9.9.9");

    const result = spawnSync(process.execPath, [binWrapper, "--version"], {
      cwd: tmpDir,
      env: { HAIAI_BINARY_PATH: envBin, PATH: "" },
      timeout: 5000,
    });

    expect(result.stdout?.toString()).toContain("haiai 9.9.9");
    expect(result.status).toBe(0);
  });

  it("rejects PATH binary with mismatched version", () => {
    // No .cargo-local — force fallback to PATH
    const pathDir = path.join(tmpDir, "pathbin");
    makeStub(path.join(pathDir, "haiai"), "0.1.4");

    const result = spawnSync(process.execPath, [binWrapper, "--version"], {
      cwd: tmpDir,
      env: { HAIAI_BINARY_PATH: "", PATH: `${pathDir}:/usr/bin` },
      timeout: 5000,
    });

    const stderr = result.stderr?.toString() ?? "";
    expect(stderr).toContain("skipping PATH binary");
    expect(stderr).toContain("0.1.4");
    expect(result.status).not.toBe(0);
  });

  it("accepts PATH binary with matching version", () => {
    // No .cargo-local — force fallback to PATH
    const pathDir = path.join(tmpDir, "pathbin");
    makeStub(path.join(pathDir, "haiai"), SDK_VERSION);

    const result = spawnSync(process.execPath, [binWrapper, "--version"], {
      cwd: tmpDir,
      env: { HAIAI_BINARY_PATH: "", PATH: `${pathDir}:/usr/bin` },
      timeout: 5000,
    });

    expect(result.stdout?.toString()).toContain(`haiai ${SDK_VERSION}`);
    expect(result.status).toBe(0);
  });

  it("error message includes required SDK version", () => {
    // Nothing available at all
    const result = spawnSync(process.execPath, [binWrapper], {
      cwd: tmpDir,
      env: { HAIAI_BINARY_PATH: "", PATH: "" },
      timeout: 5000,
    });

    const stderr = result.stderr?.toString() ?? "";
    expect(stderr).toContain(`v${SDK_MAJOR_MINOR}`);
    expect(result.status).not.toBe(0);
  });
});

describe("SDK_VERSION sync", () => {
  it("SDK_VERSION in haiai.cjs matches package.json version", () => {
    const wrapperContent = readFileSync(binWrapper, "utf-8");
    const m = wrapperContent.match(/const SDK_VERSION = "(\d+\.\d+\.\d+)"/);
    expect(m).not.toBeNull();

    const pkgJson = JSON.parse(
      readFileSync(path.resolve(__dirname, "..", "package.json"), "utf-8"),
    );
    expect(m![1]).toBe(pkgJson.version);
  });
});
