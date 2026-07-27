mod common;

use axum::http::{Method, StatusCode};
use common::*;
use common::blob::{BlobCall, InMemoryBlobStore};
use ghostpost_backend::r#import::limits::MAX_ARCHIVE_BYTES;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

fn with_idempotency<'a>(
    mut headers: Vec<(&'a str, String)>,
    key: &str,
) -> Vec<(&'a str, String)> {
    headers.push(("idempotency-key", key.into()));
    headers
}

fn assert_archive_response_is_sanitized(body: &Value, expected_id: &str) {
    assert_eq!(body["id"], expected_id);
    assert!(matches!(
        body["platform"].as_str(),
        Some("reddit" | "x")
    ));
    assert!(matches!(
        body["status"].as_str(),
        Some(
            "awaiting_upload"
                | "uploaded"
                | "queued"
                | "parsing"
                | "normalizing"
                | "ready"
                | "failed"
                | "rejected"
                | "cancelled"
                | "deleting"
                | "deleted"
        )
    ));
    for forbidden in [
        "rawStorageKey",
        "rawStorageVersionId",
        "archiveFingerprint",
        "contentSha256",
        "etag",
        "uploadExpiresAt",
        "rawDeleteAfter",
    ] {
        assert!(
            body.get(forbidden).is_none(),
            "archive DTO leaked server-only field {forbidden}: {body}"
        );
    }
}

#[tokio::test]
async fn archive_routes_authenticate_reads_and_mutations_instead_of_hiding_missing_routes() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let app = build_app(pool, Arc::new(MockWorkos::new()), KEY_K1_JSON);
    let id = Uuid::new_v4();
    let cases = [
        (Method::POST, "/v1/archive-imports".to_string(), Some(json!({
            "platform":"reddit",
            "contentLength":128,
            "contentType":"application/zip"
        }))),
        (Method::GET, "/v1/archive-imports".to_string(), None),
        (Method::GET, format!("/v1/archive-imports/{id}"), None),
        (Method::POST, format!("/v1/archive-imports/{id}/complete"), None),
        (Method::DELETE, format!("/v1/archive-imports/{id}"), None),
    ];

    let mut wrong_statuses = Vec::new();
    for (method, path, body) in cases {
        let response = call(&app, method.clone(), &path, None, &[], body).await;
        if response.status != StatusCode::UNAUTHORIZED {
            wrong_statuses.push(format!("{method} {path}: {}", response.status));
        }
    }
    assert!(
        wrong_statuses.is_empty(),
        "every archive route exists and requires authentication; wrong statuses: {wrong_statuses:?}"
    );
}

#[tokio::test]
async fn web_archive_mutations_require_origin_csrf_and_idempotency_key() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool, mock.clone(), KEY_K1_JSON);
    let mut jar = CookieJar::new();
    login_web(&app, &mock, &mut jar, &unique_workos_id("archive-web-guards")).await;
    let csrf = fetch_csrf(&app, &mut jar).await;
    let body = json!({
        "platform":"reddit",
        "contentLength":128,
        "contentType":"application/zip"
    });

    let no_csrf = call(
        &app,
        Method::POST,
        "/v1/archive-imports",
        Some(&mut jar),
        &[("idempotency-key", "archive-no-csrf".into())],
        Some(body.clone()),
    )
    .await;
    assert_eq!(no_csrf.status, StatusCode::FORBIDDEN, "{}", no_csrf.text());

    let no_idempotency = call(
        &app,
        Method::POST,
        "/v1/archive-imports",
        Some(&mut jar),
        &web_mutation_headers(&csrf),
        Some(body),
    )
    .await;
    assert_eq!(no_idempotency.status, StatusCode::BAD_REQUEST, "{}", no_idempotency.text());
    no_idempotency.assert_problem_json();

    let cross_site_headers = vec![
        ("origin", "https://attacker.invalid".into()),
        ("sec-fetch-site", "cross-site".into()),
        ("sec-fetch-mode", "cors".into()),
        ("x-csrf-token", csrf),
        ("idempotency-key", "archive-cross-site".into()),
    ];
    let cross_site = call(
        &app,
        Method::DELETE,
        &format!("/v1/archive-imports/{}", Uuid::new_v4()),
        Some(&mut jar),
        &cross_site_headers,
        None,
    )
    .await;
    assert_eq!(cross_site.status, StatusCode::FORBIDDEN, "{}", cross_site.text());
}

