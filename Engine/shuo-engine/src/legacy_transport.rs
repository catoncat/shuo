use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc::Sender, Arc};
use std::thread;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::runtime::Builder;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use crate::config::ContextConfig;
use crate::engine_state::ContextSnapshot;

type WsWrite = futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;

#[derive(Debug)]
pub(crate) enum LegacyTransportCommand {
    ConfigSnapshot { config: ContextConfig },
    Reset,
    Warmup { force: bool },
    RefreshAuth,
    UpdateContext { context: ContextSnapshot },
    Audio(Vec<i16>),
    Partial { utterance_id: u64 },
    Flush { utterance_id: u64 },
    Close,
}

#[derive(Debug, Clone)]
pub(crate) enum LegacyTransportEvent {
    Ready {
        app_key: Option<String>,
        token_source: Option<String>,
        expires_at_ms: Option<u64>,
        context_source: Option<String>,
    },
    Status {
        name: String,
        context: Option<Value>,
        timings: Option<Value>,
    },
    Partial {
        text: String,
        utterance_id: u64,
        timings: Option<Value>,
    },
    Final {
        text: String,
        utterance_id: u64,
        timings: Option<Value>,
    },
    Error {
        message: String,
        recoverable: bool,
    },
    Disconnected {
        reason: String,
    },
}

pub(crate) fn spawn_legacy_transport(
    server_url: String,
    event_tx: Sender<LegacyTransportEvent>,
) -> UnboundedSender<LegacyTransportCommand> {
    let (cmd_tx, mut cmd_rx): (
        UnboundedSender<LegacyTransportCommand>,
        UnboundedReceiver<LegacyTransportCommand>,
    ) = unbounded_channel();

    thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build legacy transport runtime");

        runtime.block_on(async move {
            let connected = Arc::new(AtomicBool::new(false));
            let closing = Arc::new(AtomicBool::new(false));
            let supports_host_context_update = Arc::new(AtomicBool::new(true));
            let mut write: Option<WsWrite> = None;
            let mut reader: Option<JoinHandle<()>> = None;

            while let Some(command) = cmd_rx.recv().await {
                if !matches!(command, LegacyTransportCommand::Close)
                    && (write.is_none() || !connected.load(Ordering::SeqCst))
                {
                    if let Some(handle) = reader.take() {
                        handle.abort();
                    }
                    match connect_legacy_socket(
                        &server_url,
                        event_tx.clone(),
                        connected.clone(),
                        closing.clone(),
                        supports_host_context_update.clone(),
                    )
                    .await
                    {
                        Ok((next_write, next_reader)) => {
                            write = Some(next_write);
                            reader = Some(next_reader);
                        }
                        Err(error) => {
                            let _ = event_tx.send(LegacyTransportEvent::Error {
                                message: error,
                                recoverable: true,
                            });
                            continue;
                        }
                    }
                }

                let payload = match command {
                    LegacyTransportCommand::ConfigSnapshot { .. } => None,
                    LegacyTransportCommand::Reset => {
                        Some(Message::Text(json!({"action": "reset"}).to_string()))
                    }
                    LegacyTransportCommand::Warmup { force } => Some(Message::Text(
                        json!({"action": "warmup", "force": force}).to_string(),
                    )),
                    LegacyTransportCommand::RefreshAuth => Some(Message::Text(
                        json!({"action": "warmup", "force": true}).to_string(),
                    )),
                    LegacyTransportCommand::UpdateContext { context } => Some(Message::Text(
                        if supports_host_context_update.load(Ordering::SeqCst) {
                            json!({
                                "action": "update_context",
                                "frontmost_bundle_id": context.frontmost_bundle_id,
                                "text_before_cursor": context.text_before_cursor,
                                "text_after_cursor": context.text_after_cursor,
                                "cursor_position": context.cursor_position,
                                "capture_source": context.capture_source,
                                "captured_at_ms": context.captured_at_ms,
                            })
                            .to_string()
                        } else {
                            String::new()
                        },
                    ))
                    .filter(|message| match message {
                        Message::Text(text) => !text.is_empty(),
                        _ => true,
                    }),
                    LegacyTransportCommand::Audio(samples) => {
                        Some(Message::Binary(encode_audio(samples)))
                    }
                    LegacyTransportCommand::Partial { utterance_id } => Some(Message::Text(
                        json!({"action": "partial", "utterance_id": utterance_id}).to_string(),
                    )),
                    LegacyTransportCommand::Flush { utterance_id } => Some(Message::Text(
                        json!({"action": "flush", "utterance_id": utterance_id}).to_string(),
                    )),
                    LegacyTransportCommand::Close => {
                        closing.store(true, Ordering::SeqCst);
                        None
                    }
                };

                if let Some(payload) = payload {
                    let Some(writer) = write.as_mut() else {
                        let _ = event_tx.send(LegacyTransportEvent::Error {
                            message: "legacy transport write requested without connection"
                                .to_string(),
                            recoverable: true,
                        });
                        continue;
                    };
                    if let Err(error) = writer.send(payload).await {
                        connected.store(false, Ordering::SeqCst);
                        write = None;
                        let _ = event_tx.send(LegacyTransportEvent::Error {
                            message: format!("legacy transport write error: {error}"),
                            recoverable: true,
                        });
                    }
                    continue;
                }

                if let Some(mut writer) = write.take() {
                    let _ = writer.close().await;
                }
                if let Some(handle) = reader.take() {
                    let _ = handle.await;
                }
                break;
            }
        });
    });

    cmd_tx
}

