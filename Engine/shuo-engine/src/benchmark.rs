use std::cmp::Reverse;
use std::collections::BTreeMap;

use audiopus::coder::Encoder as OpusEncoder;
use audiopus::{Application, Channels, SampleRate};
use futures_util::{SinkExt, StreamExt};
use http::{header::HeaderName, HeaderValue, Request};
use serde::Serialize;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::runtime::Builder;
use tokio::time::{sleep_until, timeout, Duration, Instant};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

use crate::config::{load_context_config, ContextConfig};
use crate::frontier_auth::{resolve_frontier_auth, resolve_frontier_auth_for_profile, FrontierAuthMaterial};
use crate::frontier_protocol::{
    build_audio_frame, build_effective_request_profile, build_finish_session, build_start_session,
    build_start_task, FrontierRuntimeContext, DEFAULT_FRONTIER_WS_URL,
};
use crate::state::now_millis;
use crate::{Args, FrontierProfile};

type WsWrite = futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;

const READY_TIMEOUT: Duration = Duration::from_secs(4);
const FINAL_TIMEOUT: Duration = Duration::from_secs(8);
const PCM_SAMPLE_RATE: usize = 16_000;
const PCM_FRAME_MS: u64 = 20;
const DEFAULT_DEVICE_KEY: &str = "4285264416738169+W";
const ANDROID_USER_AGENT: &str = "com.bytedance.android.doubaoime/100102018 (Linux; U; Android 16; en_US; Pixel 7 Pro; Build/BP2A.250605.031.A2; Cronet/TTNetVersion:94cf429a 2025-11-17 QuicVersion:1f89f732 2025-05-08)";
const ANDROID_WS_BASE_URL: &str = "wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws";

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    ok: bool,
    profile: String,
    audio_file: Option<String>,
    audio_ms: u64,
    sample_count: usize,
    auth_source: Option<String>,
    ws_url: Option<String>,
    connect_started_at_ms: Option<u64>,
    ready_at_ms: Option<u64>,
    first_audio_sent_at_ms: Option<u64>,
    finish_sent_at_ms: Option<u64>,
    first_result_at_ms: Option<u64>,
    first_partial_at_ms: Option<u64>,
    first_final_at_ms: Option<u64>,
    final_at_ms: Option<u64>,
    warmup_ms: Option<u64>,
    first_result_frame_ms: Option<u64>,
    first_partial_frame_ms: Option<u64>,
    first_final_frame_ms: Option<u64>,
    first_result_after_audio_ms: Option<u64>,
    infer_ms: Option<u64>,
    partial_count: u64,
    final_count: u64,
    final_text: String,
    error: Option<String>,
}

#[derive(Debug, Default)]
struct BenchState {
    ready_at_ms: Option<u64>,
    first_result_at_ms: Option<u64>,
    first_partial_at_ms: Option<u64>,
    first_final_at_ms: Option<u64>,
    final_text: String,
    partial_count: u64,
    final_count: u64,
    terminal: bool,
}

#[derive(Debug)]
struct ParsedFrontierResponse {
    event: String,
    status_code: u64,
    status_text: String,
    payload_json: Option<Value>,
}

#[derive(Clone, Debug)]
struct ResultCandidate {
    order: usize,
    index: Option<i64>,
    text: String,
    has_seq_id: bool,
    stream_asr_finish: bool,
}

#[derive(Clone, Copy)]
enum AndroidFrameState {
    First = 1,
    Middle = 3,
    Last = 9,
}

struct AndroidOpusState {
    encoder: OpusEncoder,
    timestamp_ms: u64,
}

impl AndroidOpusState {
    fn new() -> Result<Self, String> {
        let encoder = OpusEncoder::new(SampleRate::Hz16000, Channels::Mono, Application::Audio)
            .map_err(|e| format!("opus encoder init failed: {e}"))?;
        Ok(Self {
            encoder,
            timestamp_ms: now_millis(),
        })
    }

    fn encode_frame(&mut self, pcm: &[i16]) -> Result<Vec<u8>, String> {
        let mut output = vec![0u8; 4000];
        let size = self
            .encoder
            .encode(pcm, &mut output)
            .map_err(|e| format!("opus encode failed: {e}"))?;
        output.truncate(size);
        Ok(output)
    }
}

