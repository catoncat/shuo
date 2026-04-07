use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::{cmp::Reverse, collections::BTreeMap};

use futures_util::{SinkExt, StreamExt};
use http::{header::HeaderName, HeaderValue, Request};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::runtime::Builder;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

use crate::config::ContextConfig;
use crate::frontier_auth::{refresh_frontier_auth, resolve_frontier_auth, FrontierAuthMaterial};
use crate::frontier_protocol::{
    build_audio_frame, build_effective_request_profile, build_finish_session, build_start_session,
    build_start_task, FrontierRuntimeContext, DEFAULT_FRONTIER_WS_URL,
};
use crate::legacy_transport::{LegacyTransportCommand, LegacyTransportEvent};
use crate::state::now_millis;
use crate::Args;

const CONNECT_READY_TIMEOUT: Duration = Duration::from_secs(2);
const FINAL_TIMEOUT: Duration = Duration::from_secs(6);
const SAMPLE_RATE: usize = 16_000;
const SAMPLE_WIDTH: usize = 2;
const PCM_CHUNK_BYTES: usize = 320 * SAMPLE_WIDTH;
const DEFAULT_DEVICE_KEY: &str = "4285264416738169+W";

type WsWrite = futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;

#[derive(Debug, Default)]
struct FrontierSharedState {
    ready_at_ms: Option<u64>,
    latest_partial_text: String,
    latest_final_text: String,
    error: Option<String>,
    terminal: bool,
    finish_done_at_ms: Option<u64>,
}

struct FrontierSession {
    writer: WsWrite,
    read_task: JoinHandle<()>,
    shared: Arc<Mutex<FrontierSharedState>>,
    final_notify: Arc<Notify>,
    auth: FrontierAuthMaterial,
    frontier_session_id: String,
    runtime_context: FrontierRuntimeContext,
    context_config: ContextConfig,
    connect_started_at_ms: u64,
    finish_sent_at_ms: Option<u64>,
    audio_bytes_sent: usize,
    pending_pcm: Vec<u8>,
}

struct FrontierRuntime {
    args: Args,
    event_tx: Sender<LegacyTransportEvent>,
    context_config: ContextConfig,
    runtime_context: Option<FrontierRuntimeContext>,
    session: Option<FrontierSession>,
}

pub(crate) fn spawn_frontier_transport(
    args: Args,
    event_tx: Sender<LegacyTransportEvent>,
) -> UnboundedSender<LegacyTransportCommand> {
    let (cmd_tx, mut cmd_rx): (
        UnboundedSender<LegacyTransportCommand>,
        UnboundedReceiver<LegacyTransportCommand>,
    ) = unbounded_channel();

    thread::spawn(move || {
        let runtime = Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to build frontier transport runtime");

        runtime.block_on(async move {
            let mut frontier = FrontierRuntime {
                args,
                event_tx,
                context_config: ContextConfig::default(),
                runtime_context: None,
                session: None,
            };

            while let Some(command) = cmd_rx.recv().await {
                let should_continue = frontier.handle_command(command).await;
                if !should_continue {
                    break;
                }
            }

            frontier.close_session().await;
        });
    });

    cmd_tx
}

