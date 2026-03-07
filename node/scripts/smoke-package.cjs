#!/usr/bin/env node
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const npmCacheDir = path.join(os.tmpdir(), 'haisdk-npm-cache');

try {
  fs.mkdirSync(npmCacheDir, { recursive: true });
} catch {
  // Best effort.
}

function run(cmd, args, cwd) {
  const result = spawnSync(cmd, args, {
    cwd,
    encoding: 'utf8',
    env: {
      ...process.env,
      npm_config_cache: npmCacheDir,
    },
  });
  if (result.status !== 0) {
    const output = `${result.stdout || ''}${result.stderr || ''}`;
    throw new Error(`Command failed (${cmd} ${args.join(' ')}):\n${output}`);
  }
  return result;
}

async function main() {
  const root = path.resolve(__dirname, '..');

  // Dist-level checks (before packing/installing)
  const cjs = require(path.join(root, 'dist', 'cjs', 'index.js'));
  if (typeof cjs.HaiClient !== 'function') {
    throw new Error('CJS dist export missing HaiClient');
  }

  const esm = await import(pathToFileURL(path.join(root, 'dist', 'esm', 'index.js')).href);
  if (typeof esm.HaiClient !== 'function') {
    throw new Error('ESM dist export missing HaiClient');
  }

  const cliHelp = run('node', [path.join(root, 'dist', 'esm', 'cli.js'), '--help'], root);
  if (!(`${cliHelp.stdout}${cliHelp.stderr}`).includes('Usage: haisdk')) {
    throw new Error('CLI help output did not include expected usage text');
  }

  // Packaged-artifact checks (matches npm publish payload)
  const packed = run('npm', ['pack', '--silent'], root).stdout.trim();
  const tarballName = packed.split('\n').pop();
  if (!tarballName) {
    throw new Error('npm pack did not produce a tarball name');
  }

  const tarballPath = path.join(root, tarballName);
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'haisdk-node-pack-'));
  const packageDir = path.join(tempDir, 'package');

  try {
    run('tar', ['-xzf', tarballPath, '-C', tempDir], root);

    run(
      'node',
      ['-e', `const sdk=require(${JSON.stringify(packageDir)}); if (typeof sdk.HaiClient !== 'function') throw new Error('Missing HaiClient in CJS package');`],
      root,
    );
    run(
      'node',
      ['-e', `import(${JSON.stringify(pathToFileURL(path.join(packageDir, 'dist', 'esm', 'index.js')).href)}).then((sdk)=>{ if (typeof sdk.HaiClient !== 'function') throw new Error('Missing HaiClient in ESM package'); }).catch((err)=>{ console.error(err); process.exit(1); });`],
      root,
    );

    const installedCliHelp = run(
      'node',
      [path.join(packageDir, 'dist', 'esm', 'cli.js'), '--help'],
      root,
    );
    if (!(`${installedCliHelp.stdout}${installedCliHelp.stderr}`).includes('Usage: haisdk')) {
      throw new Error('Packaged CLI help output did not include expected usage text');
    }
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
}

main().catch((error) => {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`${message}\n`);
  process.exit(1);
});