pub(crate) fn run_benchmark_replay(args: Args) {
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build benchmark runtime");
    let report = runtime.block_on(run_benchmark_replay_async(args));
    let text = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{\"ok\":false}".to_string());
    println!("{text}");
}

async fn run_benchmark_replay_async(args: Args) -> BenchmarkReport {
    let mut report = BenchmarkReport {
        ok: false,
        profile: args.frontier_profile.as_str().to_string(),
        audio_file: args.benchmark_input_wav.clone(),
        audio_ms: 0,
        sample_count: 0,
        auth_source: None,
        ws_url: None,
        connect_started_at_ms: None,
        ready_at_ms: None,
        first_audio_sent_at_ms: None,
        finish_sent_at_ms: None,
        first_result_at_ms: None,
        first_partial_at_ms: None,
        first_final_at_ms: None,
        final_at_ms: None,
        warmup_ms: None,
        first_result_frame_ms: None,
        first_partial_frame_ms: None,
        first_final_frame_ms: None,
        first_result_after_audio_ms: None,
        infer_ms: None,
        partial_count: 0,
        final_count: 0,
        final_text: String::new(),
        error: None,
    };

    let audio_path = match args.benchmark_input_wav.as_deref() {
        Some(path) => path,
        None => {
            report.error = Some("missing --benchmark-input-wav".to_string());
            return report;
        }
    };
    let samples = match load_wav_i16(audio_path) {
        Ok(samples) => samples,
        Err(error) => {
            report.error = Some(error);
            return report;
        }
    };
    report.sample_count = samples.len();
    report.audio_ms = ((samples.len() as f64 / PCM_SAMPLE_RATE as f64) * 1000.0) as u64;

    let auth = match resolve_auth_for_benchmark(&args) {
        Ok(auth) => auth,
        Err(error) => {
            report.error = Some(error);
            return report;
        }
    };
    report.auth_source = Some(auth.source.clone());

    let ws_url = bench_ws_url(&args, &auth);
    report.ws_url = Some(ws_url.clone());

    if args.benchmark_warmup {
        if let Err(error) = run_warmup_pass(&args, &auth, &ws_url).await {
            report.error = Some(format!("warmup failed: {error}"));
            return report;
        }
    }

    let request = match build_request_for_profile(&args, &auth, &ws_url) {
        Ok(request) => request,
        Err(error) => {
            report.error = Some(error);
            return report;
        }
    };

    let connect_started_at_ms = now_millis();
    report.connect_started_at_ms = Some(connect_started_at_ms);
    let (socket, _) = match connect_async(request).await {
        Ok(value) => value,
        Err(error) => {
            report.error = Some(format!("frontier connect error: {error}"));
            return report;
        }
    };

    let (mut writer, mut reader) = socket.split();
    let session_id = Uuid::new_v4().to_string().to_uppercase();
    let context_config = load_context_config();
    let runtime_context = FrontierRuntimeContext {
        app_bundle_id: Some("com.apple.Terminal".to_string()),
        text_context_text: None,
        text_context_cursor_position: None,
        capture_ms: now_millis(),
        source: "benchmark_replay".to_string(),
    };

    if let Err(error) = send_start_sequence(
        &args,
        &auth,
        &session_id,
        &context_config,
        &runtime_context,
        &mut writer,
    )
    .await
    {
        report.error = Some(error);
        let _ = writer.close().await;
        return report;
    }

    let mut state = BenchState::default();
    let ready_deadline = Instant::now() + READY_TIMEOUT;
    while state.ready_at_ms.is_none() {
        let remaining = ready_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            report.error = Some("frontier ready timeout".to_string());
            let _ = writer.close().await;
            return report;
        }
        match timeout(remaining, reader.next()).await {
            Ok(Some(Ok(message))) => {
                if let Err(error) = handle_message(message, &mut state) {
                    report.error = Some(error);
                    let _ = writer.close().await;
                    return report;
                }
            }
            Ok(Some(Err(error))) => {
                report.error = Some(format!("frontier read error: {error}"));
                let _ = writer.close().await;
                return report;
            }
            Ok(None) => {
                report.error = Some("frontier closed before ready".to_string());
                let _ = writer.close().await;
                return report;
            }
            Err(_) => {
                report.error = Some("frontier ready timeout".to_string());
                let _ = writer.close().await;
                return report;
            }
        }
    }

    report.ready_at_ms = state.ready_at_ms;
    report.warmup_ms = state.ready_at_ms.map(|ts| ts.saturating_sub(connect_started_at_ms));

    let chunk_ms = args.benchmark_chunk_ms.max(10);
    let chunk_samples = ((PCM_SAMPLE_RATE as u64 * chunk_ms) / 1000) as usize;
    let chunk_samples = chunk_samples.max(1);
    let start_instant = Instant::now();
    let mut first_audio_sent_at_ms = None;
    let mut finish_sent_at_ms = None;
    let mut sample_offset = 0usize;
    let mut frame_index = 0usize;
    let mut opus_state = if args.frontier_profile.uses_opus() {
        match AndroidOpusState::new() {
            Ok(state) => Some(state),
            Err(error) => {
                report.error = Some(error);
                let _ = writer.close().await;
                return report;
            }
        }
    } else {
        None
    };

    loop {
        if sample_offset >= samples.len() && finish_sent_at_ms.is_some() && (!state.final_text.is_empty() || state.terminal) {
            break;
        }
        let next_audio_deadline = if sample_offset < samples.len() {
            Some(start_instant + Duration::from_millis((frame_index as u64) * chunk_ms))
        } else if finish_sent_at_ms.is_none() {
            Some(Instant::now())
        } else {
            None
        };

        if let Some(deadline) = next_audio_deadline {
            tokio::select! {
                _ = sleep_until(deadline) => {
                    if sample_offset < samples.len() {
                        let end = (sample_offset + chunk_samples).min(samples.len());
                        let chunk = &samples[sample_offset..end];
                        let now = now_millis();
                        if first_audio_sent_at_ms.is_none() {
                            first_audio_sent_at_ms = Some(now);
                        }
                        if let Err(error) = send_audio_chunk_for_profile(
                            &args,
                            &auth,
                            &session_id,
                            chunk,
                            sample_offset == 0,
                            end >= samples.len(),
                            &mut writer,
                            opus_state.as_mut(),
                        ).await {
                            report.error = Some(error);
                            let _ = writer.close().await;
                            return report;
                        }
                        sample_offset = end;
                        frame_index += 1;
                    } else if finish_sent_at_ms.is_none() {
                        if let Err(error) = send_finish_for_profile(&args, &auth, &session_id, &mut writer).await {
                            report.error = Some(error);
                            let _ = writer.close().await;
                            return report;
                        }
                        finish_sent_at_ms = Some(now_millis());
                    }
                }
                maybe_message = reader.next() => {
                    match maybe_message {
                        Some(Ok(message)) => {
                            if let Err(error) = handle_message(message, &mut state) {
                                report.error = Some(error);
                                let _ = writer.close().await;
                                return report;
                            }
                        }
                        Some(Err(error)) => {
                            report.error = Some(format!("frontier read error: {error}"));
                            let _ = writer.close().await;
                            return report;
                        }
                        None => {
                            break;
                        }
                    }
                }
            }
        } else {
            match timeout(FINAL_TIMEOUT, reader.next()).await {
                Ok(Some(Ok(message))) => {
                    if let Err(error) = handle_message(message, &mut state) {
                        report.error = Some(error);
                        let _ = writer.close().await;
                        return report;
                    }
                }
                Ok(Some(Err(error))) => {
                    report.error = Some(format!("frontier read error: {error}"));
                    let _ = writer.close().await;
                    return report;
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        if finish_sent_at_ms.is_some() && Instant::now().duration_since(start_instant) > Duration::from_secs_f64(args.benchmark_timeout_secs.max(1.0)) {
            break;
        }
    }

    let _ = writer.close().await;
    report.ok = !state.final_text.is_empty() || state.final_count > 0;
    report.first_audio_sent_at_ms = first_audio_sent_at_ms;
    report.finish_sent_at_ms = finish_sent_at_ms;
    report.first_result_at_ms = state.first_result_at_ms;
    report.first_partial_at_ms = state.first_partial_at_ms;
    report.first_final_at_ms = state.first_final_at_ms;
    report.final_at_ms = state.first_final_at_ms.or(state.first_result_at_ms);
    report.first_result_frame_ms = state.first_result_at_ms.map(|ts| ts.saturating_sub(connect_started_at_ms));
    report.first_partial_frame_ms = state.first_partial_at_ms.map(|ts| ts.saturating_sub(connect_started_at_ms));
    report.first_final_frame_ms = state.first_final_at_ms.map(|ts| ts.saturating_sub(connect_started_at_ms));
    report.first_result_after_audio_ms = match (state.first_result_at_ms, first_audio_sent_at_ms) {
        (Some(result), Some(first_audio)) => Some(result.saturating_sub(first_audio)),
        _ => None,
    };
    report.infer_ms = match (state.first_final_at_ms, finish_sent_at_ms) {
        (Some(final_at), Some(finish_at)) => Some(final_at.saturating_sub(finish_at)),
        _ => None,
    };
    report.partial_count = state.partial_count;
    report.final_count = state.final_count;
    report.final_text = state.final_text;
    if !report.ok && report.error.is_none() {
        report.error = Some("no final transcript received".to_string());
    }
    report
}

fn resolve_auth_for_benchmark(args: &Args) -> Result<FrontierAuthMaterial, String> {
    match args.frontier_profile {
        FrontierProfile::CurrentPcm | FrontierProfile::CurrentOpus => resolve_frontier_auth(args),
        FrontierProfile::AndroidPcm | FrontierProfile::AndroidOpus => {
            resolve_frontier_auth_for_profile(args, args.frontier_profile)
        }
    }
}

fn load_wav_i16(path: &str) -> Result<Vec<i16>, String> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|error| format!("open wav failed: {error}"))?;
    let spec = reader.spec();
    if spec.channels != 1 {
        return Err(format!("wav must be mono, got {} channels", spec.channels));
    }
    if spec.sample_rate != PCM_SAMPLE_RATE as u32 {
        return Err(format!("wav must be 16000Hz, got {}Hz", spec.sample_rate));
    }
    if spec.bits_per_sample != 16 {
        return Err(format!("wav must be 16-bit PCM, got {} bits", spec.bits_per_sample));
    }
    let samples = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read wav samples failed: {error}"))?;
    Ok(samples)
}