impl FrontierRuntime {
    async fn handle_command(&mut self, command: LegacyTransportCommand) -> bool {
        match command {
            LegacyTransportCommand::ConfigSnapshot { config } => {
                let should_invalidate = self.should_invalidate_for_config(&config);
                self.context_config = config;
                if should_invalidate {
                    self.close_session().await;
                    self.emit_profile_invalidated("config_updated");
                }
            }
            LegacyTransportCommand::Reset => {
                self.close_session().await;
                self.runtime_context = None;
                let _ = self.event_tx.send(LegacyTransportEvent::Status {
                    name: "noop".to_string(),
                    context: None,
                    timings: None,
                });
            }
            LegacyTransportCommand::Warmup { force } => {
                if force {
                    self.close_session().await;
                }
                match self.ensure_session().await {
                    Ok(session) => {
                        let timings = snapshot_timings(session, 0, 0, 0).await;
                        let ready_event = LegacyTransportEvent::Ready {
                            app_key: Some(session.auth.app_key.clone()),
                            token_source: Some(format!(
                                "{}:exp={}",
                                session.auth.source, session.auth.exp
                            )),
                            expires_at_ms: if session.auth.exp > 0 {
                                Some(session.auth.exp.saturating_mul(1000))
                            } else {
                                None
                            },
                            context_source: Some(effective_context_source(
                                &session.runtime_context,
                                &session.context_config,
                            )),
                        };
                        let warmed_event = LegacyTransportEvent::Status {
                            name: "warmed".to_string(),
                            context: Some(build_context_summary(
                                &session.runtime_context,
                                &session.context_config,
                            )),
                            timings: Some(timings),
                        };
                        let event_tx = self.event_tx.clone();
                        let _ = event_tx.send(ready_event);
                        let _ = event_tx.send(warmed_event);
                    }
                    Err(error) => {
                        let _ = self.event_tx.send(LegacyTransportEvent::Error {
                            message: error,
                            recoverable: true,
                        });
                    }
                }
            }
            LegacyTransportCommand::RefreshAuth => {
                self.close_session().await;
                match self.ensure_session_with_refresh().await {
                    Ok(session) => {
                        let timings = snapshot_timings(session, 0, 0, 0).await;
                        let ready_event = LegacyTransportEvent::Ready {
                            app_key: Some(session.auth.app_key.clone()),
                            token_source: Some(format!(
                                "{}:exp={}",
                                session.auth.source, session.auth.exp
                            )),
                            expires_at_ms: if session.auth.exp > 0 {
                                Some(session.auth.exp.saturating_mul(1000))
                            } else {
                                None
                            },
                            context_source: Some(effective_context_source(
                                &session.runtime_context,
                                &session.context_config,
                            )),
                        };
                        let warmed_event = LegacyTransportEvent::Status {
                            name: "warmed".to_string(),
                            context: Some(build_context_summary(
                                &session.runtime_context,
                                &session.context_config,
                            )),
                            timings: Some(timings),
                        };
                        let event_tx = self.event_tx.clone();
                        let _ = event_tx.send(ready_event);
                        let _ = event_tx.send(warmed_event);
                    }
                    Err(error) => {
                        let _ = self.event_tx.send(LegacyTransportEvent::Error {
                            message: error,
                            recoverable: true,
                        });
                    }
                }
            }
            LegacyTransportCommand::UpdateContext { context } => {
                let runtime_context = FrontierRuntimeContext::from_context_snapshot(&context);
                let should_invalidate = self.should_invalidate_for_context(&runtime_context);
                self.runtime_context = Some(runtime_context.clone());
                if should_invalidate {
                    self.close_session().await;
                    self.emit_profile_invalidated("context_updated");
                } else if let Some(session) = self.session.as_mut() {
                    session.runtime_context = runtime_context.clone();
                }
                let _ = self.event_tx.send(LegacyTransportEvent::Status {
                    name: "context_updated".to_string(),
                    context: Some(build_context_summary(&runtime_context, &self.context_config)),
                    timings: Some(serde_json::json!({
                        "context_capture_ms": runtime_context.capture_ms,
                        "context_available": effective_context_available(&runtime_context, &self.context_config),
                        "context_source": effective_context_source(&runtime_context, &self.context_config),
                        "elapsed_ms": 0,
                        "reason": runtime_context.source,
                    })),
                });
            }
            LegacyTransportCommand::Audio(samples) => {
                if let Err(error) = self.send_audio(samples).await {
                    let _ = self.event_tx.send(LegacyTransportEvent::Error {
                        message: error,
                        recoverable: true,
                    });
                }
            }
            LegacyTransportCommand::Partial { utterance_id } => {
                let (text, timings) = match self.snapshot_partial().await {
                    Some((text, timings)) => (text, Some(timings)),
                    None => (String::new(), None),
                };
                let _ = self.event_tx.send(LegacyTransportEvent::Partial {
                    text,
                    utterance_id,
                    timings,
                });
            }
            LegacyTransportCommand::Flush { utterance_id } => match self.finish_session().await {
                Ok(Some((text, timings))) => {
                    let _ = self.event_tx.send(LegacyTransportEvent::Final {
                        text,
                        utterance_id,
                        timings: Some(timings),
                    });
                }
                Ok(None) => {}
                Err(error) => {
                    let _ = self.event_tx.send(LegacyTransportEvent::Error {
                        message: error,
                        recoverable: true,
                    });
                }
            },
            LegacyTransportCommand::Close => return false,
        }
        true
    }

