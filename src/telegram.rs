use std::{time::Duration};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{
    app::AppState,
    companion,
    config::{TelegramRuntimeConfig, WhisperConfig},
    database::TelegramBotRecord,
    error::{AppError, Result},
    whisper,
};

pub fn spawn_gateway(state: AppState, config: TelegramRuntimeConfig, whisper_config: WhisperConfig) {
    tokio::spawn(async move {
        let interval = Duration::from_secs(config.poll_interval_secs.max(1));

        loop {
            if let Err(error) = run_poll_cycle(&state, &config, &whisper_config).await {
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

async fn run_poll_cycle(
    state: &AppState,
    config: &TelegramRuntimeConfig,
    whisper_config: &WhisperConfig,
) -> Result<()> {
    let bots = state.database().list_active_telegram_bots()?;
    if bots.is_empty() {
        return Ok(());
    }

    for bot in bots {
        if let Err(error) = poll_bot(state, config, whisper_config, &bot).await {
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
    whisper_config: &WhisperConfig,
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
            process_message(state, config, whisper_config, bot, message).await?;
        }
    }

    database.set_telegram_poll_offset(&bot.bot_token, next_offset)?;
    Ok(())
}

async fn process_message(
    state: &AppState,
    config: &TelegramRuntimeConfig,
    whisper_config: &WhisperConfig,
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
    } else if let Some(voice) = message.voice.as_ref() {
        match transcribe_voice_message(
            &state.http_client(),
            config,
            whisper_config,
            &bot.bot_token,
            voice,
        )
        .await
        {
            Ok(transcript) => {
                let tenant = state
                    .tenant(&account.tenant_id)
                    .await
                    .ok_or_else(|| {
                        AppError::InvalidState(
                            "tenant not found for telegram account".to_string(),
                        )
                    })?;
                companion::respond_to_user_message(
                    state.database().as_ref(),
                    &state.http_client(),
                    &tenant,
                    &account,
                    &transcript,
                )
                .await?
                .content
            }
            Err(error) => {
                warn!(
                    account_id = account.id,
                    tenant_id = %account.tenant_id,
                    error = %error,
                    "voice-note transcription failed"
                );
                "I couldn't transcribe that voice note right now. Please make sure your whisper.cpp server is running and that `whisper.worker_url` points to it, then try again.".to_string()
            }
        }
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

async fn transcribe_voice_message(
    client: &reqwest::Client,
    telegram_config: &TelegramRuntimeConfig,
    whisper_config: &WhisperConfig,
    bot_token: &str,
    voice: &TelegramVoice,
) -> Result<String> {
    let file_path = get_file_path(client, telegram_config, bot_token, &voice.file_id).await?;
    let bytes = download_telegram_file(client, telegram_config, bot_token, &file_path).await?;
    let file_name = file_path
        .rsplit('/')
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("voice.ogg");
    let mime_type = voice
        .mime_type
        .as_deref()
        .unwrap_or("audio/ogg");
    whisper::transcribe_audio_bytes(client, whisper_config, file_name, mime_type, bytes).await
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

#[derive(Debug, Serialize)]
struct GetFileRequest {
    file_id: String,
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
struct TelegramVoice {
    file_id: String,
    #[serde(default)]
    mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramFile {
    file_path: String,
}

async fn get_file_path(
    client: &reqwest::Client,
    config: &TelegramRuntimeConfig,
    bot_token: &str,
    file_id: &str,
) -> Result<String> {
    let url = format!(
        "{}/bot{}/getFile",
        config.api_base_url.trim_end_matches('/'),
        bot_token
    );

    let response = client
        .post(url)
        .json(&GetFileRequest {
            file_id: file_id.to_string(),
        })
        .send()
        .await?
        .error_for_status()?;

    let envelope = response.json::<TelegramEnvelope<TelegramFile>>().await?;
    if envelope.ok {
        Ok(envelope.result.file_path)
    } else {
        Err(AppError::InvalidState(
            envelope
                .description
                .unwrap_or_else(|| "telegram getFile failed".to_string()),
        ))
    }
}

async fn download_telegram_file(
    client: &reqwest::Client,
    config: &TelegramRuntimeConfig,
    bot_token: &str,
    file_path: &str,
) -> Result<Vec<u8>> {
    let url = format!(
        "{}/file/bot{}/{}",
        config.api_base_url.trim_end_matches('/'),
        bot_token,
        file_path.trim_start_matches('/')
    );

    let response = client.get(url).send().await?.error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}
