use std::env;

use serde::{Deserialize, Serialize};

use crate::{
    config::ModelConfig,
    error::{AppError, Result},
};

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

pub async fn generate_reply(
    client: &reqwest::Client,
    model: &ModelConfig,
    messages: Vec<ProviderMessage>,
) -> Result<String> {
    let api_key_env = model
        .api_key_env
        .clone()
        .unwrap_or_else(|| "GITHUB_TOKEN".to_string());
    let token = env::var(&api_key_env).map_err(|_| {
        AppError::InvalidState(format!(
            "missing API token environment variable '{api_key_env}' for provider '{}'",
            model.provider
        ))
    })?;

    let base_url = model.base_url.trim_end_matches('/');
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
