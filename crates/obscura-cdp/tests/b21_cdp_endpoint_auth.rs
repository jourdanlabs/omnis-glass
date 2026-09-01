//! B21 — CDP endpoint auth (fail-closed default + constant-time path).

use obscura_cdp::cdp_auth;

#[test]
fn b21_default_config_auth_required() {
    std::env::remove_var("OMNIS_GLASS_CDP_AUTH");
    assert!(
        cdp_auth::cdp_auth_required(),
        "shipped default must require CDP token (fail-closed)"
    );
    let token = cdp_auth::generate_cdp_token();
    let head = "GET /devtools/browser HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
    assert!(
        !cdp_auth::token_from_request_head(head, &token),
        "unauthenticated /devtools/browser must be rejected when auth is on"
    );
}

#[test]
fn b21_unauthenticated_cdp_path_rejected() {
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
    let token = cdp_auth::generate_cdp_token();
    let head = "GET /devtools/browser HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
    assert!(!cdp_auth::token_from_request_head(head, &token));
}

#[test]
fn b21_authenticated_cdp_path_accepted() {
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
    let token = cdp_auth::generate_cdp_token();
    let head = format!("GET /devtools/browser/{token} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    assert!(cdp_auth::token_from_request_head(&head, &token));
}

#[test]
fn b21_wrong_token_rejected_constant_time_safe() {
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
    let token = cdp_auth::generate_cdp_token();
    let head = format!("GET /devtools/browser/wrong-token-value HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    assert!(!cdp_auth::token_from_request_head(&head, &token));
}
