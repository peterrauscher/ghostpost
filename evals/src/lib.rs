//! Ghostpost eval harness for scan-v1 gold/replay suites.

pub mod cli;
pub mod compare;
pub mod manifest;
pub mod metrics;
pub mod providers;
pub mod report;
pub mod runner;
pub mod validate;

pub use cli::{run_cli, ExitCode};
