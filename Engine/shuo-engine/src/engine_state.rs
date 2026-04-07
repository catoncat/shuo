use crate::state::now_millis;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionState {
    Idle,
    WarmingUp,
    Recording,
    Flushing,
    WaitingFinal,
    Failed,
}

impl SessionState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::WarmingUp => "warming_up",
            Self::Recording => "recording",
            Self::Flushing => "flushing",
            Self::WaitingFinal => "waiting_final",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthState {
    Cold,
    Refreshing,
    Ready,
    Degraded,
    Failed,
}

impl AuthState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Refreshing => "refreshing",
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ContextSnapshot {
    pub(crate) frontmost_bundle_id: Option<String>,
    pub(crate) text_before_cursor: String,
    pub(crate) text_after_cursor: String,
    pub(crate) cursor_position: usize,
    pub(crate) capture_source: String,
    pub(crate) captured_at_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveSession {
    pub(crate) session_id: String,
    pub(crate) trigger: String,
    pub(crate) context_snapshot: ContextSnapshot,
    pub(crate) utterance_id: u64,
    pub(crate) recording_started_at_ms: u64,
    pub(crate) flush_sent_at_ms: Option<u64>,
    pub(crate) partial_request_in_flight: bool,
    pub(crate) partial_sent_count: u64,
    pub(crate) partial_received_count: u64,
    pub(crate) partial_skipped_busy_count: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct CompletedSession {
    pub(crate) session_id: String,
    pub(crate) utterance_id: u64,
    pub(crate) trigger: String,
    pub(crate) recording_started_at_ms: u64,
    pub(crate) flush_sent_at_ms: Option<u64>,
}

#[derive(Debug)]
pub(crate) struct EngineStateMachine {
    session_state: SessionState,
    auth_state: AuthState,
    next_utterance_id: u64,
    active_session: Option<ActiveSession>,
    last_error: Option<String>,
}

impl Default for EngineStateMachine {
    fn default() -> Self {
        Self {
            session_state: SessionState::Idle,
            auth_state: AuthState::Cold,
            next_utterance_id: 1,
            active_session: None,
            last_error: None,
        }
    }
}

impl EngineStateMachine {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn session_state(&self) -> SessionState {
        self.session_state
    }

    pub(crate) fn auth_state(&self) -> AuthState {
        self.auth_state
    }

    pub(crate) fn set_auth_state(&mut self, auth_state: AuthState) {
        self.auth_state = auth_state;
    }

    pub(crate) fn active_session(&self) -> Option<&ActiveSession> {
        self.active_session.as_ref()
    }

    pub(crate) fn next_utterance_id(&self) -> u64 {
        self.next_utterance_id
    }

    pub(crate) fn begin_warmup(&mut self) -> Result<(), &'static str> {
        match self.session_state {
            SessionState::Idle => {
                self.session_state = SessionState::WarmingUp;
                Ok(())
            }
            SessionState::WarmingUp => Ok(()),
            SessionState::Recording
            | SessionState::Flushing
            | SessionState::WaitingFinal
            | SessionState::Failed => Err("warmup_invalid_state"),
        }
    }

    pub(crate) fn finish_warmup(&mut self) {
        if self.session_state == SessionState::WarmingUp {
            self.session_state = SessionState::Idle;
        }
    }

    pub(crate) fn begin_recording(
        &mut self,
        session_id: String,
        trigger: String,
        context_snapshot: ContextSnapshot,
        started_at_ms: u64,
    ) -> Result<ActiveSession, &'static str> {
        match self.session_state {
            SessionState::Idle | SessionState::WarmingUp => {
                let session = ActiveSession {
                    session_id,
                    trigger,
                    context_snapshot,
                    utterance_id: self.next_utterance_id,
                    recording_started_at_ms: started_at_ms,
                    flush_sent_at_ms: None,
                    partial_request_in_flight: false,
                    partial_sent_count: 0,
                    partial_received_count: 0,
                    partial_skipped_busy_count: 0,
                };
                self.active_session = Some(session.clone());
                self.session_state = SessionState::Recording;
                Ok(session)
            }
            SessionState::Recording
            | SessionState::Flushing
            | SessionState::WaitingFinal
            | SessionState::Failed => Err("recording_invalid_state"),
        }
    }

    pub(crate) fn replace_context(&mut self, context_snapshot: ContextSnapshot) {
        if let Some(session) = self.active_session.as_mut() {
            session.context_snapshot = context_snapshot;
        }
    }

    pub(crate) fn mark_partial_requested(&mut self, skipped: bool) -> Result<(), &'static str> {
        let Some(session) = self.active_session.as_mut() else {
            return Err("partial_without_session");
        };
        if skipped {
            session.partial_skipped_busy_count += 1;
            return Ok(());
        }
        session.partial_request_in_flight = true;
        session.partial_sent_count += 1;
        Ok(())
    }

    pub(crate) fn clear_partial_inflight(&mut self) {
        if let Some(session) = self.active_session.as_mut() {
            session.partial_request_in_flight = false;
        }
    }

    pub(crate) fn mark_partial_received(&mut self) {
        if let Some(session) = self.active_session.as_mut() {
            session.partial_request_in_flight = false;
            session.partial_received_count += 1;
        }
    }

    pub(crate) fn begin_flush(&mut self, sent_at_ms: u64) -> Result<u64, &'static str> {
        if self.session_state != SessionState::Recording {
            return Err("flush_invalid_state");
        }
        let Some(session) = self.active_session.as_mut() else {
            return Err("flush_without_session");
        };
        session.flush_sent_at_ms = Some(sent_at_ms);
        session.partial_request_in_flight = false;
        self.session_state = SessionState::Flushing;
        Ok(session.utterance_id)
    }

    pub(crate) fn mark_waiting_final(&mut self) -> Result<(), &'static str> {
        if self.session_state != SessionState::Flushing {
            return Err("waiting_final_invalid_state");
        }
        self.session_state = SessionState::WaitingFinal;
        self.next_utterance_id += 1;
        Ok(())
    }

    pub(crate) fn complete_final(
        &mut self,
        utterance_id: u64,
    ) -> Result<CompletedSession, &'static str> {
        if self.session_state != SessionState::WaitingFinal {
            return Err("final_invalid_state");
        }
        let Some(session) = self.active_session.take() else {
            return Err("final_without_session");
        };
        if session.utterance_id != utterance_id {
            self.active_session = Some(session);
            return Err("final_stale_utterance");
        }
        self.session_state = SessionState::Idle;
        Ok(CompletedSession {
            session_id: session.session_id,
            utterance_id: session.utterance_id,
            trigger: session.trigger,
            recording_started_at_ms: session.recording_started_at_ms,
            flush_sent_at_ms: session.flush_sent_at_ms,
        })
    }

    pub(crate) fn finish_without_final(&mut self) -> Option<CompletedSession> {
        let session = self.active_session.take()?;
        self.session_state = SessionState::Idle;
        Some(CompletedSession {
            session_id: session.session_id,
            utterance_id: session.utterance_id,
            trigger: session.trigger,
            recording_started_at_ms: session.recording_started_at_ms,
            flush_sent_at_ms: session.flush_sent_at_ms,
        })
    }

    pub(crate) fn fail(&mut self, code: &str) {
        self.last_error = Some(code.to_string());
        self.active_session = None;
        self.session_state = SessionState::Failed;
    }

    pub(crate) fn recover(&mut self) {
        self.last_error = None;
        if self.session_state == SessionState::Failed {
            self.session_state = SessionState::Idle;
        }
    }
}

