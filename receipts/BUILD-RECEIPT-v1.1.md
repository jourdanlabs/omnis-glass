# OMNIS GLASS v1.1 build receipt

**Builder:** Caulifla, Codex CLI, `gpt-6.1-terra` (the model reported by this session).

**Gate:** Pan (Claude): NOT READY on `daf00fb`, re-gate pending.

**readyForGate:** false. Lane G work and final verification are not yet complete.

The pane observes the server's live CDP targets, takes server-owned control, and routes human commands through the engine's governance. The observer contract changes the meaning of B1, B4–B7, B12, B15, B16, B18 and B19: these concern the cross-connection observer path and its server-owned lock, rather than the removed local `PaneSession`.

## Acceptance evidence map

Paths below are repository-relative. A listed test identifies the evidence; a passing test is not itself Pan's gate verdict.

| Row | Test file | Evidence and limits |
|---|---|---|
| B1 | `crates/glass-pane-bridge/tests/observer_client.rs` | Mirror UI requests do not send human commands; `crates/obscura-cdp/tests/pane_observer_auth.rs` checks mirror-only refusal. Observer meaning. |
| B2 | `crates/glass-pane/tests/real_engine.rs` | Attached headless pane receives at least five animated frames from the real engine. |
| B3 | `crates/glass-pane-bridge/tests/b3_rail_latency.rs` | Action receipt reaches the rail. |
| B4 | `crates/glass-pane-bridge/tests/real_engine.rs` | Human disallowed navigation is refused and URL remains unchanged. Observer meaning. |
| B5 | `crates/glass-pane-bridge/tests/real_engine.rs` | Human allowed navigation actually runs and writes one human action receipt. Observer meaning. |
| B6 | `crates/obscura-cdp/tests/pane_observer_auth.rs` | Server-owned control refuses agent mutations and reports waiting. Observer meaning. |
| B7 | `crates/glass-pane-bridge/tests/real_engine.rs` | Release permits the agent to navigate again. Observer meaning. |
| B8 | `crates/glass-pane-bridge/tests/b20_screencast.rs` | Screencast is not a live capture. |
| B9 | `crates/glass-pane-bridge/tests/b2_screencast_frames.rs`, `crates/glass-pane-bridge/tests/b3_rail_latency.rs` | Deliberately failing WEAKEN mirrors are ignored in the ordinary suite; this receipt does not claim they ran. |
| B10 | `crates/glass-pane-bridge/tests/release_binary_v1_1.rs` | Pane bypass guard. |
| B11 | `crates/glass-pane/tests/real_engine.rs` | Co-launch rejects a wildcard bind; the co-launch test exercises the loopback path. |
| B12 | `crates/glass-pane/tests/real_engine.rs`, `crates/glass-pane-bridge/tests/real_engine.rs` | Attach observes a real engine and routes control across connections. Observer meaning. |
| B13 | `crates/glass-pane-bridge/tests/b13_rail_redaction.rs` | Rail redaction. |
| B14 | `crates/glass-terminus/tests/acceptance_v1.rs` | v1 acceptance tests; A3 now passes no invented TLS hash. |
| B15 | `crates/glass-pane/tests/real_engine.rs`, `crates/glass-pane-bridge/tests/release_binary_v1_1.rs` | Real headless attach and co-launch plus release pin. Observer meaning. |
| B16 | `crates/glass-pane-bridge/tests/b16_ipc_channel_auth.rs`, `crates/obscura-cdp/tests/pane_observer_auth.rs` | Pane IPC capability and separate observer token. Observer meaning. |
| B17 | `crates/obscura-cdp/tests/inbound_cdp_gate.rs` | Inbound CDP refusal and action receipt. |
| B18 | `crates/obscura-cdp/tests/pane_observer_auth.rs` | In-flight handoff cancellation and receipt through the server. Observer meaning. |
| B19 | `crates/glass-pane-bridge/tests/real_engine.rs` | Real human navigation and click navigation carry human driver receipts. Observer meaning. |
| B20 | `crates/glass-pane-bridge/tests/b20_screencast.rs` | Screencast excluded from provenance. |
| B21 | `crates/obscura-cdp/tests/b21_cdp_endpoint_auth.rs` | CDP token required and checked. |

## Limits and policy changes

- A left mouse press on a focusable page control moves focus unless the page cancels the press. A human at the pane can type into a focused text field, and its live value is drawn in the picture; password values are masked. Each key event carrying text writes one `type` receipt.
- The picture draws a live textarea value once, shows checked and unchecked checkboxes and radio buttons with lighter disabled controls, and shows centered input-button labels. Submit and reset inputs without a value use their default labels.

- One page per connection is live. A page's script state does not survive the agent working in another page of the same connection. The pane follows the page the agent is working in.
- Lane H builder: Caulifla, `gpt-6.1-terra` (session log header). Lane H head before the S12 receipt update: `4ad81cb33fd10ea81983d33394cbdf682aac9134`. The release manifest records the head of the stamped build.

