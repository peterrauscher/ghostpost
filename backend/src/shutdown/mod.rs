use std::future::Future;
use std::time::Duration;
use tokio::signal;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

#[derive(Clone)]
pub struct Shutdown {
    token: CancellationToken,
    deadline_secs: u64,
}

impl Shutdown {
    pub fn new(deadline_secs: u64) -> Self {
        Self {
            token: CancellationToken::new(),
            deadline_secs,
        }
    }

    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    pub fn cancel(&self) {
        self.token.cancel();
    }

    pub fn deadline_secs(&self) -> u64 {
        self.deadline_secs
    }

    pub async fn wait_for_signal(&self) {
        let ctrl_c = async {
            signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => info!("received SIGINT"),
            _ = terminate => info!("received SIGTERM"),
        }
        self.token.cancel();
    }

    /// Await `drain` until `SHUTDOWN_DEADLINE_SECS` elapses.
    ///
    /// On timeout, invoke `on_timeout` (typically aborting outstanding tasks),
    /// then return an error so callers emit an honest failure signal.
    pub async fn drain_with_deadline<F, T, A>(
        &self,
        drain: F,
        on_timeout: A,
    ) -> Result<T, crate::error::AppError>
    where
        F: Future<Output = T>,
        A: FnOnce(),
    {
        match tokio::time::timeout(Duration::from_secs(self.deadline_secs), drain).await {
            Ok(value) => Ok(value),
            Err(_elapsed) => {
                warn!(
                    deadline_secs = self.deadline_secs,
                    "shutdown drain deadline exceeded; aborting outstanding tasks"
                );
                on_timeout();
                Err(crate::error::AppError::Shutdown(format!(
                    "drain exceeded SHUTDOWN_DEADLINE_SECS ({})",
                    self.deadline_secs
                )))
            }
        }
    }
}

/// Helper used by serve to log join outcomes without panicking.
pub fn log_join_result<T, E>(label: &str, result: Result<Result<T, E>, tokio::task::JoinError>)
where
    E: std::fmt::Display,
{
    match result {
        Ok(Ok(_)) => info!("{label} stopped cleanly"),
        Ok(Err(err)) => error!(error = %err, "{label} error"),
        Err(err) if err.is_cancelled() => warn!("{label} aborted after shutdown deadline"),
        Err(err) => error!(error = %err, "{label} task join error"),
    }
}