    fn should_invalidate_for_config(&self, next_config: &ContextConfig) -> bool {
        self.session
            .as_ref()
            .filter(|session| session.finish_sent_at_ms.is_none() && session.audio_bytes_sent == 0)
            .map(|session| session.context_config != *next_config)
            .unwrap_or(false)
    }

    fn should_invalidate_for_context(&self, next_context: &FrontierRuntimeContext) -> bool {
        self.session
            .as_ref()
            .filter(|session| session.finish_sent_at_ms.is_none() && session.audio_bytes_sent == 0)
            .map(|session| session.runtime_context != *next_context)
            .unwrap_or(false)
    }

    fn emit_profile_invalidated(&self, reason: &str) {
        let runtime_context = self
            .runtime_context
            .clone()
            .unwrap_or(FrontierRuntimeContext {
                app_bundle_id: None,
                text_context_text: None,
                text_context_cursor_position: None,
                capture_ms: 0,
                source: "none".to_string(),
            });
        let _ = self.event_tx.send(LegacyTransportEvent::Status {
            name: "session_profile_invalidated".to_string(),
            context: Some(build_context_summary(&runtime_context, &self.context_config)),
            timings: Some(serde_json::json!({
                "reason": reason,
                "context_source": effective_context_source(&runtime_context, &self.context_config),
                "context_available": effective_context_available(&runtime_context, &self.context_config),
            })),
        });
    }

    async fn ensure_session(&mut self) -> Result<&mut FrontierSession, String> {
        self.ensure_session_with_resolver(false).await
    }

    async fn ensure_session_with_refresh(&mut self) -> Result<&mut FrontierSession, String> {
        self.ensure_session_with_resolver(true).await
    }

