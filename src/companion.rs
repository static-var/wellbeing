use chrono::Utc;

use crate::{
    database::{AppDatabase, AuthenticatedAccount, ChatMessageRecord, DueCheckin},
    error::{AppError, Result},
    guardrails::{self, GuardrailDecision},
    provider::{self, ProviderMessage, ResolvedProviderConfig},
    tenant::TenantRuntime,
};

pub async fn respond_to_user_message(
    database: &AppDatabase,
    client: &reqwest::Client,
    tenant: &TenantRuntime,
    account: &AuthenticatedAccount,
    input: &str,
) -> Result<ChatMessageRecord> {
    let message = input.trim();
    if message.is_empty() {
        return Err(AppError::InvalidState("message must not be empty".to_string()));
    }

    database.append_chat_message(account.id, "user", message)?;

    let reply = match guardrails::evaluate_user_message(message) {
        GuardrailDecision::Reply(message) => message,
        GuardrailDecision::Allow => {
            let history = database.list_chat_messages(account.id, 24)?;
            let latest_memory_summary = database.latest_memory_summary(account.id)?;
            let mut messages = Vec::with_capacity(history.len() + 1);
            messages.push(ProviderMessage {
                role: "system".to_string(),
                content: guardrails::system_prompt(
                    tenant,
                    &account.profile,
                    latest_memory_summary.as_deref(),
                ),
            });
            for message in history {
                messages.push(ProviderMessage {
                    role: message.role,
                    content: message.content,
                });
            }

            let provider_config = resolve_provider_config(database, tenant, account)?;
            let raw_reply = provider::generate_reply(client, &provider_config, messages).await?;
            guardrails::sanitize_assistant_reply(raw_reply)
        }
    };

    database.append_chat_message(account.id, "assistant", &reply)?;
    maybe_refresh_memory_summary(database, account)?;
    Ok(ChatMessageRecord {
        role: "assistant".to_string(),
        content: reply,
        created_at: Utc::now().to_rfc3339(),
    })
}

pub fn build_checkin_message(user: &DueCheckin) -> String {
    let greeting = user.user_name.as_deref().unwrap_or("there");
    let companion = &user.companion_name;

    match user.checkin_style.as_deref() {
        Some("scale") => format!(
            "Hi {greeting}, it's {companion}. Gentle check-in: how are you feeling right now on a 1-5 scale?"
        ),
        Some("prompt") => format!(
            "Hi {greeting}, it's {companion}. Here's a gentle check-in prompt: what feeling has been with you most today?"
        ),
        _ => format!(
            "Hi {greeting}, it's {companion}. Just checking in gently. How are you feeling today?"
        ),
    }
}

fn resolve_provider_config(
    database: &AppDatabase,
    tenant: &TenantRuntime,
    account: &AuthenticatedAccount,
) -> Result<ResolvedProviderConfig> {
    if account.profile.personal_inference_enabled {
        let api_key = database.personal_inference_api_key(account.id)?.ok_or_else(|| {
            AppError::InvalidState(
                "personal inference is enabled but no Gemini API key is stored".to_string(),
            )
        })?;
        return Ok(ResolvedProviderConfig::gemini_personal(
            api_key,
            account.profile.personal_inference_model.clone(),
        ));
    }

    Ok(ResolvedProviderConfig::from_tenant(&tenant.model))
}

fn maybe_refresh_memory_summary(
    database: &AppDatabase,
    account: &AuthenticatedAccount,
) -> Result<()> {
    let history = database.list_chat_messages(account.id, 12)?;
    let user_messages = history
        .iter()
        .filter(|message| message.role == "user")
        .map(|message| message.content.trim())
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>();
    if user_messages.len() < 3 {
        return Ok(());
    }

    let recent_themes = user_messages
        .iter()
        .rev()
        .take(4)
        .rev()
        .map(|content| truncate(content, 140))
        .collect::<Vec<_>>();

    let summary = format!(
        "Companion: {}. User: {}. Goals: {}. Boundaries: {}. Recent themes: {}.",
        account.profile.companion_name,
        account.profile.user_name.as_deref().unwrap_or("not shared"),
        account
            .profile
            .support_goals
            .as_deref()
            .unwrap_or("not shared"),
        account
            .profile
            .boundaries
            .as_deref()
            .unwrap_or("no explicit boundaries recorded"),
        recent_themes.join(" | ")
    );

    if database.latest_memory_summary(account.id)?.as_deref() != Some(summary.as_str()) {
        database.append_memory_summary(account.id, &summary)?;
    }

    Ok(())
}

fn truncate(value: &str, max_len: usize) -> String {
    if value.chars().count() <= max_len {
        value.to_string()
    } else {
        let truncated = value.chars().take(max_len).collect::<String>();
        format!("{truncated}...")
    }
}
