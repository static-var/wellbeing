use chrono::Utc;

use crate::{
    database::{AppDatabase, AuthenticatedAccount, ChatMessageRecord, DueCheckin},
    error::{AppError, Result},
    guardrails::{self, GuardrailDecision},
    provider::{self, ProviderMessage},
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
            let mut messages = Vec::with_capacity(history.len() + 1);
            messages.push(ProviderMessage {
                role: "system".to_string(),
                content: guardrails::system_prompt(tenant, &account.profile),
            });
            for message in history {
                messages.push(ProviderMessage {
                    role: message.role,
                    content: message.content,
                });
            }

            provider::generate_reply(client, &tenant.model, messages).await?
        }
    };

    database.append_chat_message(account.id, "assistant", &reply)?;
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
