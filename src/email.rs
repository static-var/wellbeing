use std::env;

use aws_config::BehaviorVersion;
use aws_sdk_sesv2::{
    types::{Body, Content, Destination, EmailContent, Message},
    Client,
};
use reqwest::Url;

use crate::error::{AppError, Result};

#[derive(Clone, Default)]
pub struct EmailVerificationRuntime {
    client: Option<Client>,
    from_email: Option<String>,
    public_base_url: Option<Url>,
}

impl EmailVerificationRuntime {
    pub async fn from_env() -> Result<Self> {
        let from_email = env::var("WELLBEING_SES_FROM_EMAIL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let public_base_url = env::var("WELLBEING_PUBLIC_BASE_URL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if from_email.is_none() && public_base_url.is_none() {
            return Ok(Self::default());
        }

        let from_email = from_email.ok_or_else(|| {
            AppError::InvalidConfig(
                "WELLBEING_SES_FROM_EMAIL is required when email verification is enabled"
                    .to_string(),
            )
        })?;
        let public_base_url = public_base_url.ok_or_else(|| {
            AppError::InvalidConfig(
                "WELLBEING_PUBLIC_BASE_URL is required when email verification is enabled"
                    .to_string(),
            )
        })?;
        let public_base_url = Url::parse(&public_base_url).map_err(|error| {
            AppError::InvalidConfig(format!(
                "WELLBEING_PUBLIC_BASE_URL must be a valid absolute URL: {error}"
            ))
        })?;

        let config = aws_config::defaults(BehaviorVersion::latest()).load().await;
        Ok(Self {
            client: Some(Client::new(&config)),
            from_email: Some(from_email),
            public_base_url: Some(public_base_url),
        })
    }

    pub fn enabled(&self) -> bool {
        self.client.is_some()
    }

    pub async fn send_verification_email(
        &self,
        tenant_name: &str,
        recipient_email: &str,
        token: &str,
    ) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| {
            AppError::InvalidState("email verification is not configured".to_string())
        })?;
        let from_email = self.from_email.as_deref().ok_or_else(|| {
            AppError::InvalidState("email verification sender address is missing".to_string())
        })?;
        let verification_url = self.verification_url(token)?;
        let subject = format!("Verify your email for {tenant_name}");
        let text_body = format!(
            "Welcome to {tenant_name}.\n\nClick this link to verify your email and activate your account:\n\n{verification_url}\n\nThis link expires in 24 hours.\n\nIf you did not request this, you can ignore this email."
        );
        let html_body = format!(
            "<p>Welcome to {tenant_name}.</p>\
<p>Click the link below to verify your email and activate your account:</p>\
<p><a href=\"{verification_url}\">{verification_url}</a></p>\
<p>This link expires in 24 hours.</p>\
<p>If you did not request this, you can ignore this email.</p>"
        );

        client
            .send_email()
            .from_email_address(from_email)
            .destination(Destination::builder().to_addresses(recipient_email).build())
            .content(
                EmailContent::builder()
                    .simple(
                        Message::builder()
                            .subject(build_content(subject)?)
                            .body(
                                Body::builder()
                                    .text(build_content(text_body)?)
                                    .html(build_content(html_body)?)
                                    .build(),
                            )
                            .build(),
                    )
                    .build(),
            )
            .send()
            .await
            .map_err(|error| {
                AppError::InvalidState(format!("failed to send verification email: {error}"))
            })?;

        Ok(())
    }

    fn verification_url(&self, token: &str) -> Result<Url> {
        let mut url = self.public_base_url.clone().ok_or_else(|| {
            AppError::InvalidState("email verification base URL is missing".to_string())
        })?;
        url.set_path("verify-email");
        url.set_query(Some(&format!("token={token}")));
        Ok(url)
    }
}

fn build_content(data: String) -> Result<Content> {
    Content::builder()
        .data(data)
        .charset("UTF-8")
        .build()
        .map_err(|error| {
            AppError::InvalidState(format!("failed to build SES email content: {error}"))
        })
}
