#!/usr/bin/env node
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { pathToFileURL } = require('node:url');

function run(cmd, args, cwd) {
  const result = spawnSync(cmd, args, {
    cwd,
    encoding: 'utf8',
    env: process.env,
  });
  if (result.status !== 0) {
    const output = `${result.stdout || ''}${result.stderr || ''}`;
    throw new Error(`Command failed (${cmd} ${args.join(' ')}):\n${output}`);
  }
  return result;
}

async function main() {
  const root = path.resolve(__dirname, '..');

  // 1. Dist-level CJS import (deps available via node_modules)
  const cjs = require(path.join(root, 'dist', 'cjs', 'index.js'));
  if (typeof cjs.HaiClient !== 'function') {
    throw new Error('CJS dist export missing HaiClient');
  }
  console.log('  CJS dist: HaiClient OK');

  // 2. Dist-level ESM import
  const esm = await import(pathToFileURL(path.join(root, 'dist', 'esm', 'index.js')).href);
  if (typeof esm.HaiClient !== 'function') {
    throw new Error('ESM dist export missing HaiClient');
  }
  console.log('  ESM dist: HaiClient OK');

  // 3. Binary wrapper exists
  const binWrapper = path.join(root, 'bin', 'haiai.cjs');
  if (!fs.existsSync(binWrapper)) {
    throw new Error('Binary wrapper bin/haiai.cjs not found');
  }
  console.log('  bin/haiai.cjs: exists');

  // 4. Package tarball structure verification
  const packed = run('npm', ['pack', '--silent'], root).stdout.trim();
  const tarballName = packed.split('\n').pop();
  if (!tarballName) {
    throw new Error('npm pack did not produce a tarball name');
  }

  const tarballPath = path.join(root, tarballName);
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'haiai-node-pack-'));
  const packageDir = path.join(tempDir, 'package');

  try {
    run('tar', ['-xzf', tarballPath, '-C', tempDir], root);

    // Verify expected files exist in the tarball
    const requiredFiles = [
      'package.json',
      'LICENSE',
      'README.md',
      'bin/haiai.cjs',
      'dist/cjs/index.js',
      'dist/esm/index.js',
      'dist/types/index.d.ts',
    ];
    for (const file of requiredFiles) {
      const fullPath = path.join(packageDir, file);
      if (!fs.existsSync(fullPath)) {
        throw new Error(`Missing file in package: ${file}`);
      }
    }
    console.log('  tarball: all required files present');

    // Source, tests, build tooling, and runtime-generated identity data must
    // never be published. The package.json `files` allowlist is the primary
    // boundary; this check makes release CI fail if that boundary regresses.
    const forbiddenPrefixes = [
      'src/',
      'tests/',
      'examples/',
      'scripts/',
      'jacs_data/',
      'private/',
      'var/',
      'npm/',
      'node_modules/',
    ];
    const forbiddenFiles = new Set([
      'jacs.config.json',
      'package-lock.json',
      'publish.deps.json',
      'tsconfig.json',
      'tsconfig.cjs.json',
      'vitest.config.ts',
    ]);
    const packagedFiles = [];
    const visit = (directory, prefix = '') => {
      for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
        const relative = path.posix.join(prefix, entry.name);
        if (entry.isDirectory()) {
          visit(path.join(directory, entry.name), relative);
        } else {
          packagedFiles.push(relative);
        }
      }
    };
    visit(packageDir);
    for (const file of packagedFiles) {
      if (
        forbiddenFiles.has(file)
        || forbiddenPrefixes.some((prefix) => file.startsWith(prefix))
      ) {
        throw new Error(`Forbidden file found in package: ${file}`);
      }
    }
    console.log('  tarball: source, tests, tooling, and generated data excluded');

    // Verify no stale CLI files in tarball
    const staleFiles = [
      'dist/esm/cli.js',
      'dist/cjs/cli.js',
      'dist/esm/mcp-server.js',
      'dist/cjs/mcp-server.js',
      'dist/types/cli.d.ts',
      'dist/types/mcp-server.d.ts',
      'dist/esm/crypt.js',
      'dist/cjs/crypt.js',
      'dist/types/crypt.d.ts',
      'dist/esm/hash.js',
      'dist/cjs/hash.js',
      'dist/types/hash.d.ts',
    ];
    for (const file of staleFiles) {
      const fullPath = path.join(packageDir, file);
      if (fs.existsSync(fullPath)) {
        throw new Error(`Stale file found in package: ${file} (source entry point was removed)`);
      }
    }
    console.log('  tarball: no stale removed entry points');

    // Verify package.json has correct fields
    const pkg = JSON.parse(fs.readFileSync(path.join(packageDir, 'package.json'), 'utf8'));
    if (pkg.name !== '@haiai/haiai') {
      throw new Error(`Wrong package name: ${pkg.name}`);
    }
    if (!pkg.bin || !pkg.bin.haiai) {
      throw new Error('Missing bin.haiai in package.json');
    }
    if (!pkg.dependencies?.haiinpm || !pkg.dependencies?.['@hai.ai/jacs']) {
      throw new Error('Missing required JACS or haiinpm native runtime dependency');
    }
    const requiredCliPackages = [
      '@haiai/cli-darwin-arm64',
      '@haiai/cli-darwin-x64',
      '@haiai/cli-linux-arm64',
      '@haiai/cli-linux-x64',
      '@haiai/cli-win32-x64',
    ];
    for (const dependency of requiredCliPackages) {
      if (!pkg.optionalDependencies?.[dependency]) {
        throw new Error(`Missing optional native CLI dependency: ${dependency}`);
      }
    }
    console.log('  tarball: package.json valid');

  } finally {
    try {
      fs.rmSync(tempDir, { recursive: true, force: true });
    } catch {
      // Best effort cleanup.
    }
    try {
      fs.unlinkSync(tarballPath);
    } catch {
      // Best effort cleanup.
    }
  }

  console.log('Smoke test passed.');
}

main().catch((error) => {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`${message}\n`);
  process.exit(1);
});
