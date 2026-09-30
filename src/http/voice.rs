//! Transcript and streaming voice HTTP/WebSocket handlers.

use axum::{
    Json,
    body::Body,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use crate::{
    device_control_auth::{DeviceControlAuthenticationError, DeviceControlPrincipal},
    search::{QueryParserInput, SearchConstraints},
    voice::SpeechStreamConfig,
};

use super::{
    account,
    control_auth::authenticate_control_ingress,
    state::AppState,
    transport::{
        NormalizedQueryDto, StationResultDto, VoiceCommandResponseDto, error_response,
        parse_json_request, request_id, retry_after, unauthorized_response, with_request_id,
    },
};

#[path = "voice/device.rs"]
mod device;
#[path = "voice/protocol.rs"]
mod protocol;

use device::*;
use protocol::*;

pub(super) use protocol::VoiceSlot;

/// Upgrades an anonymous or authenticated request to the streaming voice WebSocket.
pub(super) async fn voice_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !headers.contains_key(axum::http::header::AUTHORIZATION) {
        return public_voice_stream(State(state), headers, upgrade).await;
    }
    let request_id = request_id(&headers);
    if state.is_authorized(&headers) {
        return voice_stream_impl(state, headers, upgrade, request_id, None, None).await;
    }
    let Some(resolver) = state.control_session_resolver.as_ref() else {
        return unauthorized_response(&request_id);
    };
    let principal = match authenticate_control_ingress(&headers, resolver.as_ref()).await {
        Ok(principal) => principal,
        Err(DeviceControlAuthenticationError::InvalidCredential) => {
            return unauthorized_response(&request_id);
        }
        Err(DeviceControlAuthenticationError::Unavailable) => {
            return retry_after(
                error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "control_auth_unavailable",
                    "Device authentication is temporarily unavailable.",
                    &request_id,
                    json!({}),
                ),
                1,
            );
        }
    };
    voice_stream_impl(state, headers, upgrade, request_id, None, Some(principal)).await
}

/// Admits the approved anonymous WebSocket voice session without trusting forwarded headers.
pub(super) async fn public_voice_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let request_id = request_id(&headers);
    if let Err(response) =
        state.public_request_allowed("voice_stream", VOICE_UPGRADE_LIMIT, &request_id)
    {
        return *response;
    }
    let slot = match state.reserve_voice_slot(&request_id) {
        Ok(slot) => slot,
        Err(response) => return *response,
    };
    voice_stream_impl(state, headers, upgrade, request_id, Some(slot), None).await
}

/// Runs one authenticated or anonymous voice WebSocket session.
async fn voice_stream_impl(
    state: AppState,
    _headers: HeaderMap,
    upgrade: WebSocketUpgrade,
    request_id: String,
    slot: Option<VoiceSlot>,
    principal: Option<DeviceControlPrincipal>,
) -> Response {
    let socket_request_id = request_id.clone();
    let response = upgrade
        .max_message_size(MAX_STREAM_AUDIO_CHUNK_BYTES + 1024)
        .on_upgrade(move |socket| async move {
            let _slot = slot;
            run_voice_stream(socket, state, socket_request_id, principal).await
        });
    with_request_id(response, &request_id)
}

