use glass_pane_bridge::is_pane_ui_url;

#[test]
fn only_embedded_ui_origin_is_allowed() {
    assert!(is_pane_ui_url("glass-pane://localhost/index.html"));
    for url in ["https://localhost/", "file:///tmp/index.html", "data:text/html,x", "javascript:alert(1)", "glass-pane://localhost.evil/", "glass-pane://user@localhost/", "glass-pane://localhost:123/"] {
        assert!(!is_pane_ui_url(url), "unexpectedly allowed {url}");
    }
}
