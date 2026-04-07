use std::thread;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::runtime::Builder;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::input::{clear_partial_typing_state, commit_partial_text, sync_partial_text, type_text};
use crate::state::{
    now_millis, BACKEND_READY, IS_RECORDING, LAST_ABANDONED_UTTERANCE_ID, LAST_FINAL_UTTERANCE_ID,
    LAST_FLUSH_SENT_MS, LAST_FLUSH_UTTERANCE_ID, LAST_RECORDING_STARTED_MS, NEXT_UTTERANCE_ID,
    PARTIAL_RECEIVED_COUNT, PARTIAL_REQUEST_IN_FLIGHT, PARTIAL_SENT_COUNT,
    PARTIAL_SKIPPED_BUSY_COUNT, SHOW_SUBTITLE_OVERLAY, SHUTTING_DOWN, VERBOSE,
};
use crate::subtitle::{
    dispatch_subtitle_hide, dispatch_subtitle_update, subtitle_session_apply_final,
    subtitle_session_apply_partial, subtitle_session_reset,
};
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) const KEEPALIVE_WARMUP_INTERVAL_MS: u64 = 30_000;
pub(crate) static TYPE_PARTIAL: AtomicBool = AtomicBool::new(false);

macro_rules! verbose_backend_log {
    ($($arg:tt)*) => {
        if VERBOSE.load(Ordering::SeqCst) {
            eprintln!($($arg)*);
        }
    };
}

