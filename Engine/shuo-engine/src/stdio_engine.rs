use std::io::{self, BufRead, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use objc2::rc::Retained;
use objc2_avf_audio::{AVAudioEngine, AVAudioPCMBuffer, AVAudioTime};

use crate::audio::{
    audio_common_format_name, audio_levels, float_to_i16, pcm_buffer_to_mono_f32, resample_linear,
};
use crate::config::{load_context_config, load_context_config_from_path, ContextConfig};
use crate::engine_ipc::{
    AudioLevelPayload, AuthStatePayload, ContextSnapshotPayload, EngineEvent, ErrorPayload,
    HostCommand, MetricsPayload, ReadyPayload, RecordingPayload, TranscriptPayload,
};
use crate::engine_state::{AuthState, ContextSnapshot, EngineStateMachine, SessionState};
use crate::frontier_auth::{preview_frontier_auth, preview_frontier_auth_refresh};
use crate::frontier_transport::spawn_frontier_transport;
use crate::frontier_protocol::{
    build_audio_frame, build_effective_request_profile, build_finish_session, build_start_session,
    build_start_task, FrontierRuntimeContext, DEFAULT_APP_KEY,
};
use crate::legacy_transport::{
    spawn_legacy_transport, LegacyTransportCommand, LegacyTransportEvent,
};
use crate::state::now_millis;
use crate::{Args, HELPER_VERSION, TransportKind};

const TARGET_SAMPLE_RATE: f64 = 16_000.0;
const MIN_UTTERANCE_MS: u64 = 180;
const SPEECH_PEAK_THRESHOLD: f32 = 0.015;
const SPEECH_RMS_THRESHOLD: f32 = 0.004;
const AUDIO_LEVEL_PUSH_INTERVAL_MS: u64 = 80;

enum EngineInput {
    Command(Result<HostCommand, String>),
    Transport(LegacyTransportEvent),
    AudioLevel(AudioLevelPayload),
}

struct CaptureState {
    voice_started: bool,
    sent_samples: usize,
    audio_callbacks: u64,
    unsupported_buffers: u64,
    last_level_emit_ms: u64,
}

#[derive(Debug, Clone, Copy)]
struct CaptureSummary {
    voice_started: bool,
    sent_samples: usize,
    audio_callbacks: u64,
    unsupported_buffers: u64,
}

impl CaptureState {
    fn new() -> Self {
        Self {
            voice_started: false,
            sent_samples: 0,
            audio_callbacks: 0,
            unsupported_buffers: 0,
            last_level_emit_ms: 0,
        }
    }

    fn snapshot(&self) -> CaptureSummary {
        CaptureSummary {
            voice_started: self.voice_started,
            sent_samples: self.sent_samples,
            audio_callbacks: self.audio_callbacks,
            unsupported_buffers: self.unsupported_buffers,
        }
    }
}

pub(crate) fn run_stdio_engine(args: Args) {
    let (input_tx, input_rx) = mpsc::channel();
    let (transport_event_tx, transport_event_rx) = mpsc::channel();
    let transport_tx = match args.transport {
        TransportKind::LegacyLocalWs => {
            spawn_legacy_transport(args.server_url.clone(), transport_event_tx)
        }
        TransportKind::DirectFrontier => {
            spawn_frontier_transport(args.clone(), transport_event_tx)
        }
    };
    spawn_stdin_reader(input_tx.clone());
    let forward_tx = input_tx.clone();
    std::thread::spawn(move || {
        while let Ok(event) = transport_event_rx.recv() {
            let _ = forward_tx.send(EngineInput::Transport(event));
        }
    });

    let mut engine = StdioEngine::new(args, input_tx, input_rx, transport_tx);
    if let Err(error) = engine.run() {
        let _ = engine.emit(EngineEvent::Fatal {
            code: "stdio_engine_failed".to_string(),
            message: error,
        });
    }
}

struct StdioEngine {
    args: Args,
    stdin_tx: Sender<EngineInput>,
    input_rx: Receiver<EngineInput>,
    transport_tx: tokio::sync::mpsc::UnboundedSender<LegacyTransportCommand>,
    state: EngineStateMachine,
    context_config: ContextConfig,
    transport_name: &'static str,
    latest_context: Option<ContextSnapshot>,
    audio_engine: Retained<AVAudioEngine>,
    capture_state: Option<Arc<Mutex<CaptureState>>>,
    last_partial_request_ms: u64,
}

impl StdioEngine {
    fn new(
        args: Args,
        stdin_tx: Sender<EngineInput>,
        input_rx: Receiver<EngineInput>,
        transport_tx: tokio::sync::mpsc::UnboundedSender<LegacyTransportCommand>,
    ) -> Self {
        Self {
            transport_name: args.transport.as_str(),
            args,
            stdin_tx,
            input_rx,
            transport_tx,
            state: EngineStateMachine::new(),
            context_config: load_context_config(),
            latest_context: None,
            audio_engine: unsafe { AVAudioEngine::new() },
            capture_state: None,
            last_partial_request_ms: 0,
        }
    }

    fn run(&mut self) -> Result<(), String> {
        let _ = self.transport_tx.send(LegacyTransportCommand::ConfigSnapshot {
            config: self.context_config.clone(),
        });
        self.emit_ready()?;
        loop {
            match self.input_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(input) => {
                    if !self.handle_input(input)? {
                        break;
                    }
                    self.maybe_request_partial();
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.maybe_request_partial();
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        let _ = self.transport_tx.send(LegacyTransportCommand::Close);
        Ok(())
    }

    fn handle_input(&mut self, input: EngineInput) -> Result<bool, String> {
        match input {
            EngineInput::Command(command) => self.handle_command(command),
            EngineInput::Transport(event) => {
                self.handle_transport_event(event)?;
                Ok(true)
            }
            EngineInput::AudioLevel(level) => {
                self.emit(EngineEvent::AudioLevel(level))?;
                Ok(true)
            }
        }
    }

    fn handle_command(&mut self, command: Result<HostCommand, String>) -> Result<bool, String> {
        let command = match command {
            Ok(command) => command,
            Err(error) => {
                self.emit(EngineEvent::Error(ErrorPayload {
                    code: "invalid_command".to_string(),
                    message: error,
                    recoverable: true,
                    session_id: self
                        .state
                        .active_session()
                        .map(|session| session.session_id.clone()),
                }))?;
                return Ok(true);
            }
        };

        match command {
            HostCommand::Hello { protocol_version } => {
                if let Some(protocol_version) = protocol_version {
                    if protocol_version != 1 {
                        self.emit(EngineEvent::Error(ErrorPayload {
                            code: "protocol_version_mismatch".to_string(),
                            message: format!(
                                "host requested protocol_version={}, engine supports protocol_version=1",
                                protocol_version
                            ),
                            recoverable: true,
                            session_id: None,
                        }))?;
                    }
                }
                self.emit_ready()?;
            }
            HostCommand::LoadConfig { config_path } => {
                self.reload_context_config(config_path.as_deref())?;
                let _ = self.transport_tx.send(LegacyTransportCommand::ConfigSnapshot {
                    config: self.context_config.clone(),
                });
                self.emit(EngineEvent::Metrics(MetricsPayload {
                    session_id: None,
                    name: "config_loaded".to_string(),
                    value: serde_json::json!({
                        "config_path": config_path,
                        "hotwords": self.context_config.hotwords,
                        "user_terms": self.context_config.user_terms,
                        "text_context_mode": self.context_config.text_context.mode,
                    }),
                }))?;
            }
            HostCommand::ReloadConfig { config_path } => {
                self.reload_context_config(config_path.as_deref())?;
                let _ = self.transport_tx.send(LegacyTransportCommand::ConfigSnapshot {
                    config: self.context_config.clone(),
                });
                self.emit(EngineEvent::Metrics(MetricsPayload {
                    session_id: self
                        .state
                        .active_session()
                        .map(|session| session.session_id.clone()),
                    name: "config_reloaded".to_string(),
                    value: serde_json::json!({
                        "config_path": config_path,
                        "hotwords": self.context_config.hotwords,
                        "user_terms": self.context_config.user_terms,
                        "text_context_mode": self.context_config.text_context.mode,
                    }),
                }))?;
            }
            HostCommand::Warmup { force } => {
                if let Err(code) = self.state.begin_warmup() {
                    self.emit(EngineEvent::Error(ErrorPayload {
                        code: code.to_string(),
                        message: "warmup requested in invalid state".to_string(),
                        recoverable: true,
                        session_id: self
                            .state
                            .active_session()
                            .map(|session| session.session_id.clone()),
                    }))?;
                } else {
                    if let Some(context) = self.latest_context.clone() {
                        let _ = self
                            .transport_tx
                            .send(LegacyTransportCommand::UpdateContext { context });
                    }
                    let _ = self.transport_tx.send(LegacyTransportCommand::Warmup {
                        force: force.unwrap_or(false),
                    });
                }
            }
            HostCommand::StartRecording {
                session_id,
                trigger,
                context_snapshot,
            } => {
                self.start_recording(session_id, trigger, context_snapshot)?;
            }
            HostCommand::StopRecording => {
                self.stop_recording(true)?;
            }
            HostCommand::CancelRecording => {
                self.stop_recording(false)?;
            }
            HostCommand::UpdateContext { context_snapshot } => {
                let context = to_context_snapshot(context_snapshot);
                self.latest_context = Some(context.clone());
                self.state.replace_context(context.clone());
                let _ = self
                    .transport_tx
                    .send(LegacyTransportCommand::UpdateContext { context });
            }
            HostCommand::RefreshAuth => {
                self.state.set_auth_state(AuthState::Refreshing);
                self.emit(EngineEvent::AuthState(AuthStatePayload {
                    state: self.state.auth_state().as_str(),
                    source: self.transport_name.to_string(),
                    expires_at_ms: None,
                }))?;
                let _ = self.transport_tx.send(LegacyTransportCommand::RefreshAuth);
            }
            HostCommand::ExportDiagnostics => {
                let runtime_context = self
                    .latest_context
                    .as_ref()
                    .map(FrontierRuntimeContext::from_context_snapshot);
                let effective_request_profile = build_effective_request_profile(
                    &self.context_config,
                    runtime_context.as_ref(),
                );
                let preview_session_id = "preview-session";
                let preview_now_ms = now_millis();
                let start_task_preview = hex_string(&build_start_task(
                    preview_session_id,
                    None,
                    DEFAULT_APP_KEY,
                ));
                let start_session_preview = build_start_session(
                    preview_session_id,
                    None,
                    DEFAULT_APP_KEY,
                    "pcm",
                    Some(&effective_request_profile),
                    None,
                    preview_now_ms / 1000,
                )
                .ok()
                .map(|(message, payload, decoded_context)| {
                    serde_json::json!({
                        "message_hex": hex_string(&message),
                        "payload": payload,
                        "decoded_context": decoded_context,
                        "ws_url": crate::frontier_protocol::DEFAULT_FRONTIER_WS_URL,
                    })
                });
                let finish_session_preview = hex_string(&build_finish_session(
                    preview_session_id,
                    DEFAULT_APP_KEY,
                ));
                let task_request_preview = hex_string(&build_audio_frame(
                    preview_session_id,
                    &[0, 1, 2, 3],
                    preview_now_ms,
                    0,
                ));
                self.emit(EngineEvent::Metrics(MetricsPayload {
                    session_id: self
                        .state
                        .active_session()
                        .map(|session| session.session_id.clone()),
                    name: "engine_snapshot".to_string(),
                    value: serde_json::json!({
                        "session_state": self.state.session_state().as_str(),
                        "auth_state": self.state.auth_state().as_str(),
                        "auth_preview": preview_frontier_auth(&self.args),
                        "auth_refresh_preview": preview_frontier_auth_refresh(&self.args),
                        "next_utterance_id": self.state.next_utterance_id(),
                        "server_url": self.args.server_url,
                        "effective_request_profile": effective_request_profile,
                        "start_task_preview": start_task_preview,
                        "start_session_preview": start_session_preview,
                        "finish_session_preview": finish_session_preview,
                        "task_request_preview": task_request_preview,
                    }),
                }))?;
            }
            HostCommand::Shutdown => {
                if self.state.session_state() == SessionState::Recording {
                    self.stop_recording(false)?;
                }
                return Ok(false);
            }
        }

        Ok(true)
    }

    fn handle_transport_event(&mut self, event: LegacyTransportEvent) -> Result<(), String> {
        match event {
            LegacyTransportEvent::Ready {
                app_key,
                token_source,
                expires_at_ms,
                context_source,
            } => {
                self.state.set_auth_state(AuthState::Ready);
                self.state.recover();
                self.emit(EngineEvent::AuthState(AuthStatePayload {
                    state: self.state.auth_state().as_str(),
                    source: token_source
                        .or(context_source)
                        .unwrap_or_else(|| "legacy_local_ws".to_string()),
                    expires_at_ms,
                }))?;
                self.emit(EngineEvent::Metrics(MetricsPayload {
                    session_id: None,
                    name: "transport_ready".to_string(),
                    value: serde_json::json!({
                        "app_key": app_key,
                    }),
                }))?;
            }
            LegacyTransportEvent::Status {
                name,
                context,
                timings,
            } => {
                if name == "warmed" {
                    self.state.finish_warmup();
                }
                self.emit(EngineEvent::Metrics(MetricsPayload {
                    session_id: self
                        .state
                        .active_session()
                        .map(|session| session.session_id.clone()),
                    name,
                    value: serde_json::json!({
                        "context": context,
                        "timings": timings,
                    }),
                }))?;
            }
            LegacyTransportEvent::Partial {
                text,
                utterance_id,
                timings,
            } => {
                let Some((session_id, active_utterance_id)) = self
                    .state
                    .active_session()
                    .map(|session| (session.session_id.clone(), session.utterance_id))
                else {
                    return Ok(());
                };
                let resolved_utterance_id = if utterance_id == 0 {
                    active_utterance_id
                } else {
                    utterance_id
                };
                if active_utterance_id != resolved_utterance_id {
                    return Ok(());
                }
                self.state.mark_partial_received();
                if text.trim().is_empty() {
                    return Ok(());
                }
                self.emit(EngineEvent::Partial(TranscriptPayload {
                    session_id: session_id.clone(),
                    utterance_id: resolved_utterance_id,
                    text,
                    is_stale: false,
                }))?;
                if let Some(timings) = timings {
                    self.emit(EngineEvent::Metrics(MetricsPayload {
                        session_id: Some(session_id),
                        name: "partial_timings".to_string(),
                        value: timings,
                    }))?;
                }
            }
            LegacyTransportEvent::Final {
                text,
                utterance_id,
                timings,
            } => {
                let active_session_id = self
                    .state
                    .active_session()
                    .map(|session| session.session_id.clone());
                let completed = match self.state.complete_final(utterance_id) {
                    Ok(completed) => completed,
                    Err("final_invalid_state") | Err("final_stale_utterance") => return Ok(()),
                    Err(code) => {
                        self.emit(EngineEvent::Error(ErrorPayload {
                            code: code.to_string(),
                            message: "final received in invalid state".to_string(),
                            recoverable: true,
                            session_id: active_session_id,
                        }))?;
                        return Ok(());
                    }
                };
                self.emit(EngineEvent::Final(TranscriptPayload {
                    session_id: completed.session_id.clone(),
                    utterance_id,
                    text,
                    is_stale: false,
                }))?;
                self.emit(EngineEvent::Metrics(MetricsPayload {
                    session_id: Some(completed.session_id),
                    name: "final_timings".to_string(),
                    value: serde_json::json!({
                        "trigger": completed.trigger,
                        "recording_started_at_ms": completed.recording_started_at_ms,
                        "flush_sent_at_ms": completed.flush_sent_at_ms,
                        "transport": timings,
                    }),
                }))?;
            }
            LegacyTransportEvent::Error {
                message,
                recoverable,
            } => {
                self.state.clear_partial_inflight();
                if !recoverable {
                    self.state.set_auth_state(AuthState::Failed);
                    self.state.fail("transport_error");
                } else {
                    self.state.set_auth_state(AuthState::Degraded);
                }
                self.emit(EngineEvent::Error(ErrorPayload {
                    code: "transport_error".to_string(),
                    message,
                    recoverable,
                    session_id: self
                        .state
                        .active_session()
                        .map(|session| session.session_id.clone()),
                }))?;
            }
            LegacyTransportEvent::Disconnected { reason } => {
                self.state.clear_partial_inflight();
                self.state.set_auth_state(AuthState::Degraded);
                self.emit(EngineEvent::Error(ErrorPayload {
                    code: "transport_disconnected".to_string(),
                    message: reason,
                    recoverable: true,
                    session_id: self
                        .state
                        .active_session()
                        .map(|session| session.session_id.clone()),
                }))?;
            }
        }
        Ok(())
    }

    fn start_recording(
        &mut self,
        session_id: String,
        trigger: String,
        context_snapshot: ContextSnapshotPayload,
    ) -> Result<(), String> {
        let context = to_context_snapshot(context_snapshot);
        self.latest_context = Some(context.clone());
        let active = self
            .state
            .begin_recording(session_id, trigger, context.clone(), now_millis())
            .map_err(|code| code.to_string())?;

        let _ = self
            .transport_tx
            .send(LegacyTransportCommand::UpdateContext { context });
        if self.transport_name == "legacy_local_ws" {
            let _ = self.transport_tx.send(LegacyTransportCommand::Reset);
            let _ = self
                .transport_tx
                .send(LegacyTransportCommand::Warmup { force: false });
        }

        self.install_audio_tap(active.session_id.clone())?;
        self.last_partial_request_ms = 0;
        self.emit(EngineEvent::RecordingStarted(RecordingPayload {
            session_id: active.session_id.clone(),
            utterance_id: active.utterance_id,
            trigger: active.trigger.clone(),
        }))?;
        Ok(())
    }

    fn stop_recording(&mut self, flush_result: bool) -> Result<(), String> {
        if self.state.session_state() != SessionState::Recording {
            self.emit(EngineEvent::Error(ErrorPayload {
                code: "stop_invalid_state".to_string(),
                message: "stop requested while not recording".to_string(),
                recoverable: true,
                session_id: self
                    .state
                    .active_session()
                    .map(|session| session.session_id.clone()),
            }))?;
            return Ok(());
        }

        let active = self
            .state
            .active_session()
            .cloned()
            .expect("active session while recording");
        self.remove_audio_tap();
        let summary = self
            .capture_state
            .as_ref()
            .map(|state| state.lock().expect("capture state").snapshot())
            .unwrap_or(CaptureSummary {
                voice_started: false,
                sent_samples: 0,
                audio_callbacks: 0,
                unsupported_buffers: 0,
            });
        self.capture_state = None;

        if !flush_result {
            let completed = self.state.finish_without_final();
            let _ = self.transport_tx.send(LegacyTransportCommand::Reset);
            self.emit(EngineEvent::RecordingStopped {
                session_id: active.session_id,
                utterance_id: completed.map(|item| item.utterance_id),
                reason: "cancelled".to_string(),
            })?;
            return Ok(());
        }

        let min_samples = (TARGET_SAMPLE_RATE as usize * MIN_UTTERANCE_MS as usize) / 1000;
        if summary.sent_samples < min_samples {
            let completed = self.state.finish_without_final();
            let _ = self.transport_tx.send(LegacyTransportCommand::Reset);
            self.emit(EngineEvent::RecordingStopped {
                session_id: active.session_id.clone(),
                utterance_id: completed.map(|item| item.utterance_id),
                reason: "too_short".to_string(),
            })?;
            self.emit(EngineEvent::Metrics(MetricsPayload {
                session_id: Some(active.session_id),
                name: "capture_summary".to_string(),
                value: serde_json::json!({
                    "voice_started": summary.voice_started,
                    "sent_samples": summary.sent_samples,
                    "audio_callbacks": summary.audio_callbacks,
                    "unsupported_buffers": summary.unsupported_buffers,
                }),
            }))?;
            return Ok(());
        }

        let utterance_id = self
            .state
            .begin_flush(now_millis())
            .map_err(|code| code.to_string())?;
        let _ = self
            .transport_tx
            .send(LegacyTransportCommand::Flush { utterance_id });
        self.state
            .mark_waiting_final()
            .map_err(|code| code.to_string())?;
        self.emit(EngineEvent::RecordingStopped {
            session_id: active.session_id.clone(),
            utterance_id: Some(utterance_id),
            reason: "flush_pending".to_string(),
        })?;
        self.emit(EngineEvent::Metrics(MetricsPayload {
            session_id: Some(active.session_id),
            name: "capture_summary".to_string(),
            value: serde_json::json!({
                "voice_started": summary.voice_started,
                "sent_samples": summary.sent_samples,
                "audio_callbacks": summary.audio_callbacks,
                "unsupported_buffers": summary.unsupported_buffers,
            }),
        }))?;
        Ok(())
    }

    fn install_audio_tap(&mut self, session_id: String) -> Result<(), String> {
        let microphone = unsafe { self.audio_engine.inputNode() };
        let native_format = unsafe { microphone.outputFormatForBus(0) };
        let native_sample_rate = unsafe { native_format.sampleRate() as u32 };
        let native_common_format = unsafe { native_format.commonFormat() };
        let native_channels = unsafe { native_format.channelCount() };
        let native_interleaved = unsafe { native_format.isInterleaved() };
        eprintln!(
            "[hj-engine-helper] native sample rate: {}Hz format={} channels={} interleaved={}",
            native_sample_rate,
            audio_common_format_name(native_common_format),
            native_channels,
            native_interleaved
        );

        let capture_state = Arc::new(Mutex::new(CaptureState::new()));
        let capture_state_for_block = capture_state.clone();
        let input_tx = self.stdin_tx.clone();
        let transport_tx = self.transport_tx.clone();
        let session_id_for_block = session_id.clone();

        let tap_block = block2::RcBlock::new(
            move |buffer: std::ptr::NonNull<AVAudioPCMBuffer>,
                  _time: std::ptr::NonNull<AVAudioTime>| {
                let buffer = unsafe { buffer.as_ref() };
                let Some(samples) = pcm_buffer_to_mono_f32(buffer) else {
                    let mut state = capture_state_for_block
                        .lock()
                        .expect("capture state mutex poisoned");
                    state.audio_callbacks += 1;
                    state.unsupported_buffers += 1;
                    return;
                };

                let resampled =
                    resample_linear(&samples, native_sample_rate, TARGET_SAMPLE_RATE as u32);
                let (peak, rms) = audio_levels(&resampled);
                let is_speech = peak >= SPEECH_PEAK_THRESHOLD || rms >= SPEECH_RMS_THRESHOLD;
                let now = now_millis();
                let pcm = float_to_i16(&resampled);

                let mut state = capture_state_for_block
                    .lock()
                    .expect("capture state mutex poisoned");
                state.audio_callbacks += 1;

                if now.saturating_sub(state.last_level_emit_ms) >= AUDIO_LEVEL_PUSH_INTERVAL_MS {
                    state.last_level_emit_ms = now;
                    let _ = input_tx.send(EngineInput::AudioLevel(AudioLevelPayload {
                        session_id: Some(session_id_for_block.clone()),
                        level_peak: peak,
                        level_rms: rms,
                        vad_state: if is_speech {
                            "speech".to_string()
                        } else {
                            "silence".to_string()
                        },
                        is_live: true,
                    }));
                }

                if is_speech {
                    state.voice_started = true;
                }
                state.sent_samples += pcm.len();
                drop(state);
                let _ = transport_tx.send(LegacyTransportCommand::Audio(pcm));
            },
        );

        unsafe {
            microphone.installTapOnBus_bufferSize_format_block(
                0,
                512,
                Some(&native_format),
                &*tap_block as *const _ as *mut _,
            );
        }
        unsafe { self.audio_engine.prepare() };
        unsafe { self.audio_engine.startAndReturnError() }
            .map_err(|error| format!("audio engine start error: {error:?}"))?;
        self.capture_state = Some(capture_state);
        Ok(())
    }

    fn remove_audio_tap(&self) {
        let microphone = unsafe { self.audio_engine.inputNode() };
        unsafe { microphone.removeTapOnBus(0) };
        unsafe { self.audio_engine.stop() };
        unsafe { self.audio_engine.reset() };
    }

    fn maybe_request_partial(&mut self) {
        if self.args.partial_interval_ms == 0 {
            return;
        }
        if matches!(self.args.transport, TransportKind::DirectFrontier) {
            return;
        }
        let Some(active) = self.state.active_session() else {
            return;
        };
        if self.state.session_state() != SessionState::Recording {
            return;
        }
        let utterance_id = active.utterance_id;
        let partial_in_flight = active.partial_request_in_flight;
        let now = now_millis();
        if now.saturating_sub(self.last_partial_request_ms) < self.args.partial_interval_ms {
            return;
        }
        if partial_in_flight {
            let _ = self.state.mark_partial_requested(true);
            self.last_partial_request_ms = now;
            return;
        }
        if self
            .transport_tx
            .send(LegacyTransportCommand::Partial { utterance_id })
            .is_ok()
        {
            let _ = self.state.mark_partial_requested(false);
            self.last_partial_request_ms = now;
        }
    }

    fn emit_ready(&mut self) -> Result<(), String> {
        self.emit(EngineEvent::Ready(ReadyPayload {
            protocol_version: 1,
            engine_version: HELPER_VERSION,
            session_state: self.state.session_state().as_str(),
            auth_state: self.state.auth_state().as_str(),
            transport: self.transport_name,
        }))
    }

    fn reload_context_config(&mut self, config_path: Option<&str>) -> Result<(), String> {
        self.context_config = load_context_config_from_path(config_path)?;
        Ok(())
    }

    fn emit(&mut self, event: EngineEvent) -> Result<(), String> {
        let stdout = io::stdout();
        let mut lock = stdout.lock();
        let mut value = serde_json::to_value(&event)
            .map_err(|error| format!("stdout serialize failed: {error}"))?;
        if let serde_json::Value::Object(ref mut map) = value {
            map.insert(
                "emitted_at_ms".to_string(),
                serde_json::Value::from(now_millis()),
            );
        }
        serde_json::to_writer(&mut lock, &value)
            .map_err(|error| format!("stdout write serialize failed: {error}"))?;
        lock.write_all(b"\n")
            .map_err(|error| format!("stdout write failed: {error}"))?;
        lock.flush()
            .map_err(|error| format!("stdout flush failed: {error}"))?;
        Ok(())
    }
}

fn spawn_stdin_reader(input_tx: Sender<EngineInput>) {
    std::thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let payload = match line {
                Ok(line) => line,
                Err(error) => {
                    let _ = input_tx.send(EngineInput::Command(Err(format!(
                        "stdin read failed: {error}"
                    ))));
                    break;
                }
            };
            if payload.trim().is_empty() {
                continue;
            }
            let command = serde_json::from_str::<HostCommand>(&payload)
                .map_err(|error| format!("command decode failed: {error}; payload={payload}"));
            let _ = input_tx.send(EngineInput::Command(command));
        }
    });
}

fn to_context_snapshot(payload: ContextSnapshotPayload) -> ContextSnapshot {
    ContextSnapshot {
        frontmost_bundle_id: payload.frontmost_bundle_id,
        text_before_cursor: payload.text_before_cursor,
        text_after_cursor: payload.text_after_cursor,
        cursor_position: payload.cursor_position,
        capture_source: payload.capture_source,
        captured_at_ms: payload.captured_at_ms,
    }
    .with_defaults()
}

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