    async fn ensure_session_with_resolver(
        &mut self,
        force_refresh: bool,
    ) -> Result<&mut FrontierSession, String> {
        let reusable = self
            .session
            .as_ref()
            .map(|session| session.finish_sent_at_ms.is_none())
            .unwrap_or(false);
        if reusable {
            return Ok(self.session.as_mut().expect("reusable session"));
        }

        self.close_session().await;
        let auth = if force_refresh {
            refresh_frontier_auth(&self.args)?
        } else {
            resolve_frontier_auth(&self.args)?
        };
        let runtime_context = self
            .runtime_context
            .clone()
            .unwrap_or_else(|| FrontierRuntimeContext {
                app_bundle_id: None,
                text_context_text: None,
                text_context_cursor_position: None,
                capture_ms: 0,
                source: "none".to_string(),
            });
        let request_profile =
            build_effective_request_profile(&self.context_config, Some(&runtime_context));
        let frontier_session_id = Uuid::new_v4().to_string().to_uppercase();
        let request = build_frontier_request(
            self.args.server_url.as_str(),
            &self.args,
            &auth,
        )?;
        let connect_started_at_ms = now_millis();
        let (socket, _) = connect_async(request)
            .await
            .map_err(|error| format!("frontier connect error: {error}"))?;
        let (mut writer, mut reader) = socket.split();

        let shared = Arc::new(Mutex::new(FrontierSharedState::default()));
        let ready_notify = Arc::new(Notify::new());
        let final_notify = Arc::new(Notify::new());
        let terminal_notify = Arc::new(Notify::new());
        let read_shared = shared.clone();
        let read_ready_notify = ready_notify.clone();
        let read_final_notify = final_notify.clone();
        let read_terminal_notify = terminal_notify.clone();
        let read_event_tx = self.event_tx.clone();

        let read_task = tokio::spawn(async move {
            while let Some(message) = reader.next().await {
                match message {
                    Ok(Message::Binary(bytes)) => {
                        if let Some(parsed) = parse_frontier_response(&bytes) {
                            handle_frontier_response(
                                parsed,
                                read_shared.clone(),
                                read_ready_notify.clone(),
                                read_final_notify.clone(),
                                read_terminal_notify.clone(),
                                read_event_tx.clone(),
                            )
                            .await;
                        }
                    }
                    Ok(Message::Close(frame)) => {
                        let mut shared = read_shared.lock().await;
                        if shared.error.is_none() && !shared.terminal {
                            shared.error = Some(format!(
                                "frontier closed code={} reason={}",
                                frame.as_ref().map(|value| value.code.to_string()).unwrap_or_else(|| "-".to_string()),
                                frame
                                    .as_ref()
                                    .map(|value| value.reason.to_string())
                                    .filter(|value| !value.is_empty())
                                    .unwrap_or_else(|| "-".to_string())
                            ));
                        }
                        shared.terminal = true;
                        read_terminal_notify.notify_waiters();
                        read_final_notify.notify_waiters();
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let mut shared = read_shared.lock().await;
                        if shared.error.is_none() {
                            shared.error = Some(format!("frontier recv failed: {error}"));
                        }
                        shared.terminal = true;
                        read_terminal_notify.notify_waiters();
                        read_final_notify.notify_waiters();
                        break;
                    }
                }
            }
        });

        let start_task = build_start_task(
            &frontier_session_id,
            auth.request_token_field(),
            &auth.app_key,
        );
        writer
            .send(Message::Binary(start_task.into()))
            .await
            .map_err(|error| format!("frontier start_task send failed: {error}"))?;

        let (start_session, _payload, _decoded_context) = build_start_session(
            &frontier_session_id,
            auth.request_token_field(),
            &auth.app_key,
            "pcm",
            Some(&request_profile),
            None,
            now_millis() / 1000,
        )?;
        writer
            .send(Message::Binary(start_session.into()))
            .await
            .map_err(|error| format!("frontier start_session send failed: {error}"))?;

        wait_for_ready(shared.clone(), ready_notify.clone()).await?;
        self.session = Some(FrontierSession {
            writer,
            read_task,
            shared,
            final_notify,
            auth,
            frontier_session_id,
            runtime_context,
            context_config: self.context_config.clone(),
            connect_started_at_ms,
            finish_sent_at_ms: None,
            audio_bytes_sent: 0,
            pending_pcm: Vec::new(),
        });

        Ok(self.session.as_mut().expect("session inserted"))
    }

    async fn send_audio(&mut self, samples: Vec<i16>) -> Result<(), String> {
        let session = self.ensure_session().await?;
        if session.finish_sent_at_ms.is_some() {
            return Err("frontier session already finished".to_string());
        }

        for sample in samples {
            session.pending_pcm.extend_from_slice(&sample.to_le_bytes());
        }

        let flush_eagerly = session.audio_bytes_sent == 0;
        while session.pending_pcm.len() >= PCM_CHUNK_BYTES {
            let chunk = session.pending_pcm.drain(..PCM_CHUNK_BYTES).collect::<Vec<_>>();
            send_audio_chunk(session, &chunk).await?;
        }
        if flush_eagerly && !session.pending_pcm.is_empty() {
            let chunk = session.pending_pcm.drain(..).collect::<Vec<_>>();
            send_audio_chunk(session, &chunk).await?;
        }
        Ok(())
    }

    async fn snapshot_partial(&mut self) -> Option<(String, Value)> {
        let session = self.session.as_mut()?;
        let shared = session.shared.lock().await;
        let text = if !shared.latest_partial_text.trim().is_empty() {
            shared.latest_partial_text.trim().to_string()
        } else if !shared.latest_final_text.trim().is_empty() {
            shared.latest_final_text.trim().to_string()
        } else {
            return None;
        };
        drop(shared);
        Some((text, snapshot_timings(session, 0, 0, 0).await))
    }