#[derive(Debug)]
pub(crate) enum BackendCommand {
    Audio(Vec<i16>),
    Flush { utterance_id: u64 },
    Reset,
    CaptureContext { reason: &'static str },
    Partial,
    Warmup { force: bool },
    Close,
}

#[derive(Deserialize)]
struct ServerTimings {
    audio_ms: Option<u64>,
    warmup_ms: Option<u64>,
    warmup_reason: Option<String>,
    infer_ms: Option<u64>,
    context_capture_ms: Option<u64>,
    context_available: Option<bool>,
    context_source: Option<String>,
    postprocess_ms: Option<u64>,
    llm_ms: Option<u64>,
    llm_timeout_sec: Option<f64>,
    llm_used: Option<bool>,
    llm_provider: Option<String>,
    llm_model: Option<String>,
    total_ms: Option<u64>,
    elapsed_ms: Option<u64>,
    reason: Option<String>,
}

#[derive(Deserialize)]
struct ServerMessage {
    status: Option<String>,
    text: Option<String>,
    is_partial: Option<bool>,
    error: Option<String>,
    utterance_id: Option<u64>,
    timings: Option<ServerTimings>,
}

pub(crate) fn queue_backend_command(
    tx: &UnboundedSender<BackendCommand>,
    command: BackendCommand,
    label: &str,
) -> bool {
    if let Err(error) = tx.send(command) {
        BACKEND_READY.store(false, Ordering::SeqCst);
        eprintln!(
            "[hj-dictation] backend command send failed action={} error={}",
            label, error
        );
        dispatch_subtitle_hide();
        return false;
    }
    true
}

pub(crate) fn spawn_backend_worker(
    server_url: String,
    partial_interval_ms: u64,
) -> UnboundedSender<BackendCommand> {
    let (cmd_tx, mut cmd_rx): (
        UnboundedSender<BackendCommand>,
        UnboundedReceiver<BackendCommand>,
    ) = unbounded_channel();
    let thread_cmd_tx = cmd_tx.clone();

    thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");

        runtime.block_on(async move {
            match connect_async(&server_url).await {
                Ok((ws_stream, _)) => {
                    let (mut write, mut read) = ws_stream.split();
                    BACKEND_READY.store(true, Ordering::SeqCst);
                    verbose_backend_log!("[hj-dictation] backend ready");

                    let writer_tx = thread_cmd_tx.clone();
                    let partial_thread = if partial_interval_ms > 0 {
                        Some(thread::spawn(move || {
                            let mut last = Instant::now();
                            while !SHUTTING_DOWN.load(Ordering::SeqCst) {
                                if IS_RECORDING.load(Ordering::SeqCst)
                                    && BACKEND_READY.load(Ordering::SeqCst)
                                    && last.elapsed() >= Duration::from_millis(partial_interval_ms)
                                {
                                    if PARTIAL_REQUEST_IN_FLIGHT.load(Ordering::SeqCst) {
                                        PARTIAL_SKIPPED_BUSY_COUNT.fetch_add(1, Ordering::SeqCst);
                                        last = Instant::now();
                                        thread::sleep(Duration::from_millis(50));
                                        continue;
                                    }
                                    if !queue_backend_command(
                                        &writer_tx,
                                        BackendCommand::Partial,
                                        "partial",
                                    ) {
                                        break;
                                    }
                                    PARTIAL_REQUEST_IN_FLIGHT.store(true, Ordering::SeqCst);
                                    PARTIAL_SENT_COUNT.fetch_add(1, Ordering::SeqCst);
                                    last = Instant::now();
                                }
                                thread::sleep(Duration::from_millis(50));
                            }
                        }))
                    } else {
                        None
                    };

                    let keepalive_tx = thread_cmd_tx.clone();
                    let keepalive_thread = thread::spawn(move || {
                        let mut last = Instant::now();
                        while !SHUTTING_DOWN.load(Ordering::SeqCst) {
                            if !IS_RECORDING.load(Ordering::SeqCst)
                                && BACKEND_READY.load(Ordering::SeqCst)
                                && last.elapsed()
                                    >= Duration::from_millis(KEEPALIVE_WARMUP_INTERVAL_MS)
                            {
                                if !queue_backend_command(
                                    &keepalive_tx,
                                    BackendCommand::Warmup { force: true },
                                    "warmup",
                                ) {
                                    break;
                                }
                                last = Instant::now();
                            }
                            thread::sleep(Duration::from_millis(250));
                        }
                    });

                    let reader = tokio::spawn(async move {
                        while let Some(message) = read.next().await {
                            match message {
                                Ok(Message::Text(text)) => {
                                    if let Ok(msg) = serde_json::from_str::<ServerMessage>(&text) {
                                        if let Some(error) = msg.error {
                                            eprintln!("[hj-dictation] backend error: {error}");
                                        } else if msg.status.as_deref() == Some("ready") {
                                            BACKEND_READY.store(true, Ordering::SeqCst);
                                        } else if matches!(
                                            msg.status.as_deref(),
                                            Some("warmed") | Some("noop")
                                        ) {
                                            if let Some(timings) = msg.timings {
                                                verbose_backend_log!(
                                                    "[hj-dictation] backend_warmup status={} elapsed_ms={} reason={}",
                                                    msg.status.unwrap_or_else(|| "-".to_string()),
                                                    timings.elapsed_ms.unwrap_or(0),
                                                    timings.reason.unwrap_or_else(|| "-".to_string()),
                                                );
                                            }
                                        } else if let Some(text) = msg.text {
                                            if msg.is_partial.unwrap_or(false) {
                                                let utterance_id = msg.utterance_id.unwrap_or(0);
                                                let abandoned_utterance_id =
                                                    LAST_ABANDONED_UTTERANCE_ID.load(Ordering::SeqCst);
                                                let completed_utterance_id =
                                                    LAST_FINAL_UTTERANCE_ID.load(Ordering::SeqCst);
                                                let current_utterance_id =
                                                    NEXT_UTTERANCE_ID.load(Ordering::SeqCst);
                                                if utterance_id > 0
                                                    && (utterance_id <= abandoned_utterance_id
                                                        || utterance_id <= completed_utterance_id
                                                        || utterance_id != current_utterance_id)
                                                {
                                                    verbose_backend_log!(
                                                        "[hj-dictation] stale partial ignored utterance_id={} current_utterance_id={} last_final_utterance_id={} last_abandoned_utterance_id={}",
                                                        utterance_id,
                                                        current_utterance_id,
                                                        completed_utterance_id,
                                                        abandoned_utterance_id
                                                    );
                                                    continue;
                                                }
                                                PARTIAL_REQUEST_IN_FLIGHT.store(false, Ordering::SeqCst);
                                                PARTIAL_RECEIVED_COUNT.fetch_add(1, Ordering::SeqCst);
                                                if !text.is_empty() {
                                                    verbose_backend_log!("[hj-dictation] partial: {text}");
                                                    let normalized = text.trim();
                                                    let subtitle_snapshot = if normalized.is_empty() {
                                                        None
                                                    } else {
                                                        Some(subtitle_session_apply_partial(normalized))
                                                    };
                                                    if SHOW_SUBTITLE_OVERLAY.load(Ordering::SeqCst) {
                                                        if let Some(snapshot) = &subtitle_snapshot {
                                                            if !snapshot.display_text.is_empty() {
                                                                dispatch_subtitle_update(
                                                                    snapshot.display_text.clone(),
                                                                    false,
                                                                );
                                                            }
                                                        }
                                                    }
                                                    if TYPE_PARTIAL.load(Ordering::SeqCst)
                                                        && !normalized.is_empty()
                                                    {
                                                        let type_started_at_ms = now_millis();
                                                        let (
                                                            prefix_chars,
                                                            deleted_chars,
                                                            appended_chars,
                                                        ) = sync_partial_text(normalized);
                                                        let type_elapsed_ms = now_millis()
                                                            .saturating_sub(type_started_at_ms);
                                                        if deleted_chars > 0 || appended_chars > 0 {
                                                            verbose_backend_log!(
                                                                "[hj-dictation] partial_typed chars={} prefix_chars={} deleted_chars={} appended_chars={} type_ms={}",
                                                                normalized.chars().count(),
                                                                prefix_chars,
                                                                deleted_chars,
                                                                appended_chars,
                                                                type_elapsed_ms
                                                            );
                                                        }
                                                    }
                                                }
                                            } else {
                                                PARTIAL_REQUEST_IN_FLIGHT.store(false, Ordering::SeqCst);
                                                let received_at_ms = now_millis();
                                                let flush_utterance_id =
                                                    LAST_FLUSH_UTTERANCE_ID.load(Ordering::SeqCst);
                                                let flush_sent_at_ms =
                                                    LAST_FLUSH_SENT_MS.load(Ordering::SeqCst);
                                                let recording_started_at_ms =
                                                    LAST_RECORDING_STARTED_MS.load(Ordering::SeqCst);
                                                let utterance_id = msg.utterance_id.unwrap_or(0);
                                                let abandoned_utterance_id =
                                                    LAST_ABANDONED_UTTERANCE_ID.load(Ordering::SeqCst);
                                                if utterance_id > 0
                                                    && utterance_id <= abandoned_utterance_id
                                                {
                                                    verbose_backend_log!(
                                                        "[hj-dictation] stale final ignored utterance_id={} last_abandoned_utterance_id={}",
                                                        utterance_id,
                                                        abandoned_utterance_id
                                                    );
                                                    continue;
                                                }
                                                if utterance_id > 0 {
                                                    LAST_FINAL_UTTERANCE_ID.store(
                                                        utterance_id,
                                                        Ordering::SeqCst,
                                                    );
                                                }

                                                let final_text = text.trim();
                                                if final_text.is_empty() {
                                                    clear_partial_typing_state();
                                                    subtitle_session_reset();
                                                    dispatch_subtitle_hide();
                                                    continue;
                                                }

                                                let subtitle_snapshot =
                                                    subtitle_session_apply_final(final_text);
                                                if SHOW_SUBTITLE_OVERLAY.load(Ordering::SeqCst) {
                                                    dispatch_subtitle_update(
                                                        subtitle_snapshot.display_text.clone(),
                                                        true,
                                                    );
                                                }
                                                verbose_backend_log!("[hj-dictation] final: {text}");
                                                let type_started_at_ms = now_millis();
                                                let commit_text = subtitle_snapshot.commit_text.trim();
                                                if TYPE_PARTIAL.load(Ordering::SeqCst) {
                                                    let (prefix_chars, deleted_chars, appended_chars) =
                                                        commit_partial_text(commit_text);
                                                    if deleted_chars > 0 || appended_chars > 0 {
                                                        verbose_backend_log!(
                                                            "[hj-dictation] final_patched chars={} prefix_chars={} deleted_chars={} appended_chars={}",
                                                            commit_text.chars().count(),
                                                            prefix_chars,
                                                            deleted_chars,
                                                            appended_chars
                                                        );
                                                    }
                                                } else {
                                                    type_text(commit_text);
                                                    clear_partial_typing_state();
                                                }
                                                let type_elapsed_ms =
                                                    now_millis().saturating_sub(type_started_at_ms);
                                                if let Some(timings) = msg.timings {
                                                    let flush_roundtrip_ms =
                                                        if utterance_id == flush_utterance_id {
                                                            received_at_ms.saturating_sub(
                                                                flush_sent_at_ms,
                                                            )
                                                        } else {
                                                            0
                                                        };
                                                    let capture_ms = if utterance_id
                                                        == flush_utterance_id
                                                    {
                                                        flush_sent_at_ms.saturating_sub(
                                                            recording_started_at_ms,
                                                        )
                                                    } else {
                                                        0
                                                    };
                                                    verbose_backend_log!(
                                                        "[hj-dictation] timings utterance_id={} capture_ms={} flush_roundtrip_ms={} audio_ms={} warmup_ms={} infer_ms={} context_capture_ms={} context_available={} context_source={} postprocess_ms={} llm_ms={} llm_used={} llm_timeout_sec={} llm_provider={} llm_model={} backend_total_ms={} type_ms={} partial_sent={} partial_returned={} partial_skipped={} warmup_reason={}",
                                                        utterance_id,
                                                        capture_ms,
                                                        flush_roundtrip_ms,
                                                        timings.audio_ms.unwrap_or(0),
                                                        timings.warmup_ms.unwrap_or(0),
                                                        timings.infer_ms.unwrap_or(0),
                                                        timings.context_capture_ms.unwrap_or(0),
                                                        timings.context_available.unwrap_or(false),
                                                        timings.context_source.unwrap_or_else(|| "-".to_string()),
                                                        timings.postprocess_ms.unwrap_or(0),
                                                        timings.llm_ms.unwrap_or(0),
                                                        timings.llm_used.unwrap_or(false),
                                                        timings.llm_timeout_sec.unwrap_or(0.0),
                                                        timings.llm_provider.unwrap_or_else(|| "-".to_string()),
                                                        timings.llm_model.unwrap_or_else(|| "-".to_string()),
                                                        timings.total_ms.unwrap_or(0),
                                                        type_elapsed_ms,
                                                        PARTIAL_SENT_COUNT.load(Ordering::SeqCst),
                                                        PARTIAL_RECEIVED_COUNT.load(Ordering::SeqCst),
                                                        PARTIAL_SKIPPED_BUSY_COUNT.load(Ordering::SeqCst),
                                                        timings.warmup_reason.unwrap_or_else(|| "-".to_string()),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                Ok(Message::Close(_)) => break,
                                Ok(_) => {}
                                Err(error) => {
                                    eprintln!("[hj-dictation] backend read error: {error}");
                                    break;
                                }
                            }
                        }
                        BACKEND_READY.store(false, Ordering::SeqCst);
                        PARTIAL_REQUEST_IN_FLIGHT.store(false, Ordering::SeqCst);
                        clear_partial_typing_state();
                        dispatch_subtitle_hide();
                    });

                    while let Some(command) = cmd_rx.recv().await {
                        let send_result = match command {
                            BackendCommand::Audio(samples) => write
                                .send(Message::Binary(encode_audio(samples)))
                                .await,
                            BackendCommand::Flush { utterance_id } => {
                                let payload =
                                    format!("{{\"action\":\"flush\",\"utterance_id\":{}}}", utterance_id);
                                write.send(Message::Text(payload)).await
                            }
                            BackendCommand::Reset => {
                                write.send(Message::Text("{\"action\":\"reset\"}".into())).await
                            }
                            BackendCommand::CaptureContext { reason } => {
                                let payload = format!(
                                    "{{\"action\":\"capture_context\",\"reason\":\"{}\"}}",
                                    reason
                                );
                                write.send(Message::Text(payload)).await
                            }
                            BackendCommand::Partial => {
                                let utterance_id = NEXT_UTTERANCE_ID.load(Ordering::SeqCst);
                                let payload = format!(
                                    "{{\"action\":\"partial\",\"utterance_id\":{}}}",
                                    utterance_id
                                );
                                write.send(Message::Text(payload)).await
                            }
                            BackendCommand::Warmup { force } => {
                                let payload = if force {
                                    "{\"action\":\"warmup\",\"force\":true}"
                                } else {
                                    "{\"action\":\"warmup\"}"
                                };
                                write.send(Message::Text(payload.into())).await
                            }
                            BackendCommand::Close => {
                                let _ = write
                                    .send(Message::Text("{\"action\":\"close\"}".into()))
                                    .await;
                                break;
                            }
                        };

                        if let Err(error) = send_result {
                            eprintln!("[hj-dictation] backend write error: {error}");
                            break;
                        }
                    }

                    BACKEND_READY.store(false, Ordering::SeqCst);
                    let _ = write.close().await;
                    let _ = reader.await;
                    if let Some(handle) = partial_thread {
                        let _ = handle.join();
                    }
                    let _ = keepalive_thread.join();
                }
                Err(error) => {
                    PARTIAL_REQUEST_IN_FLIGHT.store(false, Ordering::SeqCst);
                    eprintln!("[hj-dictation] backend connect error: {error}");
                }
            }
        });
    });

    cmd_tx
}

fn encode_audio(samples: Vec<i16>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}
