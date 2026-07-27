//! CLI entrypoints and process exit codes.

use crate::compare;
use crate::manifest;
use crate::runner;
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::ExitCode as StdExit;

/// Process exit codes per Plan 005.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    Ok = 0,
    Threshold = 10,
    Hash = 11,
    Privacy = 12,
    Approval = 13,
    UserError = 20,
}

impl From<ExitCode> for StdExit {
    fn from(c: ExitCode) -> Self {
        StdExit::from(c as u8)
    }
}

#[derive(Debug, Parser)]
#[command(name = "ghostpost-evals", about = "Ghostpost scan-v1 eval harness")]
pub struct Cli {
    /// Root of the evals crate (defaults to CARGO_MANIFEST_DIR or cwd).
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,

    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manifest operations.
    Manifest {
        #[command(subcommand)]
        cmd: ManifestCmd,
    },
    /// Run a gold/edge suite against a provider.
    Run {
        #[arg(long, default_value = "all")]
        suite: String,
        #[arg(long, default_value = "replay-fixture")]
        provider: String,
        #[arg(long)]
        report: PathBuf,
    },
    /// Compare two eval reports (promotion gate).
    Compare {
        #[arg(long)]
        baseline: PathBuf,
        #[arg(long)]
        candidate: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum ManifestCmd {
    /// List known manifests under manifests/.
    List,
    /// Verify prompt/schema hashes and model/approval references.
    Verify,
}

pub async fn run_cli() -> Result<ExitCode> {
    let cli = Cli::parse();
    let root = resolve_root(cli.root.as_deref())?;
    match cli.cmd {
        Command::Manifest { cmd } => match cmd {
            ManifestCmd::List => {
                manifest::list_manifests(&root)?;
                Ok(ExitCode::Ok)
            }
            ManifestCmd::Verify => match manifest::verify_manifests(&root) {
                Ok(()) => {
                    println!("manifest verify: ok");
                    Ok(ExitCode::Ok)
                }
                Err(manifest::VerifyError::Hash(msg)) => {
                    eprintln!("manifest verify hash: {msg}");
                    Ok(ExitCode::Hash)
                }
                Err(manifest::VerifyError::Other(e)) => Err(e),
            },
        },
        Command::Run {
            suite,
            provider,
            report,
        } => {
            let outcome = runner::run_suite(&root, &suite, &provider, &report).await?;
            if outcome.privacy_findings > 0 {
                eprintln!("privacy findings: {}", outcome.privacy_findings);
                return Ok(ExitCode::Privacy);
            }
            if outcome.approval_blocked {
                eprintln!("approval manifest blocked provider");
                return Ok(ExitCode::Approval);
            }
            if !outcome.passed {
                eprintln!("thresholds failed");
                return Ok(ExitCode::Threshold);
            }
            println!(
                "run ok suite={} provider={} report={}",
                suite,
                provider,
                report.display()
            );
            Ok(ExitCode::Ok)
        }
        Command::Compare {
            baseline,
            candidate,
        } => {
            let result = compare::compare_reports(&baseline, &candidate)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(ExitCode::Ok)
        }
    }
}

fn resolve_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Ok(m) = std::env::var("CARGO_MANIFEST_DIR") {
        let p = PathBuf::from(m);
        if p.join("manifests").is_dir() {
            return Ok(p);
        }
    }
    // Walk up from cwd looking for evals/manifests
    let mut cur = std::env::current_dir().context("cwd")?;
    for _ in 0..6 {
        if cur.join("manifests/models").is_dir() {
            return Ok(cur);
        }
        if cur.join("evals/manifests/models").is_dir() {
            return Ok(cur.join("evals"));
        }
        if !cur.pop() {
            break;
        }
    }
    bail!("could not locate evals root (pass --root)");
}
