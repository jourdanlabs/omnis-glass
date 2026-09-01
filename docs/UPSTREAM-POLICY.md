# Upstream policy — Obscura

**Upstream:** https://github.com/h4ckf0r0day/obscura (Apache 2.0)  
**Remote name:** `upstream` → `h4ckf0r0day/obscura`  
**Origin:** `origin` → `jourdanlabs/omnis-glass`

## Branch model

| Branch | Purpose |
|---|---|
| `main` | Tracks upstream Obscura only (clean merge base) |
| `omnis/main` | OMNIS GLASS product layer — all governed-fork commits land here |

**OMNIS base commit:** `14da23d` (Merge pull request #784 — render cascade explicit stack)

## Rules

1. **Never merge upstream into `omnis/main`.** Upstream is **cherry-picked** onto `omnis/main` for security fixes and rendering regressions only — one commit at a time, reviewed.
2. **`main` stays upstream-clean.** OMNIS layer does not live on `main`.
3. **Do not clone Obscura Cloud** (managed hosting, residential proxies).
4. **Keep internal crate names** (`obscura_*`) for merge compatibility; user-facing branding is OMNIS GLASS.
5. **GPL firewall:** read upstream docs/papers for concepts; do not port GPL source.

## Cherry-pick workflow

```bash
git fetch upstream
git checkout omnis/main
git log --oneline omnis/main..upstream/main   # review
git cherry-pick <sha>                         # one commit at a time
cargo test && cargo test --release
```

Document every cherry-pick in `CHANGELOG-OMNIS.md` with upstream SHA and reason.
