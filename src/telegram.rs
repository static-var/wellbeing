use std::{time::Duration};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{
    app::AppState,
    companion,
    config::TelegramRuntimeConfig,
    database::TelegramBotRecord,
    error::{AppError, Result},
};

pub fn spawn_gateway(state: AppState, config: TelegramRuntimeConfig) {
    tokio::spawn(async move {
        let interval = Duration::from_secs(config.poll_interval_secs.max(1));

        loop {
            if let Err(error) = run_poll_cycle(&state, &config).await {
                warn!("telegram gateway tick failed: {error}");
            }

            tokio::time::sleep(interval).await;
        }
    });
}

pub async fn send_text_message(
    client: &reqwest::Client,
    api_base_url: &str,
    bot_token: &str,
    chat_id: i64,
    text: &str,
) -> Result<()> {
    let url = format!(
        "{}/bot{}/sendMessage",
        api_base_url.trim_end_matches('/'),
        bot_token
    );

    let response = client
        .post(url)
        .json(&SendMessageRequest {
            chat_id,
            text: text.to_string(),
        })
        .send()
        .await?
        .error_for_status()?;

    let envelope = response.json::<TelegramEnvelope<TelegramMessage>>().await?;
    if envelope.ok {
        Ok(())
    } else {
        Err(AppError::InvalidState(
            envelope
                .description
                .unwrap_or_else(|| "telegram sendMessage failed".to_string()),
        ))
    }
}

async fn run_poll_cycle(state: &AppState, config: &TelegramRuntimeConfig) -> Result<()> {
    let bots = state.database().list_active_telegram_bots()?;
    if bots.is_empty() {
        return Ok(());
    }

    for bot in bots {
        if let Err(error) = poll_bot(state, config, &bot).await {
            warn!(
                account_id = bot.account_id,
                tenant_id = %bot.tenant_id,
                error = %error,
                "telegram bot poll failed"
            );
        }
    }

    Ok(())
}

async fn poll_bot(
    state: &AppState,
    config: &TelegramRuntimeConfig,
    bot: &TelegramBotRecord,
) -> Result<()> {
    let database = state.database();
    let offset = database.telegram_poll_offset(&bot.bot_token)?;
    let updates = get_updates(&state.http_client(), &config.api_base_url, &bot.bot_token, offset).await?;
    if updates.is_empty() {
        return Ok(());
    }

    let mut next_offset = offset;
    for update in updates {
        next_offset = next_offset.max(update.update_id + 1);

        if let Some(message) = update.message {
            process_message(state, config, bot, message).await?;
        }
    }

    database.set_telegram_poll_offset(&bot.bot_token, next_offset)?;
    Ok(())
}

async fn process_message(
    state: &AppState,
    config: &TelegramRuntimeConfig,
    bot: &TelegramBotRecord,
    message: TelegramMessage,
) -> Result<()> {
    let Some(account) = state
        .database()
        .find_account_by_telegram_bot_token(&bot.bot_token)?
    else {
        warn!(
            account_id = bot.account_id,
            "telegram bot token is configured but no matching account was found"
        );
        return Ok(());
    };

    state.database().upsert_telegram_binding(
        account.id,
        &bot.bot_token,
        message.chat.id,
        message.from.as_ref().map(|from| from.id),
        message.from.as_ref().and_then(|from| from.username.clone()),
        &message.chat.chat_type,
    )?;

    let reply = if let Some(text) = message.text.as_deref() {
        if text.trim() == "/start" {
            build_start_message(&account)
        } else {
            let tenant = state
                .tenant(&account.tenant_id)
                .await
                .ok_or_else(|| AppError::InvalidState("tenant not found for telegram account".to_string()))?;
            companion::respond_to_user_message(
                state.database().as_ref(),
                &state.http_client(),
                &tenant,
                &account,
                text,
            )
            .await?
            .content
        }
    } else if message.voice.is_some() {
        "I can see your voice note came through, but audio-note support isn't wired up yet. For now, send me a text message here and I'll reply.".to_string()
    } else {
        "I can help best with text messages right now. Send me a short message and I'll reply here.".to_string()
    };

    send_text_message(
        &state.http_client(),
        &config.api_base_url,
        &bot.bot_token,
        message.chat.id,
        &reply,
    )
    .await?;
    info!(
        account_id = account.id,
        tenant_id = %account.tenant_id,
        chat_id = message.chat.id,
        "telegram message processed"
    );
    Ok(())
}

fn build_start_message(account: &crate::database::AuthenticatedAccount) -> String {
    let companion = &account.profile.companion_name;
    if account.profile.onboarding_complete {
        format!(
            "Hi{} — I'm {}. This Telegram chat is now linked, so you can talk to me here whenever you like.",
            account
                .profile
                .user_name
                .as_deref()
                .map(|name| format!(" {name}"))
                .unwrap_or_default(),
            companion
        )
    } else {
        format!(
            "Hi — I'm {}. Your Telegram bot is linked. Finish the web onboarding once, then you can keep talking to me here too.",
            companion
        )
    }
}

async fn get_updates(
    client: &reqwest::Client,
    api_base_url: &str,
    bot_token: &str,
    offset: i64,
) -> Result<Vec<TelegramUpdate>> {
    let url = format!(
        "{}/bot{}/getUpdates",
        api_base_url.trim_end_matches('/'),
        bot_token
    );

    let response = client
        .post(url)
        .json(&GetUpdatesRequest {
            offset,
            timeout: 0,
            allowed_updates: vec!["message".to_string()],
        })
        .send()
        .await?
        .error_for_status()?;

    let envelope = response
        .json::<TelegramEnvelope<Vec<TelegramUpdate>>>()
        .await?;
    if envelope.ok {
        Ok(envelope.result)
    } else {
        Err(AppError::InvalidState(
            envelope
                .description
                .unwrap_or_else(|| "telegram getUpdates failed".to_string()),
        ))
    }
}

#[derive(Debug, Serialize)]
struct GetUpdatesRequest {
    offset: i64,
    timeout: u64,
    allowed_updates: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SendMessageRequest {
    chat_id: i64,
    text: String,
}

#[derive(Debug, Deserialize)]
struct TelegramEnvelope<T> {
    ok: bool,
    result: T,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    #[serde(default)]
    message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    chat: TelegramChat,
    #[serde(default)]
    from: Option<TelegramUser>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    voice: Option<TelegramVoice>,
}

#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
    #[serde(rename = "type")]
    chat_type: String,
}

#[derive(Debug, Deserialize)]
struct TelegramUser {
    id: i64,
    #[serde(default)]
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramVoice {}
