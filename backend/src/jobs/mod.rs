pub mod archive_import;
pub mod purge_content;
pub mod purge_retention;
pub mod scan_posts;
pub mod cancel;
pub mod queue;
pub mod runner;
pub mod sweeper;

/// Pure backoff helper used by queue commit-failure path.
pub fn retry_backoff_secs(attempt_count: i32) -> i64 {
    let exp = attempt_count.saturating_sub(1).clamp(0, 16) as u32;
    let secs = 2i64.saturating_pow(exp);
    secs.clamp(1, 300)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps() {
        assert_eq!(retry_backoff_secs(1), 1);
        assert_eq!(retry_backoff_secs(2), 2);
        assert_eq!(retry_backoff_secs(3), 4);
        assert_eq!(retry_backoff_secs(20), 300);
    }
}