fn bench_ws_url(args: &Args, auth: &FrontierAuthMaterial) -> String {
    if args.frontier_profile.uses_android_payload() {
        auth.ws_url.clone().unwrap_or_else(|| ANDROID_WS_BASE_URL.to_string())
    } else if args.server_url.starts_with("ws://127.0.0.1") {
        auth.ws_url
            .clone()
            .unwrap_or_else(|| DEFAULT_FRONTIER_WS_URL.to_string())
    } else {
        args.server_url.clone()
    }
}

fn build_request_for_profile(
    args: &Args,
    _auth: &FrontierAuthMaterial,
    ws_url: &str,
) -> Result<Request<()>, String> {
    let mut request = ws_url
        .into_client_request()
        .map_err(|error| format!("frontier request build failed: {error}"))?;
    let headers = request.headers_mut();
    headers.insert(HeaderName::from_static("proto-version"), HeaderValue::from_static("v2"));
    headers.insert(HeaderName::from_static("x-custom-keepalive"), HeaderValue::from_static("true"));
    headers.insert(HeaderName::from_static("x-keepalive-interval"), HeaderValue::from_static("3"));
    headers.insert(HeaderName::from_static("x-keepalive-timeout"), HeaderValue::from_static("3600"));
    if args.frontier_profile.uses_android_payload() {
        headers.insert(
            HeaderName::from_static("user-agent"),
            HeaderValue::from_static(ANDROID_USER_AGENT),
        );
    } else {
        let device_key = std::env::var("FRONTIER_DEVICE_KEY")
            .unwrap_or_else(|_| DEFAULT_DEVICE_KEY.to_string());
        headers.insert(
            HeaderName::from_static("x-tt-e-k"),
            HeaderValue::from_str(&device_key).map_err(|error| error.to_string())?,
        );
    }
    Ok(request)
}

