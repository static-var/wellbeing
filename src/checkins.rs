use std::{str::FromStr, sync::Arc, time::Duration};

use chrono::{DateTime, Datelike, Days, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use tracing::{info, warn};

use crate::{
    config::CheckinRuntimeConfig,
    database::{AppDatabase, DueCheckin},
    error::{AppError, Result},
};

pub fn spawn_checkin_scheduler(database: Arc<AppDatabase>, config: CheckinRuntimeConfig) {
    tokio::spawn(async move {
        let interval = Duration::from_secs(config.tick_interval_secs);

        loop {
            if let Err(error) = run_checkin_tick(&database, &config).await {
                warn!("check-in tick failed: {error}");
            }

            tokio::time::sleep(interval).await;
        }
    });
}

async fn run_checkin_tick(database: &AppDatabase, config: &CheckinRuntimeConfig) -> Result<()> {
    let now = Utc::now();
    let due = database.due_checkins(now, config.batch_size)?;

    if due.is_empty() {
        return Ok(());
    }

    for user in due {
        let next_checkin_at = compute_next_checkin_at(&user, now)?;

        if was_active_recently(&user, now, config.recent_activity_grace_minutes) {
            database.defer_checkin(user.account_id, next_checkin_at)?;
            info!(
                tenant_id = %user.tenant_id,
                email = %user.email,
                "deferring check-in because user was active recently"
            );
            continue;
        }

        database.record_checkin_attempt(user.account_id, now, next_checkin_at)?;
        info!(
            tenant_id = %user.tenant_id,
            email = %user.email,
            companion_name = %user.companion_name,
            user_name = ?user.user_name,
            preferred_channel = ?user.preferred_channel,
            timezone = %user.timezone,
            quiet_hours = ?user.quiet_hours,
            due_at = %user.next_checkin_at,
            next_checkin_at = %next_checkin_at,
            "check-in is due and ready for dispatch"
        );
    }

    Ok(())
}

fn was_active_recently(
    user: &DueCheckin,
    now: DateTime<Utc>,
    grace_minutes: i64,
) -> bool {
    let Some(last_active_at) = user.last_active_at else {
        return false;
    };

    let minutes_since_active = (now - last_active_at).num_minutes();
    minutes_since_active >= 0 && minutes_since_active < grace_minutes
}

fn compute_next_checkin_at(user: &DueCheckin, now: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let timezone = Tz::from_str(&user.timezone).map_err(|error| {
        AppError::InvalidState(format!(
            "invalid timezone '{}' for account {}: {error}",
            user.timezone, user.email
        ))
    })?;

    let local_time = NaiveTime::parse_from_str(&user.checkin_local_time, "%H:%M").map_err(
        |error| {
            AppError::InvalidState(format!(
                "invalid local time '{}' for account {}: {error}",
                user.checkin_local_time, user.email
            ))
        },
    )?;

    let local_now = now.with_timezone(&timezone);
    let start_date = local_now.date_naive();
    let allowed_days = normalize_days(&user.checkin_days);

    for offset in 0..14 {
        let Some(candidate_date) = start_date.checked_add_days(Days::new(offset)) else {
            continue;
        };

        if !allowed_days.is_empty()
            && !allowed_days.contains(&candidate_date.weekday().number_from_monday())
        {
            continue;
        }

        let candidate = resolve_local_datetime(timezone, candidate_date, local_time)?;
        if candidate > now {
            return Ok(candidate);
        }
    }

    let cadence_days = user.cadence_days.max(1) as u64;
    let fallback_date = start_date
        .checked_add_days(Days::new(cadence_days))
        .ok_or_else(|| AppError::InvalidState("failed to compute next check-in date".to_string()))?;

    resolve_local_datetime(timezone, fallback_date, local_time)
}

fn resolve_local_datetime(
    timezone: Tz,
    date: chrono::NaiveDate,
    time: NaiveTime,
) -> Result<DateTime<Utc>> {
    let local = date.and_time(time);

    match timezone.from_local_datetime(&local) {
        chrono::LocalResult::Single(value) => Ok(value.with_timezone(&Utc)),
        chrono::LocalResult::Ambiguous(first, _) => Ok(first.with_timezone(&Utc)),
        chrono::LocalResult::None => Err(AppError::InvalidState(format!(
            "local datetime {local} does not exist in timezone {timezone}"
        ))),
    }
}

fn normalize_days(days: &[u32]) -> Vec<u32> {
    let mut normalized = days
        .iter()
        .copied()
        .filter(|day| (1..=7).contains(day))
        .collect::<Vec<_>>();
    normalized.sort_unstable();
    normalized.dedup();
    normalized
}
