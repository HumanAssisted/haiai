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
    expect(content).toContain("Fall back to TypeScript CLI");
    expect(content).toContain("process.exit(1)");
  });
});
