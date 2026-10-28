//! Jev-only scan E2E (requires migrated local Postgres): real `scan_posts`
//! worker and real `JevHttp` client against a local mock of TypeSafe's
//! `POST /v1/systemone` that answers every question type the classifier asks.
//! Each scenario writes `target/e2e-artifacts/scan_jev_<scenario>.json`.
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use ghostpost_backend::config::Config;
use ghostpost_backend::db::migrate;
use ghostpost_backend::jobs::scan_posts;
use ghostpost_backend::repository::{onboarding, scans, users, work_items};
use ghostpost_backend::scan::llm::jev::{JevHttp, JevScanProvider};
use parking_lot::Mutex;
use scan_jev::JevLimits;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

const INSULT: &str = "Ugh, long day. You are worthless and everyone knows it.";
const THREAT: &str = "I will beat you up if you show your face";
const BENIGN: [&str; 8] = [
    "made blueberry pancakes this morning",
    "so excited for the game tonight, go team",
    "finally finished my chem lab report",
    "anyone have a good playlist for studying?",
    "my dog learned to roll over today",
    "sunset at the lake was unreal",
    "trying a new running route this weekend",
    "grateful for my friends this semester",
];

fn test_env() {
    let _ = dotenvy::from_filename(".env");
    for (k, v) in [
        ("DATABASE_URL", "postgres://ghostpost_migrator:ghostpost_migrator@127.0.0.1:5432/ghostpost"),
        ("DATABASE_URL_APP", "postgres://ghostpost_app:ghostpost_app@127.0.0.1:5432/ghostpost"),
        ("DATABASE_APP_ROLE", "ghostpost_app"),
        ("GHOSTPOST_ENV", "development"),
        ("SCAN_BATCH_HMAC_KEYS", r#"[{"id":"local_v1","secret":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}]"#),
    ] {
        if std::env::var(k).is_err() {
            std::env::set_var(k, v);
        }
    }
}

async fn migrated_pool() -> PgPool {
    test_env();
    let config = Config::load_for_migrate().expect("config");
    migrate::run(&config, false).await.expect("migrate");
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("DATABASE_URL_APP").unwrap())
        .await
        .expect("connect app pool")
}

// ---- mock TypeSafe System One ------------------------------------------------

#[derive(Clone)]
struct Mock {
    overloaded: bool,
    log: Arc<Mutex<Vec<Value>>>,
}

fn hostile(text: &str) -> bool {
    text.contains("worthless") || text.contains("beat you")
}

/// Rule-based answers keyed on the classifier's question-id prefixes:
/// p/c = screen/confirm Noul, r = risk Score, k = reason Choice,
/// e = evidence Choice, g = primary-category Choice.
async fn systemone(State(m): State<Mock>, headers: HeaderMap, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    assert_eq!(headers.get("authorization").and_then(|v| v.to_str().ok()), Some("Bearer test-key"));
    m.log.lock().push(body.clone());
    if m.overloaded {
        return (StatusCode::from_u16(529).unwrap(), Json(json!({"error": "overloaded"})));
    }
    let posts = body["state"]["posts"].as_array().unwrap();
    let mut answers = serde_json::Map::new();
    for (key, q) in body["questions"].as_object().unwrap() {
        let local: usize = key[1..key.find(':').unwrap()].parse().unwrap();
        let text = posts[local].as_str().unwrap();
        let answer = match key.as_bytes()[0] {
            b'p' | b'c' => json!({"type": "noul", "noul": if hostile(text) { 0.9 } else { 0.02 }}),
            b'r' => json!({"type": "score", "score": 1.8, "probabilities": {"0": 0.05, "1": 0.1, "2": 0.8, "3": 0.05}, "confidence": 0.7}),
            b'k' => json!({"type": "choice", "choice": "targeted_insult", "probabilities": {"targeted_insult": 0.8}, "confidence": 0.8}),
            b'e' => {
                let pick = q["criteria"].as_object().unwrap().iter()
                    .find(|(_, v)| hostile(v.as_str().unwrap())).map(|(k, _)| k.clone()).unwrap();
                json!({"type": "choice", "choice": pick, "probabilities": {}, "confidence": 0.9})
            }
            b'g' => json!({"type": "choice", "choice": "inappropriate_language",
                           "probabilities": {"inappropriate_language": 0.7, "negativity": 0.3}, "confidence": 0.6}),
            other => panic!("unexpected question prefix {}", other as char),
        };
        answers.insert(key.clone(), answer);
    }
    (StatusCode::OK, Json(json!({"model": "jev-1.13.0", "answers": answers, "usage": {"input_tokens": 500, "output_tokens": 9}})))
}

