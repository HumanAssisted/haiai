#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');

// TypeScript does not remove outputs for deleted source files. Always compile
// into an empty directory so removed Node CLI/MCP/crypto shims cannot leak
// into a later release tarball from a developer's previous build.
fs.rmSync(path.resolve(__dirname, '..', 'dist'), {
  recursive: true,
  force: true,
});
