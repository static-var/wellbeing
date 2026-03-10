use std::env;

use serde::{Deserialize, Serialize};

use crate::{
    config::ModelConfig,
    error::{AppError, Result},
};

pub const GEMINI_PROVIDER: &str = "gemini-openai";
pub const GEMINI_OPENAI_BASE_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/openai";

#[derive(Clone, Debug, Serialize)]
pub struct ProviderMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
struct ChatCompletionsRequest {
    model: String,
    messages: Vec<ProviderMessage>,
    temperature: f32,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: AssistantMessage,
}

#[derive(Debug, Deserialize)]
struct AssistantMessage {
    content: String,
}

#[derive(Clone, Debug)]
pub struct ResolvedProviderConfig {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
}

impl ResolvedProviderConfig {
    pub fn from_tenant(model: &ModelConfig) -> Self {
        Self {
            provider: model.provider.clone(),
            base_url: model.base_url.clone(),
            model: model.model.clone(),
            api_key: None,
            api_key_env: model.api_key_env.clone(),
        }
    }

    pub fn gemini_personal(api_key: String, model: Option<String>) -> Self {
        Self {
            provider: GEMINI_PROVIDER.to_string(),
            base_url: GEMINI_OPENAI_BASE_URL.to_string(),
            model: model.unwrap_or_else(|| "gemini-2.5-flash".to_string()),
            api_key: Some(api_key),
            api_key_env: None,
        }
    }
}

pub async fn generate_reply(
    client: &reqwest::Client,
    model: &ResolvedProviderConfig,
    messages: Vec<ProviderMessage>,
) -> Result<String> {
    let provider = model.provider.trim().to_ascii_lowercase();
    if !matches!(provider.as_str(), "gemini" | "gemini-openai" | "openai-compatible") {
        return Err(AppError::InvalidConfig(format!(
            "provider '{}' is not supported; only Gemini's OpenAI-compatible endpoint is allowed",
            model.provider
        )));
    }

    let token = match model.api_key.clone() {
        Some(value) => value,
        None => {
            let api_key_env = model
                .api_key_env
                .clone()
                .unwrap_or_else(|| default_api_key_env(&provider));
            env::var(&api_key_env).map_err(|_| {
                AppError::InvalidState(format!(
                    "missing API token environment variable '{api_key_env}' for provider '{}'",
                    model.provider
                ))
            })?
        }
    };

    let base_url = resolved_base_url(&provider, &model.base_url);
    let url = format!("{base_url}/chat/completions");
    let response = client
        .post(url)
        .bearer_auth(token)
        .header("Content-Type", "application/json")
        .json(&ChatCompletionsRequest {
            model: model.model.clone(),
            messages,
            temperature: 0.8,
        })
        .send()
        .await?
        .error_for_status()?
        .json::<ChatCompletionsResponse>()
        .await?;

    response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .filter(|content| !content.trim().is_empty())
        .ok_or_else(|| AppError::InvalidState("provider returned no message content".to_string()))
}

fn default_api_key_env(provider: &str) -> String {
    let _ = provider;
    "GEMINI_API_KEY".to_string()
}

fn resolved_base_url(provider: &str, configured: &str) -> String {
    let trimmed = configured.trim().trim_end_matches('/');
    if !trimmed.is_empty() && trimmed != GEMINI_OPENAI_BASE_URL {
        return GEMINI_OPENAI_BASE_URL.to_string();
    }

    let _ = provider;
    GEMINI_OPENAI_BASE_URL.to_string()
}
