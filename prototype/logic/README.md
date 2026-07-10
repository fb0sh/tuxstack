# LOGIC Prototype: daemon JSON-RPC protocol

**Question:** Does the Unix socket JSON-RPC protocol (line-based) handle
streaming responses (logs, events), concurrent clients, and error recovery?

**When done:** capture findings in NOTES.md, then delete this dir.

## Run

```bash
cd daemon && cargo run --bin prototype-logic
```

Uses a temp Unix socket at `/tmp/tuxstack-prototype.sock`.