#[tokio::test]
async fn reserve_validates_platform_length_type_and_replays_one_server_minted_key() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);
    let (session, token) = login_native(&app, &mock, &unique_workos_id("archive-reserve")).await;
    let headers = with_idempotency(bearer_headers(&token), "archive-reserve-replay");
    let body = json!({
        "platform":"reddit",
        "contentLength":12_345,
        "contentType":"application/zip"
    });

    let first = call(
        &app,
        Method::POST,
        "/v1/archive-imports",
        None,
        &headers,
        Some(body.clone()),
    )
    .await;
    assert_eq!(first.status, StatusCode::CREATED, "{}", first.text());
    let first_body = first.json();
    let id = first_body["id"].as_str().expect("archive id");
    Uuid::parse_str(id).expect("UUID archive id");
    assert_eq!(first_body["upload"]["method"], "POST");
    assert_eq!(first_body["upload"]["fields"]["Content-Type"], "application/zip");
    assert_eq!(first_body["upload"]["fields"]["success_action_status"], "204");
    let key = first_body["upload"]["fields"]["key"]
        .as_str()
        .expect("server-minted object key");
    assert!(key.starts_with("archive-imports/"));
    assert!(key.ends_with("/source.zip"));
    assert!(first_body.get("contentSha256").is_none());

    let replay = call(
        &app,
        Method::POST,
        "/v1/archive-imports",
        None,
        &headers,
        Some(body),
    )
    .await;
    assert_eq!(replay.status, StatusCode::CREATED, "{}", replay.text());
    assert_eq!(replay.json(), first_body, "idempotent replay returns the same reservation and policy");

    let user_id = Uuid::parse_str(session["user"]["id"].as_str().unwrap()).unwrap();
    let imports: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM archive_imports WHERE user_id=$1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(imports, 1, "reserve replay creates no duplicate import");

    for (name, invalid, expected_code) in [
        (
            "future platform",
            json!({"platform":"facebook","contentLength":1,"contentType":"application/zip"}),
            Some("PLATFORM_COMING_SOON"),
        ),
        (
            "zero bytes",
            json!({"platform":"reddit","contentLength":0,"contentType":"application/zip"}),
            None,
        ),
        (
            "over maximum",
            json!({"platform":"x","contentLength":MAX_ARCHIVE_BYTES + 1,"contentType":"application/zip"}),
            None,
        ),
        (
            "wrong type",
            json!({"platform":"reddit","contentLength":1,"contentType":"text/plain"}),
            None,
        ),
    ] {
        let invalid_headers = with_idempotency(bearer_headers(&token), &format!("invalid-{name}"));
        let response = call(
            &app,
            Method::POST,
            "/v1/archive-imports",
            None,
            &invalid_headers,
            Some(invalid),
        )
        .await;
        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "case {name}: {}",
            response.text()
        );
        response.assert_problem_json();
        if let Some(expected_code) = expected_code {
            assert_eq!(response.problem_code().as_deref(), Some(expected_code), "case {name}");
        }
    }
}