impl ContextSnapshot {
    pub(crate) fn with_defaults(mut self) -> Self {
        if self.capture_source.trim().is_empty() {
            self.capture_source = "unknown".to_string();
        }
        if self.captured_at_ms == 0 {
            self.captured_at_ms = now_millis();
        }
        self.cursor_position = self
            .cursor_position
            .min(self.text_before_cursor.chars().count());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{ContextSnapshot, EngineStateMachine, SessionState};

    #[test]
    fn supports_warmup_to_recording_to_final() {
        let mut state = EngineStateMachine::new();
        let context = ContextSnapshot::default().with_defaults();
        state.begin_warmup().expect("warmup");
        assert_eq!(state.session_state(), SessionState::WarmingUp);
        state.finish_warmup();
        assert_eq!(state.session_state(), SessionState::Idle);

        let active = state
            .begin_recording("session-1".into(), "hold".into(), context, 100)
            .expect("recording");
        assert_eq!(active.utterance_id, 1);
        assert_eq!(state.session_state(), SessionState::Recording);

        state.mark_partial_requested(false).expect("partial");
        state.mark_partial_received();
        let utterance_id = state.begin_flush(200).expect("flush");
        assert_eq!(utterance_id, 1);
        state.mark_waiting_final().expect("waiting");
        let completed = state.complete_final(1).expect("final");
        assert_eq!(completed.utterance_id, 1);
        assert_eq!(state.session_state(), SessionState::Idle);
        assert_eq!(state.next_utterance_id(), 2);
    }

    #[test]
    fn rejects_double_recording() {
        let mut state = EngineStateMachine::new();
        state
            .begin_recording(
                "session-1".into(),
                "hold".into(),
                ContextSnapshot::default().with_defaults(),
                100,
            )
            .expect("recording");
        assert!(matches!(
            state.begin_recording(
                "session-2".into(),
                "hold".into(),
                ContextSnapshot::default().with_defaults(),
                101,
            ),
            Err("recording_invalid_state")
        ));
    }

    #[test]
    fn resets_failed_state() {
        let mut state = EngineStateMachine::new();
        state.fail("transport_error");
        assert_eq!(state.session_state(), SessionState::Failed);
        state.recover();
        assert_eq!(state.session_state(), SessionState::Idle);
    }
}
