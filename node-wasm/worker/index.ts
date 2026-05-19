// @haiai/wasm/worker — Web Worker bridge (HAIAI_WASM_PRD §3.1 /
// Task 035). Long-running ops (multi-MB email send with attachments,
// pq2025 keygen, raw-MIME hashing) can run inside a Web Worker so the
// main thread stays responsive.
//
// Pattern mirrors `@jacs/wasm/worker`: callers spawn the worker
// (browser bundler treats `./haiai-worker.js` as a Web Worker entry),
// then post `{ id, op, args }` requests and receive `{ id, ok, value }`
// or `{ id, ok: false, error: { code, message, details? } }` replies.

import type {
  Algorithm,
  HaiaiWasmErrorPayload,
  SendEmailOptions,
} from "../types.js";

let nextId = 1;

interface PendingResolver {
  resolve: (value: unknown) => void;
  reject: (err: HaiaiWasmErrorPayload) => void;
}

/** Wire protocol op names. Keep in sync with `worker/haiai-worker.js`. */
export type WorkerOp =
  | "init"
  | "createEphemeral"
  | "importEncrypted"
  | "sign"
  | "verify"
  | "sendSignedEmail"
  | "signEmailRaw";

export interface WorkerInitOptions {
  /** Optional override for the haiai-wasm module path; defaults to the
   * built-in URL the worker bundle resolves to. */
  wasmUrl?: string;
  /** HAI API base URL. */
  baseUrl?: string;
}

/**
 * Spawn a `@haiai/wasm` Web Worker and return a typed proxy. The
 * worker runs in its own JS realm — main-thread state (including
 * `globalThis.HAIAI_WASM_DEBUG`) is NOT shared.
 *
 * Disconnect handling (JACS_WASM ISSUE 011 lesson): when the worker
 * terminates mid-request, pending promises reject with
 * `{ code: "WorkerDisconnected", message: "worker terminated" }`.
 */
export function createBrowserAgentWorker(opts: WorkerInitOptions = {}): BrowserAgentWorkerProxy {
  const worker = new Worker(new URL("./haiai-worker.js", import.meta.url), {
    type: "module",
  });

  const pending = new Map<number, PendingResolver>();

  worker.addEventListener("message", (evt: MessageEvent) => {
    const data = evt.data as { id: number; ok: boolean; value?: unknown; error?: HaiaiWasmErrorPayload };
    const resolver = pending.get(data.id);
    if (!resolver) return;
    pending.delete(data.id);
    if (data.ok) {
      resolver.resolve(data.value);
    } else {
      resolver.reject(
        data.error ?? { code: "Internal", message: "worker returned no error payload" },
      );
    }
  });

  worker.addEventListener("error", () => {
    for (const [, resolver] of pending) {
      resolver.reject({ code: "WorkerDisconnected", message: "worker terminated" });
    }
    pending.clear();
  });

  const post = <T>(op: WorkerOp, args: unknown): Promise<T> => {
    const id = nextId++;
    return new Promise<T>((resolve, reject) => {
      pending.set(id, {
        resolve: resolve as (v: unknown) => void,
        reject: (err) => reject(err),
      });
      worker.postMessage({ id, op, args });
    });
  };

  // Eager init — caller can `await proxy.ready` before issuing other ops.
  const ready = post<void>("init", opts);

  return new BrowserAgentWorkerProxyImpl(worker, post, ready);
}

export interface BrowserAgentWorkerProxy {
  /** Resolves when the worker has loaded the wasm module + called `initHaiaiWasm`. */
  readonly ready: Promise<void>;

  createEphemeral(algorithm: Algorithm): Promise<{ jacsId: string; algorithm: Algorithm }>;
  importEncrypted(materialJson: string, password: string): Promise<{ jacsId: string }>;
  sign(payload: unknown): Promise<unknown>;
  verify(signed: unknown): Promise<unknown>;
  sendSignedEmail(options: SendEmailOptions): Promise<unknown>;
  signEmailRaw(rawEmailB64: string): Promise<string>;

  /** Terminate the worker. Pending requests reject with `WorkerDisconnected`. */
  terminate(): void;
}

class BrowserAgentWorkerProxyImpl implements BrowserAgentWorkerProxy {
  constructor(
    private readonly worker: Worker,
    private readonly post: <T>(op: WorkerOp, args: unknown) => Promise<T>,
    public readonly ready: Promise<void>,
  ) {}

  createEphemeral(algorithm: Algorithm) {
    return this.post<{ jacsId: string; algorithm: Algorithm }>("createEphemeral", { algorithm });
  }
  importEncrypted(materialJson: string, password: string) {
    return this.post<{ jacsId: string }>("importEncrypted", { materialJson, password });
  }
  sign(payload: unknown) {
    return this.post<unknown>("sign", payload);
  }
  verify(signed: unknown) {
    return this.post<unknown>("verify", signed);
  }
  sendSignedEmail(options: SendEmailOptions) {
    return this.post<unknown>("sendSignedEmail", options);
  }
  signEmailRaw(rawEmailB64: string) {
    return this.post<string>("signEmailRaw", { rawEmailB64 });
  }

  terminate() {
    this.worker.terminate();
  }
}
