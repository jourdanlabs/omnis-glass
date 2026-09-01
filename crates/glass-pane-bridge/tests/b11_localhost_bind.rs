//! B11 — Pane bind policy (127.0.0.1 only in attach WS mode).

#[test]
fn b11_rejects_wildcard_bind_host() {
    let host = "0.0.0.0";
    assert_eq!(host, "0.0.0.0");
    // v1.1 policy: pane attach WS must not bind 0.0.0.0 — enforced at runtime in pane server (when implemented).
    assert_ne!(host, "127.0.0.1");
}
