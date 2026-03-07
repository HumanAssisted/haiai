 ---                                                                                                                                                                                                                                                                                                                                                                                                                                                        
  1. Auth Header — identical logic, 4 implementations                                                                                                                                                                                                                                                                                                                                                                                                        
                                                                                                                                                                                                                                                                                                                                                                                                                                                             
  Rust rust/haiai/src/client.rs:150-154                                                                                                                                                                                                                                                                                                                                                                                                                      
  let ts = OffsetDateTime::now_utc().unix_timestamp();                                                                                                                                                                                                                                                                                                                                                                                                       
  let message = format!("{}:{ts}", self.jacs.jacs_id());                                                                                                                                                                                                                                                                                                                                                                                                     
  let signature = self.jacs.sign_string(&message)?;                                                                                                                                                                                                                                                                                                                                                                                                          
  Ok(format!("JACS {}:{ts}:{signature}", self.jacs.jacs_id()))                                                                                                                                                                                                                                                                                                                                                                                               

  Python python/src/jacs/hai/client.py:221-239
  timestamp = int(time.time())
  message = f"{cfg.jacs_id}:{timestamp}"
  signature = agent.sign_string(message)
  return f"JACS {cfg.jacs_id}:{timestamp}:{signature}"

  Node node/src/client.ts:238-245
  const timestamp = Math.floor(Date.now() / 1000).toString();
  const message = `${this.jacsId}:${timestamp}`;
  const signature = this.agent.signStringSync(message);
  return `JACS ${this.jacsId}:${timestamp}:${signature}`;

  Go go/auth.go:73-86
  timestamp := strconv.FormatInt(time.Now().Unix(), 10)
  message := authHeaderMessage(c.jacsID, timestamp)
  sigB64, err := c.crypto.SignString(message)
  return fmt.Sprintf("JACS %s:%s:%s", jacsID, timestamp, signatureB64)

  ---
  2. Sign Response Envelope — identical document structure, 3 implementations

  Rust rust/haiai/src/jacs.rs:199-227
  let doc = serde_json::json!({
      "version": "1.0.0",
      "document_type": "job_response",
      "data": data,
      "metadata": { "issuer": self.jacs_id, "document_id": ..., "created_at": now, "hash": hash },
      "jacsSignature": { "agentID": self.jacs_id, "date": now, "signature": signature },
  });

  Python python/src/jacs/hai/signing.py:339-389
  jacs_doc = {
      "version": "1.0.0",
      "document_type": "job_response",
      "data": sorted_data,
      "metadata": { "issuer": jacs_id, "document_id": doc_id, "created_at": now, "hash": payload_hash },
      "jacsSignature": { "agentID": jacs_id, "date": now, "signature": signature },
  }

  Node node/src/signing.ts:168-206
  const jacsDoc = {
      version: '1.0.0',
      document_type: 'job_response',
      data: sortedData,
      metadata: { issuer: jacsId, document_id: documentId, created_at: now, hash },
      jacsSignature: { agentID: jacsId, date: now, signature },
  };

  ---
  3. Canonical JSON — 3 implementations

  Rust rust/haiai/src/jacs.rs:237-239 — uses serde_json_canonicalizer (RFC 8785)
  Python python/src/jacs/hai/signing.py:37-42 — json.dumps(obj, sort_keys=True, separators=(",",":"))
  Node node/src/signing.ts:57-68 — recursive key-sorting JSON.stringify replacer

  ---
  4. Verify Link Generation — 3 implementations

  Same base64url encoding + URL construction + length check + hosted doc ID extraction in:
  - Rust rust/haiai/src/verify.rs:9-33
  - Python python/src/jacs/hai/client.py:3613-3677
  - Node node/src/verify.ts:41-63

  ---
  5. Unwrap Signed Events — 2 full implementations

  Event signature verification with server public key lookup, canonical JSON, fallback to hash verification:
  - Python python/src/jacs/hai/signing.py:270-331
  - Node node/src/signing.ts:80-157

  ---
  Summary

  ┌────────────────────────┬────────┬──────┬─────┬─────────┬─────────────────────────────┐
  │        Pattern         │ Python │ Node │ Go  │  Rust   │         Belongs in          │
  ├────────────────────────┼────────┼──────┼─────┼─────────┼─────────────────────────────┤
  │ Auth header            │ yes    │ yes  │ yes │ yes     │ JACS                        │
  ├────────────────────────┼────────┼──────┼─────┼─────────┼─────────────────────────────┤
  │ Sign response envelope │ yes    │ yes  │ —   │ yes     │ JACS                        │
  ├────────────────────────┼────────┼──────┼─────┼─────────┼─────────────────────────────┤
  │ Canonical JSON         │ yes    │ yes  │ —   │ yes     │ JACS (already has RFC 8785) │
  ├────────────────────────┼────────┼──────┼─────┼─────────┼─────────────────────────────┤
  │ Verify link generation │ yes    │ yes  │ —   │ yes     │ JACS                        │
  ├────────────────────────┼────────┼──────┼─────┼─────────┼─────────────────────────────┤
  │ Unwrap signed events   │ yes    │ yes  │ —   │ partial │ JACS                        │
  └────────────────────────┴────────┴──────┴─────┴─────────┴─────────────────────────────┘
