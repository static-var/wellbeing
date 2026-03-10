use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::{
    error::{AppError, Result},
    secrets,
};

#[derive(Clone, Debug)]
pub struct AppDatabase {
    path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct AccountRecord {
    pub id: i64,
    pub password_hash: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuthenticatedAccount {
    pub id: i64,
    pub tenant_id: String,
    pub email: String,
    pub created_at: String,
    pub profile: ProfileRecord,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileRecord {
    pub companion_name: String,
    pub user_name: Option<String>,
    pub pronouns: Option<String>,
    pub user_context: Option<String>,
    pub boundaries: Option<String>,
    pub support_goals: Option<String>,
    pub preferred_style: Option<String>,
    pub companion_tone: Option<String>,
    pub checkin_frequency: Option<String>,
    pub checkin_style: Option<String>,
    pub telegram_bot_token: Option<String>,
    pub telegram_bot_username: Option<String>,
    pub personal_inference_enabled: bool,
    pub personal_inference_model: Option<String>,
    pub personal_inference_api_key_configured: bool,
    pub onboarding_complete: bool,
    pub checkins_enabled: bool,
    pub timezone: String,
    pub checkin_local_time: String,
    pub checkin_days: Vec<u32>,
    pub quiet_hours: Vec<String>,
    pub last_active_at: Option<String>,
    pub next_checkin_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChatMessageRecord {
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Clone, Debug)]
pub struct MemoryItemRecord {
    pub kind: String,
    pub content: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SessionMetrics {
    pub message_count: usize,
    pub started_at: Option<String>,
    pub last_message_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct UpsertProfileInput {
    pub companion_name: String,
    pub user_name: Option<String>,
    pub pronouns: Option<String>,
    pub user_context: Option<String>,
    pub boundaries: Option<String>,
    pub support_goals: Option<String>,
    pub preferred_style: Option<String>,
    pub companion_tone: Option<String>,
    pub checkin_frequency: Option<String>,
    pub checkin_style: Option<String>,
    pub telegram_bot_token: Option<String>,
    pub telegram_bot_username: Option<String>,
    pub personal_inference_enabled: bool,
    pub personal_inference_model: Option<String>,
    pub personal_inference_api_key: Option<String>,
    pub onboarding_complete: bool,
    pub checkins_enabled: bool,
    pub timezone: String,
    pub checkin_local_time: String,
    pub checkin_days: Vec<u32>,
    pub quiet_hours: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct DueCheckin {
    pub account_id: i64,
    pub tenant_id: String,
    pub email: String,
    pub companion_name: String,
    pub user_name: Option<String>,
    pub timezone: String,
    pub preferred_channel: Option<String>,
    pub cadence_days: i64,
    pub checkin_style: Option<String>,
    pub checkin_local_time: String,
    pub checkin_days: Vec<u32>,
    pub quiet_hours: Vec<String>,
    pub telegram_bot_token: Option<String>,
    pub telegram_chat_id: Option<i64>,
    pub last_active_at: Option<DateTime<Utc>>,
    pub next_checkin_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct TelegramBotRecord {
    pub account_id: i64,
    pub tenant_id: String,
    pub bot_token: String,
}

impl AppDatabase {
    pub fn open(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| AppError::CreateDirectory {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let database = Self { path };
        database.init_schema()?;
        Ok(database)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn create_account(
        &self,
        tenant_id: &str,
        email: &str,
        password_hash: &str,
        default_companion_name: &str,
    ) -> Result<AuthenticatedAccount> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();

        connection.execute(
            r#"
            INSERT INTO accounts (tenant_id, email, password_hash, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?4)
            "#,
            params![tenant_id, email, password_hash, now],
        )?;
        let account_id = connection.last_insert_rowid();

        connection.execute(
            r#"
            INSERT INTO profiles (
                account_id,
                companion_name,
                onboarding_complete,
                checkins_enabled,
                timezone,
                checkin_local_time,
                checkin_days_json,
                quiet_hours_json,
                created_at,
                updated_at
            )
            VALUES (?1, ?2, 0, 0, 'UTC', '09:00', '[]', '[]', ?3, ?3)
            "#,
            params![account_id, default_companion_name, now],
        )?;

        self.get_account_with_profile(account_id)?
            .ok_or_else(|| AppError::InvalidState("new account was not readable".to_string()))
    }

    pub fn find_account_by_email_in_tenant(
        &self,
        tenant_id: &str,
        email: &str,
    ) -> Result<Option<AccountRecord>> {
        let connection = self.connect()?;
        connection
            .query_row(
                r#"
                SELECT id, tenant_id, email, password_hash
                FROM accounts
                WHERE tenant_id = ?1 AND email = ?2 AND deleted_at IS NULL
                "#,
                params![tenant_id, email],
                |row| {
                    Ok(AccountRecord {
                        id: row.get(0)?,
                        password_hash: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn create_session(
        &self,
        account_id: i64,
        session_token_hash: &str,
        expires_at: &str,
    ) -> Result<()> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();
        connection.execute(
            r#"
            INSERT INTO sessions (account_id, session_token_hash, expires_at, created_at, last_seen_at)
            VALUES (?1, ?2, ?3, ?4, ?4)
            "#,
            params![account_id, session_token_hash, expires_at, now],
        )?;
        Ok(())
    }

    pub fn delete_session(&self, session_token_hash: &str) -> Result<()> {
        let connection = self.connect()?;
        connection.execute(
            "DELETE FROM sessions WHERE session_token_hash = ?1",
            params![session_token_hash],
        )?;
        Ok(())
    }

    pub fn get_account_by_session(
        &self,
        session_token_hash: &str,
        now: &str,
    ) -> Result<Option<AuthenticatedAccount>> {
        let connection = self.connect()?;
        let account_id: Option<i64> = connection
            .query_row(
                r#"
                SELECT account_id
                FROM sessions
                WHERE session_token_hash = ?1
                  AND expires_at > ?2
                "#,
                params![session_token_hash, now],
                |row| row.get(0),
            )
            .optional()?;

        let Some(account_id) = account_id else {
            return Ok(None);
        };

        connection.execute(
            "UPDATE sessions SET last_seen_at = ?1 WHERE session_token_hash = ?2",
            params![Utc::now().to_rfc3339(), session_token_hash],
        )?;

        self.get_account_with_profile(account_id)
    }

    pub fn get_account_with_profile(&self, account_id: i64) -> Result<Option<AuthenticatedAccount>> {
        let connection = self.connect()?;
        connection
            .query_row(
                r#"
                SELECT
                    a.id,
                    a.tenant_id,
                    a.email,
                    a.created_at,
                    p.companion_name,
                    p.user_name,
                    p.pronouns,
                    p.user_context,
                    p.boundaries,
                    p.support_goals,
                    p.preferred_style,
                    p.companion_tone,
                    p.checkin_frequency,
                    p.checkin_style,
                    p.telegram_bot_token,
                    p.telegram_bot_username,
                    p.personal_inference_enabled,
                    p.personal_inference_model,
                    p.personal_inference_api_key_encrypted,
                    p.onboarding_complete,
                    p.checkins_enabled,
                    p.timezone,
                    p.checkin_local_time,
                    p.checkin_days_json,
                    p.quiet_hours_json,
                    p.last_active_at,
                    p.next_checkin_at
                FROM accounts a
                JOIN profiles p ON p.account_id = a.id
                WHERE a.id = ?1 AND a.deleted_at IS NULL
                "#,
                params![account_id],
                |row| Ok(map_account_with_profile(row)),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn find_account_by_telegram_bot_token(
        &self,
        bot_token: &str,
    ) -> Result<Option<AuthenticatedAccount>> {
        let connection = self.connect()?;
        let account_id: Option<i64> = connection
            .query_row(
                r#"
                SELECT account_id
                FROM profiles
                WHERE telegram_bot_token = ?1
                "#,
                params![bot_token],
                |row| row.get(0),
            )
            .optional()?;

        let Some(account_id) = account_id else {
            return Ok(None);
        };

        self.get_account_with_profile(account_id)
    }

    pub fn update_profile(&self, account_id: i64, input: &UpsertProfileInput) -> Result<ProfileRecord> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();
        let next_checkin_at = if input.checkins_enabled {
            Some(now.clone())
        } else {
            None
        };
        let existing_encrypted_key: Option<String> = connection
            .query_row(
                "SELECT personal_inference_api_key_encrypted FROM profiles WHERE account_id = ?1",
                params![account_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let personal_inference_api_key_encrypted = if input.personal_inference_enabled {
            match normalize_optional(&input.personal_inference_api_key) {
                Some(api_key) => Some(secrets::encrypt_user_secret(&api_key)?),
                None => existing_encrypted_key,
            }
        } else {
            None
        };

        connection.execute(
            r#"
            UPDATE profiles
            SET companion_name = ?1,
                user_name = ?2,
                pronouns = ?3,
                user_context = ?4,
                boundaries = ?5,
                support_goals = ?6,
                preferred_style = ?7,
                companion_tone = ?8,
                checkin_frequency = ?9,
                checkin_style = ?10,
                telegram_bot_token = ?11,
                telegram_bot_username = ?12,
                personal_inference_enabled = ?13,
                personal_inference_model = ?14,
                personal_inference_api_key_encrypted = ?15,
                onboarding_complete = ?16,
                checkins_enabled = ?17,
                timezone = ?18,
                checkin_local_time = ?19,
                checkin_days_json = ?20,
                quiet_hours_json = ?21,
                next_checkin_at = ?22,
                updated_at = ?23
            WHERE account_id = ?24
            "#,
            params![
                input.companion_name.trim(),
                normalize_optional(&input.user_name),
                normalize_optional(&input.pronouns),
                normalize_optional(&input.user_context),
                normalize_optional(&input.boundaries),
                normalize_optional(&input.support_goals),
                normalize_optional(&input.preferred_style),
                normalize_optional(&input.companion_tone),
                normalize_optional(&input.checkin_frequency),
                normalize_optional(&input.checkin_style),
                normalize_optional(&input.telegram_bot_token),
                normalize_optional(&input.telegram_bot_username),
                bool_to_i64(input.personal_inference_enabled),
                normalize_optional(&input.personal_inference_model),
                personal_inference_api_key_encrypted,
                bool_to_i64(input.onboarding_complete),
                bool_to_i64(input.checkins_enabled),
                input.timezone.trim(),
                input.checkin_local_time.trim(),
                serde_json::to_string(&input.checkin_days).map_err(|error| {
                    AppError::InvalidState(format!("invalid checkin_days payload: {error}"))
                })?,
                serde_json::to_string(&input.quiet_hours).map_err(|error| {
                    AppError::InvalidState(format!("invalid quiet_hours payload: {error}"))
                })?,
                next_checkin_at,
                now,
                account_id
            ],
        )?;

        self.get_account_with_profile(account_id)?
            .map(|account| account.profile)
            .ok_or_else(|| AppError::InvalidState("profile update failed".to_string()))
    }

    pub fn list_chat_messages(&self, account_id: i64, limit: usize) -> Result<Vec<ChatMessageRecord>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            r#"
            SELECT role, content, created_at
            FROM chat_messages
            WHERE account_id = ?1
              AND id > COALESCE((
                    SELECT MAX(id)
                    FROM chat_messages
                    WHERE account_id = ?1
                      AND role = 'session_reset'
                ), 0)
              AND role != 'session_reset'
            ORDER BY id DESC
            LIMIT ?2
            "#,
        )?;

        let rows = statement.query_map(params![account_id, limit as i64], |row| {
            Ok(ChatMessageRecord {
                role: row.get(0)?,
                content: row.get(1)?,
                created_at: row.get(2)?,
            })
        })?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }
        messages.reverse();
        Ok(messages)
    }

    pub fn current_session_metrics(&self, account_id: i64) -> Result<SessionMetrics> {
        let connection = self.connect()?;
        connection
            .query_row(
                r#"
                SELECT
                    COUNT(*) AS message_count,
                    MIN(created_at) AS started_at,
                    MAX(created_at) AS last_message_at
                FROM chat_messages
                WHERE account_id = ?1
                  AND id > COALESCE((
                        SELECT MAX(id)
                        FROM chat_messages
                        WHERE account_id = ?1
                          AND role = 'session_reset'
                    ), 0)
                  AND role != 'session_reset'
                "#,
                params![account_id],
                |row| {
                    Ok(SessionMetrics {
                        message_count: row.get::<_, i64>(0)? as usize,
                        started_at: row.get(1)?,
                        last_message_at: row.get(2)?,
                    })
                },
            )
            .map_err(Into::into)
    }

    pub fn latest_memory_summary(&self, account_id: i64) -> Result<Option<String>> {
        let connection = self.connect()?;
        connection
            .query_row(
                r#"
                SELECT summary
                FROM memory_summaries
                WHERE account_id = ?1
                ORDER BY id DESC
                LIMIT 1
                "#,
                params![account_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn append_memory_summary(&self, account_id: i64, summary: &str) -> Result<()> {
        let connection = self.connect()?;
        connection.execute(
            r#"
            INSERT INTO memory_summaries (account_id, summary, created_at)
            VALUES (?1, ?2, ?3)
            "#,
            params![account_id, summary.trim(), Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn replace_memory_items(
        &self,
        account_id: i64,
        kind: &str,
        items: &[String],
    ) -> Result<()> {
        let connection = self.connect()?;
        let transaction = connection.unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM memory_items WHERE account_id = ?1 AND kind = ?2",
            params![account_id, kind],
        )?;

        let now = Utc::now().to_rfc3339();
        for item in items {
            let trimmed = item.trim();
            if trimmed.is_empty() {
                continue;
            }

            transaction.execute(
                r#"
                INSERT INTO memory_items (account_id, kind, content, updated_at)
                VALUES (?1, ?2, ?3, ?4)
                "#,
                params![account_id, kind, trimmed, now],
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn list_memory_items(&self, account_id: i64, limit: usize) -> Result<Vec<MemoryItemRecord>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            r#"
            SELECT kind, content
            FROM memory_items
            WHERE account_id = ?1
            ORDER BY
                CASE kind
                    WHEN 'identity' THEN 0
                    WHEN 'goal' THEN 1
                    WHEN 'boundary' THEN 2
                    WHEN 'preference' THEN 3
                    WHEN 'person' THEN 4
                    WHEN 'relationship' THEN 5
                    WHEN 'key_event' THEN 6
                    WHEN 'recurring_theme' THEN 7
                    WHEN 'session_summary' THEN 8
                    ELSE 9
                END,
                updated_at DESC,
                id DESC
            LIMIT ?2
            "#,
        )?;

        let rows = statement.query_map(params![account_id, limit as i64], |row| {
            Ok(MemoryItemRecord {
                kind: row.get(0)?,
                content: row.get(1)?,
            })
        })?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    pub fn personal_inference_api_key(&self, account_id: i64) -> Result<Option<String>> {
        let connection = self.connect()?;
        let encrypted: Option<String> = connection
            .query_row(
                r#"
                SELECT personal_inference_api_key_encrypted
                FROM profiles
                WHERE account_id = ?1
                "#,
                params![account_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();

        encrypted
            .map(|value| secrets::decrypt_user_secret(&value))
            .transpose()
    }

    pub fn append_chat_message(&self, account_id: i64, role: &str, content: &str) -> Result<()> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();

        connection.execute(
            r#"
            INSERT INTO chat_messages (account_id, role, content, created_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            params![account_id, role, content, now],
        )?;

        if role == "user" {
            connection.execute(
                r#"
                UPDATE profiles
                SET last_active_at = ?1,
                    updated_at = ?1
                WHERE account_id = ?2
                "#,
                params![Utc::now().to_rfc3339(), account_id],
            )?;
        }

        Ok(())
    }

    pub fn start_new_session(&self, account_id: i64) -> Result<()> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();
        connection.execute(
            r#"
            INSERT INTO chat_messages (account_id, role, content, created_at)
            VALUES (?1, 'session_reset', 'Started a new conversation', ?2)
            "#,
            params![account_id, now],
        )?;
        Ok(())
    }

    pub fn reset_companion(&self, account_id: i64) -> Result<()> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();
        connection.execute("DELETE FROM chat_messages WHERE account_id = ?1", params![account_id])?;
        connection.execute("DELETE FROM memory_summaries WHERE account_id = ?1", params![account_id])?;
        connection.execute("DELETE FROM memory_items WHERE account_id = ?1", params![account_id])?;
        connection.execute(
            r#"
            UPDATE profiles
            SET companion_name = 'Hope',
                user_name = NULL,
                pronouns = NULL,
                user_context = NULL,
                boundaries = NULL,
                support_goals = NULL,
                preferred_style = NULL,
                companion_tone = NULL,
                checkin_frequency = NULL,
                checkin_style = NULL,
                telegram_bot_token = NULL,
                telegram_bot_username = NULL,
                personal_inference_enabled = 0,
                personal_inference_model = NULL,
                personal_inference_api_key_encrypted = NULL,
                onboarding_complete = 0,
                checkins_enabled = 0,
                timezone = 'UTC',
                checkin_local_time = '09:00',
                checkin_days_json = '[]',
                quiet_hours_json = '[]',
                last_active_at = NULL,
                last_checkin_attempted_at = NULL,
                last_checkin_sent_at = NULL,
                next_checkin_at = NULL,
                updated_at = ?1
            WHERE account_id = ?2
            "#,
            params![now, account_id],
        )?;
        connection.execute("DELETE FROM telegram_bindings WHERE account_id = ?1", params![account_id])?;
        Ok(())
    }

    pub fn delete_account(&self, account_id: i64) -> Result<()> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();
        connection.execute(
            "UPDATE accounts SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2",
            params![now, account_id],
        )?;
        connection.execute("DELETE FROM sessions WHERE account_id = ?1", params![account_id])?;
        connection.execute("DELETE FROM profiles WHERE account_id = ?1", params![account_id])?;
        connection.execute("DELETE FROM chat_messages WHERE account_id = ?1", params![account_id])?;
        connection.execute("DELETE FROM memory_summaries WHERE account_id = ?1", params![account_id])?;
        connection.execute("DELETE FROM memory_items WHERE account_id = ?1", params![account_id])?;
        connection.execute("DELETE FROM telegram_bindings WHERE account_id = ?1", params![account_id])?;
        Ok(())
    }

    pub fn due_checkins(&self, now: DateTime<Utc>, limit: usize) -> Result<Vec<DueCheckin>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            r#"
            SELECT
                a.id,
                a.tenant_id,
                a.email,
                p.companion_name,
                p.user_name,
                p.timezone,
                CASE WHEN tb.chat_id IS NOT NULL THEN 'telegram' ELSE NULL END AS preferred_channel,
                1 AS cadence_days,
                p.checkin_style,
                p.checkin_local_time,
                p.checkin_days_json,
                p.quiet_hours_json,
                p.telegram_bot_token,
                tb.chat_id,
                p.last_active_at,
                p.next_checkin_at
            FROM accounts a
            JOIN profiles p ON p.account_id = a.id
            LEFT JOIN telegram_bindings tb
              ON tb.account_id = a.id
             AND tb.bot_token = p.telegram_bot_token
            WHERE a.deleted_at IS NULL
              AND p.checkins_enabled = 1
              AND p.next_checkin_at IS NOT NULL
              AND p.next_checkin_at <= ?1
            ORDER BY p.next_checkin_at ASC
            LIMIT ?2
            "#,
        )?;

        let rows = statement.query_map(params![now.to_rfc3339(), limit as i64], |row| {
            let checkin_days_json: String = row.get(10)?;
            let quiet_hours_json: String = row.get(11)?;
            let last_active_at: Option<String> = row.get(14)?;
            let next_checkin_at: String = row.get(15)?;

            Ok(DueCheckin {
                account_id: row.get(0)?,
                tenant_id: row.get(1)?,
                email: row.get(2)?,
                companion_name: row.get(3)?,
                user_name: row.get(4)?,
                timezone: row.get(5)?,
                preferred_channel: row.get(6)?,
                cadence_days: row.get(7)?,
                checkin_style: row.get(8)?,
                checkin_local_time: row.get(9)?,
                checkin_days: serde_json::from_str(&checkin_days_json).unwrap_or_default(),
                quiet_hours: serde_json::from_str(&quiet_hours_json).unwrap_or_default(),
                telegram_bot_token: row.get(12)?,
                telegram_chat_id: row.get(13)?,
                last_active_at: last_active_at
                    .as_deref()
                    .map(parse_timestamp)
                    .transpose()
                    .map_err(map_sql_error)?,
                next_checkin_at: parse_timestamp(&next_checkin_at).map_err(map_sql_error)?,
            })
        })?;

        let mut due = Vec::new();
        for row in rows {
            due.push(row?);
        }
        Ok(due)
    }

    pub fn record_checkin_attempt(
        &self,
        account_id: i64,
        attempted_at: DateTime<Utc>,
        next_checkin_at: DateTime<Utc>,
    ) -> Result<()> {
        let connection = self.connect()?;
        connection.execute(
            r#"
            UPDATE profiles
            SET last_checkin_attempted_at = ?1,
                next_checkin_at = ?2,
                updated_at = ?1
            WHERE account_id = ?3
            "#,
            params![attempted_at.to_rfc3339(), next_checkin_at.to_rfc3339(), account_id],
        )?;
        Ok(())
    }

    pub fn defer_checkin(&self, account_id: i64, next_checkin_at: DateTime<Utc>) -> Result<()> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();
        connection.execute(
            r#"
            UPDATE profiles
            SET next_checkin_at = ?1,
                updated_at = ?2
            WHERE account_id = ?3
            "#,
            params![next_checkin_at.to_rfc3339(), now, account_id],
        )?;
        Ok(())
    }

    pub fn disable_checkins(&self, account_id: i64) -> Result<()> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();
        connection.execute(
            r#"
            UPDATE profiles
            SET checkins_enabled = 0,
                next_checkin_at = NULL,
                updated_at = ?1
            WHERE account_id = ?2
            "#,
            params![now, account_id],
        )?;
        Ok(())
    }

    pub fn record_checkin_sent(&self, account_id: i64, sent_at: DateTime<Utc>) -> Result<()> {
        let connection = self.connect()?;
        connection.execute(
            r#"
            UPDATE profiles
            SET last_checkin_sent_at = ?1,
                updated_at = ?1
            WHERE account_id = ?2
            "#,
            params![sent_at.to_rfc3339(), account_id],
        )?;
        Ok(())
    }

    pub fn list_active_telegram_bots(&self) -> Result<Vec<TelegramBotRecord>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            r#"
            SELECT a.id, a.tenant_id, p.telegram_bot_token
            FROM accounts a
            JOIN profiles p ON p.account_id = a.id
            LEFT JOIN telegram_bot_state tbs ON tbs.bot_token = p.telegram_bot_token
            WHERE a.deleted_at IS NULL
              AND p.telegram_bot_token IS NOT NULL
              AND TRIM(p.telegram_bot_token) != ''
              AND (tbs.suspended_until IS NULL OR tbs.suspended_until <= ?1)
            "#,
        )?;

        let now = Utc::now().to_rfc3339();
        let rows = statement.query_map(params![now], |row| {
            Ok(TelegramBotRecord {
                account_id: row.get(0)?,
                tenant_id: row.get(1)?,
                bot_token: row.get(2)?,
            })
        })?;

        let mut bots = Vec::new();
        for row in rows {
            bots.push(row?);
        }
        Ok(bots)
    }

    pub fn telegram_poll_offset(&self, bot_token: &str) -> Result<i64> {
        let connection = self.connect()?;
        let offset = connection
            .query_row(
                r#"
                SELECT next_update_id
                FROM telegram_bot_state
                WHERE bot_token = ?1
                "#,
                params![bot_token],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        Ok(offset)
    }

    pub fn set_telegram_poll_offset(&self, bot_token: &str, next_update_id: i64) -> Result<()> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();
        connection.execute(
            r#"
            INSERT INTO telegram_bot_state (
                bot_token,
                next_update_id,
                updated_at,
                failure_count,
                suspended_until,
                last_error
            )
            VALUES (?1, ?2, ?3, 0, NULL, NULL)
            ON CONFLICT(bot_token) DO UPDATE SET
                next_update_id = excluded.next_update_id,
                updated_at = excluded.updated_at
            "#,
            params![bot_token, next_update_id, now],
        )?;
        Ok(())
    }

    pub fn record_telegram_poll_failure(
        &self,
        bot_token: &str,
        suspended_until: Option<DateTime<Utc>>,
        error_message: &str,
    ) -> Result<()> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();
        connection.execute(
            r#"
            INSERT INTO telegram_bot_state (
                bot_token,
                next_update_id,
                updated_at,
                failure_count,
                suspended_until,
                last_error
            )
            VALUES (?1, 0, ?2, 1, ?3, ?4)
            ON CONFLICT(bot_token) DO UPDATE SET
                updated_at = excluded.updated_at,
                failure_count = telegram_bot_state.failure_count + 1,
                suspended_until = excluded.suspended_until,
                last_error = excluded.last_error
            "#,
            params![
                bot_token,
                now,
                suspended_until.map(|value| value.to_rfc3339()),
                truncate_for_storage(error_message, 500)
            ],
        )?;
        Ok(())
    }

    pub fn clear_telegram_poll_failure(&self, bot_token: &str) -> Result<()> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();
        connection.execute(
            r#"
            INSERT INTO telegram_bot_state (
                bot_token,
                next_update_id,
                updated_at,
                failure_count,
                suspended_until,
                last_error
            )
            VALUES (?1, 0, ?2, 0, NULL, NULL)
            ON CONFLICT(bot_token) DO UPDATE SET
                updated_at = excluded.updated_at,
                failure_count = 0,
                suspended_until = NULL,
                last_error = NULL
            "#,
            params![bot_token, now],
        )?;
        Ok(())
    }

    pub fn upsert_telegram_binding(
        &self,
        account_id: i64,
        bot_token: &str,
        chat_id: i64,
        telegram_user_id: Option<i64>,
        telegram_username: Option<String>,
        chat_type: &str,
    ) -> Result<()> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();
        connection.execute(
            r#"
            INSERT INTO telegram_bindings (
                account_id,
                bot_token,
                chat_id,
                telegram_user_id,
                telegram_username,
                chat_type,
                created_at,
                updated_at,
                last_inbound_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?7)
            ON CONFLICT(account_id) DO UPDATE SET
                bot_token = excluded.bot_token,
                chat_id = excluded.chat_id,
                telegram_user_id = excluded.telegram_user_id,
                telegram_username = excluded.telegram_username,
                chat_type = excluded.chat_type,
                updated_at = excluded.updated_at,
                last_inbound_at = excluded.last_inbound_at
            "#,
            params![
                account_id,
                bot_token,
                chat_id,
                telegram_user_id,
                normalize_optional(&telegram_username),
                chat_type.trim(),
                now
            ],
        )?;
        Ok(())
    }

    fn connect(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Ok(connection)
    }

    fn init_schema(&self) -> Result<()> {
        let connection = self.connect()?;
        connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS accounts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tenant_id TEXT NOT NULL,
                email TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                deleted_at TEXT
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id INTEGER NOT NULL,
                session_token_hash TEXT NOT NULL UNIQUE,
                expires_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                last_seen_at TEXT NOT NULL,
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS profiles (
                account_id INTEGER PRIMARY KEY,
                companion_name TEXT NOT NULL,
                user_name TEXT,
                pronouns TEXT,
                user_context TEXT,
                boundaries TEXT,
                support_goals TEXT,
                preferred_style TEXT,
                companion_tone TEXT,
                checkin_frequency TEXT,
                checkin_style TEXT,
                telegram_bot_token TEXT,
                telegram_bot_username TEXT,
                personal_inference_enabled INTEGER NOT NULL DEFAULT 0,
                personal_inference_model TEXT,
                personal_inference_api_key_encrypted TEXT,
                onboarding_complete INTEGER NOT NULL DEFAULT 0,
                checkins_enabled INTEGER NOT NULL DEFAULT 0,
                timezone TEXT NOT NULL DEFAULT 'UTC',
                checkin_local_time TEXT NOT NULL DEFAULT '09:00',
                checkin_days_json TEXT NOT NULL DEFAULT '[]',
                quiet_hours_json TEXT NOT NULL DEFAULT '[]',
                last_active_at TEXT,
                last_checkin_attempted_at TEXT,
                last_checkin_sent_at TEXT,
                next_checkin_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS chat_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id INTEGER NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS memory_summaries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id INTEGER NOT NULL,
                summary TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS memory_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id INTEGER NOT NULL,
                kind TEXT NOT NULL,
                content TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS telegram_bindings (
                account_id INTEGER PRIMARY KEY,
                bot_token TEXT NOT NULL,
                chat_id INTEGER NOT NULL,
                telegram_user_id INTEGER,
                telegram_username TEXT,
                chat_type TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_inbound_at TEXT,
                FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS telegram_bot_state (
                bot_token TEXT PRIMARY KEY,
                next_update_id INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL,
                failure_count INTEGER NOT NULL DEFAULT 0,
                suspended_until TEXT,
                last_error TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_sessions_token ON sessions(session_token_hash);
            CREATE INDEX IF NOT EXISTS idx_profiles_due_checkins ON profiles(checkins_enabled, next_checkin_at);
            CREATE INDEX IF NOT EXISTS idx_chat_messages_account ON chat_messages(account_id, id);
            CREATE INDEX IF NOT EXISTS idx_memory_items_account_kind ON memory_items(account_id, kind, id);
            CREATE INDEX IF NOT EXISTS idx_telegram_bindings_chat ON telegram_bindings(chat_id);
            "#,
        )?;
        ensure_profile_column(&connection, "pronouns", "TEXT")?;
        ensure_profile_column(&connection, "boundaries", "TEXT")?;
        ensure_profile_column(&connection, "companion_tone", "TEXT")?;
        ensure_profile_column(&connection, "checkin_frequency", "TEXT")?;
        ensure_profile_column(&connection, "checkin_style", "TEXT")?;
        ensure_profile_column(
            &connection,
            "personal_inference_enabled",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_profile_column(&connection, "personal_inference_model", "TEXT")?;
        ensure_profile_column(
            &connection,
            "personal_inference_api_key_encrypted",
            "TEXT",
        )?;
        ensure_table_column(
            &connection,
            "telegram_bot_state",
            "failure_count",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_table_column(&connection, "telegram_bot_state", "suspended_until", "TEXT")?;
        ensure_table_column(&connection, "telegram_bot_state", "last_error", "TEXT")?;
        Ok(())
    }
}

fn map_account_with_profile(row: &rusqlite::Row<'_>) -> AuthenticatedAccount {
    let checkin_days_json: String = row.get(23).unwrap_or_else(|_| "[]".to_string());
    let quiet_hours_json: String = row.get(24).unwrap_or_else(|_| "[]".to_string());
    let encrypted_api_key: Option<String> = row.get(18).ok();

    AuthenticatedAccount {
        id: row.get(0).unwrap_or_default(),
        tenant_id: row.get(1).unwrap_or_default(),
        email: row.get(2).unwrap_or_default(),
        created_at: row.get(3).unwrap_or_default(),
        profile: ProfileRecord {
            companion_name: row.get(4).unwrap_or_else(|_| "Companion".to_string()),
            user_name: row.get(5).ok(),
            pronouns: row.get(6).ok(),
            user_context: row.get(7).ok(),
            boundaries: row.get(8).ok(),
            support_goals: row.get(9).ok(),
            preferred_style: row.get(10).ok(),
            companion_tone: row.get(11).ok(),
            checkin_frequency: row.get(12).ok(),
            checkin_style: row.get(13).ok(),
            telegram_bot_token: row.get(14).ok(),
            telegram_bot_username: row.get(15).ok(),
            personal_inference_enabled: i64_to_bool(row.get::<_, i64>(16).unwrap_or(0)),
            personal_inference_model: row.get(17).ok(),
            personal_inference_api_key_configured: encrypted_api_key
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty()),
            onboarding_complete: i64_to_bool(row.get::<_, i64>(19).unwrap_or(0)),
            checkins_enabled: i64_to_bool(row.get::<_, i64>(20).unwrap_or(0)),
            timezone: row.get(21).unwrap_or_else(|_| "UTC".to_string()),
            checkin_local_time: row.get(22).unwrap_or_else(|_| "09:00".to_string()),
            checkin_days: serde_json::from_str(&checkin_days_json).unwrap_or_default(),
            quiet_hours: serde_json::from_str(&quiet_hours_json).unwrap_or_default(),
            last_active_at: row.get(25).ok(),
            next_checkin_at: row.get(26).ok(),
        },
    }
}

fn normalize_optional(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn bool_to_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn i64_to_bool(value: i64) -> bool {
    value != 0
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)
        .map_err(|error| AppError::InvalidState(format!("invalid timestamp '{value}': {error}")))?
        .with_timezone(&Utc))
}

fn map_sql_error(error: AppError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(error),
    )
}

fn ensure_profile_column(connection: &Connection, column: &str, sql_type: &str) -> Result<()> {
    let sql = format!("ALTER TABLE profiles ADD COLUMN {column} {sql_type}");
    match connection.execute(&sql, []) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("duplicate column name") =>
        {
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn ensure_table_column(
    connection: &Connection,
    table: &str,
    column: &str,
    sql_type: &str,
) -> Result<()> {
    let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {sql_type}");
    match connection.execute(&sql, []) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("duplicate column name") =>
        {
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn truncate_for_storage(value: &str, max_len: usize) -> String {
    value.chars().take(max_len).collect()
}
