//! B16: pane capability tokens reject invalid inputs.
use glass_pane_bridge::{format_ipc_message, parse_ipc_message, PaneAuth};

#[test]
fn b16_pane_token_refuses_invalid() {
    let auth = PaneAuth::generate();
    assert!(!auth.check(None));
    assert!(!auth.check(Some("bad-token")));
    let body = format_ipc_message(auth.token(), "take_control");
    let (token, command) = parse_ipc_message(&body).unwrap();
    assert!(auth.check(Some(token)));
    assert_eq!(command, "take_control");
}