- The authorize script writes a separate `terminus.actions.jsonl`; nothing verifies that file, and its writes are not locked.
- A typed navigation produces one action receipt: the approved initial document GET reuses the navigation approval. Redirects and other requests are checked separately.
- Reload and history navigation are gated, except that reload and history moves to `about:blank` do not gate or write a receipt.
- `Target.createTarget` with a URL is gated; creating an `about:blank` target is not.
- Observer streams are invalidated for the watched target after navigation, including link clicks with frames awaiting acknowledgement. The headless pane can require frames after a URL change.
- `Runtime.getProperties` and `DOM.describeNode` are refused during human control because their handlers can execute page script.
- Action, capture and frame ledger locks have bounded waits. A failed action receipt refuses the governed action.
- The canvas reserves space for the action rail, and pending mouse moves are sent before a press or release.
- Autonomous requests and page-initiated navigations use the human driver only for the target controlled by the observer; other pages remain gated as agent work. Refusals are receipted.
- Malformed Rust CSS import and font-face scans no longer retry the same tail. Their generic URL fallback remains, though classification of later special rules in a malformed tail may differ.
- A refused redirect leaves the previously displayed document's URL as the reported target URL. Link-click GET navigations reuse their first approval and write one allowed navigation receipt.
- Taking control during an in-flight navigation remains open. An attempted cancellation with watchdog cleanup aborted in V8 while the page event loop entered an isolate; the experiment was removed. Lane G has not added an in-flight task cancellation.
- Sweep regression tests cover the agent-action lock, private-network refusal receipts, owner-side control checks, empty observer tokens, reload gating, same-length token mismatches, ledger termination and HTTPS capture evidence. Their line checks are recorded in the lane G note.
- HTTPS captures record `not_observed` when no certificate hash is available; there is no certificate hash observation yet.
- MCP stdio sessions run in another process and are not shown in the pane.
- Human mouse events are not receipted individually; navigation they cause is gated and receipted.
- The server's private-network flag is now honored by the CDP navigation gate; the prior gate was strict even when the flag was set. Lane and host policy still apply.
- Eight other potential superlinear groups in `bootstrap.js` remain unmeasured: media dimensions; custom-element name validation; XML comment, CDATA, processing-instruction and DOCTYPE stripping; XML tag matching; color percentage suffixes; iframe HTML stripping; `@font-face` matching; and font shorthand matching.
- During an earlier no-state test run, the default `~/.omnis-glass/web.captures.jsonl` grew. It was not opened, repaired or deleted. Subsequent isolation audit and metadata comparisons are documented in the lane D note; the incident is not erased by those later results.

## Builder evidence so far

- Binaries from `scripts/build-release.sh` are compiled from the checkout that ran it; a plain `cargo build` into a target folder used by another checkout can mix code from both while still printing the current checkout's commit, so do not release from one.
- At `f7850f4`, the local-only obstacle course printed `correctness: 32/33 stages passed   |   median latency: 3022.3ms across stages`; `observer-intersection` expected `io:50` and returned an empty result.
- `scripts/build-release.sh` builds both binaries, enumerates the release tests, and stamps the resulting files. Enumeration can relink `omnis-glass`, so the pin covers the post-enumeration binary rather than assuming the build-only binary is identical.
- At `8fa15fd`, `cargo nextest run --release --features render --no-fail-fast` with the driver state folder printed `Summary [  12.390s] 1701 tests run: 1701 passed, 9 skipped`. The same command with the state variable unset printed `Summary [  11.065s] 1701 tests run: 1701 passed, 9 skipped`; before and after file metadata compared unchanged. These are not final-head results.
- On the item 10 code, the focused release command recorded in the lane D note printed `Summary [   2.093s] 9 tests run: 9 passed, 225 skipped`.
- `node scripts/check-cdp-bypass.mjs`: `check-cdp-bypass: OK`. `node scripts/check-pane-bypass.mjs`: `check-pane-bypass: OK`.

- At `b860bdf`, `cargo nextest run --release --features render --no-fail-fast` printed `Summary [  10.706s] 1707 tests run: 1707 passed, 9 skipped`. With the state variable unset, the same command printed `Summary [  10.091s] 1707 tests run: 1707 passed, 9 skipped`, and metadata snapshots compared unchanged. The lane D note lists each skip and the earlier state incident.
- At `b860bdf`, `cargo nextest run --release --features render -p glass-terminus --test release_binary -p glass-pane-bridge --test release_binary_v1_1 --no-fail-fast` printed `Summary [   0.503s] 4 tests run: 4 passed, 0 skipped`.
- `python3 /tmp/glass-d-perf.py` interleaved the `8fa15fd` and `b860bdf` release binaries on a local CDP fixture. A Python summary of the repeat output reported 20 runs per revision. Old/new minimum, median and maximum for navigation were `4.479 / 5.246 / 9.799` and `4.575 / 5.332 / 9.514` ms; for per-run median evaluations `0.273 / 0.342 / 0.562` and `0.282 / 0.372 / 0.582` ms; for capture `9.330 / 10.043 / 22.512` and `9.531 / 10.123 / 24.894` ms; for sampled peak RSS `49840 / 50888 / 51328` and `49584 / 50816 / 51712` KiB. The repeat's evaluation medians were within the stated approximate noise floor. This is a local synthetic comparison.

The lane D build, stamp, test output and version lines describe its earlier head, not the current lane G head. Final-head verification is pending.

A key press is receipted without any digest of the key.

Scripts run in one page at a time. A page that is not the working page keeps its content but loses its scripts, timers, and event handlers until it is loaded again; the pane shows a notice while the displayed page has stopped scripts, and Reload brings them back. `Glass.listTargets` reports the `scriptsStopped` boolean, and authorized observers receive `Glass.scriptsStoppedChanged` when it changes.

The `key_event_rechecks_private_dns_for_agent_and_human` test runs on macOS only and requires clang.
