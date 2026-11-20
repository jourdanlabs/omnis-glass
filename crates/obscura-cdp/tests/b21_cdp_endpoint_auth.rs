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

#[tokio::test(flavor = "current_thread")]
async fn b21_websocket_wrong_token_of_same_length_is_refused() {
    let state = tempfile::tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
    std::env::set_var("OMNIS_GLASS_CDP_TOKEN", "correct-token");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port(); drop(listener);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(obscura_cdp::start_with_pane_observer_options(port, "127.0.0.1", None, false, None, false, None, false, 8, obscura_cdp::PaneObserverOptions::default())).unwrap();
    });
    let good = format!("ws://127.0.0.1:{port}/devtools/browser/correct-token");
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop { if tokio_tungstenite::connect_async(&good).await.is_ok() { break; } tokio::time::sleep(std::time::Duration::from_millis(20)).await; }
    }).await.unwrap();
    let bad = format!("ws://127.0.0.1:{port}/devtools/browser/correct-tokem");
    assert!(tokio_tungstenite::connect_async(&bad).await.is_err(), "wrong token was accepted");
}