    async fn finish_session(&mut self) -> Result<Option<(String, Value)>, String> {
        let Some(mut session) = self.session.take() else {
            return Ok(None);
        };

        if !session.pending_pcm.is_empty() {
            let chunk = session.pending_pcm.drain(..).collect::<Vec<_>>();
            send_audio_chunk(&mut session, &chunk).await?;
        }

        let finish_payload =
            build_finish_session(&session.frontier_session_id, &session.auth.app_key);
        session
            .writer
            .send(Message::Binary(finish_payload.into()))
            .await
            .map_err(|error| format!("frontier finish_session send failed: {error}"))?;
        session.finish_sent_at_ms = Some(now_millis());

        let shared = session.shared.clone();
        let final_notify = session.final_notify.clone();
        timeout(FINAL_TIMEOUT, wait_for_final(shared.clone(), final_notify)).await
            .map_err(|_| "frontier final timeout".to_string())??;

        let shared_state = shared.lock().await;
        if let Some(error) = &shared_state.error {
            let error = error.clone();
            drop(shared_state);
            close_frontier_session(session).await;
            return Err(error);
        }

        let finish_done_at_ms = shared_state.finish_done_at_ms.unwrap_or_else(now_millis);
        let infer_ms = session
            .finish_sent_at_ms
            .map(|started| finish_done_at_ms.saturating_sub(started))
            .unwrap_or(0);
        let total_ms = session
            .finish_sent_at_ms
            .map(|started| now_millis().saturating_sub(started))
            .unwrap_or(0);
        let text = if !shared_state.latest_final_text.trim().is_empty() {
            shared_state.latest_final_text.trim().to_string()
        } else {
            shared_state.latest_partial_text.trim().to_string()
        };
        drop(shared_state);
        let timings = snapshot_timings(&session, infer_ms, total_ms, 0).await;
        close_frontier_session(session).await;
        Ok(Some((text, timings)))
    }

    async fn close_session(&mut self) {
        if let Some(session) = self.session.take() {
            close_frontier_session(session).await;
        }
    }
}

fn build_frontier_request(
    server_url: &str,
    _args: &Args,
    auth: &FrontierAuthMaterial,
) -> Result<Request<()>, String> {
    let request_url = if server_url.starts_with("ws://127.0.0.1") {
        auth.ws_url
            .as_deref()
            .unwrap_or(DEFAULT_FRONTIER_WS_URL)
    } else {
        server_url
    };
    let uses_android_virtual_device = auth.source.starts_with("android_virtual_device:");
    let device_key = std::env::var("FRONTIER_DEVICE_KEY")
        .unwrap_or_else(|_| DEFAULT_DEVICE_KEY.to_string());
    let mut request = request_url
        .into_client_request()
        .map_err(|error| format!("frontier request build failed: {error}"))?;
    let headers = request.headers_mut();
    headers.insert(
        HeaderName::from_static("proto-version"),
        HeaderValue::from_static("v2"),
    );
    headers.insert(
        HeaderName::from_static("x-custom-keepalive"),
        HeaderValue::from_static("true"),
    );
    headers.insert(
        HeaderName::from_static("x-keepalive-interval"),
        HeaderValue::from_static("3"),
    );
    headers.insert(
        HeaderName::from_static("x-keepalive-timeout"),
        HeaderValue::from_static("3600"),
    );
    if !uses_android_virtual_device {
        headers.insert(
            HeaderName::from_static("x-tt-e-k"),
            HeaderValue::from_str(&device_key).map_err(|error| error.to_string())?,
        );
    }
    Ok(request)
}

async fn send_audio_chunk(session: &mut FrontierSession, chunk: &[u8]) -> Result<(), String> {
    let payload = build_audio_frame(
        &session.frontier_session_id,
        chunk,
        now_millis(),
        0,
    );
    session
        .writer
        .send(Message::Binary(payload.into()))
        .await
        .map_err(|error| format!("frontier audio send failed: {error}"))?;
    session.audio_bytes_sent += chunk.len();
    Ok(())
}

async fn close_frontier_session(mut session: FrontierSession) {
    let _ = session.writer.close().await;
    session.read_task.abort();
    let _ = session.read_task.await;
}

async fn wait_for_ready(
    shared: Arc<Mutex<FrontierSharedState>>,
    ready_notify: Arc<Notify>,
) -> Result<(), String> {
    loop {
        {
            let shared = shared.lock().await;
            if shared.ready_at_ms.is_some() {
                return Ok(());
            }
            if let Some(error) = &shared.error {
                return Err(error.clone());
            }
        }
        timeout(CONNECT_READY_TIMEOUT, ready_notify.notified())
            .await
            .map_err(|_| "frontier ready timeout".to_string())?;
    }
}

