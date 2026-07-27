use clap::Parser;
use ghostpost_backend::auth::{
    reseal, AuthConfig, AuthService, SdkWorkosProvider, WorkosIdentityProvider,
};
use ghostpost_backend::cli::{AuthCommand, Cli, Command, ServeRole};
use ghostpost_backend::config::Config;
use ghostpost_backend::db::{migrate, pool};
use ghostpost_backend::error::AppError;
use ghostpost_backend::jobs::runner;
use ghostpost_backend::blob::{s3::S3BlobStore, BlobStore};
use ghostpost_backend::shutdown::{self, Shutdown};
use ghostpost_backend::telemetry;
use std::sync::Arc;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("ghostpost-backend fatal: {err:#}");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Migrate { dry_run } => {
            telemetry::init_from_env()?;
            let config = Config::load_for_migrate()?;
            info!(dry_run, "running migrations");
            migrate::run(&config, dry_run).await?;
            Ok(())
        }
        Command::Serve { role, bind } => {
            telemetry::init_from_env()?;
            let mut config = Config::load_for_serve()?;
            if let Some(bind) = bind {
                config.bind_addr = bind;
            }
            run_serve(config, role).await
        }
        Command::Auth { command } => {
            telemetry::init_from_env()?;
            let config = Config::load_for_serve()?;
            let auth = AuthConfig::load_from_env(config.is_development)?;
            let pool = pool::connect_app(&config).await?;
            match command {
                AuthCommand::ResealWorkosSessions => {
                    let n = reseal::reseal_workos_sessions(&pool, &auth).await?;
                    info!(resealed = n, "auth reseal-workos-sessions complete");
                    pool.close().await;
                    Ok(())
                }
            }
        }
    }
}

fn build_auth_service(
    pool: sqlx::PgPool,
    config: &Config,
) -> anyhow::Result<Arc<AuthService>> {
    let auth_config = AuthConfig::load_from_env(config.is_development)?;
    let provider: Arc<dyn WorkosIdentityProvider> = Arc::new(
        SdkWorkosProvider::new(
            &auth_config.workos_api_key,
            &auth_config.workos_client_id,
            &auth_config.workos_webhook_secret,
        )
        .map_err(|err| AppError::Config(err.to_string()))?,
    );
    Ok(Arc::new(AuthService::new(pool, auth_config, provider)))
}

async fn build_blob_store() -> anyhow::Result<Arc<dyn BlobStore>> {
    let bucket = std::env::var("ARCHIVE_BUCKET")
        .map_err(|_| AppError::Config("missing required env ARCHIVE_BUCKET".into()))?;
    let region = std::env::var("ARCHIVE_S3_REGION")
        .map_err(|_| AppError::Config("missing required env ARCHIVE_S3_REGION".into()))?;
    let force_path_style = std::env::var("ARCHIVE_S3_FORCE_PATH_STYLE")
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);
    let endpoint_override = std::env::var("ARCHIVE_S3_ENDPOINT").ok();
    let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_sdk_s3::config::Region::new(region.clone()))
        .load()
        .await;
    let provider = sdk.credentials_provider()
        .ok_or_else(|| AppError::Config("AWS credentials provider missing from resolved config".into()))?;
    let mut builder = aws_sdk_s3::config::Builder::from(&sdk).force_path_style(force_path_style);
    if let Some(endpoint) = endpoint_override.as_deref() {
        builder = builder.endpoint_url(endpoint);
    }
    let signing_endpoint = std::env::var("ARCHIVE_S3_PUBLIC_ENDPOINT").ok().or_else(|| endpoint_override.clone());
    let endpoint = match signing_endpoint {
        Some(base) if force_path_style => format!("{}/{}", base.trim_end_matches('/'), bucket),
        Some(base) => base,
        None => format!("https://{bucket}.s3.{region}.amazonaws.com"),
    };
    Ok(Arc::new(S3BlobStore::new_with_credentials_provider(
        aws_sdk_s3::Client::from_conf(builder.build()),
        provider,
        bucket,
        region,
        endpoint,
        std::env::var("ARCHIVE_S3_REQUIRE_KMS").map(|v| v != "false").unwrap_or(true),
        std::env::var("ARCHIVE_S3_KMS_KEY_ID").ok(),
    )))
}

async fn run_serve(config: Config, role: ServeRole) -> anyhow::Result<()> {
    let shutdown = Shutdown::new(config.shutdown_deadline_secs);
    let pool = pool::connect_app(&config).await?;
    let auth = build_auth_service(pool.clone(), &config)?;

    let mut api_handle = if matches!(role, ServeRole::Api | ServeRole::All) {
        let app_state = ghostpost_backend::api::router::AppState {
            pool: pool.clone(),
            restore_replay_pending: config.restore_replay_pending,
            worker_ready: matches!(role, ServeRole::All),
            auth: auth.clone(),
        };
        let blob_store = build_blob_store().await?;
        let router = ghostpost_backend::api::router::build_with_blob_store(app_state, blob_store);
        let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
        info!(addr = %config.bind_addr, ?role, "listening");
        let token = shutdown.token();
        Some(tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    token.cancelled().await;
                })
                .await
        }))
    } else {
        info!(?role, "worker-only; no HTTP bind");
        None
    };

    let mut worker_handle = if matches!(role, ServeRole::Worker | ServeRole::All) {
        let worker_config = config.clone();
        let worker_pool = pool.clone();
        let worker_auth = auth.clone();
        let worker_blob = build_blob_store().await?;
        let token = shutdown.token();
        Some(tokio::spawn(async move {
            runner::run(worker_pool, worker_config, worker_auth, worker_blob, token).await
        }))
    } else {
        None
    };

    let api_abort = api_handle.as_ref().map(|h| h.abort_handle());
    let worker_abort = worker_handle.as_ref().map(|h| h.abort_handle());

    let mut unexpected: Option<&'static str> = None;
    tokio::select! {
        _ = shutdown.wait_for_signal() => {
            info!("shutdown signal received; draining");
        }
        res = join_optional(&mut api_handle) => {
            unexpected = Some("api");
            error!(?res, "api task exited unexpectedly; initiating shutdown");
            shutdown.cancel();
            shutdown::log_join_result("api", res);
        }
        res = join_optional(&mut worker_handle) => {
            unexpected = Some("worker");
            error!(?res, "worker task exited unexpectedly; initiating shutdown");
            shutdown.cancel();
            shutdown::log_join_result("worker", res);
        }
    }

    let drain = async {
        if let Some(handle) = api_handle.take() {
            shutdown::log_join_result("api", handle.await);
        }
        if let Some(handle) = worker_handle.take() {
            shutdown::log_join_result("worker", handle.await);
        }
    };

    let drain_result = shutdown
        .drain_with_deadline(drain, || {
            if let Some(a) = api_abort {
                a.abort();
            }
            if let Some(a) = worker_abort {
                a.abort();
            }
        })
        .await;

    pool.close().await;

    match drain_result {
        Ok(()) => {
            if let Some(name) = unexpected {
                return Err(AppError::Shutdown(format!(
                    "{name} task exited unexpectedly"
                ))
                .into());
            }
            info!("shutdown complete");
            Ok(())
        }
        Err(err) => {
            error!(error = %err, "shutdown drain failed");
            Err(err.into())
        }
    }
}

async fn join_optional<T>(
    slot: &mut Option<tokio::task::JoinHandle<T>>,
) -> Result<T, tokio::task::JoinError> {
    match slot.take() {
        Some(handle) => handle.await,
        None => {
            std::future::pending::<()>().await;
            unreachable!()
        }
    }
}
