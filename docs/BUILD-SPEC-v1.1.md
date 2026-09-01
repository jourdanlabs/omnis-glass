# OMNIS GLASS v1.1 — Glass Pane — Build Spec

**From:** Pan (architect + validator lane) · **Builder:** Tifa · **Date:** 2026-09-01
**Status:** BUILD ORDER (executable) · **Plan gate:** Pan 2026-09-01 — Tifa's draft + 3 fixes + 2 tightenings folded. This is the spec of record.
**Depends on:** OMNIS GLASS v1 CLEAR. v1.1 does not gate until v1 gates.
**Codename:** Glass Pane — lightweight native window: agent mirror + governed human browse.

## North star (Captain, 2026-09-01)

Eventually OMNIS GLASS becomes a **JL-owned browser engine, not a fork** — the browser runtime fully owned like the chamber's other load-bearing pieces. **v1 and v1.1 stay the Obscura fork** (`h4ckf0r0day/obscura`, Apache 2.0, attribution intact). The own-engine goal is not scoped here — it is the direction, not this build.

## Gate-independence (declare at gate start)

Builder: Tifa (Cursor). **Record the composer's base model in `receipts/BUILD-RECEIPT-v1.1.md`.**
If Claude-family → **Bulma (GPT-5.6) owns the security verdict** on: human-browse egress, session lock, the pane bypass guard, localhost bind policy, the screencast ingress kind, **the pane frame-channel authentication (Fix 1), and the `--attach` enforcement model (Fix 2).**

## Goal

A native lightweight browser window on top of headless v1 so a human can:
1. **Mirror** — see what the agent sees (live viewport + action rail), read-only.
2. **Browse** — take the wheel under a separate lane (`browse:human`), full TERMINUS governance.

Agents keep headless + MCP. Glass Pane is the **observability and optional human surface** — not a second browser engine.

## Phase 0

See `docs/PANE-SPIKE.md`. **`--attach` ships** (inbound CDP gated).

## Build order

1. Phase 0 spike → `docs/PANE-SPIKE.md`
2. `browser-lanes.yaml` + `browse:human` + policy v1.1
3. `glass-capture`: `CaptureKind::Screencast` + provenance exclusion
4. `glass-govern`: session lock, driver field, pane navigate, in-flight cancel
5. `glass-pane-bridge` + tests B1–B8, B11–B13, B16, B18–B20
6. `glass-pane-ui` + `glass-pane` binary
7. `check-pane-bypass.mjs` + B10
8. Attach + B12 + B17 + B21
9. v1 regression B14
10. `BUILD-RECEIPT-v1.1`

Full acceptance table: B1–B21 per Pan gate packet.

🐦‍⬛ + 🔑