#[tokio::test]
async fn list_get_and_delete_are_tenant_scoped_sanitized_and_preserve_shared_content() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);
    let (owner_session, owner_token) =
        login_native(&app, &mock, &unique_workos_id("archive-owner")).await;
    let (_other_session, other_token) =
        login_native(&app, &mock, &unique_workos_id("archive-other")).await;
    let create_headers = with_idempotency(bearer_headers(&owner_token), "archive-owner-create");
    let create = call(
        &app,
        Method::POST,
        "/v1/archive-imports",
        None,
        &create_headers,
        Some(json!({
            "platform":"x",
            "contentLength":64,
            "contentType":"application/octet-stream"
        })),
    )
    .await;
    assert_eq!(create.status, StatusCode::CREATED, "{}", create.text());
    let import_id = create.json()["id"].as_str().unwrap().to_string();
    let import_uuid = Uuid::parse_str(&import_id).unwrap();
    let owner_user_id =
        Uuid::parse_str(owner_session["user"]["id"].as_str().expect("owner user id")).unwrap();
    let tenant_id: Uuid =
        sqlx::query_scalar("SELECT tenant_id FROM users WHERE id=$1")
            .bind(owner_user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let second_import = ghostpost_backend::repository::archive_imports::create_import(
        &pool,
        tenant_id,
        owner_user_id,
        "x",
    )
    .await
    .expect("second import sharing canonical content");
    let shared_item =
        ghostpost_backend::repository::archive_imports::insert_content_item(
            &pool,
            tenant_id,
            owner_user_id,
            "x",
            "post",
            "owner_authored",
            Some("synthetic shared canonical content"),
        )
        .await
        .unwrap();
    for linked_import in [import_uuid, second_import.id] {
        sqlx::query(
            "INSERT INTO archive_import_content_items(tenant_id,import_id,content_item_id) VALUES($1,$2,$3)",
        )
        .bind(tenant_id)
        .bind(linked_import)
        .bind(shared_item.id)
        .execute(&pool)
        .await
        .unwrap();
    }

    let get = call(
        &app,
        Method::GET,
        &format!("/v1/archive-imports/{import_id}"),
        None,
        &bearer_headers(&owner_token),
        None,
    )
    .await;
    assert_eq!(get.status, StatusCode::OK, "{}", get.text());
    assert_archive_response_is_sanitized(&get.json(), &import_id);

    let list = call(
        &app,
        Method::GET,
        "/v1/archive-imports?platform=x&status=awaiting_upload&limit=1",
        None,
        &bearer_headers(&owner_token),
        None,
    )
    .await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.text());
    assert_eq!(list.json()["items"].as_array().unwrap().len(), 1);
    assert_archive_response_is_sanitized(&list.json()["items"][0], &import_id);
    assert_eq!(list.json()["nextCursor"], Value::Null);

    let wrong_tenant = call(
        &app,
        Method::GET,
        &format!("/v1/archive-imports/{import_id}"),
        None,
        &bearer_headers(&other_token),
        None,
    )
    .await;
    assert_eq!(wrong_tenant.status, StatusCode::NOT_FOUND);
    let wrong_delete_headers =
        with_idempotency(bearer_headers(&other_token), "archive-other-delete");
    let wrong_delete = call(
        &app,
        Method::DELETE,
        &format!("/v1/archive-imports/{import_id}"),
        None,
        &wrong_delete_headers,
        None,
    )
    .await;
    assert_eq!(wrong_delete.status, StatusCode::NOT_FOUND);
    let wrong_complete_headers =
        with_idempotency(bearer_headers(&other_token), "archive-other-complete");
    let wrong_complete = call(
        &app,
        Method::POST,
        &format!("/v1/archive-imports/{import_id}/complete"),
        None,
        &wrong_complete_headers,
        None,
    )
    .await;
    assert_eq!(wrong_complete.status, StatusCode::NOT_FOUND);

    let delete_headers = with_idempotency(bearer_headers(&owner_token), "archive-owner-delete");
    let delete = call(
        &app,
        Method::DELETE,
        &format!("/v1/archive-imports/{import_id}"),
        None,
        &delete_headers,
        None,
    )
    .await;
    assert_eq!(delete.status, StatusCode::ACCEPTED, "{}", delete.text());
    assert_eq!(delete.json(), json!({"id":import_id,"status":"deleting"}));
    let deleting_status: String = sqlx::query_scalar(
        "SELECT status FROM archive_imports WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_id)
    .bind(import_uuid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(deleting_status, "deleting");
    let purge_jobs: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM work_items WHERE tenant_id=$1 AND kind='import.purge_raw' AND payload->>'import_id'=$2",
    )
    .bind(tenant_id)
    .bind(import_id.clone())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(purge_jobs, 1, "DELETE enqueues one raw/staging purge");
    let delete_replay = call(
        &app,
        Method::DELETE,
        &format!("/v1/archive-imports/{import_id}"),
        None,
        &delete_headers,
        None,
    )
    .await;
    assert_eq!(delete_replay.status, StatusCode::ACCEPTED);
    assert_eq!(delete_replay.json(), delete.json());
    let purge_jobs_after_replay: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM work_items WHERE tenant_id=$1 AND kind='import.purge_raw' AND payload->>'import_id'=$2",
    )
    .bind(tenant_id)
    .bind(import_id.clone())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(purge_jobs_after_replay, 1, "DELETE replay does not enqueue twice");
    let shared_survives: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM content_items c JOIN archive_import_content_items l ON l.tenant_id=c.tenant_id AND l.content_item_id=c.id WHERE c.tenant_id=$1 AND c.id=$2 AND l.import_id=$3",
    )
    .bind(tenant_id)
    .bind(shared_item.id)
    .bind(second_import.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        shared_survives, 1,
        "deleting one import preserves canonical content referenced by another import"
    );
}

