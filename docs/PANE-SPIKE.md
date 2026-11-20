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

**Conclusion:** This proves the inbound gate for the tested command; it does not prove that a second connection can see another connection's page. Each CDP connection owns its own pages. Attach uses the token-authorized `Glass.*` observer surface to list targets and route commands and screencast frames to their owning connection. The pane control lock lives in the server process. MCP stdio sessions run in a different process and are not visible to the pane in v1.1. Human mouse events are not receipted one by one; navigation they cause follows the existing navigation gate and receipt path.

```
cargo test -p obscura-cdp --test inbound_cdp_gate -- --nocapture
```

## Other spike answers

| # | Question | Result | Decision |
|---|---|---|---|
| 1 | Live frames without MCP poll? | `Page.startScreencast` → `Page.screencastFrame` | **Use CDP screencast** — B2 test |
| 3 | CDP endpoint auth? | v1 had open WS; v1.1 adds path token | **Required by default** (`cdp_auth_required()` → `true`) |
| 4 | Co-launch single Page? | `glass-pane --co-launch` | **Default mode** |
| 5 | Human navigate bypass? | Engine gates the routed navigation | **One engine gate** |
| 6 | Screencast stealth? | Observer-only raster | **No extra DOM APIs** |

## B2 / B3 (un-stubbed)

- **B2:** `crates/glass-pane-bridge/tests/b2_screencast_frames.rs` — fixture HTTP page → navigate → `Page.startScreencast` → assert **≥5** `Page.screencastFrame` events.
- **B3:** `crates/glass-pane-bridge/tests/b3_rail_latency.rs` — `gate_browser_action(Navigate)` → `ReceiptTailer` shows ALLOW within **500ms**.

## Gate decision

The spike alone does not establish cross-connection attach. The observer surface provides that route when enabled; both the CDP token and the separate observer token are required.

— Spike closed with test evidence. 👊 + 🔑

The webview navigation lock is the first layer: only the embedded `glass-pane://localhost` UI can load, new windows are refused, and IPC checks the request origin. The per-launch IPC token is the second layer and is checked on each message.

The attach decision above predates the current CDP server architecture and does not establish shared-page attachment on this head. Each WebSocket connection has its own CDP context and pages (`crates/obscura-cdp/src/server.rs`, `run_connection`); an independent pane connection cannot see the agent connection’s page. Shared-page attachment requires engine support.

## Observer control refusals

| Method | Refusal |
|---|---|
| `Glass.takeControl` | `CONTROL_HELD` when another observer owns control |
| `Glass.release` | `NOT_CONTROL_OWNER` unless sent by the owner |
| `Glass.humanCommand` | `NOT_CONTROL_OWNER` unless sent by the owner |
