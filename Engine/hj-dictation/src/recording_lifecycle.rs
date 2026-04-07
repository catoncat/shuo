use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use crate::input::clear_partial_typing_state;
use crate::state::{
    LAST_ABANDONED_UTTERANCE_ID, LAST_FINAL_UTTERANCE_ID, LAST_FLUSH_UTTERANCE_ID,
    PARTIAL_REQUEST_IN_FLIGHT, SHUTTING_DOWN,
};
use crate::subtitle::dispatch_subtitle_hide;

const FLUSH_WATCHDOG_TIMEOUT_MS: u64 = 12_000;

pub(crate) fn spawn_flush_watchdog(utterance_id: u64) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(FLUSH_WATCHDOG_TIMEOUT_MS));
        if SHUTTING_DOWN.load(Ordering::SeqCst) {
            return;
        }
        if LAST_FLUSH_UTTERANCE_ID.load(Ordering::SeqCst) != utterance_id {
            return;
        }
        if LAST_FINAL_UTTERANCE_ID.load(Ordering::SeqCst) >= utterance_id {
            return;
        }
        eprintln!(
            "[hj-dictation] flush watchdog timeout utterance_id={} after_ms={}",
            utterance_id, FLUSH_WATCHDOG_TIMEOUT_MS
        );
        LAST_ABANDONED_UTTERANCE_ID.store(utterance_id, Ordering::SeqCst);
        PARTIAL_REQUEST_IN_FLIGHT.store(false, Ordering::SeqCst);
        clear_partial_typing_state();
        dispatch_subtitle_hide();
    });
}

pub(crate) fn pending_flush_utterance_id() -> Option<u64> {
    let flush_utterance_id = LAST_FLUSH_UTTERANCE_ID.load(Ordering::SeqCst);
    if flush_utterance_id == 0 {
        return None;
    }
    if flush_utterance_id <= LAST_FINAL_UTTERANCE_ID.load(Ordering::SeqCst) {
        return None;
    }
    if flush_utterance_id <= LAST_ABANDONED_UTTERANCE_ID.load(Ordering::SeqCst) {
        return None;
    }
    Some(flush_utterance_id)
}
