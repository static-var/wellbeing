use std::path::Path;

use serde::Serialize;

use crate::{
    config::{GatewayBindings, ModelConfig, ProactiveConfig, TenantConfig},
    error::{AppError, Result},
};

#[derive(Clone, Debug)]
pub struct TenantRuntime {
    pub id: String,
    pub display_name: String,
    pub route: String,
    pub persona: String,
    pub bootstrap: String,
    pub memory_path: String,
    pub gateways: GatewayBindings,
    pub model: ModelConfig,
    pub proactive: ProactiveConfig,
}

impl TenantRuntime {
    pub fn from_config(base_dir: &Path, config: &TenantConfig) -> Result<Self> {
        let agent_path = config.resolve_path(base_dir, &config.agent_path);
        let bootstrap_path = config.resolve_path(base_dir, &config.bootstrap_path);

        let persona = std::fs::read_to_string(&agent_path).map_err(|source| AppError::ReadFile {
            path: agent_path,
            source,
        })?;

        let bootstrap =
            std::fs::read_to_string(&bootstrap_path).map_err(|source| AppError::ReadFile {
                path: bootstrap_path,
                source,
            })?;

        Ok(Self {
            id: config.id.clone(),
            display_name: config.display_name.clone(),
            route: config.route.clone(),
            persona,
            bootstrap,
            memory_path: config.memory_path.clone(),
            gateways: config.gateways.clone(),
            model: config.model.clone(),
            proactive: config.proactive.clone(),
        })
    }

    pub fn summary(&self) -> TenantSummary {
        TenantSummary {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            route: self.route.clone(),
            memory_path: self.memory_path.clone(),
            model_provider: self.model.provider.clone(),
            model_base_url: self.model.base_url.clone(),
            model_name: self.model.model.clone(),
            model_api_key_env: self.model.api_key_env.clone(),
            enabled_gateways: self
                .gateways
                .enabled_names()
                .into_iter()
                .map(str::to_string)
                .collect(),
            gentle_checkins_enabled: self.proactive.gentle_checkins_enabled,
            persona_preview: preview(&self.persona),
            bootstrap_preview: preview(&self.bootstrap),
        }
    }

    pub fn update_model(&mut self, model: ModelConfig) {
        self.model = model;
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct TenantSummary {
    pub id: String,
    pub display_name: String,
    pub route: String,
    pub memory_path: String,
    pub model_provider: String,
    pub model_base_url: String,
    pub model_name: String,
    pub model_api_key_env: Option<String>,
    pub enabled_gateways: Vec<String>,
    pub gentle_checkins_enabled: bool,
    pub persona_preview: String,
    pub bootstrap_preview: String,
}

fn preview(value: &str) -> String {
    const MAX_LEN: usize = 180;

    let trimmed = value.trim();
    if trimmed.len() <= MAX_LEN {
        trimmed.to_string()
    } else {
        format!("{}...", &trimmed[..MAX_LEN])
    }
}
