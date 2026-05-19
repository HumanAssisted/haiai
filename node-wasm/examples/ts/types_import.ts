// HAIAI_WASM_PRD §3.1 / Task 034: validate that the public type
// surface is reachable via the `@haiai/wasm/types` subpath. `tsc
// --noEmit` failing on this file means a type was renamed / removed.
//
// In this repo we resolve the subpath as a relative path (we don't
// publish into node_modules from the source tree). The published
// package emits `types.{js,d.ts}` so external consumers do
// `import type { EmailMessage } from "@haiai/wasm/types"`.

import type {
  Algorithm,
  EmailMessage,
  HaiEvent,
  HelloResult,
  RegisterAgentOptions,
  RegistrationResult,
  SendEmailOptions,
  SendEmailResult,
  HaiaiWasmMetrics,
} from "../../types.js";

export function smokeTypes(): {
  alg: Algorithm;
  hello: HelloResult;
  reg: { opts: RegisterAgentOptions; res: RegistrationResult };
  email: { opts: SendEmailOptions; res: SendEmailResult; msg: EmailMessage };
  ev: HaiEvent;
  metrics: HaiaiWasmMetrics;
} {
  return {
    alg: "ed25519",
    hello: {
      timestamp: "",
      client_ip: "",
      hai_public_key_fingerprint: "",
      message: "",
      hai_signed_ack: "",
      hello_id: "",
    },
    reg: {
      opts: { agent_json: "{}" },
      res: {
        success: true,
        agent_id: "",
        jacs_id: "",
        dns_verified: false,
        registrations: [],
        registered_at: "",
      },
    },
    email: {
      opts: { to: "x@y", subject: "s", body: "b" },
      res: { message_id: "", status: "" },
      msg: {
        id: "",
        from_address: "",
        to_addresses: [],
        cc_addresses: [],
        bcc_addresses: [],
        subject: "",
        body_text: "",
        created_at: "",
        read: false,
        archived: false,
        labels: [],
      },
    },
    ev: { event_type: "ping", data: null, raw: "{}" },
    metrics: {
      httpRequestCount: 0,
      httpErrorCount: 0,
      signCount: 0,
      verifyCount: 0,
      sseEventsDelivered: 0,
      wsEventsDelivered: 0,
      lastHttpDurationMs: 0,
      lastSignDurationMs: 0,
      lastVerifyDurationMs: 0,
    },
  };
}
