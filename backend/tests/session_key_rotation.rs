//! Plan 003 Step 9 — APP_SESSION_KEYS rotation.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use std::sync::Arc;

#[tokio::test]
async fn session_key_rotation_old_key_still_validates() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());

    // Mint under k1.
    let app_k1 = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);
    let mut jar = CookieJar::new();
    let user = unique_workos_id("keyrot");
    login_web(&app_k1, &mock, &mut jar, &user).await;
    let old_token = jar.get("gp_session").unwrap().to_string();

    let key_id: String = sqlx::query_scalar(
        r#"
SELECT key_id FROM auth_sessions
WHERE token_hash = $1 AND revoked_at IS NULL
"#,
    )
    .bind({
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(old_token.as_bytes());
        h.finalize().to_vec()
    })
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(key_id, "k1");

    // Rebuild app with k2 newest-first; k1 still present.
    let app_rot = build_app(pool.clone(), mock.clone(), KEY_K2_FRONT_JSON);
    let me = call(
        &app_rot,
        Method::GET,
        "/v1/me",
        Some(&mut jar),
        &[],
        None,
    )
    .await;
    assert_eq!(me.status, StatusCode::OK, "old k1 session must validate: {}", me.text());

    // New login uses k2.
    let mut jar2 = CookieJar::new();
    let user2 = unique_workos_id("keyrot_new");
    login_web(&app_rot, &mock, &mut jar2, &user2).await;
    let new_token = jar2.get("gp_session").unwrap().to_string();
    let new_key: String = sqlx::query_scalar(
        r#"
SELECT key_id FROM auth_sessions
WHERE token_hash = $1 AND revoked_at IS NULL
"#,
    )
    .bind({
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(new_token.as_bytes());
        h.finalize().to_vec()
    })
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(new_key, "k2");
}
