//! Plan 006: guessed unlock/grant/billing/cancel/model-run routes → 404/405.
mod common;

use axum::http::{Method, StatusCode};
use common::*;
use std::sync::Arc;

#[tokio::test]
async fn guessed_route_not_registered() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool, mock, KEY_K1_JSON);

    let cases = [
        ("POST", "/v1/scans/00000000-0000-0000-0000-000000000001/cancel"),
        ("GET", "/v1/scans/00000000-0000-0000-0000-000000000001/model-run"),
        ("GET", "/v1/model-runs/x"),
        ("GET", "/v1/source-accounts"),
        ("POST", "/v1/source-accounts"),
        ("POST", "/v1/billing/checkout"),
        ("POST", "/v1/home/unlock"),
        ("POST", "/home/unlock"),
        ("POST", "/v1/entitlement/grant"),
        ("POST", "/v1/me/entitlement/grant"),
        ("POST", "/v1/demo/reset"),
        ("POST", "/demo/reset"),
        ("POST", "/v1/posts/1/actions"),
        ("GET", "/scan"),
        ("GET", "/dashboard"),
        ("GET", "/review"),
        ("GET", "/posts/1"),
    ];

    for (method, path) in cases {
        let m = match method {
            "GET" => Method::GET,
            "POST" => Method::POST,
            _ => unreachable!(),
        };
        let res = call(&app, m, path, None, &[], None).await;
        assert!(
            res.status == StatusCode::NOT_FOUND || res.status == StatusCode::METHOD_NOT_ALLOWED,
            "{method} {path} got {}",
            res.status
        );
        assert!(!res.status.is_success(), "{method} {path} must not succeed");
    }
}