async fn run_warmup_pass(
    args: &Args,
    auth: &FrontierAuthMaterial,
    ws_url: &str,
) -> Result<(), String> {
    let request = build_request_for_profile(args, auth, ws_url)?;
    let (socket, _) = connect_async(request)
        .await
        .map_err(|error| format!("warmup connect error: {error}"))?;
    let (mut writer, mut reader) = socket.split();
    let session_id = Uuid::new_v4().to_string().to_uppercase();
    let context_config = load_context_config();
    let runtime_context = FrontierRuntimeContext {
        app_bundle_id: Some("com.apple.Terminal".to_string()),
        text_context_text: None,
        text_context_cursor_position: None,
        capture_ms: now_millis(),
        source: "benchmark_warmup".to_string(),
    };

    send_start_sequence(
        args,
        auth,
        &session_id,
        &context_config,
        &runtime_context,
        &mut writer,
    )
    .await?;

    let ready_deadline = Instant::now() + READY_TIMEOUT;
    let mut ready = false;
    let mut state = BenchState::default();
    while !ready {
        let remaining = ready_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = writer.close().await;
            return Err("frontier ready timeout".to_string());
        }
        match timeout(remaining, reader.next()).await {
            Ok(Some(Ok(message))) => {
                handle_message(message, &mut state)?;
                ready = state.ready_at_ms.is_some();
            }
            Ok(Some(Err(error))) => {
                let _ = writer.close().await;
                return Err(format!("frontier read error: {error}"));
            }
            Ok(None) => {
                let _ = writer.close().await;
                return Err("frontier closed before ready".to_string());
            }
            Err(_) => {
                let _ = writer.close().await;
                return Err("frontier ready timeout".to_string());
            }
        }
    }

    send_finish_for_profile(args, auth, &session_id, &mut writer).await?;
    let _ = timeout(FINAL_TIMEOUT, async {
        while let Some(message) = reader.next().await {
            let message = message.map_err(|error| format!("frontier read error: {error}"))?;
            handle_message(message, &mut state)?;
            if state.terminal {
                break;
            }
        }
        Ok::<(), String>(())
    })
    .await;
    let _ = writer.close().await;
    Ok(())
}

