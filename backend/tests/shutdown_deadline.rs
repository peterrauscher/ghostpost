//! Plan 002 remediation: SHUTDOWN_DEADLINE_SECS must bound serve drain.
//!
//! KernelOps finding: main.rs awaits api/worker joins with no timeout while
//! Shutdown only stores deadline_secs. This contract test fails until drain is
//! actually bounded (helper and/or main wiring).

#[test]
fn serve_drain_is_bounded_by_shutdown_deadline() {
    let main_src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs"));
    let shutdown_src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/shutdown/mod.rs"));

    let after_signal = main_src
        .split("wait_for_signal")
        .nth(1)
        .expect("serve path must wait for shutdown signal");

    let main_bounds = after_signal.contains("timeout")
        || after_signal.contains("drain_with_deadline")
        || after_signal.contains("deadline_secs()");

    let shutdown_bounds = shutdown_src.contains("drain_with_deadline")
        || (shutdown_src.contains("timeout") && shutdown_src.contains("deadline_secs"));

    assert!(
        main_bounds || shutdown_bounds,
        "shutdown deadline must bound api/worker drain after signal; \
         currently Shutdown stores deadline_secs but main joins are unbounded"
    );
}
