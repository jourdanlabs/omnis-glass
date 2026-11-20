# OMNIS GLASS

Governed browser runtime — fork of [Obscura](https://github.com/h4ckf0r0day/obscura) with TERMINUS egress gate, `web.capture` ingress provenance, and `browser.action` receipt ledger.

> OMNIS GLASS gates every agent-directed browser action through TERMINUS before it runs, stamps every captured byte with web provenance, and hash-chains a receipt of what the agent did — without raw form values, cookies, or auth headers ever entering the ledger.

Upstream engine: Obscura (Apache 2.0). See `docs/UPSTREAM-POLICY.md`.

## Quick start

```bash
cargo build --release -p obscura-cli --bin omnis-glass
./target/release/omnis-glass fetch https://example.com --dump html
./target/release/omnis-glass receipts status
```

## Governance

- **Egress:** `scripts/terminus-authorize.mjs` (`kind: browser`) — ALLOW / HOLD / REFUSE before CDP side effects
- **Ingress:** `web.capture` receipts with structural `capture_kind`
- **Ledger:** `browser.action` hash-chain in `$OMNIS_GLASS_STATE_DIR` (default `~/.omnis-glass`)

## Lanes

Configure via `browser-lanes.yaml` or `OMNIS_GLASS_LANES`. Default lane: `browse:research`.

## License

Business Source License 1.1 (see `LICENSE`). You may use OMNIS GLASS in production, including inside your own organization, except to offer it, or something built from it, to others in competition with Forseti Research. Each version converts to Apache 2.0 four years after it is published. Version 1.0 stays under Apache 2.0.

Code from the upstream Obscura project stays under Apache 2.0 (`LICENSE-APACHE-2.0`), with its attribution in `NOTICE`. The `obscura-dom` and `obscura` crates are upstream code and keep the Apache 2.0 licence.