async fn wait_for_final(
    shared: Arc<Mutex<FrontierSharedState>>,
    final_notify: Arc<Notify>,
) -> Result<(), String> {
    loop {
        {
            let shared = shared.lock().await;
            if !shared.latest_final_text.is_empty() || shared.terminal {
                return Ok(());
            }
            if let Some(error) = &shared.error {
                return Err(error.clone());
            }
        }
        final_notify.notified().await;
    }
}

async fn handle_frontier_response(
    parsed: ParsedFrontierResponse,
    shared: Arc<Mutex<FrontierSharedState>>,
    ready_notify: Arc<Notify>,
    final_notify: Arc<Notify>,
    terminal_notify: Arc<Notify>,
    event_tx: Sender<LegacyTransportEvent>,
) {
    if parsed.status_code != 20_000_000 {
        let error = format!(
            "frontier {} failed: {} {}",
            if parsed.event.is_empty() {
                "response"
            } else {
                parsed.event.as_str()
            },
            parsed.status_code,
            parsed.status_text
        );
        let mut shared = shared.lock().await;
        shared.error = Some(error.clone());
        shared.terminal = true;
        drop(shared);
        let _ = event_tx.send(LegacyTransportEvent::Error {
            message: error,
            recoverable: true,
        });
        final_notify.notify_waiters();
        terminal_notify.notify_waiters();
        return;
    }

    if parsed.event == "SessionStarted" {
        let mut shared = shared.lock().await;
        shared.ready_at_ms = Some(now_millis());
        drop(shared);
        ready_notify.notify_waiters();
    } else if matches!(parsed.event.as_str(), "SessionFinished" | "TaskFinished") {
        let mut shared = shared.lock().await;
        shared.terminal = true;
        shared.finish_done_at_ms = Some(now_millis());
        drop(shared);
        final_notify.notify_waiters();
        terminal_notify.notify_waiters();
    }

    let Some(payload) = parsed.payload_json else {
        return;
    };
    let (partial_text, final_text, saw_terminal) = extract_latest_result(&payload);
    let mut shared = shared.lock().await;
    if !partial_text.is_empty() {
        shared.latest_partial_text = partial_text.clone();
    }
    if !final_text.is_empty() {
        shared.latest_final_text = final_text.clone();
        shared.latest_partial_text = shared.latest_final_text.clone();
        shared.finish_done_at_ms = Some(now_millis());
    }
    if saw_terminal {
        shared.terminal = true;
        shared.finish_done_at_ms = Some(now_millis());
    }
    drop(shared);
    if !partial_text.is_empty() {
        let _ = event_tx.send(LegacyTransportEvent::Partial {
            text: partial_text,
            utterance_id: 0,
            timings: None,
        });
    }
    if !final_text.is_empty() || saw_terminal {
        final_notify.notify_waiters();
    }
    if saw_terminal {
        terminal_notify.notify_waiters();
    }
}

#[derive(Debug)]
struct ParsedFrontierResponse {
    event: String,
    status_code: u64,
    status_text: String,
    payload_json: Option<Value>,
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

