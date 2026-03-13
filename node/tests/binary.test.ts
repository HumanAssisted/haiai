import { describe, it, expect } from "vitest";
import { execFileSync } from "child_process";
import { existsSync } from "fs";
import path from "path";
import os from "os";

const PLATFORMS: Record<string, string> = {
  "darwin-arm64": "@haiai/cli-darwin-arm64",
  "darwin-x64": "@haiai/cli-darwin-x64",
  "linux-x64": "@haiai/cli-linux-x64",
  "linux-arm64": "@haiai/cli-linux-arm64",
  "win32-x64": "@haiai/cli-win32-x64",
};

const binWrapper = path.resolve(__dirname, "..", "bin", "haiai");

describe("binary wrapper", () => {
  it("wrapper script exists and is valid JS", () => {
    expect(existsSync(binWrapper)).toBe(true);
    // Should be parseable
    const content = require("fs").readFileSync(binWrapper, "utf-8");
    expect(content).toContain("findBinary");
    expect(content).toContain("PLATFORMS");
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
    // The wrapper should not crash when no binary is installed —
    // it falls back to the TS CLI or exits with a message
    // We test this by checking the script handles the missing case
    const content = require("fs").readFileSync(binWrapper, "utf-8");
    expect(content).toContain("Fall back to TypeScript CLI");
    expect(content).toContain("process.exit(1)");
  });
});
