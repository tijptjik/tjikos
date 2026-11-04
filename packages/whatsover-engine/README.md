# Whatsover Rust engine

Shared `oxidezap/whatsapp-rust` library for the
`apps/whatsover-cloudflare` Durable Object service. The SDK is pinned by Git
revision in Cargo.toml and dependencies are locked in Cargo.lock.

The library supplies durable inbox capture, immutable send intents, native SQLite
storage, and a WASM host adapter for the private Cloudflare bridge.


- Native auth, Signal state, inbox, and outbox share the SDK SQLite connection
  pool, using synchronous FULL commits.
- Encrypted inbound direct/group messages are saved by the SDK's durability hook
  before acknowledgement. Deduplication uses `(chat, sender, message ID)`.
- A send reserves its request ID durably before attempting network I/O. Reusing
  the ID with the same input returns the saved result; different input is rejected.
- `submitted` only means the SDK send call completed. Delivery and read receipts
  are tracked separately.
- Interrupted attempts become `uncertain` on restart. They are not automatically
  resent. Inspect the recipient conversation before choosing a new request ID.
- Channel/newsletter messages and historical archive import have separate SDK
  capture paths and are not currently part of this inbox contract.

## Verification

```sh
cargo test --manifest-path packages/whatsover-engine/Cargo.toml --locked
```

The Cloudflare workspace tests additionally run the compiled Rust backend inside
workerd, exercising the actual JS/SQLite boundary, conditional buffer operations,
state merges, full SDK startup, and Durable Object eviction recovery.