async fn start_mock(overloaded: bool) -> (String, Arc<Mutex<Vec<Value>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new().route("/v1/systemone", post(systemone)).with_state(Mock { overloaded, log: log.clone() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), log)
}

// ---- scenario ------------------------------------------------------------------

struct Outcome {
    scan: Uuid,
    handle_ok: bool,
    status: String,
    error_code: Option<String>,
    requests: Vec<Value>,
    flags: Vec<(String, Option<String>, Option<String>, Option<String>)>,
    attempts: Vec<(String, Option<String>, Option<String>, Option<String>, Option<i32>, Option<f64>)>,
}

async fn run_scenario(overloaded: bool) -> Outcome {
    let pool = migrated_pool().await;
    let tenant = users::create_tenant(&pool).await.unwrap();
    let user = users::create_user(&pool, tenant.id, None, Some("jev@example.com"), Some("J")).await.unwrap();
    onboarding::upsert(
        &pool, tenant.id, user.id,
        &["job_interviews".into()],
        &["negativity".into(), "inappropriate_language".into()],
        &["x".into()],
    )
    .await
    .unwrap();
    let import_id: Uuid = sqlx::query_scalar(
        "INSERT INTO archive_imports (tenant_id, user_id, platform, status) VALUES ($1, $2, 'x', 'ready') RETURNING id",
    )
    .bind(tenant.id).bind(user.id).fetch_one(&pool).await.unwrap();
    let bodies: Vec<&str> = BENIGN.iter().copied().chain([INSULT, THREAT]).collect();
    for (i, body) in bodies.iter().enumerate() {
        let id: Uuid = sqlx::query_scalar(
            r#"INSERT INTO content_items (tenant_id, user_id, platform, kind, authorship, body,
                 source_logical_id, source_revision_id, content_hmac)
               VALUES ($1, $2, 'x', 'tweet', 'owner_authored', $3, $4, $4, $5) RETURNING id"#,
        )
        .bind(tenant.id).bind(user.id).bind(*body)
        .bind(format!("logical-{i:02}-{}", Uuid::new_v4()))
        .bind(vec![i as u8 + 1; 32])
        .fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO archive_import_content_items(tenant_id,import_id,content_item_id) VALUES($1,$2,$3)")
            .bind(tenant.id).bind(import_id).bind(id).execute(&pool).await.unwrap();
    }
    let scan = scans::create_scan(&pool, tenant.id, user.id).await.unwrap();
    let work = work_items::enqueue(
        &pool, tenant.id, Some(user.id), "scan_posts",
        json!({"scanId": scan.id, "archiveImportIds": [import_id]}), None, 0, 3,
    )
    .await
    .unwrap();

    let (base, log) = start_mock(overloaded).await;
    let provider = Arc::new(JevScanProvider::new(JevHttp::new("test-key", base), JevLimits::default()));
    let handle_ok = scan_posts::handle(&pool, &work, provider).await.is_ok();

    let (status, error_code): (String, Option<String>) =
        sqlx::query_as("SELECT status, error_code FROM scans WHERE tenant_id=$1 AND id=$2")
            .bind(tenant.id).bind(scan.id).fetch_one(&pool).await.unwrap();
    let flags = sqlx::query_as(
        r#"SELECT f.risk_level, f.category, f.reason_summary, f.evidence FROM flagged_posts f
           WHERE f.tenant_id=$1 AND f.scan_id=$2 ORDER BY f.evidence"#,
    )
    .bind(tenant.id).bind(scan.id).fetch_all(&pool).await.unwrap();
    let attempts = sqlx::query_as(
        r#"SELECT outcome, provider, prompt_version, model_id, input_tokens, cost_usd::float8
           FROM model_attempts WHERE tenant_id=$1 AND scan_id=$2 ORDER BY attempt"#,
    )
    .bind(tenant.id).bind(scan.id).fetch_all(&pool).await.unwrap();
    let requests = log.lock().clone();
    Outcome { scan: scan.id, handle_ok, status, error_code, requests, flags, attempts }
}

