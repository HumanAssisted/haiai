import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    globals: true,
    environment: 'node',
    include: ['tests/**/*.test.ts'],
    setupFiles: ['tests/setup.ts'],
    testTimeout: 10000,
    // Native FFI calls (`haiinpm`) read env vars via Rust's `std::env::var`,
    // which inspects the OS-level environment block. vitest's default `threads`
    // pool wraps env vars in a per-worker JS proxy that the native side does
    // not see, so any test that constructs a real `haiinpm.HaiClient` and
    // passes `JACS_PRIVATE_KEY_PASSWORD` via `process.env` MUST run in the
    // `forks` pool. Add to this glob list when adding more native FFI tests.
    poolMatchGlobs: [
      ['**/tests/cross-lang-media.test.ts', 'forks'],
      ['**/tests/sign-image.test.ts', 'forks'],
      ['**/tests/sign-text.test.ts', 'forks'],
    ],
  },
});
