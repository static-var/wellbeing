use std::time::Duration;

use serde::Serialize;
use tracing::{debug, warn};

use crate::{
    config::{HeartbeatConfig, HeartbeatMethod},
    error::Result,
};

#[derive(Clone, Debug, Serialize)]
struct HeartbeatPayload {
    service: &'static str,
    tenant_count: usize,
}

pub fn spawn_heartbeat(config: Option<HeartbeatConfig>, tenant_count: usize) {
    let Some(config) = config else {
        return;
    };

    tokio::spawn(async move {
        let client = match reqwest::Client::builder().build() {
            Ok(client) => client,
            Err(error) => {
                warn!("failed to build heartbeat client: {error}");
                return;
            }
        };

        let payload = HeartbeatPayload {
            service: "wellbeing",
            tenant_count,
        };

        let interval = Duration::from_secs(config.interval_secs);

        loop {
            if let Err(error) = send_heartbeat(&client, &config, &payload).await {
                warn!("heartbeat failed: {error}");
            }

            tokio::time::sleep(interval).await;
        }
    });
}

async fn send_heartbeat(
    client: &reqwest::Client,
    config: &HeartbeatConfig,
    payload: &HeartbeatPayload,
) -> Result<()> {
    match config.method {
        HeartbeatMethod::Get => {
            client
                .get(&config.url)
                .query(&[
                    ("service", payload.service.to_string()),
                    ("tenant_count", payload.tenant_count.to_string()),
                ])
                .send()
                .await?
                .error_for_status()?;
        }
        HeartbeatMethod::Post => {
            client
                .post(&config.url)
                .json(payload)
                .send()
                .await?
                .error_for_status()?;
        }
    }

    debug!("heartbeat sent successfully");
    Ok(())
}
