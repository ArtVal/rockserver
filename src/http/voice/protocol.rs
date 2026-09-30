//! Wire protocol types, DTO validation, and frame serialization for voice endpoints.

use std::{
    collections::BTreeSet,
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::extract::ws::{Message, WebSocket};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::{
    device_control::CommandStatus,
    voice::{SpeechProviderError, SpeechRecognizerMode, TranscriptUpdate},
};

use super::super::{
    search::{SearchRequestDto, ValidatedSearchRequest},
    state::{PublicLimit, PublicLimitState},
    transport::{NormalizedQueryDto, StationResultDto},
};

pub(super) const MAX_STREAM_AUDIO_CHUNK_BYTES: usize = 32 * 1024;
pub(super) const MAX_STREAM_AUDIO_BYTES: usize = 2 * 1024 * 1024;
pub(super) const MAX_STREAM_AUDIO_SECONDS: usize = 60;
pub(super) const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const STREAM_WALL_TIMEOUT: Duration = Duration::from_secs(75);
pub(super) const DEFAULT_STREAM_OPERATION_TIMEOUT: Duration = Duration::from_secs(15);
pub(super) const VOICE_COMMAND_LIMIT: PublicLimit = PublicLimit {
    requests: 12,
    burst: 4,
};
pub(super) const VOICE_UPGRADE_LIMIT: PublicLimit = PublicLimit {
    requests: 6,
    burst: 2,
};

/// Releases a reserved anonymous voice slot when a WebSocket session ends.
pub(crate) struct VoiceSlot {
    pub(crate) limiter: Arc<Mutex<PublicLimitState>>,
}

impl Drop for VoiceSlot {
    fn drop(&mut self) {
        if let Ok(mut state) = self.limiter.lock() {
            state.active_voice = state.active_voice.saturating_sub(1);
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum VoiceStreamStartDto {
    Start {
        #[serde(default)]
        locale: Option<String>,
        sample_rate_hz: u32,
        #[serde(default)]
        surface_id: Option<String>,
        #[serde(default)]
        recognizer_mode: Option<String>,
        #[serde(default)]
        limit: Option<u8>,
        #[serde(default)]
        exclude_station_ids: Vec<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum VoiceStreamCommitDto {
    Commit,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum VoiceStreamCancelDto {
    Cancel,
}

pub(super) struct ValidatedVoiceStreamStart {
    pub(super) locale: String,
    pub(super) sample_rate_hz: u32,
    pub(super) surface_id: Option<String>,
    pub(super) recognizer_mode: SpeechRecognizerMode,
    pub(super) limit: usize,
    pub(super) exclude_station_ids: BTreeSet<String>,
}

impl TryFrom<VoiceStreamStartDto> for ValidatedVoiceStreamStart {
    type Error = Map<String, Value>;

    fn try_from(value: VoiceStreamStartDto) -> Result<Self, Self::Error> {
        let VoiceStreamStartDto::Start {
            locale,
            sample_rate_hz,
            surface_id,
            recognizer_mode,
            limit,
            exclude_station_ids,
        } = value;
        let mut details = Map::new();
        let recognizer_mode = match recognizer_mode.as_deref().unwrap_or("buffered_v1") {
            "buffered_v1" => SpeechRecognizerMode::BufferedV1,
            "streaming_v3" => SpeechRecognizerMode::StreamingV3,
            _ => {
                details.insert(
                    "recognizer_mode".to_owned(),
                    json!("must be buffered_v1 or streaming_v3"),
                );
                SpeechRecognizerMode::default()
            }
        };
        if sample_rate_hz != 16_000 {
            details.insert("sample_rate_hz".to_owned(), json!("must equal 16000"));
        }
        if surface_id
            .as_deref()
            .is_some_and(|value| value != "voice.main")
        {
            details.insert("surface_id".to_owned(), json!("must equal voice.main"));
        }
        let validated = ValidatedSearchRequest::try_from(SearchRequestDto {
            query: "stream".to_owned(),
            locale,
            limit,
            offset: None,
            exclude_station_ids,
        });
        match validated {
            Ok(validated) if details.is_empty() => Ok(Self {
                locale: validated.locale,
                sample_rate_hz,
                surface_id,
                recognizer_mode,
                limit: validated.limit.min(10),
                exclude_station_ids: validated.exclude_station_ids,
            }),
            Ok(_) => Err(details),
            Err(mut search_details) => {
                search_details.remove("query");
                details.extend(search_details);
                Err(details)
            }
        }
    }
}

pub(super) enum StreamOperationError {
    Provider(SpeechProviderError),
    Timeout,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum VoiceStreamServerEvent {
    Ready {
        request_id: String,
        audio_format: String,
        sample_rate_hz: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        source_device_id: Option<Uuid>,
        #[serde(skip_serializing_if = "Option::is_none")]
        surface_id: Option<String>,
    },
    Transcript {
        request_id: String,
        transcript: String,
        is_final: bool,
    },
    Result {
        #[serde(flatten)]
        result: Box<VoiceStreamResultPayload>,
    },
    #[serde(rename = "result")]
    DeviceResult {
        request_id: String,
        status: CommandStatus,
    },
    Error {
        code: VoiceStreamErrorCode,
        message: String,
        request_id: String,
        details: Value,
    },
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum VoiceStreamErrorCode {
    ProtocolError,
    ValidationFailed,
    SpeechProviderUnavailable,
    SpeechProviderError,
    SpeechTimeout,
    SpeechNotRecognized,
    VoiceTimeout,
    AudioChunkInvalid,
    AudioTooLarge,
    Cancelled,
    IntentResolutionFailed,
    UnsupportedIntent,
    ClarificationRequired,
    StationNotFound,
    SearchTimeout,
    SearchUnavailable,
    TargetOffline,
    CapabilityNotSupported,
    Forbidden,
    InvalidPayload,
    CommandTimeout,
    DuplicateCommand,
    TooManyInFlight,
    PersistenceUnavailable,
    InternalError,
}

#[derive(Serialize)]
pub(super) struct VoiceStreamResultPayload {
    pub(super) request_id: String,
    pub(super) transcript: String,
    pub(super) normalized_query: NormalizedQueryDto,
    pub(super) selected_station: Option<StationResultDto>,
    pub(super) stations: Vec<StationResultDto>,
}

/// JSON transport input for one already-recognized voice command.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VoiceCommandRequestDto {
    pub(super) transcript: String,
    #[serde(default)]
    pub(super) locale: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<u8>,
    #[serde(default)]
    pub(super) exclude_station_ids: Vec<String>,
}

pub(super) struct ValidatedVoiceCommandRequest {
    pub(super) transcript: String,
    pub(super) locale: String,
    pub(super) limit: usize,
    pub(super) exclude_station_ids: BTreeSet<String>,
}

impl TryFrom<VoiceCommandRequestDto> for ValidatedVoiceCommandRequest {
    type Error = Map<String, Value>;

    fn try_from(value: VoiceCommandRequestDto) -> Result<Self, Self::Error> {
        let transcript = value.transcript;
        match ValidatedSearchRequest::try_from(SearchRequestDto {
            query: transcript,
            locale: value.locale,
            limit: value.limit,
            offset: None,
            exclude_station_ids: value.exclude_station_ids,
        }) {
            Ok(validated) => Ok(Self {
                transcript: validated.query,
                locale: validated.locale,
                limit: validated.limit,
                exclude_station_ids: validated.exclude_station_ids,
            }),
            Err(mut details) => {
                if let Some(query_error) = details.remove("query") {
                    details.insert("transcript".to_owned(), query_error);
                }
                Err(details)
            }
        }
    }
}

pub(super) fn parse_stream_start(text: &str) -> Result<ValidatedVoiceStreamStart, Value> {
    let request = serde_json::from_str::<VoiceStreamStartDto>(text)
        .map_err(|error| json!({"start": format!("must be valid JSON: {error}")}))?;
    ValidatedVoiceStreamStart::try_from(request).map_err(Value::Object)
}

pub(super) fn is_commit_event(text: &str) -> bool {
    serde_json::from_str::<VoiceStreamCommitDto>(text).is_ok()
}

pub(super) fn is_cancel_event(text: &str) -> bool {
    serde_json::from_str::<VoiceStreamCancelDto>(text).is_ok()
}

pub(super) fn newest_final_transcript(updates: &[TranscriptUpdate]) -> Option<String> {
    updates
        .iter()
        .rev()
        .find(|update| update.is_final)
        .map(|update| update.transcript.trim().to_owned())
        .filter(|transcript| !transcript.is_empty())
}

pub(super) async fn speech_operation<T>(
    operation: impl Future<Output = Result<T, SpeechProviderError>>,
) -> Result<T, StreamOperationError> {
    match tokio::time::timeout(DEFAULT_STREAM_OPERATION_TIMEOUT, operation).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(StreamOperationError::Provider(error)),
        Err(_) => Err(StreamOperationError::Timeout),
    }
}

pub(super) async fn send_transcript_updates(
    socket: &mut WebSocket,
    request_id: &str,
    updates: Vec<TranscriptUpdate>,
) -> Result<(), axum::Error> {
    for update in updates {
        send_stream_event(
            socket,
            &VoiceStreamServerEvent::Transcript {
                request_id: request_id.to_owned(),
                transcript: update.transcript,
                is_final: update.is_final,
            },
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn send_speech_failure(
    socket: &mut WebSocket,
    request_id: &str,
    error: StreamOperationError,
) {
    match error {
        StreamOperationError::Provider(error) => {
            log_speech_error(request_id, &error);
            let _ = send_stream_error(
                socket,
                request_id,
                VoiceStreamErrorCode::SpeechProviderError,
                "Streaming speech recognition failed.",
                json!({}),
            )
            .await;
        }
        StreamOperationError::Timeout => {
            let _ = send_stream_error(
                socket,
                request_id,
                VoiceStreamErrorCode::SpeechTimeout,
                "Streaming speech provider timed out.",
                json!({"timeout_ms": DEFAULT_STREAM_OPERATION_TIMEOUT.as_millis()}),
            )
            .await;
        }
    }
}

pub(super) fn log_speech_error(request_id: &str, _error: &SpeechProviderError) {
    tracing::warn!(%request_id, "streaming speech recognition failed");
}

pub(super) async fn send_stream_error(
    socket: &mut WebSocket,
    request_id: &str,
    code: VoiceStreamErrorCode,
    message: &str,
    details: Value,
) -> Result<(), axum::Error> {
    send_stream_event(
        socket,
        &VoiceStreamServerEvent::Error {
            code,
            message: message.to_owned(),
            request_id: request_id.to_owned(),
            details,
        },
    )
    .await
}

pub(super) async fn send_stream_event(
    socket: &mut WebSocket,
    event: &VoiceStreamServerEvent,
) -> Result<(), axum::Error> {
    let payload = serde_json::to_string(event)
        .expect("stream server events contain only serializable transport values");
    socket.send(Message::Text(payload.into())).await
}
