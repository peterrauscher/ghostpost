//! Ignored local session fixture for smoke scripts (Plan 003 Step 12).
//!
//! This module intentionally does not mint sessions unless `--ignored` is passed and
//! live WorkOS + database env are present. Integration suites under `backend/tests`
//! own the mock-provider coverage.

#[cfg(test)]
mod tests {
    #[tokio::test]
    #[ignore = "requires live WORKOS_* + DATABASE_URL; used by local smoke harness"]
    async fn local_session_fixture() {
        // Operators: run authorize→exchange against a staging WorkOS app, then export
        // GP_TEST_SESSION from the resulting native bearer / web gp_session cookie.
        // Automated minting against live WorkOS is intentionally not embedded here so
        // CI never depends on network auth.
        let has_workos = std::env::var("WORKOS_API_KEY").is_ok()
            && std::env::var("WORKOS_CLIENT_ID").is_ok();
        let has_db = std::env::var("DATABASE_URL_APP").is_ok() || std::env::var("DATABASE_URL").is_ok();
        assert!(
            has_workos && has_db,
            "set WORKOS_* and DATABASE_URL(_APP) before running ignored local_session_fixture"
        );
    }
}
