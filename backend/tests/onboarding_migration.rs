//! Data migration for the three-step onboarding catalog.
//!
//! The migration SQL is applied to a session-local temp table named
//! `onboarding_profiles` (pg_temp shadows `public` for unqualified names) that
//! carries the pre-migration shape, so existing rows can be seeded with step 4 and
//! obsolete concern ids without touching the shared schema.

use sqlx::postgres::PgPoolOptions;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

const MIGRATION: &str = include_str!("../migrations/20261111000017_onboarding_three_step_catalog.sql");

async fn seed(
    conn: &mut PgConnection,
    concerns: &[&str],
    step: i16,
    revision: i64,
    completed: bool,
) -> Uuid {
    let id = Uuid::new_v4();
    let concerns: Vec<String> = concerns.iter().map(|c| c.to_string()).collect();
    sqlx::query(
        r#"
INSERT INTO onboarding_profiles
  (tenant_id, id, user_id, coming_up, concerns, platforms, revision, current_step,
   consent_version, consent_accepted_at, completed_at)
VALUES ($1, $1, $1, ARRAY['college_apps'], $2, ARRAY['x'], $3, $4,
        '2026-07-23', now(), CASE WHEN $5 THEN now() ELSE NULL END)
"#,
    )
    .bind(id)
    .bind(concerns)
    .bind(revision)
    .bind(step)
    .bind(completed)
    .execute(&mut *conn)
    .await
    .expect("seed row");
    id
}

struct Profile {
    concerns: Vec<String>,
    step: i16,
    revision: i64,
    completed: bool,
    consent_version: Option<String>,
    consent_accepted: bool,
}

async fn load(conn: &mut PgConnection, id: Uuid) -> Profile {
    let row = sqlx::query(
        r#"
SELECT concerns, current_step, revision, completed_at IS NOT NULL AS completed,
       consent_version, consent_accepted_at IS NOT NULL AS consent_accepted
FROM onboarding_profiles WHERE id = $1
"#,
    )
    .bind(id)
    .fetch_one(&mut *conn)
    .await
    .expect("load row");
    Profile {
        concerns: row.get("concerns"),
        step: row.get("current_step"),
        revision: row.get("revision"),
        completed: row.get("completed"),
        consent_version: row.get("consent_version"),
        consent_accepted: row.get("consent_accepted"),
    }
}

#[tokio::test]
async fn migrates_legacy_onboarding_rows_without_stranding_users() {
    let _ = dotenvy::from_filename(".env");
    let url = std::env::var("DATABASE_URL_APP").unwrap_or_else(|_| {
        "postgres://ghostpost_app:ghostpost_app@127.0.0.1:5432/ghostpost".into()
    });
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("connect");
    let mut conn = pool.acquire().await.expect("connection");

    sqlx::raw_sql(
        r#"
CREATE TEMP TABLE onboarding_profiles (
  tenant_id uuid NOT NULL,
  id uuid NOT NULL,
  user_id uuid NOT NULL,
  coming_up text[] NOT NULL DEFAULT '{}',
  concerns text[] NOT NULL DEFAULT '{}',
  platforms text[] NOT NULL DEFAULT '{}',
  revision bigint NOT NULL DEFAULT 0,
  current_step smallint NOT NULL DEFAULT 1,
  consent_version text,
  consent_accepted_at timestamptz,
  completed_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant_id, id),
  CONSTRAINT onboarding_profiles_current_step_check CHECK (current_step BETWEEN 1 AND 4)
)
"#,
    )
    .execute(&mut *conn)
    .await
    .expect("create legacy temp table");

    let expanded = seed(&mut conn, &["drinking_drugs", "negativity", "drug_use"], 4, 5, true).await;
    let only_other = seed(&mut conn, &["other"], 4, 2, true).await;
    let other_in_progress = seed(&mut conn, &["inappropriate_language", "other"], 2, 1, false).await;
    let mixed = seed(&mut conn, &["drinking_drugs", "other", "public_image"], 4, 0, true).await;
    let step4_in_progress = seed(&mut conn, &["political_takes"], 4, 3, false).await;
    let duplicates = seed(&mut conn, &["negativity", "negativity"], 3, 1, false).await;
    let already_valid = seed(&mut conn, &["public_image", "hate"], 3, 7, true).await;
    let empty_early = seed(&mut conn, &[], 1, 0, false).await;
    let empty_late_in_progress = seed(&mut conn, &[], 3, 4, false).await;

    sqlx::raw_sql(MIGRATION)
        .execute(&mut *conn)
        .await
        .expect("apply migration SQL");

    // drinking_drugs expands in place, de-duplicated; retired step capped; stays completed.
    let r = load(&mut conn, expanded).await;
    assert_eq!(r.concerns, ["underage_drinking", "drug_use", "negativity"]);
    assert_eq!((r.step, r.revision, r.completed), (3, 6, true));

    // Only `other`: no concerns remain, so the profile reopens at the concerns step
    // and keeps its consent.
    let r = load(&mut conn, only_other).await;
    assert!(r.concerns.is_empty());
    assert_eq!((r.step, r.revision, r.completed), (2, 3, false));
    assert_eq!(r.consent_version.as_deref(), Some("2026-07-23"));
    assert!(r.consent_accepted);

    // `other` alongside real concerns is dropped without reopening.
    let r = load(&mut conn, other_in_progress).await;
    assert_eq!(r.concerns, ["inappropriate_language"]);
    assert_eq!((r.step, r.revision, r.completed), (2, 2, false));

    let r = load(&mut conn, mixed).await;
    assert_eq!(r.concerns, ["underage_drinking", "drug_use", "public_image"]);
    assert_eq!((r.step, r.revision, r.completed), (3, 1, true));

    // In-progress rows on the retired step 4 move to step 3 with answers intact.
    let r = load(&mut conn, step4_in_progress).await;
    assert_eq!(r.concerns, ["political_takes"]);
    assert_eq!((r.step, r.revision, r.completed), (3, 4, false));

    let r = load(&mut conn, duplicates).await;
    assert_eq!(r.concerns, ["negativity"]);
    assert_eq!((r.step, r.revision), (3, 2));

    // Rows already valid under the new contract are not touched (revision unchanged).
    let r = load(&mut conn, already_valid).await;
    assert_eq!(r.concerns, ["public_image", "hate"]);
    assert_eq!((r.step, r.revision, r.completed), (3, 7, true));

    // Rows that never had concerns are not reopened or revised.
    let r = load(&mut conn, empty_early).await;
    assert_eq!((r.step, r.revision, r.completed), (1, 0, false));
    let r = load(&mut conn, empty_late_in_progress).await;
    assert_eq!((r.step, r.revision, r.completed), (3, 4, false));

    // The step constraint now rejects the retired fourth step.
    let stale = sqlx::query("UPDATE onboarding_profiles SET current_step = 4 WHERE id = $1")
        .bind(already_valid)
        .execute(&mut *conn)
        .await;
    assert!(stale.is_err(), "current_step 4 must violate the new check constraint");

    sqlx::raw_sql("DROP TABLE pg_temp.onboarding_profiles")
        .execute(&mut *conn)
        .await
        .expect("drop temp table");
}
