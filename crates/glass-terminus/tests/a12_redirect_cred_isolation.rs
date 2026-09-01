//! A12 — Redirect credential isolation: cross-origin 302 must not forward creds.

use glass_govern::strip_cross_origin_auth;
use obscura_net::CookieJar;
use std::collections::HashMap;
use url::Url;

#[test]
fn a12_cross_origin_redirect_strips_authorization_header() {
    let from = Url::parse("https://example.com/start").unwrap();
    let to = Url::parse("https://other.example.org/landing").unwrap();
    let mut headers = HashMap::from([
        ("Authorization".to_string(), "Bearer session-token".to_string()),
        ("Accept".to_string(), "text/html".to_string()),
    ]);
    strip_cross_origin_auth(&mut headers, &from, &to);
    assert!(
        !headers.contains_key("Authorization"),
        "Authorization must not forward on cross-origin redirect"
    );
    assert!(headers.contains_key("Accept"));
}

#[test]
fn a12_cross_origin_redirect_cookie_jar_does_not_leak_prior_origin() {
    let jar = CookieJar::new();
    let origin_a = Url::parse("https://example.com/page").unwrap();
    jar.set_cookie("session=from-a; Path=/; Domain=example.com", &origin_a);
    let redirect_target = Url::parse("https://other.example.org/page").unwrap();
    let cookie_header = jar.get_cookie_header(&redirect_target);
    assert!(
        !cookie_header.contains("session=from-a"),
        "origin-A cookies must not attach to cross-origin redirect target, got: {cookie_header}"
    );
}

#[test]
fn a12_same_origin_redirect_keeps_authorization() {
    let from = Url::parse("https://example.com/a").unwrap();
    let to = Url::parse("https://example.com/b").unwrap();
    let mut headers = HashMap::from([(
        "Authorization".to_string(),
        "Bearer keep-me".to_string(),
    )]);
    strip_cross_origin_auth(&mut headers, &from, &to);
    assert_eq!(
        headers.get("Authorization").map(String::as_str),
        Some("Bearer keep-me")
    );
}
