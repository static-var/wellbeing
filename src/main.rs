mod app;
mod auth;
mod checkins;
mod companion;
mod config;
mod database;
mod email;
mod error;
mod guardrails;
mod heartbeat;
mod provider;
mod secrets;
mod telegram;
mod tenant;
mod turnstile;
mod whisper;

use std::{env, net::SocketAddr, path::PathBuf, sync::Arc};

use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::{
    app::AppState, config::AppConfig, database::AppDatabase, email::EmailVerificationRuntime,
    error::Result, tenant::TenantRuntime, turnstile::TurnstileRuntime,
};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let config_path = parse_config_path()?;
    let config = AppConfig::load(&config_path)?;
    let config_dir = config_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let project_root = config_dir
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| config_dir.clone());
    let bind_addr = config.bind_addr.clone();
    let checkins = config.checkins.clone();
    let telegram = config.telegram.clone();
    let whisper = config.whisper.clone();
    let database_path = resolve_runtime_path(&project_root, &config.database.path);
    let heartbeat = config.heartbeat.clone();
    let web_root = project_root.join("static");

    let tenants = config
        .tenants
        .iter()
        .map(|tenant| TenantRuntime::from_config(&config_dir, tenant))
        .collect::<Result<Vec<_>>>()?;

    let database = Arc::new(AppDatabase::open(database_path)?);
    let turnstile = TurnstileRuntime::from_env()?;
    let email_verification = EmailVerificationRuntime::from_env().await?;
    let state = AppState::new(
        config_path.clone(),
        config,
        tenants,
        database.clone(),
        web_root,
        turnstile,
        email_verification,
    );
    heartbeat::spawn_heartbeat(heartbeat, state.tenant_count().await);
    checkins::spawn_checkin_scheduler(
        database.clone(),
        state.http_client(),
        telegram.clone(),
        checkins,
    );
    telegram::spawn_gateway(state.clone(), telegram, whisper);

    let app = app::router(state);
    let addr: SocketAddr = bind_addr
        .parse()
        .map_err(|error| error::AppError::InvalidConfig(format!("invalid bind_addr: {error}")))?;

    let listener = TcpListener::bind(addr).await?;
    info!("sqlite runtime state at {}", database.path().display());
    info!("wellbeing runtime listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}

fn parse_config_path() -> Result<PathBuf> {
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        if arg == "--config" {
            if let Some(value) = args.next() {
                return Ok(PathBuf::from(value));
            }
        }
    }

    Ok(PathBuf::from("config/config.json"))
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();
}

fn resolve_runtime_path(base_dir: &std::path::Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}
