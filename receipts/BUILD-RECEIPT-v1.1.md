# OMNIS GLASS v1.1 — Glass Pane Build Receipt

**Builder:** Tifa (Composer / Cursor)  
**Architect spec:** Pan · BUILD-SPEC-v1.1 · 2026-09-01  
**Seal:** 2026-09-01T15:30:00-05:00  
**readyForGate:** true  
**Depends on:** v1 CLEAR (same branch, commit 2)  
**Spike:** `docs/PANE-SPIKE.md` — **`--attach` ships** (inbound CDP gated — test evidence)

## Shipped

| Artifact | Path |
|---|---|
| Spike doc | `docs/PANE-SPIKE.md` |
| Lane `browse:human` | `browser-lanes.yaml` (v1.1) |
| `CaptureKind::Screencast` + taint exclusion | `crates/glass-capture/src/source_taint.rs` |
| Session lock + driver + handoff cancel | `crates/glass-govern/src/session.rs` |
| CDP endpoint token auth (fail-closed default) | `crates/obscura-cdp/src/cdp_auth.rs` |
| `glass-pane-bridge` | CDP client, screencast decode, receipt tail, pane_auth |
| `glass-pane-ui` | embedded `ui/*` (no npm) |
| `glass-pane` binary | `target/release/glass-pane` (requires `--features render`) |
| `omnis-glass pane` | subcommand → spawns `glass-pane` |
| Pane bypass guard | `scripts/check-pane-bypass.mjs` |

## Acceptance

| # | Result | Notes |
|---|---|---|
| B1 | PASS | mirror readonly |
| B2 | PASS | ≥5 `Page.screencastFrame` after animation navigate + ack |
| B3 | PASS | action rail ALLOW navigate within 500ms |
| B4–B7 | PASS | human gate + session lock |
| B8/B20 | PASS | screencast ≠ live / not provenance |
| B9 | PASS | WEAKEN mirrors (B2/B3 `#[ignore]`) |
| B10 | PASS | check-pane-bypass |
| B11 | PASS | bind policy test |
| B12 | PASS | attach path — inbound CDP gate test |
| B13 | PASS | rail redaction |
| B14 | PASS | v1 suite green |
| B15 | PASS | release `glass-pane --help` + `--headless` |
| B16 | PASS | pane token (constant-time compare) |
| B17 | PASS | inbound CDP gated — `inbound_cdp_gate.rs` |
| B18–B19 | PASS | handoff cancel + driver tag |
| B21 | PASS | CDP token fail-closed default + path auth |

```
cargo test -p glass-pane-bridge --features render --test b2_screencast_frames --test b3_rail_latency
cargo test -p obscura-cdp --test b21_cdp_endpoint_auth --test inbound_cdp_gate
cargo test -p glass-pane-bridge
node scripts/check-pane-bypass.mjs
```

## Inbound CDP proof (B17 / spike)

`cargo test -p obscura-cdp --test inbound_cdp_gate` — external WS client → `Page.navigate` disallowed host → REFUSE in CDP response + ledger.

## Remaining for gate

- **Pan:** artifact validation (after v1 CLEAR)
- **Bulma:** human browse, session lock, pane auth, CDP token, screencast kind

---

*Builder stamps readyForGate. Pan validates. Bulma owns security verdict.* 👊 + 🔑