#[tokio::test]
async fn complete_verifies_latest_object_once_pins_its_version_and_replays_without_reheading() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let workos = Arc::new(MockWorkos::new());
    let blob = Arc::new(InMemoryBlobStore::default());
    let app = build_app_with_blob_store(
        pool.clone(),
        workos.clone(),
        KEY_K1_JSON,
        blob.clone(),
    );
    let (session, token) =
        login_native(&app, &workos, &unique_workos_id("archive-complete")).await;
    let reserve = call(
        &app,
        Method::POST,
        "/v1/archive-imports",
        None,
        &with_idempotency(bearer_headers(&token), "archive-complete-reserve"),
        Some(json!({
            "platform": "reddit",
            "contentLength": 12,
            "contentType": "application/zip"
        })),
    )
    .await;
    assert_eq!(reserve.status, StatusCode::CREATED, "{}", reserve.text());
    let reservation = reserve.json();
    let import_id = reservation["id"].as_str().unwrap();
    let storage_key = reservation["upload"]["fields"]["key"].as_str().unwrap();
    blob.put_latest(
        storage_key,
        "immutable-version-a",
        "application/zip",
        &b"PK synthetic"[..],
    );
    blob.clear_calls();

    let mut nonempty_headers =
        with_idempotency(bearer_headers(&token), "archive-complete-body");
    nonempty_headers.push(("content-length", "1".into()));
    let nonempty = call(
        &app,
        Method::POST,
        &format!("/v1/archive-imports/{import_id}/complete"),
        None,
        &nonempty_headers,
        Some(json!({"clientVersion": "must-not-be-trusted"})),
    )
    .await;
    assert_eq!(nonempty.status, StatusCode::BAD_REQUEST, "{}", nonempty.text());
    assert!(
        blob.calls().is_empty(),
        "a nonempty completion body is rejected before object storage is consulted"
    );

    let complete_headers =
        with_idempotency(bearer_headers(&token), "archive-complete-empty");
    let first = call(
        &app,
        Method::POST,
        &format!("/v1/archive-imports/{import_id}/complete"),
        None,
        &complete_headers,
        None,
    )
    .await;
    assert_eq!(first.status, StatusCode::ACCEPTED, "{}", first.text());
    assert_eq!(first.json()["status"], "queued");
    assert_eq!(
        blob.calls(),
        vec![BlobCall::Head {
            key: storage_key.into()
        }],
        "completion verifies the latest object exactly once before pinning its immutable version"
    );

    let import_uuid = Uuid::parse_str(import_id).unwrap();
    let user_id = Uuid::parse_str(session["user"]["id"].as_str().unwrap()).unwrap();
    let tenant_id: Uuid = sqlx::query_scalar("SELECT tenant_id FROM users WHERE id=$1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let pinned: (String, Option<String>, bool) = sqlx::query_as(
        "SELECT status,raw_storage_version_id,raw_delete_after IS NOT NULL FROM archive_imports WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_id)
    .bind(import_uuid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        pinned,
        ("queued".into(), Some("immutable-version-a".into()), true)
    );
    let jobs: Vec<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT kind,payload FROM work_items WHERE tenant_id=$1 AND payload->>'import_id'=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        jobs,
        vec![(
            "import.normalize".into(),
            json!({
                "import_id": import_id,
                "key": storage_key,
                "version_id": "immutable-version-a"
            })
        )],
        "the queued worker payload contains only the server-pinned key and immutable version"
    );

    blob.put_latest(
        storage_key,
        "overwritten-version-b",
        "application/zip",
        &b"PK replacement"[..],
    );
    let replay = call(
        &app,
        Method::POST,
        &format!("/v1/archive-imports/{import_id}/complete"),
        None,
        &complete_headers,
        None,
    )
    .await;
    assert_eq!(replay.status, StatusCode::ACCEPTED, "{}", replay.text());
    assert_eq!(replay.json(), first.json());
    assert_eq!(
        blob.calls(),
        vec![BlobCall::Head {
            key: storage_key.into()
        }],
        "completion replay never replaces the immutable pin with a newer object version"
    );
    let job_count: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM work_items WHERE tenant_id=$1 AND kind='import.normalize' AND payload->>'import_id'=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(job_count, 1, "completion replay cannot enqueue twice");
}

