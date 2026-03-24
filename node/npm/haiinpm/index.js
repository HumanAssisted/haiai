// Platform-appropriate .node native addon loader for haiinpm
const path = require('path');
const os = require('os');

function getBinding() {
  const platform = os.platform();
  const arch = os.arch();

  // Map Node os.platform()/os.arch() to the napi-rs triple used in .node filenames.
  // napi-rs uses its own naming convention for the compiled binaries:
  //   haiinpm.<napi-triple>.node
  // where <napi-triple> depends on the Rust target:
  //   aarch64-apple-darwin     -> darwin-arm64
  //   x86_64-apple-darwin      -> darwin-x64
  //   x86_64-unknown-linux-gnu -> linux-x64-gnu
  //   x86_64-pc-windows-msvc   -> win32-x64-msvc
  const tripleMap = {
    'darwin-arm64': 'darwin-arm64',
    'darwin-x64': 'darwin-x64',
    'linux-x64': 'linux-x64-gnu',
    'win32-x64': 'win32-x64-msvc',
  };

  const key = `${platform}-${arch}`;
  const napiTriple = tripleMap[key];

  const candidates = [];
  if (napiTriple) {
    candidates.push(path.join(__dirname, `haiinpm.${napiTriple}.node`));
  }
  // Fallback: unqualified name (useful for local dev builds)
  candidates.push(path.join(__dirname, 'haiinpm.node'));

  for (const candidate of candidates) {
    try {
      return require(candidate);
    } catch (_) {
      continue;
    }
  }

  throw new Error(
    `Failed to load haiinpm native binding for ${platform}-${arch}. ` +
    `Tried: ${candidates.join(', ')}. ` +
    'Ensure the native addon is built for your platform.'
  );
}

module.exports = getBinding();
