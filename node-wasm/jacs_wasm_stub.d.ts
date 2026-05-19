// Minimal `@jacs/wasm` type stub for `tsc --noEmit` from a fresh
// checkout (no node_modules). Mirrors only the surface `index.ts`
// imports — `localStore.{saveEncryptedAgent,loadEncryptedAgent,...}`.
//
// The real types ship with the `@jacs/wasm` npm package; this stub
// exists exclusively so the IDE / CI TS check works without
// `npm install`. Keep in sync with `jacs-wasm/index.ts::localStore`.

declare module "@jacs/wasm" {
  export const localStore: {
    saveEncryptedAgent(key: string, materialJson: string): void;
    loadEncryptedAgent(key: string): string | null;
    saveDocument(key: string, signedJson: string): void;
    loadDocument(key: string): string | null;
    listKeys(prefix?: string): string[];
    remove(key: string): void;
    clearAll(): void;
  };
}
