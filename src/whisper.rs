use std::time::Duration;

use reqwest::multipart::{Form, Part};
use serde::Deserialize;

use crate::{
    config::WhisperConfig,
    error::{AppError, Result},
};

#[derive(Debug, Deserialize)]
struct OpenAiTranscriptionResponse {
    text: String,
}

pub async fn transcribe_audio_bytes(
    client: &reqwest::Client,
    config: &WhisperConfig,
    file_name: &str,
    mime_type: &str,
    audio_bytes: Vec<u8>,
) -> Result<String> {
    let endpoints = candidate_endpoints(&config.worker_url);
    let model_name = config
        .model
        .clone()
        .unwrap_or_else(|| "whisper-1".to_string());

    let mut last_error = None;
    for endpoint in endpoints {
        match attempt_transcription(
            client,
            &endpoint,
            &model_name,
            file_name,
            mime_type,
            audio_bytes.clone(),
            config.timeout_secs,
        )
        .await
        {
            Ok(transcript) => return Ok(transcript),
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        AppError::InvalidState("whisper worker transcription failed".to_string())
    }))
}

async fn attempt_transcription(
    client: &reqwest::Client,
    endpoint: &str,
    model_name: &str,
    file_name: &str,
    mime_type: &str,
    audio_bytes: Vec<u8>,
    timeout_secs: u64,
) -> Result<String> {
    let audio_part = Part::bytes(audio_bytes)
        .file_name(file_name.to_string())
        .mime_str(mime_type)
        .map_err(|error| AppError::InvalidConfig(format!("invalid audio mime type: {error}")))?;
    let form = Form::new()
        .part("file", audio_part)
        .text("model", model_name.to_string())
        .text("response_format", "json".to_string());

    let response = client
        .post(endpoint)
        .timeout(Duration::from_secs(timeout_secs.max(1)))
        .multipart(form)
        .send()
        .await?
        .error_for_status()?;

    let body = response.text().await?;
    parse_transcription_response(&body)
}

fn parse_transcription_response(body: &str) -> Result<String> {
    if let Ok(json) = serde_json::from_str::<OpenAiTranscriptionResponse>(body) {
        return normalized_transcript(&json.text);
    }

    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidState(
            "whisper worker returned an empty transcription".to_string(),
        ));
    }

    normalized_transcript(trimmed)
}

fn normalized_transcript(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidState(
            "whisper worker returned an empty transcription".to_string(),
        ));
    }

    Ok(trimmed.to_string())
}

fn candidate_endpoints(worker_url: &str) -> Vec<String> {
    let base = worker_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return vec![];
    }
    if base.ends_with("/v1/audio/transcriptions") || base.ends_with("/inference") {
        return vec![base.to_string()];
    }
    if base.ends_with("/v1") {
        return vec![
            format!("{base}/audio/transcriptions"),
            base.trim_end_matches("/v1").to_string() + "/inference",
        ];
    }

    vec![
        format!("{base}/v1/audio/transcriptions"),
        format!("{base}/inference"),
    ]
}

#[cfg(test)]
mod tests {
    use super::candidate_endpoints;

    #[test]
    fn expands_root_worker_url_to_openai_then_legacy() {
        assert_eq!(
            candidate_endpoints("http://127.0.0.1:2022"),
            vec![
                "http://127.0.0.1:2022/v1/audio/transcriptions".to_string(),
                "http://127.0.0.1:2022/inference".to_string()
            ]
        );
    }

    #[test]
    fn expands_v1_base_to_openai_then_legacy() {
        assert_eq!(
            candidate_endpoints("http://127.0.0.1:2022/v1"),
            vec![
                "http://127.0.0.1:2022/v1/audio/transcriptions".to_string(),
                "http://127.0.0.1:2022/inference".to_string()
            ]
        );
    }
}