async fn send_start_sequence(
    args: &Args,
    auth: &FrontierAuthMaterial,
    session_id: &str,
    context_config: &ContextConfig,
    runtime_context: &FrontierRuntimeContext,
    writer: &mut WsWrite,
) -> Result<(), String> {
    if args.frontier_profile.uses_android_payload() {
        let start_task = build_android_request(&auth.token, "StartTask", "", &[], session_id, None);
        writer
            .send(Message::Binary(start_task.into()))
            .await
            .map_err(|error| format!("frontier start_task send failed: {error}"))?;
        let session_payload = build_android_session_config(args.frontier_profile);
        let start_session = build_android_request(
            &auth.token,
            "StartSession",
            &session_payload,
            &[],
            session_id,
            None,
        );
        writer
            .send(Message::Binary(start_session.into()))
            .await
            .map_err(|error| format!("frontier start_session send failed: {error}"))?;
    } else {
        let request_profile = build_effective_request_profile(context_config, Some(runtime_context));
        let start_task = build_start_task(session_id, auth.request_token_field(), &auth.app_key);
        writer
            .send(Message::Binary(start_task.into()))
            .await
            .map_err(|error| format!("frontier start_task send failed: {error}"))?;
        let audio_format = if args.frontier_profile.uses_opus() { "speech_opus" } else { "pcm" };
        let (start_session, _, _) = build_start_session(
            session_id,
            auth.request_token_field(),
            &auth.app_key,
            audio_format,
            Some(&request_profile),
            None,
            now_millis() / 1000,
        )?;
        writer
            .send(Message::Binary(start_session.into()))
            .await
            .map_err(|error| format!("frontier start_session send failed: {error}"))?;
    }
    Ok(())
}

