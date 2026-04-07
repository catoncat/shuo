use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct ContextSnapshotPayload {
    pub(crate) frontmost_bundle_id: Option<String>,
    #[serde(default)]
    pub(crate) text_before_cursor: String,
    #[serde(default)]
    pub(crate) text_after_cursor: String,
    #[serde(default)]
    pub(crate) cursor_position: usize,
    #[serde(default)]
    pub(crate) capture_source: String,
    #[serde(default)]
    pub(crate) captured_at_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ReadyPayload {
    pub(crate) protocol_version: u32,
    pub(crate) engine_version: &'static str,
    pub(crate) session_state: &'static str,
    pub(crate) auth_state: &'static str,
    pub(crate) transport: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RecordingPayload {
    pub(crate) session_id: String,
    pub(crate) utterance_id: u64,
    pub(crate) trigger: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AudioLevelPayload {
    pub(crate) session_id: Option<String>,
    pub(crate) level_peak: f32,
    pub(crate) level_rms: f32,
    pub(crate) vad_state: String,
    pub(crate) is_live: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TranscriptPayload {
    pub(crate) session_id: String,
    pub(crate) utterance_id: u64,
    pub(crate) text: String,
    pub(crate) is_stale: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AuthStatePayload {
    pub(crate) state: &'static str,
    pub(crate) source: String,
    pub(crate) expires_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MetricsPayload {
    pub(crate) session_id: Option<String>,
    pub(crate) name: String,
    pub(crate) value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ErrorPayload {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) recoverable: bool,
    pub(crate) session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum EngineEvent {
    Ready(ReadyPayload),
    RecordingStarted(RecordingPayload),
    RecordingStopped {
        session_id: String,
        utterance_id: Option<u64>,
        reason: String,
    },
    AudioLevel(AudioLevelPayload),
    Partial(TranscriptPayload),
    Final(TranscriptPayload),
    AuthState(AuthStatePayload),
    Metrics(MetricsPayload),
    Error(ErrorPayload),
    Fatal {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum HostCommand {
    Hello {
        protocol_version: Option<u32>,
    },
    LoadConfig {
        config_path: Option<String>,
    },
    ReloadConfig {
        config_path: Option<String>,
    },
    Warmup {
        force: Option<bool>,
    },
    StartRecording {
        session_id: String,
        trigger: String,
        context_snapshot: ContextSnapshotPayload,
    },
    StopRecording,
    CancelRecording,
    UpdateContext {
        context_snapshot: ContextSnapshotPayload,
    },
    RefreshAuth,
    ExportDiagnostics,
    Shutdown,
}

#[cfg(test)]
mod tests {
    use super::HostCommand;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Envelope {
        #[serde(rename = "type")]
        kind: String,
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum FixtureEvent {
        Ready {
            protocol_version: u32,
            transport: String,
        },
        Partial {
            session_id: String,
            utterance_id: u64,
            text: String,
            is_stale: bool,
        },
        Final {
            session_id: String,
            utterance_id: u64,
            text: String,
            is_stale: bool,
        },
        RecordingStopped {
            session_id: String,
            utterance_id: Option<u64>,
            reason: String,
        },
    }

    #[test]
    fn shared_ipc_fixture_parses() {
        let raw = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../Shared/Fixtures/ipc.v1.jsonl"
        ));
        let lines: Vec<&str> = raw.lines().filter(|line| !line.trim().is_empty()).collect();
        assert_eq!(lines.len(), 7);

        for line in lines {
            let envelope: Envelope = serde_json::from_str(line).expect("fixture envelope");
            match envelope.kind.as_str() {
                "hello" | "update_context" | "start_recording" => {
                    let _: HostCommand = serde_json::from_str(line).expect("fixture host command");
                }
                "ready" => {
                    match serde_json::from_str(line).expect("fixture ready") {
                        FixtureEvent::Ready {
                            protocol_version,
                            transport,
                        } => {
                            assert_eq!(protocol_version, 1);
                            assert_eq!(transport, "direct_frontier");
                        }
                        _ => unreachable!(),
                    }
                }
                "partial" => {
                    match serde_json::from_str(line).expect("fixture partial") {
                        FixtureEvent::Partial {
                            session_id,
                            utterance_id,
                            text,
                            is_stale,
                        } => {
                            assert_eq!(session_id, "fixture-session");
                            assert_eq!(utterance_id, 1);
                            assert_eq!(text, "你好");
                            assert!(!is_stale);
                        }
                        _ => unreachable!(),
                    }
                }
                "final" => {
                    match serde_json::from_str(line).expect("fixture final") {
                        FixtureEvent::Final {
                            session_id,
                            utterance_id,
                            text,
                            is_stale,
                        } => {
                            assert_eq!(session_id, "fixture-session");
                            assert_eq!(utterance_id, 1);
                            assert_eq!(text, "你好世界。");
                            assert!(!is_stale);
                        }
                        _ => unreachable!(),
                    }
                }
                "recording_stopped" => {
                    match serde_json::from_str(line).expect("fixture stopped") {
                        FixtureEvent::RecordingStopped {
                            session_id,
                            utterance_id,
                            reason,
                        } => {
                            assert_eq!(session_id, "fixture-session");
                            assert_eq!(utterance_id, Some(1));
                            assert_eq!(reason, "flush_pending");
                        }
                        _ => unreachable!(),
                    }
                }
                other => panic!("unexpected fixture kind: {other}"),
            }
        }
    }
}
