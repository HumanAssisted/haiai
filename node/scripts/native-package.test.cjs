const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const { test } = require('node:test');
const { platforms, filename, assemble, validatePackage, packAndValidate } = require('./native-package.cjs');
const template = path.resolve(__dirname, '../npm/haiinpm');

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'haiinpm-pack-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const artifacts = path.join(root, 'artifacts');
  const stage = path.join(root, 'stage');
  const bytes = new Map();
  for (const target of platforms) {
    const directory = path.join(artifacts, `haiinpm-${target.triple}`);
    fs.mkdirSync(directory, { recursive: true });
    const content = Buffer.from(`synthetic artifact for ${target.triple}\n`);
    fs.writeFileSync(path.join(directory, filename(target)), content);
    bytes.set(filename(target), content);
  }
  return { root, artifacts, stage, bytes };
}

const artifactPath = (f, target = platforms[0]) => path.join(f.artifacts, `haiinpm-${target.triple}`, filename(target));

test('four unique artifacts survive real npm pack/extraction and the production loader selects each name', t => {
  const f = fixture(t);
  const output = path.join(f.root, 'packed');
  // Exercise the same entry point as the release job, not just its helpers.
  execFileSync(process.execPath, [path.join(__dirname, 'native-package.cjs'), f.artifacts, template, output]);
  const archives = fs.readdirSync(output).filter(name => name.endsWith('.tgz'));
  assert.equal(archives.length, 1);
  const archive = path.join(output, archives[0]);
  const consumer = path.join(f.root, 'consumer');
  fs.mkdirSync(consumer);
  execFileSync('tar', ['-xzf', archive, '-C', consumer]);
  const pkg = path.join(consumer, 'package');
  validatePackage(pkg);
  for (const [name, bytes] of f.bytes) assert.deepEqual(fs.readFileSync(path.join(pkg, name)), bytes);
  assert.deepEqual(fs.readFileSync(path.join(pkg, 'index.js')), fs.readFileSync(path.join(template, 'index.js')));
});

for (const [name, mutate, expected] of [
  ['missing', f => fs.rmSync(artifactPath(f)), /Missing platform artifact/],
  ['duplicate', f => {
    const extra = path.join(f.artifacts, 'haiinpm-duplicate');
    fs.mkdirSync(extra);
    fs.copyFileSync(artifactPath(f), path.join(extra, filename(platforms[0])));
  }, /Duplicate platform artifact/],
  ['unqualified', f => fs.renameSync(artifactPath(f), path.join(path.dirname(artifactPath(f)), 'haiinpm.node')), /Unexpected or unqualified artifact/],
  ['unexpected platform', f => fs.writeFileSync(path.join(path.dirname(artifactPath(f)), 'haiinpm.linux-arm64-gnu.node'), 'unexpected'), /Unexpected or unqualified artifact/],
  ['empty', f => fs.truncateSync(artifactPath(f)), /nonempty regular file/],
  ['wrong platform directory', f => fs.renameSync(path.dirname(artifactPath(f)), path.join(f.artifacts, 'haiinpm-wrong')), /wrong platform directory/],
  ['extra empty directory', f => fs.mkdirSync(path.join(f.artifacts, 'haiinpm-extra')), /Unexpected artifact directories/],
]) {
  test(`rejects ${name} artifacts before package staging succeeds`, t => {
    const f = fixture(t);
    mutate(f);
    assert.throws(() => assemble(f.artifacts, template, f.stage), expected);
    assert(!fs.existsSync(f.stage), 'invalid inputs must not produce a staged package');
  });
}

test('npm files rules that omit native addons fail archive validation', t => {
  const f = fixture(t);
  assemble(f.artifacts, template, f.stage);
  const pkgPath = path.join(f.stage, 'package.json');
  const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
  pkg.files = ['index.js'];
  fs.writeFileSync(pkgPath, JSON.stringify(pkg));
  const output = path.join(f.root, 'packed');
  assert.throws(() => packAndValidate(f.stage, output), /Unexpected or missing package files/);
  assert(!fs.readdirSync(output).some(name => name.endsWith('.tgz')), 'invalid archive must not be publishable');
});

test('loader drift is rejected even when all four correctly named files exist', t => {
  const f = fixture(t);
  const changedTemplate = path.join(f.root, 'template');
  fs.cpSync(template, changedTemplate, { recursive: true });
  const loader = path.join(changedTemplate, 'index.js');
  fs.writeFileSync(loader, fs.readFileSync(loader, 'utf8').replace("'linux-x64-gnu'", "'linux-x64-musl'"));
  assert.throws(() => assemble(f.artifacts, changedTemplate, f.stage), /Failed to load haiinpm native binding/);
  assert(!fs.existsSync(f.stage));
});