async fn send_audio_chunk_for_profile(
    args: &Args,
    auth: &FrontierAuthMaterial,
    session_id: &str,
    chunk: &[i16],
    is_first: bool,
    is_last_audio_chunk: bool,
    writer: &mut WsWrite,
    opus_state: Option<&mut AndroidOpusState>,
) -> Result<(), String> {
    if args.frontier_profile.uses_opus() {
        let opus_state = opus_state.ok_or_else(|| "opus state missing".to_string())?;
        let padded_chunk = normalize_opus_chunk(chunk, args.benchmark_chunk_ms);
        let payload = opus_state.encode_frame(&padded_chunk)?;
        if args.frontier_profile.uses_android_payload() {
            let frame_state = if is_first {
                AndroidFrameState::First
            } else if is_last_audio_chunk {
                AndroidFrameState::Last
            } else {
                AndroidFrameState::Middle
            };
            let metadata = serde_json::json!({
                "extra": {},
                "timestamp_ms": opus_state.timestamp_ms,
            });
            opus_state.timestamp_ms = opus_state.timestamp_ms.saturating_add(PCM_FRAME_MS);
            let packet = build_android_request(
                "",
                "TaskRequest",
                &serde_json::to_string(&metadata).unwrap_or_default(),
                &payload,
                session_id,
                Some(frame_state),
            );
            writer
                .send(Message::Binary(packet.into()))
                .await
                .map_err(|error| format!("frontier audio send failed: {error}"))?;
        } else {
            let packet = build_audio_frame(session_id, &payload, now_millis(), 0);
            writer
                .send(Message::Binary(packet.into()))
                .await
                .map_err(|error| format!("frontier audio send failed: {error}"))?;
        }
    } else {
        let mut pcm = Vec::with_capacity(chunk.len() * 2);
        for sample in chunk {
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
        let packet = if args.frontier_profile.uses_android_payload() {
            build_android_request("", "TaskRequest", &format!(r#"{{\"timestamp_ms\":{}}}"#, now_millis()), &pcm, session_id, None)
        } else {
            build_audio_frame(session_id, &pcm, now_millis(), 0)
        };
        writer
            .send(Message::Binary(packet.into()))
            .await
            .map_err(|error| format!("frontier audio send failed: {error}"))?;
    }
    let _ = auth;
    Ok(())
}

fn normalize_opus_chunk(chunk: &[i16], chunk_ms: u64) -> Vec<i16> {
    let expected = ((PCM_SAMPLE_RATE as u64 * chunk_ms.max(10)) / 1000) as usize;
    if chunk.len() >= expected {
        return chunk.to_vec();
    }
    let mut padded = Vec::with_capacity(expected);
    padded.extend_from_slice(chunk);
    padded.resize(expected, 0);
    padded
}

async fn send_finish_for_profile(
    args: &Args,
    auth: &FrontierAuthMaterial,
    session_id: &str,
    writer: &mut WsWrite,
) -> Result<(), String> {
    let packet = if args.frontier_profile.uses_android_payload() {
        build_android_request(&auth.token, "FinishSession", "", &[], session_id, None)
    } else {
        build_finish_session(session_id, &auth.app_key)
    };
    writer
        .send(Message::Binary(packet.into()))
        .await
        .map_err(|error| format!("frontier finish_session send failed: {error}"))?;
    Ok(())
}

fn handle_message(message: Message, state: &mut BenchState) -> Result<(), String> {
    let Message::Binary(bytes) = message else {
        if matches!(message, Message::Close(_)) {
            state.terminal = true;
        }
        return Ok(());
    };
    let parsed = parse_frontier_response(&bytes).ok_or_else(|| "parse frontier response failed".to_string())?;
    if parsed.status_code != 20_000_000 {
        return Err(format!(
            "frontier {} failed: {} {}",
            if parsed.event.is_empty() { "response" } else { parsed.event.as_str() },
            parsed.status_code,
            parsed.status_text
        ));
    }
    let received_at_ms = now_millis();
    if parsed.event == "SessionStarted" {
        state.ready_at_ms = Some(received_at_ms);
    }
    if matches!(parsed.event.as_str(), "SessionFinished" | "TaskFinished") {
        state.terminal = true;
    }
    let Some(payload) = parsed.payload_json else {
        return Ok(());
    };
    let (partial_text, final_text, saw_terminal) = extract_latest_result(&payload);
    if (!partial_text.is_empty() || !final_text.is_empty()) && state.first_result_at_ms.is_none() {
        state.first_result_at_ms = Some(received_at_ms);
    }
    if !partial_text.is_empty() {
        state.partial_count += 1;
        if state.first_partial_at_ms.is_none() {
            state.first_partial_at_ms = Some(received_at_ms);
        }
    }
    if !final_text.is_empty() {
        state.final_count += 1;
        if state.first_final_at_ms.is_none() {
            state.first_final_at_ms = Some(received_at_ms);
        }
        state.final_text = final_text;
    }
    if saw_terminal {
        state.terminal = true;
    }
    Ok(())
}

fn build_android_session_config(profile: FrontierProfile) -> String {
    let audio_format = if profile.uses_opus() { "speech_opus" } else { "pcm" };
    serde_json::json!({
        "audio_info": {
            "channel": 1,
            "format": audio_format,
            "sample_rate": 16_000,
        },
        "enable_punctuation": true,
        "enable_speech_rejection": false,
        "extra": {
            "app_name": "com.android.chrome",
            "cell_compress_rate": 8,
            "enable_asr_threepass": true,
            "enable_asr_twopass": true,
            "input_mode": "tool",
        }
    })
    .to_string()
}

fn build_android_request(
    token: &str,
    method_name: &str,
    payload: &str,
    audio_data: &[u8],
    request_id: &str,
    frame_state: Option<AndroidFrameState>,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(128 + audio_data.len());
    write_length_delimited(&mut buf, 2, token.as_bytes());
    write_length_delimited(&mut buf, 3, b"ASR");
    write_length_delimited(&mut buf, 5, method_name.as_bytes());
    write_length_delimited(&mut buf, 6, payload.as_bytes());
    write_length_delimited(&mut buf, 7, audio_data);
    write_length_delimited(&mut buf, 8, request_id.as_bytes());
    if let Some(frame_state) = frame_state {
        write_varint_field(&mut buf, 9, frame_state as u64);
    }
    buf
}

fn write_length_delimited(buf: &mut Vec<u8>, field_num: u64, data: &[u8]) {
    if data.is_empty() {
        return;
    }
    write_varint(buf, (field_num << 3) | 2);
    write_varint(buf, data.len() as u64);
    buf.extend_from_slice(data);
}

fn write_varint_field(buf: &mut Vec<u8>, field_num: u64, value: u64) {
    write_varint(buf, field_num << 3);
    write_varint(buf, value);
}

fn write_varint(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            buf.push(byte);
            break;
        }
        buf.push(byte | 0x80);
    }
}

fn parse_frontier_response(bytes: &[u8]) -> Option<ParsedFrontierResponse> {
    let mut offset = 0usize;
    let mut event = String::new();
    let mut status_code = 20_000_000u64;
    let mut status_text = "OK".to_string();
    let mut payload_json = None;
    while offset < bytes.len() {
        let tag = decode_varint(bytes, &mut offset)?;
        let field_number = tag >> 3;
        let wire_type = tag & 0x7;
        match wire_type {
            0 => {
                let value = decode_varint(bytes, &mut offset)?;
                if field_number == 5 {
                    status_code = value;
                }
            }
            2 => {
                let length = decode_varint(bytes, &mut offset)? as usize;
                if offset + length > bytes.len() {
                    return None;
                }
                let chunk = &bytes[offset..offset + length];
                offset += length;
                if field_number == 4 {
                    event = String::from_utf8_lossy(chunk).to_string();
                } else if field_number == 6 {
                    status_text = String::from_utf8_lossy(chunk).to_string();
                } else if field_number == 7 {
                    if let Ok(text) = std::str::from_utf8(chunk) {
                        if let Ok(json) = serde_json::from_str::<Value>(text) {
                            payload_json = Some(json);
                        }
                    }
                }
            }
            1 => offset = offset.saturating_add(8),
            5 => offset = offset.saturating_add(4),
            _ => return None,
        }
    }
    Some(ParsedFrontierResponse { event, status_code, status_text, payload_json })
}

fn decode_varint(bytes: &[u8], offset: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0u32;
    while *offset < bytes.len() {
        let byte = bytes[*offset];
        *offset += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    None
}

fn extract_latest_result(payload: &Value) -> (String, String, bool) {
    let mut partial_candidates = Vec::new();
    let mut final_candidates = Vec::new();
    let mut saw_terminal = false;
    let Some(results) = payload.get("results").and_then(Value::as_array) else {
        return (String::new(), String::new(), false);
    };
    for (order, item) in results.iter().enumerate() {
        let Some(item) = item.as_object() else {
            continue;
        };
        let text = item.get("text").and_then(Value::as_str).unwrap_or("").trim().to_string();
        let is_interim = item.get("is_interim").and_then(Value::as_bool);
        let is_vad_finished = item.get("is_vad_finished").and_then(Value::as_bool).unwrap_or(false);
        let extra = item.get("extra").and_then(Value::as_object);
        let nonstream_result = extra.and_then(|extra| extra.get("nonstream_result")).and_then(Value::as_bool).unwrap_or(false);
        let is_terminal = nonstream_result || (is_interim == Some(false) && is_vad_finished);
        if is_terminal {
            saw_terminal = true;
        }
        if text.is_empty() {
            continue;
        }
        let candidate = ResultCandidate {
            order,
            index: item.get("index").and_then(Value::as_i64).or_else(|| item.get("index").and_then(Value::as_u64).map(|value| value as i64)),
            text,
            has_seq_id: extra.and_then(|extra| extra.get("seq_id")).is_some(),
            stream_asr_finish: item.get("stream_asr_finish").and_then(Value::as_bool).unwrap_or(false),
        };
        if is_terminal {
            final_candidates.push(candidate);
        } else {
            partial_candidates.push(candidate);
        }
    }
    (
        compose_partial_text(&partial_candidates),
        compose_final_text(&final_candidates),
        saw_terminal,
    )
}

fn compose_partial_text(candidates: &[ResultCandidate]) -> String {
    if candidates.is_empty() {
        return String::new();
    }
    if candidates.iter().any(|candidate| candidate.index.is_some()) {
        let mut grouped: BTreeMap<i64, Vec<&ResultCandidate>> = BTreeMap::new();
        for candidate in candidates {
            let key = candidate.index.unwrap_or(1_000_000 + candidate.order as i64);
            grouped.entry(key).or_default().push(candidate);
        }
        return grouped
            .values()
            .filter_map(|group| best_partial_candidate(group).map(|candidate| candidate.text.as_str()))
            .fold(String::new(), |joined, text| join_transcript_text(&joined, text));
    }
    let refs = candidates.iter().collect::<Vec<_>>();
    best_partial_candidate(&refs).map(|candidate| candidate.text.clone()).unwrap_or_default()
}

fn compose_final_text(candidates: &[ResultCandidate]) -> String {
    if candidates.is_empty() {
        return String::new();
    }
    if candidates.iter().any(|candidate| candidate.index.is_some()) {
        let mut grouped: BTreeMap<i64, Vec<&ResultCandidate>> = BTreeMap::new();
        for candidate in candidates {
            let key = candidate.index.unwrap_or(1_000_000 + candidate.order as i64);
            grouped.entry(key).or_default().push(candidate);
        }
        return grouped
            .values()
            .filter_map(|group| best_display_candidate(group).map(|candidate| candidate.text.as_str()))
            .fold(String::new(), |joined, text| join_transcript_text(&joined, text));
    }
    let refs = candidates.iter().collect::<Vec<_>>();
    best_display_candidate(&refs).map(|candidate| candidate.text.clone()).unwrap_or_default()
}

fn best_display_candidate<'a>(candidates: &'a [&'a ResultCandidate]) -> Option<&'a ResultCandidate> {
    candidates.iter().copied().max_by_key(|candidate| {
        let punctuation_score = candidate
            .text
            .chars()
            .filter(|ch| matches!(ch, '。' | '，' | '！' | '？' | ',' | '.' | '!' | '?'))
            .count();
        (
            !candidate.has_seq_id,
            candidate.stream_asr_finish,
            punctuation_score,
            candidate.text.chars().count(),
            Reverse(candidate.order),
        )
    })
}

fn best_partial_candidate<'a>(candidates: &'a [&'a ResultCandidate]) -> Option<&'a ResultCandidate> {
    let best_display = candidates.iter().copied().filter(|candidate| !candidate.has_seq_id).max_by_key(|candidate| {
        (candidate.stream_asr_finish, candidate.text.chars().count(), Reverse(candidate.order))
    });
    let best_streaming = candidates.iter().copied().filter(|candidate| candidate.has_seq_id).max_by_key(|candidate| {
        (candidate.stream_asr_finish, candidate.text.chars().count(), Reverse(candidate.order))
    });
    match (best_display, best_streaming) {
        (Some(display), Some(streaming)) => {
            if streaming.text.chars().count() >= display.text.chars().count() {
                Some(streaming)
            } else {
                Some(display)
            }
        }
        (Some(display), None) => Some(display),
        (None, Some(streaming)) => Some(streaming),
        (None, None) => None,
    }
}

fn join_transcript_text(prefix: &str, suffix: &str) -> String {
    let left = prefix.trim();
    let right = suffix.trim();
    if left.is_empty() {
        return right.to_string();
    }
    if right.is_empty() {
        return left.to_string();
    }
    if left.chars().last().map(|ch| ch.is_ascii_alphanumeric()).unwrap_or(false)
        && right.chars().next().map(|ch| ch.is_ascii_alphanumeric()).unwrap_or(false)
    {
        format!("{left} {right}")
    } else {
        format!("{left}{right}")
    }
}
