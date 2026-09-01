# Dispatch entry audit — OMNIS GLASS v1

**Date:** 2026-09-01 · **Builder:** Tifa (Phase 0b spike)

## Question

Is there genuinely one CDP dispatch point, or do MCP / CLI bypass it?

## Findings

### Governed today (via `obscura-cdp/src/dispatch.rs` → `glass::gate_cdp_command`)

| Caller | File | Notes |
|---|---|---|
| CDP WebSocket | `crates/obscura-cdp/src/server.rs:1544` | Primary agent CDP path |
| Target.sendMessageToTarget | `crates/obscura-cdp/src/dispatch.rs:865` | Recurses into gated `dispatch` |
| Internal test / page helpers | `crates/obscura-cdp/src/domains/page.rs:2170` | Calls `dispatch::dispatch` — gated |

### Ungoverned today (direct `Page` / HTTP — bypass CDP gate)

| Caller | File | Agent-facing? |
|---|---|---|
| MCP all tools | `crates/obscura-mcp/src/lib.rs` (`tool_navigate`, `tool_fill`, …) | **Yes — primary agent surface** |
| CLI fetch/scrape | `crates/obscura-cli/src/main.rs` (`run_fetch`, `run_parallel_scrape`) | Yes |
| CLI --eval | `Page::evaluate` in main.rs | Yes |
| JS `fetch()` + `--dump original` | `op_fetch_url` / `fetch_original_bytes` → `ObscuraHttpClient` | Yes (HTTP egress — Phase 2e) |

### MCP ≠ CDP dispatch

`obscura-mcp/src/lib.rs` defines its own `dispatch(method, …)` for JSON-RPC — **not** `obscura_cdp::dispatch::dispatch`. MCP tools call `page.navigate_with_wait`, `page.evaluate`, etc. directly.

## Conclusion

**Single CDP chokepoint is correct for the CDP surface only.** MCP and CLI require separate governed paths (Phase 2b, 2c). JS fetch and `--dump original` share one HTTP-client gate (Phase **2e**, acceptance A13).

## Next build steps

1. Extract shared `glass::gate_browser_action()` usable from CDP, MCP, CLI
2. Wire MCP tool handlers through it (2b)
3. Wire CLI fetch/scrape/eval through it (2c)
4. **Wrap `ObscuraHttpClient` request boundary** — script-fetch + `--dump original` (2e)
5. Tree-wide `check-cdp-bypass.mjs` + `cdp-dispatch-exemptions.json` (2d)
