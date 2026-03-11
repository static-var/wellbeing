use std::env;

use serde::Deserialize;

use crate::error::{AppError, Result};

const VERIFY_URL: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";

#[derive(Clone, Default)]
pub struct TurnstileRuntime {
    site_key: Option<String>,
    secret_key: Option<String>,
}

impl TurnstileRuntime {
    pub fn from_env() -> Result<Self> {
        let site_key = env::var("WELLBEING_TURNSTILE_SITE_KEY")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let secret_key = env::var("WELLBEING_TURNSTILE_SECRET_KEY")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        match (site_key, secret_key) {
            (None, None) => Ok(Self::default()),
            (Some(site_key), Some(secret_key)) => Ok(Self {
                site_key: Some(site_key),
                secret_key: Some(secret_key),
            }),
            _ => Err(AppError::InvalidConfig(
                "set both WELLBEING_TURNSTILE_SITE_KEY and WELLBEING_TURNSTILE_SECRET_KEY to enable Turnstile"
                    .to_string(),
            )),
        }
    }

    pub fn site_key(&self) -> Option<&str> {
        self.site_key.as_deref()
    }

    pub async fn verify_signup_token(
        &self,
        client: &reqwest::Client,
        token: Option<&str>,
    ) -> Result<()> {
        let Some(secret_key) = self.secret_key.as_deref() else {
            return Ok(());
        };
        let token = token
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::Security("complete the Turnstile check before signing up".to_string())
            })?;

        let response = client
            .post(VERIFY_URL)
            .form(&[("secret", secret_key), ("response", token)])
            .send()
            .await?;
        let envelope = response.json::<TurnstileVerifyResponse>().await?;
        if envelope.success {
            Ok(())
        } else {
            let reason = envelope
                .error_codes
                .into_iter()
                .next()
                .unwrap_or_else(|| "verification_failed".to_string());
            Err(AppError::Security(format!(
                "Turnstile verification failed: {reason}"
            )))
        }
    }
}

#[derive(Debug, Deserialize)]
struct TurnstileVerifyResponse {
    success: bool,
    #[serde(rename = "error-codes", default)]
    error_codes: Vec<String>,
}