#[tokio::test]
async fn complete_rejects_missing_length_and_type_mismatches_without_pinning_or_enqueuing() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let workos = Arc::new(MockWorkos::new());
    let blob = Arc::new(InMemoryBlobStore::default());
    let app = build_app_with_blob_store(
        pool.clone(),
        workos.clone(),
        KEY_K1_JSON,
        blob.clone(),
    );
    let (session, token) =
        login_native(&app, &workos, &unique_workos_id("archive-mismatch")).await;
    let user_id = Uuid::parse_str(session["user"]["id"].as_str().unwrap()).unwrap();
    let tenant_id: Uuid = sqlx::query_scalar("SELECT tenant_id FROM users WHERE id=$1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();

    for (name, stored, stored_type, expected_code) in [
        (
            "missing",
            None,
            "application/zip",
            "UPLOAD_NOT_FOUND",
        ),
        (
            "wrong-length",
            Some(&b"PK short"[..]),
            "application/zip",
            "INTEGRITY_MISMATCH",
        ),
        (
            "wrong-type",
            Some(&b"PK exact!!"[..]),
            "application/octet-stream",
            "INTEGRITY_MISMATCH",
        ),
    ] {
        let reserve = call(
            &app,
            Method::POST,
            "/v1/archive-imports",
            None,
            &with_idempotency(
                bearer_headers(&token),
                &format!("archive-mismatch-reserve-{name}"),
            ),
            Some(json!({
                "platform": "reddit",
                "contentLength": 10,
                "contentType": "application/zip"
            })),
        )
        .await;
        assert_eq!(reserve.status, StatusCode::CREATED, "case {name}: {}", reserve.text());
        let body = reserve.json();
        let import_id = body["id"].as_str().unwrap();
        let storage_key = body["upload"]["fields"]["key"].as_str().unwrap();
        if let Some(bytes) = stored {
            blob.put_latest(
                storage_key,
                format!("version-{name}"),
                stored_type,
                bytes,
            );
        }

        let response = call(
            &app,
            Method::POST,
            &format!("/v1/archive-imports/{import_id}/complete"),
            None,
            &with_idempotency(
                bearer_headers(&token),
                &format!("archive-mismatch-complete-{name}"),
            ),
            None,
        )
        .await;
        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "case {name}: {}",
            response.text()
        );
        assert_eq!(
            response.problem_code().as_deref(),
            Some(expected_code),
            "case {name}"
        );
        let state: (String, Option<String>) = sqlx::query_as(
            "SELECT status,raw_storage_version_id FROM archive_imports WHERE tenant_id=$1 AND id=$2",
        )
        .bind(tenant_id)
        .bind(Uuid::parse_str(import_id).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            state,
            ("awaiting_upload".into(), None),
            "case {name}: a failed completion cannot pin or advance the import"
        );
        let jobs: i64 = sqlx::query_scalar(
            "SELECT count(*)::bigint FROM work_items WHERE tenant_id=$1 AND kind='import.normalize' AND payload->>'import_id'=$2",
        )
        .bind(tenant_id)
        .bind(import_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(jobs, 0, "case {name}: mismatch cannot enqueue normalization");
    }
}
