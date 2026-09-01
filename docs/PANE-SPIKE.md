# Glass Pane — Phase 0 Spike

**Date:** 2026-09-01 · **Builder:** Tifa · **Spec:** BUILD-SPEC-v1.1.md (Pan gate)  
**Re-sealed:** 2026-09-01 after Pan NOT CLEAR (inbound CDP evidence required)

## Inbound CDP gate — exercised proof (not code trace alone)

**Question:** Does v1's `gate_cdp_command()` fire on commands received over the CDP WebSocket from an external client?

**Test:** `crates/obscura-cdp/tests/inbound_cdp_gate.rs` — `inbound_cdp_websocket_navigate_disallowed_host_refused`

**Procedure:**
1. Start `omnis-glass serve` on `127.0.0.1:{port}` with CDP token `spike-inbound-cdp-token-fixed-value`.
2. Connect as **external WebSocket client** (tokio-tungstenite) to  
   `ws://127.0.0.1:{port}/devtools/browser/{token}` — not in-process `dispatch()`.
3. `Target.createTarget` → obtain `sessionId`.
4. Send **`Page.navigate`** to `https://google.com` (not on `browse:research` allowlist).

**Observed gate decision:**
- CDP response `error.message` contains **`REFUSE`** / **`TERMINUS`** (host not on lane allowlist).
- **`browser.actions.jsonl`** in `$OMNIS_GLASS_STATE_DIR` contains a line with `"verdict":"REFUSE"` and `"action":"navigate"`.

**Conclusion:** Inbound CDP over WebSocket hits the same chokepoint as in-process dispatch (`server.rs:process_cdp_message` → `dispatch::dispatch` → `gate_cdp_command`). **`--attach` ships in v1.1.**

```
cargo test -p obscura-cdp --test inbound_cdp_gate -- --nocapture
```

## Other spike answers

| # | Question | Result | Decision |
|---|---|---|---|
| 1 | Live frames without MCP poll? | `Page.startScreencast` → `Page.screencastFrame` | **Use CDP screencast** — B2 test |
| 3 | CDP endpoint auth? | v1 had open WS; v1.1 adds path token | **Required by default** (`cdp_auth_required()` → `true`) |
| 4 | Co-launch single Page? | `glass-pane --co-launch` | **Default mode** |
| 5 | Human navigate bypass? | `gate_pane_navigate` before CDP | **Enforced in bridge** |
| 6 | Screencast stealth? | Observer-only raster | **No extra DOM APIs** |

## B2 / B3 (un-stubbed)

- **B2:** `crates/glass-pane-bridge/tests/b2_screencast_frames.rs` — fixture HTTP page → navigate → `Page.startScreencast` → assert **≥5** `Page.screencastFrame` events.
- **B3:** `crates/glass-pane-bridge/tests/b3_rail_latency.rs` — `gate_browser_action(Navigate)` → `ReceiptTailer` shows ALLOW within **500ms**.

## Gate decision

**`--attach` ships.** CDP auth required by default. Pane uses token-authenticated CDP URL.

— Spike closed with test evidence. 👊 + 🔑
