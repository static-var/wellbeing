use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default = "default_bind_addr")]
    pub bind_addr: String,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub checkins: CheckinRuntimeConfig,
    #[serde(default)]
    pub heartbeat: Option<HeartbeatConfig>,
    #[serde(default)]
    pub telegram: TelegramRuntimeConfig,
    pub whisper: WhisperConfig,
    #[serde(default)]
    pub tenants: Vec<TenantConfig>,
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path).map_err(|source| AppError::ReadConfig {
            path: path.to_path_buf(),
            source,
        })?;

        let config = serde_json::from_str::<Self>(&raw).map_err(|source| AppError::ParseConfig {
            path: path.to_path_buf(),
            source,
        })?;

        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let rendered = serde_json::to_string_pretty(self)?;
        fs::write(path, rendered).map_err(|source| AppError::WriteConfig {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        if self.bind_addr.trim().is_empty() {
            return Err(AppError::InvalidConfig(
                "bind_addr must not be empty".to_string(),
            ));
        }

        if self.database.path.trim().is_empty() {
            return Err(AppError::InvalidConfig(
                "database.path must not be empty".to_string(),
            ));
        }

        if self.tenants.is_empty() {
            return Err(AppError::InvalidConfig(
                "at least one tenant must be configured".to_string(),
            ));
        }

        let mut tenant_ids = HashSet::new();
        let mut routes = HashSet::new();
        let reserved_routes = HashSet::from(["/health", "/tenants"]);

        for tenant in &self.tenants {
            tenant.validate()?;

            if !tenant_ids.insert(tenant.id.clone()) {
                return Err(AppError::InvalidConfig(format!(
                    "duplicate tenant id '{}'",
                    tenant.id
                )));
            }

            if reserved_routes.contains(tenant.route.as_str()) {
                return Err(AppError::InvalidConfig(format!(
                    "tenant route '{}' is reserved",
                    tenant.route
                )));
            }

            if !routes.insert(tenant.route.clone()) {
                return Err(AppError::InvalidConfig(format!(
                    "duplicate tenant route '{}'",
                    tenant.route
                )));
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_database_path")]
    pub path: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: default_database_path(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CheckinRuntimeConfig {
    #[serde(default = "default_checkin_tick_interval_secs")]
    pub tick_interval_secs: u64,
    #[serde(default = "default_checkin_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_recent_activity_grace_minutes")]
    pub recent_activity_grace_minutes: i64,
}

impl Default for CheckinRuntimeConfig {
    fn default() -> Self {
        Self {
            tick_interval_secs: default_checkin_tick_interval_secs(),
            batch_size: default_checkin_batch_size(),
            recent_activity_grace_minutes: default_recent_activity_grace_minutes(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TelegramRuntimeConfig {
    #[serde(default = "default_telegram_poll_interval_secs")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_telegram_api_base_url")]
    pub api_base_url: String,
}

impl Default for TelegramRuntimeConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: default_telegram_poll_interval_secs(),
            api_base_url: default_telegram_api_base_url(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WhisperConfig {
    #[serde(default = "default_whisper_worker_url")]
    pub worker_url: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_whisper_timeout_secs")]
    pub timeout_secs: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HeartbeatConfig {
    pub url: String,
    #[serde(default = "default_heartbeat_interval_secs")]
    pub interval_secs: u64,
    #[serde(default)]
    pub method: HeartbeatMethod,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HeartbeatMethod {
    #[default]
    Get,
    Post,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TenantConfig {
    pub id: String,
    pub display_name: String,
    pub route: String,
    pub agent_path: String,
    pub bootstrap_path: String,
    pub memory_path: String,
    pub model: ModelConfig,
    #[serde(default)]
    pub proactive: ProactiveConfig,
    #[serde(default)]
    pub gateways: GatewayBindings,
}

impl TenantConfig {
    pub fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() {
            return Err(AppError::InvalidConfig(
                "tenant id must not be empty".to_string(),
            ));
        }

        if !self.route.starts_with('/') {
            return Err(AppError::InvalidConfig(format!(
                "tenant route '{}' must start with '/'",
                self.route
            )));
        }

        if self.display_name.trim().is_empty() {
            return Err(AppError::InvalidConfig(format!(
                "tenant '{}' display_name must not be empty",
                self.id
            )));
        }

        if self.gateways.enabled_names().is_empty() {
            return Err(AppError::InvalidConfig(format!(
                "tenant '{}' must enable at least one gateway",
                self.id
            )));
        }

        Ok(())
    }

    pub fn resolve_path(&self, base_dir: &Path, value: &str) -> PathBuf {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            base_dir.join(path)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelConfig {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ProactiveConfig {
    #[serde(default)]
    pub gentle_checkins_enabled: bool,
    #[serde(default)]
    pub quiet_hours: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct GatewayBindings {
    #[serde(default)]
    pub web: Option<WebGatewayConfig>,
    #[serde(default)]
    pub telegram: Option<TokenGatewayConfig>,
    #[serde(default)]
    pub whatsapp: Option<TokenGatewayConfig>,
    #[serde(default)]
    pub discord: Option<TokenGatewayConfig>,
}

impl GatewayBindings {
    pub fn enabled_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();

        if self.web.as_ref().is_some_and(|gateway| gateway.enabled) {
            names.push("web");
        }

        if self.telegram.as_ref().is_some_and(|gateway| gateway.enabled) {
            names.push("telegram");
        }

        if self.whatsapp.as_ref().is_some_and(|gateway| gateway.enabled) {
            names.push("whatsapp");
        }

        if self.discord.as_ref().is_some_and(|gateway| gateway.enabled) {
            names.push("discord");
        }

        names
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WebGatewayConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TokenGatewayConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(default)]
    pub binding: Option<String>,
}

fn default_bind_addr() -> String {
    "127.0.0.1:8080".to_string()
}

fn default_database_path() -> String {
    "data/wellbeing.sqlite".to_string()
}

fn default_checkin_tick_interval_secs() -> u64 {
    60
}

fn default_checkin_batch_size() -> usize {
    25
}

fn default_recent_activity_grace_minutes() -> i64 {
    360
}

fn default_telegram_poll_interval_secs() -> u64 {
    5
}

fn default_telegram_api_base_url() -> String {
    "https://api.telegram.org".to_string()
}

fn default_whisper_worker_url() -> String {
    "http://127.0.0.1:9000".to_string()
}

fn default_whisper_timeout_secs() -> u64 {
    30
}

fn default_heartbeat_interval_secs() -> u64 {
    60
}

fn default_true() -> bool {
    true
}
