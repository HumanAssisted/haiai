// Assemble the four release addons before npm can flatten or omit an artifact.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { execFileSync } = require('node:child_process');

const platforms = [
  { platform: 'darwin', arch: 'arm64', triple: 'darwin-arm64' },
  { platform: 'darwin', arch: 'x64', triple: 'darwin-x64' },
  { platform: 'linux', arch: 'x64', triple: 'linux-x64-gnu' },
  { platform: 'win32', arch: 'x64', triple: 'win32-x64-msvc' },
];
const filename = ({ triple }) => `haiinpm.${triple}.node`;
const expectedFiles = ['index.js', 'package.json', ...platforms.map(filename)].sort();

function nonemptyFile(file) {
  const stat = fs.lstatSync(file);
  assert(stat.isFile() && stat.size > 0, `Expected a nonempty regular file: ${file}`);
}

function validatePackage(dir) {
  assert.deepEqual(fs.readdirSync(dir).sort(), expectedFiles, 'Unexpected or missing package files');
  for (const name of expectedFiles) nonemptyFile(path.join(dir, name));
  const pkg = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'));
  assert.equal(pkg.name, 'haiinpm');
  assert.equal(pkg.main, 'index.js');
  // Execute the production loader with each supported OS/arch. Replace only
  // native loading with a filename sentinel: synthetic bytes are never dlopened.
  for (const target of platforms) {
    const context = {
      __dirname: path.resolve(dir),
      module: { exports: {} },
      require(name) {
        if (name === 'path') return path;
        if (name === 'os') return { platform: () => target.platform, arch: () => target.arch };
        assert.equal(name, path.resolve(dir, filename(target)), `Loader selected an unexpected addon for ${target.triple}`);
        nonemptyFile(name);
        return name;
      },
    };
    vm.runInNewContext(fs.readFileSync(path.join(dir, 'index.js'), 'utf8'), context, { timeout: 1000 });
    assert.equal(context.module.exports, path.resolve(dir, filename(target)));
  }
}

function assemble(artifactsDir, templateDir, destination) {
  assert(!fs.existsSync(destination), `Package staging path already exists: ${destination}`);
  const found = new Map();
  const directories = fs.readdirSync(artifactsDir, { withFileTypes: true });
  for (const directory of directories) {
    assert(directory.isDirectory(), `Unexpected artifact entry: ${directory.name}`);
    for (const entry of fs.readdirSync(path.join(artifactsDir, directory.name), { withFileTypes: true })) {
      assert(platforms.some(target => filename(target) === entry.name), `Unexpected or unqualified artifact: ${entry.name}`);
      assert(!found.has(entry.name), `Duplicate platform artifact: ${entry.name}`);
      const source = path.join(artifactsDir, directory.name, entry.name);
      nonemptyFile(source);
      found.set(entry.name, { source, directory: directory.name });
    }
  }
  for (const target of platforms) {
    const artifact = found.get(filename(target));
    assert(artifact, `Missing platform artifact: ${filename(target)}`);
    assert.equal(artifact.directory, `haiinpm-${target.triple}`, `Artifact in wrong platform directory: ${filename(target)}`);
  }
  assert.deepEqual(directories.map(entry => entry.name).sort(), platforms.map(target => `haiinpm-${target.triple}`).sort(), 'Unexpected artifact directories');
  assert.deepEqual(fs.readdirSync(templateDir).sort(), ['index.js', 'package.json'], 'Package template must not contain stale native artifacts or unexpected files');

  // Validate every input before creating a staging directory. Publish uses only
  // the archive produced below, never the unvalidated download directory.
  fs.mkdirSync(path.dirname(path.resolve(destination)), { recursive: true });
  const temporary = fs.mkdtempSync(path.join(path.dirname(path.resolve(destination)), '.haiinpm-stage-'));
  try {
    for (const name of ['index.js', 'package.json']) fs.copyFileSync(path.join(templateDir, name), path.join(temporary, name));
    for (const [name, artifact] of found) fs.copyFileSync(artifact.source, path.join(temporary, name));
    validatePackage(temporary);
    fs.renameSync(temporary, destination);
  } finally {
    fs.rmSync(temporary, { recursive: true, force: true });
  }
}

function packAndValidate(packageDir, outputDir) {
  validatePackage(packageDir);
  fs.mkdirSync(outputDir, { recursive: true });
  const packed = JSON.parse(execFileSync('npm', [
    'pack', '--offline', '--ignore-scripts', '--json', '--pack-destination', path.resolve(outputDir),
  ], { cwd: packageDir, encoding: 'utf8' }));
  assert.equal(packed.length, 1, 'Expected one npm archive');
  assert.equal(path.basename(packed[0].filename), packed[0].filename, 'Unexpected npm archive path');
  const archive = path.resolve(outputDir, packed[0].filename);
  const extracted = fs.mkdtempSync(path.join(path.resolve(outputDir), '.haiinpm-unpack-'));
  try {
    execFileSync('tar', ['-xzf', archive, '-C', extracted]);
    const unpacked = path.join(extracted, 'package');
    validatePackage(unpacked);
    for (const name of expectedFiles) {
      assert.deepEqual(fs.readFileSync(path.join(unpacked, name)), fs.readFileSync(path.join(packageDir, name)), `npm archive changed ${name}`);
    }
  } catch (error) {
    fs.rmSync(archive, { force: true });
    throw error;
  } finally {
    fs.rmSync(extracted, { recursive: true, force: true });
  }
  return archive;
}

module.exports = { platforms, filename, assemble, validatePackage, packAndValidate };

if (require.main === module) {
  try {
    const [artifacts, template, output, ...extra] = process.argv.slice(2);
    assert(artifacts && template && output && extra.length === 0,
      'Usage: node native-package.cjs ARTIFACTS_DIR TEMPLATE_DIR OUTPUT_DIR');
    const stage = path.resolve(output, 'package');
    assemble(artifacts, template, stage);
    console.log(`Validated native package: ${packAndValidate(stage, output)}`);
  } catch (error) {
    console.error(`Native package validation failed: ${error.message}`);
    process.exitCode = 1;
  }
}