/// Processes audio, transcript updates, and the final station search.
async fn run_voice_stream(
    mut socket: WebSocket,
    state: AppState,
    request_id: String,
    principal: Option<DeviceControlPrincipal>,
) {
    let Some(Ok(Message::Text(start_message))) =
        tokio::time::timeout(STREAM_IDLE_TIMEOUT, socket.recv())
            .await
            .ok()
            .flatten()
    else {
        let _ = send_stream_error(
            &mut socket,
            &request_id,
            VoiceStreamErrorCode::ProtocolError,
            "The first WebSocket message must be a JSON start event.",
            json!({}),
        )
        .await;
        return;
    };
    let start = match parse_stream_start(&start_message) {
        Ok(start) => start,
        Err(details) => {
            let _ = send_stream_error(
                &mut socket,
                &request_id,
                VoiceStreamErrorCode::ValidationFailed,
                "Streaming session validation failed.",
                details,
            )
            .await;
            return;
        }
    };
    // The frozen contract keeps legacy device clients (station-search voice,
    // for example the RockCast desktop app) on the established flow: only a
    // start frame that explicitly announces voice.main opts a device session
    // into the device intent flow.
    let principal = principal.filter(|_| start.surface_id.as_deref() == Some("voice.main"));
    if let Some(principal) = principal
        && let Err((code, message)) = validate_device_voice_start(&state, principal, &start)
    {
        let _ = send_stream_error(&mut socket, &request_id, code, message, json!({})).await;
        return;
    }

    let mut session = match tokio::time::timeout(
        DEFAULT_STREAM_OPERATION_TIMEOUT,
        state.speech_recognizers.start(
            start.recognizer_mode,
            SpeechStreamConfig {
                locale: start.locale.clone(),
                sample_rate_hz: start.sample_rate_hz,
            },
        ),
    )
    .await
    {
        Ok(Ok(session)) => session,
        Ok(Err(error)) => {
            log_speech_error(&request_id, &error);
            let _ = send_stream_error(
                &mut socket,
                &request_id,
                VoiceStreamErrorCode::SpeechProviderUnavailable,
                "Streaming speech recognition is unavailable.",
                json!({}),
            )
            .await;
            return;
        }
        Err(_) => {
            let _ = send_stream_error(
                &mut socket,
                &request_id,
                VoiceStreamErrorCode::SpeechTimeout,
                "Streaming speech provider timed out.",
                json!({"timeout_ms": DEFAULT_STREAM_OPERATION_TIMEOUT.as_millis()}),
            )
            .await;
            return;
        }
    };
    if send_stream_event(
        &mut socket,
        &VoiceStreamServerEvent::Ready {
            request_id: request_id.clone(),
            audio_format: "pcm_s16le".to_owned(),
            sample_rate_hz: start.sample_rate_hz,
            source_device_id: principal.map(|principal| principal.device_id),
            surface_id: principal.and_then(|_| start.surface_id.clone()),
        },
    )
    .await
    .is_err()
    {
        return;
    }

    let started_at = tokio::time::Instant::now();
    let mut audio_bytes = 0usize;
    let mut last_final_transcript = None;
    while started_at.elapsed() < STREAM_WALL_TIMEOUT {
        let Some(message) = tokio::time::timeout(STREAM_IDLE_TIMEOUT, socket.recv())
            .await
            .ok()
            .flatten()
        else {
            let _ = send_stream_error(
                &mut socket,
                &request_id,
                VoiceStreamErrorCode::VoiceTimeout,
                "Voice session timed out.",
                json!({"timeout_ms": STREAM_IDLE_TIMEOUT.as_millis()}),
            )
            .await;
            return;
        };
        match message {
            Ok(Message::Binary(audio)) => {
                if audio.is_empty()
                    || audio.len() > MAX_STREAM_AUDIO_CHUNK_BYTES
                    || audio.len() % 2 != 0
                {
                    let _ = send_stream_error(
                        &mut socket,
                        &request_id,
                        VoiceStreamErrorCode::AudioChunkInvalid,
                        "Audio frames must be bounded PCM16 data.",
                        json!({"max_chunk_bytes": MAX_STREAM_AUDIO_CHUNK_BYTES}),
                    )
                    .await;
                    return;
                }
                audio_bytes = audio_bytes.saturating_add(audio.len());
                if audio_bytes > MAX_STREAM_AUDIO_BYTES
                    || audio_bytes / 2 / 16_000 > MAX_STREAM_AUDIO_SECONDS
                {
                    let _ = send_stream_error(
                        &mut socket,
                        &request_id,
                        VoiceStreamErrorCode::AudioTooLarge,
                        "Streaming session audio limit was exceeded.",
                        json!({"max_bytes": MAX_STREAM_AUDIO_BYTES}),
                    )
                    .await;
                    return;
                }
                match speech_operation(session.push_audio(&audio)).await {
                    Ok(updates) => {
                        if let Some(transcript) = newest_final_transcript(&updates) {
                            last_final_transcript = Some(transcript);
                        }
                        if send_transcript_updates(&mut socket, &request_id, updates)
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(error) => {
                        send_speech_failure(&mut socket, &request_id, error).await;
                        return;
                    }
                }
            }
            Ok(Message::Text(text)) if is_cancel_event(&text) => {
                drop(session);
                let _ = send_stream_error(
                    &mut socket,
                    &request_id,
                    VoiceStreamErrorCode::Cancelled,
                    "Voice session was cancelled.",
                    json!({}),
                )
                .await;
                return;
            }
            Ok(Message::Text(text)) if is_commit_event(&text) => {
                let updates = match speech_operation(session.finish()).await {
                    Ok(updates) => updates,
                    Err(error) => {
                        send_speech_failure(&mut socket, &request_id, error).await;
                        return;
                    }
                };
                let final_transcript = newest_final_transcript(&updates).or(last_final_transcript);
                if send_transcript_updates(&mut socket, &request_id, updates)
                    .await
                    .is_err()
                {
                    return;
                }
                let Some(transcript) = final_transcript else {
                    let _ = send_stream_error(
                        &mut socket,
                        &request_id,
                        VoiceStreamErrorCode::SpeechNotRecognized,
                        "No final speech transcript was recognized.",
                        json!({}),
                    )
                    .await;
                    return;
                };
                if let Some(principal) = principal {
                    finish_device_voice(
                        &mut socket,
                        &state,
                        &request_id,
                        &start,
                        principal,
                        transcript,
                    )
                    .await;
                } else {
                    finish_stream_search(&mut socket, &state, &request_id, &start, transcript)
                        .await;
                }
                return;
            }
            Ok(Message::Close(_)) | Err(_) => return,
            Ok(Message::Ping(payload)) => {
                if socket.send(Message::Pong(payload)).await.is_err() {
                    return;
                }
            }
            Ok(Message::Pong(_)) => {}
            _ => {
                let _ = send_stream_error(
                    &mut socket,
                    &request_id,
                    VoiceStreamErrorCode::ProtocolError,
                    "Expected a binary audio chunk or JSON commit event.",
                    json!({}),
                )
                .await;
                return;
            }
        }
    }
    let _ = send_stream_error(
        &mut socket,
        &request_id,
        VoiceStreamErrorCode::VoiceTimeout,
        "Voice session timed out.",
        json!({"timeout_ms": STREAM_WALL_TIMEOUT.as_millis()}),
    )
    .await;
}

/// Resolves an already-recognized voice transcript through the existing search service.
///
/// The route does not accept audio and does not call an STT provider. This keeps provider
/// credentials and audio-upload policy outside the stable JSON command contract.
pub(super) async fn voice_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    if !headers.contains_key(axum::http::header::AUTHORIZATION) {
        return public_voice_command(State(state), headers, body).await;
    }
    let request_id = request_id(&headers);
    if !state.is_authorized(&headers)
        && account::match_native_session(&state, &headers, &request_id)
            .await
            .is_none()
    {
        return unauthorized_response(&request_id);
    }
    voice_command_impl(state, headers, body, request_id, 50).await
}