fn kind(req: &Value) -> &'static str {
    if req["questions"].as_object().unwrap().keys().all(|k| k.starts_with('p')) { "screen" } else { "confirm" }
}

fn write_artifact(name: &str, o: &Outcome) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/e2e-artifacts");
    std::fs::create_dir_all(&dir).unwrap();
    let report = json!({
        "scenario": name,
        "scan": o.scan,
        "handleOk": o.handle_ok,
        "scanStatus": o.status,
        "scanErrorCode": o.error_code,
        "jevRequests": o.requests.iter().map(|r| json!({
            "kind": kind(r),
            "model": r["model"],
            "posts": r["state"]["posts"].as_array().map(|a| a.len()),
            "questions": r["questions"].as_object().map(|q| q.len()),
        })).collect::<Vec<_>>(),
        "flags": o.flags.iter().map(|(risk, cat, reason, ev)| json!({"risk": risk, "category": cat, "reason": reason, "evidence": ev})).collect::<Vec<_>>(),
        "attempts": o.attempts.iter().map(|(out, prov, pv, model, tok, cost)| json!({
            "outcome": out, "provider": prov, "promptVersion": pv, "model": model, "inputTokens": tok, "costUsd": cost,
        })).collect::<Vec<_>>(),
    });
    let path = dir.join(format!("scan_jev_{name}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    eprintln!("artifact: {}", path.display());
}

#[tokio::test]
#[ignore = "requires migrated local Postgres"]
async fn jev_classifies_a_scan_in_eight_post_batches_and_persists_flags() {
    let o = run_scenario(false).await;
    write_artifact("classify", &o);

    assert!(o.handle_ok);
    assert_eq!(o.status, "succeeded");
    // 10 posts: screened 8 + 2; only the 2 escalated posts reach the confirm request.
    let shape: Vec<(&str, usize)> = o.requests.iter().map(|r| (kind(r), r["state"]["posts"].as_array().unwrap().len())).collect();
    assert_eq!(shape, vec![("screen", 8), ("screen", 2), ("confirm", 2)]);
    assert!(o.requests.iter().all(|r| r["model"] == "jev-1.13.0"));

    let summary = "Directs an insult at a specific person or group.";
    assert_eq!(
        o.flags,
        vec![
            ("medium".into(), Some("inappropriate_language".into()), Some(summary.into()), Some(THREAT.into())),
            ("medium".into(), Some("inappropriate_language".into()), Some(summary.into()), Some("You are worthless and everyone knows it.".into())),
        ]
    );

    assert_eq!(o.attempts.len(), 1, "one validated classifier attempt for the batch");
    let (outcome, provider, prompt_version, model, tokens, cost) = &o.attempts[0];
    assert_eq!(outcome, "validated");
    assert_eq!(provider.as_deref(), Some("typesafe-jev"));
    assert_eq!(prompt_version.as_deref(), Some("jev-scan-v1"));
    assert_eq!(model.as_deref(), Some("jev-1.13.0"));
    assert_eq!(*tokens, Some(1500));
    assert!((cost.unwrap() - 1500.0 * 0.042 / 1_000_000.0).abs() < 1e-12);
}

#[tokio::test]
#[ignore = "requires migrated local Postgres"]
async fn jev_outage_fails_the_scan_instead_of_clearing_posts() {
    let o = run_scenario(true).await;
    write_artifact("outage", &o);

    assert!(!o.handle_ok);
    assert_eq!(o.status, "failed");
    assert_eq!(o.error_code.as_deref(), Some("SCAN_PROVIDER_UNAVAILABLE"));
    assert!(o.flags.is_empty());
    assert_eq!(o.requests.len(), 5, "bounded by MAX_HTTP_CALLS");
    assert!(o.attempts.iter().all(|a| a.0 == "failed" && a.5.is_none()));
}
