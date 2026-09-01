//! B16 — Pane token auth.

use glass_pane_bridge::PaneAuth;

#[test]
fn b16_pane_token_refuses_invalid() {
    let auth = PaneAuth::generate();
    assert!(!auth.check(None));
    assert!(!auth.check(Some("bad-token")));
    assert!(auth.check(Some(auth.token())));
}