/// Serves the approved anonymous, transcript-only voice command operation.
pub(super) async fn public_voice_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let request_id = request_id(&headers);
    if let Err(response) =
        state.public_request_allowed("voice_command", VOICE_COMMAND_LIMIT, &request_id)
    {
        return *response;
    }
    voice_command_impl(state, headers, body, request_id, 10).await
}

async fn voice_command_impl(
    state: AppState,
    headers: HeaderMap,
    body: Body,
    request_id: String,
    max_limit: u8,
) -> Response {
    let request =
        match parse_json_request::<VoiceCommandRequestDto>(&headers, body, &request_id).await {
            Ok(request) => request,
            Err(response) => return response,
        };
    let validated = match ValidatedVoiceCommandRequest::try_from(request) {
        Ok(request) => request,
        Err(details) => {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed",
                "Request validation failed.",
                &request_id,
                Value::Object(details),
            );
        }
    };
    if validated.limit > usize::from(max_limit) {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "Request validation failed.",
            &request_id,
            json!({"limit": format!("must be between 1 and {max_limit}")}),
        );
    }
    let constraints = SearchConstraints {
        limit: validated.limit,
        offset: 0,
        excluded_station_ids: validated.exclude_station_ids,
    };
    let outcome = match tokio::time::timeout(
        state.voice_command_timeout,
        state.search_service.interpret_and_search_private(
            QueryParserInput {
                query: validated.transcript.clone(),
                locale: validated.locale,
            },
            &constraints,
        ),
    )
    .await
    {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(_error)) => {
            tracing::warn!(%request_id, endpoint = "voice_command", "public-safe voice command failure");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "An unexpected server error occurred.",
                &request_id,
                json!({}),
            );
        }
        Err(_) => {
            return error_response(
                StatusCode::GATEWAY_TIMEOUT,
                "search_timeout",
                "Voice command search timed out.",
                &request_id,
                json!({"timeout_ms": state.voice_command_timeout.as_millis()}),
            );
        }
    };

    let stations = outcome
        .stations
        .iter()
        .map(StationResultDto::from)
        .collect::<Vec<_>>();
    let selected_station = stations.first().cloned();
    tracing::info!(%request_id, endpoint = "voice_command", status = 200, stations = stations.len(), "public request completed");
    with_request_id(
        Json(VoiceCommandResponseDto {
            request_id: request_id.clone(),
            transcript: validated.transcript,
            normalized_query: NormalizedQueryDto::from(outcome.query),
            selected_station,
            stations,
        })
        .into_response(),
        &request_id,
    )
}

