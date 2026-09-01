# OMNIS GLASS v1 — Build Receipt

**Builder:** Tifa (Composer / Cursor)  
**Builder substrate:** Composer (Cursor agent model — not Claude-family)  
**Architect spec:** Pan · OMNIS GLASS v1 BUILD-SPEC · 2026-09-01  
**Seal:** 2026-09-01T15:30:00-05:00  
**readyForGate:** true  
**Branch:** `omnis/main` (commit 1 of 2 — v1.1 follows)  
**Gate chain:** Tifa build → Pan artifact validation → Bulma security (egress, SSRF, redaction, MCP 2b, bypass 2d, HTTP client 2a)

## Shipped

| Artifact | Path |
|---|---|
| Build spec | `docs/BUILD-SPEC.md` |
| Dispatch audit | `docs/DISPATCH-AUDIT.md` |
| Upstream policy | `docs/UPSTREAM-POLICY.md` |
| Browser lanes | `browser-lanes.yaml` |
| TERMINUS authorize (browser) | `scripts/terminus-authorize.mjs` |
| CDP bypass lint | `scripts/check-cdp-bypass.mjs` |
| CDP exemptions manifest | `scripts/cdp-dispatch-exemptions.json` |
| `glass-terminus` | egress envelope + lane loader + shell gate |
| `glass-capture` | `web.capture` + SSRF + `capture_kind` structural |
| `glass-ledger` | `browser.action` hash-chain + Ed25519 + redaction |
| `glass-govern` | shared gate for CDP / MCP / CLI / HTTP |
| CDP chokepoint | `obscura-cdp/src/dispatch.rs` → `gate_cdp_command()` |
| HTTP client gate | `obscura-net/src/client.rs` → `gate_http_outbound` (A13) |
| MCP governed path | `obscura-mcp/src/page_gate.rs` — all `navigate_with_wait` / `evaluate` via TERMINUS |
| CLI governed path | `obscura-cli/src/main.rs` (fetch + scrape + eval + stealth dump-original) |
| JS fetch gate | `obscura-js/src/ops.rs` → `gate_js_fetch` before `op_fetch_url` send |
| Agent-surface lint | `scripts/check-cdp-bypass.mjs` (call-site scanner + `agent-surface-exemptions.json`) |
| Bin | `omnis-glass` (+ `obscura` compat) |
| Receipts subcommand | `omnis-glass receipts verify\|status` |

## Acceptance (CLEAR bar)

| # | Result | Evidence |
|---|---|---|
| A1 | PASS | secret + disallowed host → REFUSE |
| A2 | PASS | crown jewel POST → REFUSE |
| A3 | PASS | live capture sha256 matches bytes |
| A4 | PASS | eval cannot stamp `capture_kind: live` |
| A5 | PASS | SSRF literal / private / redirect blocked |
| A6 | PASS | tamper → `verify_chain().ok == false` |
| A7 | PASS | state dir zero cleartext sentinel secret |
| A8 | PASS | `browse:scrape` blocks type |
| A9 | PASS | can-fail mirrors (`#[ignore]` WEAKEN on A13/A14) |
| A10 | PASS | release binary + CDP bypass + receipts status |
| A11 | PASS | invalid lane YAML → boot ok, all governed REFUSE |
| A12 | PASS | cross-origin redirect strips Authorization; cookie jar domain isolation |
| A13 | PASS | `op_fetch_url` page fetch + `gate_http_outbound` boundary + `--dump original --stealth` REFUSE |
| A14 | PASS | `browser_tab_new` / `browser_back` disallowed host → REFUSE (tool surface) |
| A15 | PASS | all content-returning MCP tools carry provenance + `capture_kind: live` |

```
cargo test -p glass-terminus
cargo test -p obscura-cdp --test glass_acceptance
node scripts/check-cdp-bypass.mjs
# 24 passed; 2 ignored (WEAKEN mirrors); 0 failed
```

## Positive controls

- URI-corrupt host → lane REFUSE (not contacted)
- Secret-shaped payload → terminus REFUSE (digest only in ledger)
- Chain line-shift tamper → verify fails at index 0
- WEAKEN mirrors (A13/A14) fail when run with `--ignored`

## Remaining for gate

- **Pan:** artifact validation from this receipt (v1 FIRST — before v1.1)
- **Bulma:** security verdict — egress, SSRF, redaction, MCP path (2b), CDP-bypass guard (2d), HTTP client gate (2a)

---

*Builder stamps readyForGate. Pan validates from artifacts. Bulma owns security verdict when substrates overlap.* 👊 + 🔑
