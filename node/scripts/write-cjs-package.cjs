#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');

const cjsDir = path.resolve(__dirname, '..', 'dist', 'cjs');
const packagePath = path.join(cjsDir, 'package.json');

fs.mkdirSync(cjsDir, { recursive: true });
fs.writeFileSync(packagePath, JSON.stringify({ type: 'commonjs' }, null, 2) + '\n', 'utf8');
