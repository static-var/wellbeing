use std::{str::FromStr, sync::Arc, time::Duration};

use chrono::{DateTime, Datelike, Days, NaiveDate, NaiveTime, TimeZone, Utc};
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
        if let Err(error) = process_due_checkin(database, &user, now, config).await {
            database.disable_checkins(user.account_id)?;
            warn!(
                tenant_id = %user.tenant_id,
                email = %user.email,
                error = %error,
                "disabled check-ins because schedule data is invalid"
            );
        }
    }

    Ok(())
}

async fn process_due_checkin(
    database: &AppDatabase,
    user: &DueCheckin,
    now: DateTime<Utc>,
    config: &CheckinRuntimeConfig,
) -> Result<()> {
    let timezone = parse_timezone(user)?;

    if let Some(quiet_until) = quiet_hours_end_at(user, timezone, now)? {
        database.defer_checkin(user.account_id, quiet_until)?;
        info!(
            tenant_id = %user.tenant_id,
            email = %user.email,
            timezone = %user.timezone,
            quiet_hours = ?user.quiet_hours,
            quiet_until = %quiet_until,
            "deferring check-in because user is currently in quiet hours"
        );
        return Ok(());
    }

    let next_checkin_at = compute_next_checkin_at(user, timezone, now)?;

    if was_active_recently(user, now, config.recent_activity_grace_minutes) {
        database.defer_checkin(user.account_id, next_checkin_at)?;
        info!(
            tenant_id = %user.tenant_id,
            email = %user.email,
            "deferring check-in because user was active recently"
        );
        return Ok(());
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

    Ok(())
}

fn was_active_recently(user: &DueCheckin, now: DateTime<Utc>, grace_minutes: i64) -> bool {
    let Some(last_active_at) = user.last_active_at else {
        return false;
    };

    let minutes_since_active = (now - last_active_at).num_minutes();
    minutes_since_active >= 0 && minutes_since_active < grace_minutes
}

fn parse_timezone(user: &DueCheckin) -> Result<Tz> {
    Tz::from_str(&user.timezone).map_err(|error| {
        AppError::InvalidState(format!(
            "invalid timezone '{}' for account {}: {error}",
            user.timezone, user.email
        ))
    })
}

fn compute_next_checkin_at(user: &DueCheckin, timezone: Tz, now: DateTime<Utc>) -> Result<DateTime<Utc>> {
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

fn quiet_hours_end_at(
    user: &DueCheckin,
    timezone: Tz,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>> {
    if user.quiet_hours.is_empty() {
        return Ok(None);
    }

    let local_now = now.with_timezone(&timezone);
    let local_date = local_now.date_naive();
    let local_time = local_now.time();
    let mut defer_until: Option<DateTime<Utc>> = None;

    for quiet_range in &user.quiet_hours {
        let Some(candidate) =
            active_quiet_hours_end(timezone, local_date, local_time, quiet_range)?
        else {
            continue;
        };

        defer_until = Some(match defer_until {
            Some(existing) => existing.min(candidate),
            None => candidate,
        });
    }

    Ok(defer_until)
}

fn active_quiet_hours_end(
    timezone: Tz,
    local_date: NaiveDate,
    local_time: NaiveTime,
    quiet_range: &str,
) -> Result<Option<DateTime<Utc>>> {
    let (start, end) = parse_quiet_range(quiet_range)?;

    if start == end {
        let next_date = local_date
            .checked_add_days(Days::new(1))
            .ok_or_else(|| AppError::InvalidState("failed to extend quiet hours".to_string()))?;
        return resolve_local_datetime(timezone, next_date, end).map(Some);
    }

    if start < end {
        if local_time >= start && local_time < end {
            return resolve_local_datetime(timezone, local_date, end).map(Some);
        }
        return Ok(None);
    }

    if local_time >= start {
        let next_date = local_date
            .checked_add_days(Days::new(1))
            .ok_or_else(|| AppError::InvalidState("failed to extend quiet hours".to_string()))?;
        return resolve_local_datetime(timezone, next_date, end).map(Some);
    }

    if local_time < end {
        return resolve_local_datetime(timezone, local_date, end).map(Some);
    }

    Ok(None)
}

fn parse_quiet_range(value: &str) -> Result<(NaiveTime, NaiveTime)> {
    let (start, end) = value.split_once('-').ok_or_else(|| {
        AppError::InvalidState(format!(
            "invalid quiet hours '{value}', expected HH:MM-HH:MM"
        ))
    })?;

    let start = NaiveTime::parse_from_str(start.trim(), "%H:%M").map_err(|error| {
        AppError::InvalidState(format!("invalid quiet hours start '{start}': {error}"))
    })?;
    let end = NaiveTime::parse_from_str(end.trim(), "%H:%M").map_err(|error| {
        AppError::InvalidState(format!("invalid quiet hours end '{end}': {error}"))
    })?;

    Ok((start, end))
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_user(quiet_hours: Vec<&str>) -> DueCheckin {
        DueCheckin {
            account_id: 1,
            tenant_id: "hope".to_string(),
            email: "test@example.com".to_string(),
            companion_name: "Hope".to_string(),
            user_name: Some("Sam".to_string()),
            timezone: "UTC".to_string(),
            preferred_channel: Some("web".to_string()),
            cadence_days: 1,
            checkin_local_time: "19:00".to_string(),
            checkin_days: vec![1, 2, 3, 4, 5, 6, 7],
            quiet_hours: quiet_hours.into_iter().map(str::to_string).collect(),
            last_active_at: None,
            next_checkin_at: Utc.with_ymd_and_hms(2026, 3, 10, 0, 0, 0).unwrap(),
        }
    }

    #[test]
    fn overnight_quiet_hours_defer_late_night_dispatch() {
        let user = sample_user(vec!["22:00-07:00"]);
        let now = Utc.with_ymd_and_hms(2026, 3, 10, 23, 15, 0).unwrap();

        let defer_until = quiet_hours_end_at(&user, Tz::UTC, now).unwrap();

        assert_eq!(
            defer_until,
            Some(Utc.with_ymd_and_hms(2026, 3, 11, 7, 0, 0).unwrap())
        );
    }

    #[test]
    fn overnight_quiet_hours_defer_early_morning_dispatch() {
        let user = sample_user(vec!["22:00-07:00"]);
        let now = Utc.with_ymd_and_hms(2026, 3, 10, 6, 30, 0).unwrap();

        let defer_until = quiet_hours_end_at(&user, Tz::UTC, now).unwrap();

        assert_eq!(
            defer_until,
            Some(Utc.with_ymd_and_hms(2026, 3, 10, 7, 0, 0).unwrap())
        );
    }

    #[test]
    fn quiet_hours_do_not_defer_when_outside_window() {
        let user = sample_user(vec!["22:00-07:00"]);
        let now = Utc.with_ymd_and_hms(2026, 3, 10, 14, 0, 0).unwrap();

        let defer_until = quiet_hours_end_at(&user, Tz::UTC, now).unwrap();

        assert_eq!(defer_until, None);
    }
}