    Some(ParsedFrontierResponse {
        event,
        status_code,
        status_text,
        payload_json,
    })
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

#[derive(Clone, Debug)]
struct ResultCandidate {
    order: usize,
    index: Option<i64>,
    text: String,
    has_seq_id: bool,
    stream_asr_finish: bool,
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
        let text = item
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let is_interim = item.get("is_interim").and_then(Value::as_bool);
        let is_vad_finished = item
            .get("is_vad_finished")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let extra = item.get("extra").and_then(Value::as_object);
        let nonstream_result = extra
            .and_then(|extra| extra.get("nonstream_result"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let is_terminal = nonstream_result || (is_interim == Some(false) && is_vad_finished);
        if is_terminal {
            saw_terminal = true;
        }
        if text.is_empty() {
            continue;
        }
        let candidate = ResultCandidate {
            order,
            index: item
                .get("index")
                .and_then(Value::as_i64)
                .or_else(|| item.get("index").and_then(Value::as_u64).map(|value| value as i64)),
            text,
            has_seq_id: extra
                .and_then(|extra| extra.get("seq_id"))
                .is_some(),
            stream_asr_finish: item
                .get("stream_asr_finish")
                .and_then(Value::as_bool)
                .unwrap_or(false),
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
            let key = candidate
                .index
                .unwrap_or(1_000_000 + candidate.order as i64);
            grouped.entry(key).or_default().push(candidate);
        }
        return grouped
            .values()
            .filter_map(|group| best_partial_candidate(group).map(|candidate| candidate.text.as_str()))
            .fold(String::new(), |joined, text| join_transcript_text(&joined, text));
    }
    let refs = candidates.iter().collect::<Vec<_>>();
    best_partial_candidate(&refs)
        .map(|candidate| candidate.text.clone())
        .unwrap_or_default()
}

fn compose_final_text(candidates: &[ResultCandidate]) -> String {
    if candidates.is_empty() {
        return String::new();
    }
    if candidates.iter().any(|candidate| candidate.index.is_some()) {
        let mut grouped: BTreeMap<i64, Vec<&ResultCandidate>> = BTreeMap::new();
        for candidate in candidates {
            let key = candidate
                .index
                .unwrap_or(1_000_000 + candidate.order as i64);
            grouped.entry(key).or_default().push(candidate);
        }
        return grouped
            .values()
            .filter_map(|group| best_display_candidate(group).map(|candidate| candidate.text.as_str()))
            .fold(String::new(), |joined, text| join_transcript_text(&joined, text));
    }
    let refs = candidates.iter().collect::<Vec<_>>();
    best_display_candidate(&refs)
        .map(|candidate| candidate.text.clone())
        .unwrap_or_default()
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
    let best_display = candidates
        .iter()
        .copied()
        .filter(|candidate| !candidate.has_seq_id)
        .max_by_key(|candidate| {
            (
                candidate.stream_asr_finish,
                candidate.text.chars().count(),
                Reverse(candidate.order),
            )
        });
    let best_streaming = candidates
        .iter()
        .copied()
        .filter(|candidate| candidate.has_seq_id)
        .max_by_key(|candidate| {
            (
                candidate.stream_asr_finish,
                candidate.text.chars().count(),
                Reverse(candidate.order),
            )
        });

    match (best_display, best_streaming) {
        (Some(display), Some(streaming)) => {
            let display_len = display.text.chars().count();
            let streaming_len = streaming.text.chars().count();
            if streaming_len >= display_len {
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
    if left
        .chars()
        .last()
        .map(is_ascii_alnum)
        .unwrap_or(false)
        && right
            .chars()
            .next()
            .map(is_ascii_alnum)
            .unwrap_or(false)
    {
        format!("{left} {right}")
    } else {
        format!("{left}{right}")
    }
}

fn is_ascii_alnum(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
}

async fn snapshot_timings(
    session: &FrontierSession,
    infer_ms: u64,
    total_ms: u64,
    extra_audio_bytes: usize,
) -> Value {
    let ready_at_ms = session.shared.lock().await.ready_at_ms.unwrap_or(0);
    serde_json::json!({
        "audio_ms": audio_ms_from_bytes(session.audio_bytes_sent + extra_audio_bytes),
        "warmup_ms": ready_at_ms.saturating_sub(session.connect_started_at_ms),
        "infer_ms": infer_ms,
        "context_capture_ms": session.runtime_context.capture_ms,
        "context_available": effective_context_available(&session.runtime_context, &session.context_config),
        "context_source": effective_context_source(&session.runtime_context, &session.context_config),
        "total_ms": total_ms,
        "provider": "doubao-frontier-direct",
    })
}

fn build_context_summary(runtime_context: &FrontierRuntimeContext, config: &ContextConfig) -> Value {
    serde_json::json!({
        "frontmost_app_bundle_id": runtime_context.app_bundle_id,
        "text_context_chars": runtime_context
            .text_context_text
            .as_ref()
            .map(|text| text.chars().count())
            .unwrap_or(0),
        "cursor_position": runtime_context.text_context_cursor_position,
        "config_text_mode": config.text_context.mode,
    })
}

fn effective_context_available(
    runtime_context: &FrontierRuntimeContext,
    config: &ContextConfig,
) -> bool {
    runtime_context.app_bundle_id.is_some()
        || runtime_context.text_available()
        || !config.hotwords.is_empty()
        || !config.user_terms.is_empty()
        || (config.text_context.mode == "static" && !config.text_context.text.is_empty())
}

fn effective_context_source(
    runtime_context: &FrontierRuntimeContext,
    config: &ContextConfig,
) -> String {
    let mut sources = Vec::new();
    if runtime_context.app_bundle_id.is_some() || runtime_context.text_available() {
        sources.push(runtime_context.source.clone());
    }
    if !config.hotwords.is_empty()
        || !config.user_terms.is_empty()
        || (config.text_context.mode == "static" && !config.text_context.text.is_empty())
    {
        sources.push("config".to_string());
    }
    if sources.is_empty() {
        "none".to_string()
    } else {
        sources.join("+")
    }
}

fn audio_ms_from_bytes(audio_bytes: usize) -> u64 {
    ((audio_bytes as f64 / SAMPLE_WIDTH as f64 / SAMPLE_RATE as f64) * 1000.0) as u64
}

#[cfg(test)]
mod tests {
    use super::{extract_latest_result, parse_frontier_response};
    use crate::frontier_auth::{decode_jwt_exp, usable_token};

    #[test]
    fn parses_frontier_binary_response() {
        let bytes = [
            0x22, 0x0e, b'S', b'e', b's', b's', b'i', b'o', b'n', b'S', b't', b'a', b'r', b't', b'e', b'd',
            0x28, 0x80, 0xda, 0xc4, 0x09,
            0x32, 0x02, b'O', b'K',
        ];
        let parsed = parse_frontier_response(&bytes).expect("parsed");
        assert_eq!(parsed.event, "SessionStarted");
        assert_eq!(parsed.status_code, 20_000_000);
        assert_eq!(parsed.status_text, "OK");
    }

    #[test]
    fn extracts_partial_and_final() {
        let payload = serde_json::json!({
            "results": [
                {"text":"你好", "is_interim": true},
                {"text":"你好世界", "is_interim": false, "is_vad_finished": true}
            ]
        });
        let (partial, final_text, saw_terminal) = extract_latest_result(&payload);
        assert_eq!(partial, "你好");
        assert_eq!(final_text, "你好世界");
        assert!(saw_terminal);
    }

    #[test]
    fn prefers_display_variant_without_seq_id() {
        let payload = serde_json::json!({
            "results": [
                {"text":"你好，世界测试123。", "is_interim": true},
                {"text":"你好世界测试一二三", "is_interim": true, "extra": {"seq_id": 113}}
            ]
        });
        let (partial, final_text, saw_terminal) = extract_latest_result(&payload);
        assert_eq!(partial, "你好，世界测试123。");
        assert!(final_text.is_empty());
        assert!(!saw_terminal);
    }

    #[test]
    fn prefers_faster_streaming_partial_when_display_variant_lags() {
        let payload = serde_json::json!({
            "results": [
                {"text":"测试", "is_interim": true},
                {"text":"测试一下云输入法", "is_interim": true, "extra": {"seq_id": 113}}
            ]
        });
        let (partial, final_text, saw_terminal) = extract_latest_result(&payload);
        assert_eq!(partial, "测试一下云输入法");
        assert!(final_text.is_empty());
        assert!(!saw_terminal);
    }

    #[test]
    fn treats_nonstream_result_as_final_and_keeps_display_text() {
        let payload = serde_json::json!({
            "results": [
                {"text":"现在是独立demo验证。", "is_interim": false, "is_vad_finished": true},
                {"text":"现在是独立 demo 验证", "is_interim": false, "extra": {"nonstream_result": true}}
            ]
        });
        let (partial, final_text, saw_terminal) = extract_latest_result(&payload);
        assert!(partial.is_empty());
        assert_eq!(final_text, "现在是独立demo验证。");
        assert!(saw_terminal);
    }

    #[test]
    fn validates_token_shape_and_jwt_exp() {
        assert!(!usable_token(""));
        assert!(usable_token("a.b.c"));
        assert_eq!(
            decode_jwt_exp("x.eyJleHAiOjEyM30.y"),
            123
        );
    }
}