async fn connect_legacy_socket(
    server_url: &str,
    event_tx: Sender<LegacyTransportEvent>,
    connected: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
    supports_host_context_update: Arc<AtomicBool>,
) -> Result<(WsWrite, JoinHandle<()>), String> {
    let (socket, _) = connect_async(server_url)
        .await
        .map_err(|error| format!("legacy transport connect error: {error}"))?;
    connected.store(true, Ordering::SeqCst);
    closing.store(false, Ordering::SeqCst);
    supports_host_context_update.store(true, Ordering::SeqCst);

    let (write, mut read) = socket.split();
    let handle = tokio::spawn(async move {
        while let Some(message) = read.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if let Some(event) =
                        parse_transport_message(&text, supports_host_context_update.as_ref())
                    {
                        let _ = event_tx.send(event);
                    }
                }
                Ok(Message::Close(frame)) => {
                    connected.store(false, Ordering::SeqCst);
                    if !closing.load(Ordering::SeqCst) {
                        let reason = frame
                            .map(|item| item.reason.to_string())
                            .filter(|item| !item.is_empty())
                            .unwrap_or_else(|| "socket_closed".to_string());
                        let _ = event_tx.send(LegacyTransportEvent::Disconnected { reason });
                    }
                    return;
                }
                Ok(_) => {}
                Err(error) => {
                    connected.store(false, Ordering::SeqCst);
                    if !closing.load(Ordering::SeqCst) {
                        let _ = event_tx.send(LegacyTransportEvent::Error {
                            message: format!("legacy transport read error: {error}"),
                            recoverable: true,
                        });
                    }
                    return;
                }
            }
        }

        connected.store(false, Ordering::SeqCst);
        if !closing.load(Ordering::SeqCst) {
            let _ = event_tx.send(LegacyTransportEvent::Disconnected {
                reason: "socket_eof".to_string(),
            });
        }
    });

    Ok((write, handle))
}

fn parse_transport_message(
    text: &str,
    supports_host_context_update: &AtomicBool,
) -> Option<LegacyTransportEvent> {
    let payload: Value = serde_json::from_str(text).ok()?;
    if let Some(error) = payload.get("error").and_then(Value::as_str) {
        if error == "unsupported action: update_context" {
            supports_host_context_update.store(false, Ordering::SeqCst);
            return Some(LegacyTransportEvent::Status {
                name: "context_update_unsupported".to_string(),
                context: None,
                timings: None,
            });
        }
        return Some(LegacyTransportEvent::Error {
            message: error.to_string(),
            recoverable: true,
        });
    }

    if let Some(status) = payload.get("status").and_then(Value::as_str) {
        return match status {
            "ready" => Some(LegacyTransportEvent::Ready {
                app_key: payload
                    .get("app_key")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                token_source: payload
                    .get("token_source")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                expires_at_ms: payload.get("expires_at_ms").and_then(Value::as_u64),
                context_source: payload
                    .get("context_source")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            }),
            "warmed" | "context_updated" | "captured" | "noop" | "closing" => {
                Some(LegacyTransportEvent::Status {
                    name: status.to_string(),
                    context: payload.get("context").cloned(),
                    timings: payload.get("timings").cloned(),
                })
            }
            _ => None,
        };
    }

    let text_value = payload
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let utterance_id = payload
        .get("utterance_id")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let timings = payload.get("timings").cloned();
    if payload
        .get("is_partial")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Some(LegacyTransportEvent::Partial {
            text: text_value,
            utterance_id,
            timings,
        });
    }

    if !text_value.is_empty() || utterance_id > 0 {
        return Some(LegacyTransportEvent::Final {
            text: text_value,
            utterance_id,
            timings,
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::parse_transport_message;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn downgrades_unsupported_update_context_to_status() {
        let supports = AtomicBool::new(true);
        let event = parse_transport_message(
            r#"{"error":"unsupported action: update_context"}"#,
            &supports,
        )
        .expect("event");
        match event {
            super::LegacyTransportEvent::Status { name, .. } => {
                assert_eq!(name, "context_update_unsupported");
            }
            _ => panic!("expected status event"),
        }
        assert!(!supports.load(Ordering::SeqCst));
    }
}

fn encode_audio(samples: Vec<i16>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}