async fn finish_stream_search(
    socket: &mut WebSocket,
    state: &AppState,
    request_id: &str,
    start: &ValidatedVoiceStreamStart,
    transcript: String,
) {
    tracing::info!(
        %request_id,
        locale = %start.locale,
        limit = start.limit,
        audio_search = true,
        "voice transcript search started"
    );
    let constraints = SearchConstraints {
        limit: start.limit,
        offset: 0,
        excluded_station_ids: start.exclude_station_ids.clone(),
    };
    let outcome = match tokio::time::timeout(
        state.voice_command_timeout,
        state.search_service.interpret_and_search_private(
            QueryParserInput {
                query: transcript.clone(),
                locale: start.locale.clone(),
            },
            &constraints,
        ),
    )
    .await
    {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(_)) => {
            tracing::error!(%request_id, "streaming voice search failed");
            let _ = send_stream_error(
                socket,
                request_id,
                VoiceStreamErrorCode::InternalError,
                "An unexpected server error occurred.",
                json!({}),
            )
            .await;
            return;
        }
        Err(_) => {
            let _ = send_stream_error(
                socket,
                request_id,
                VoiceStreamErrorCode::SearchTimeout,
                "Voice command search timed out.",
                json!({"timeout_ms": state.voice_command_timeout.as_millis()}),
            )
            .await;
            return;
        }
    };
    let stations = outcome
        .stations
        .iter()
        .map(StationResultDto::from)
        .collect::<Vec<_>>();
    let selected_station = stations.first().cloned();
    tracing::info!(%request_id, stations = stations.len(), "voice transcript search completed");
    let _ = send_stream_event(
        socket,
        &VoiceStreamServerEvent::Result {
            result: Box::new(VoiceStreamResultPayload {
                request_id: request_id.to_owned(),
                transcript,
                normalized_query: NormalizedQueryDto::from(outcome.query),
                selected_station,
                stations,
            }),
        },
    )
    .await;
}
