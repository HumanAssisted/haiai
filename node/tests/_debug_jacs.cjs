const { JacsClient } = require('@hai.ai/jacs/client');
const fs = require('fs');
const path = require('path');
const os = require('os');

const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'jacs-test-'));

process.env.JACS_PRIVATE_KEY_PASSWORD = 'Xk9#mP2vL7qR4nB8wZ';

try {
  const client = new JacsClient();
  const info = client.createSync({
    name: 'test-agent',
    password: 'Xk9#mP2vL7qR4nB8wZ',
    algorithm: 'ring-Ed25519',
    dataDirectory: path.join(tmpDir, 'data'),
    keyDirectory: path.join(tmpDir, 'keys'),
    configPath: path.join(tmpDir, 'jacs.config.json'),
  });
  console.log('createSync succeeded! Info:', info);

  // Try signing
  const sig = client.sign('hello world');
  console.log('Signed:', sig.substring(0, 40) + '...');
} catch(e) {
  console.log('Error:', e.message);
}

delete process.env.JACS_PRIVATE_KEY_PASSWORD;
