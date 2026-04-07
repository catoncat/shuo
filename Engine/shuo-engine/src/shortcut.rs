use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use objc2_core_graphics::{CGEvent, CGEventTapProxy, CGEventType};

use crate::config::{apply_shortcut_config, load_context_config};
use crate::input::SYNTHETIC_INPUT_UNTIL_MS;
use crate::state::{now_millis, IS_RECORDING, VERBOSE, VOICE_STARTED};
use crate::ui::{dispatch_action_on_main, MainAction};

const SHORT_TAP_CANCEL_MS: u64 = 220;
const DOUBLE_TAP_THRESHOLD_MS: u64 = 400;

pub(crate) const NX_DEVICERCMDKEYMASK: u64 = 0x10;
pub(crate) const NX_DEVICELCMDKEYMASK: u64 = 0x08;
pub(crate) const NX_DEVICEROPTKEYMASK: u64 = 0x40;
pub(crate) const NX_DEVICELOPTKEYMASK: u64 = 0x20;
pub(crate) const NX_DEVICERSHIFTKEYMASK: u64 = 0x04;
pub(crate) const NX_DEVICELSHIFTKEYMASK: u64 = 0x02;
pub(crate) const NX_DEVICERCTLKEYMASK: u64 = 0x2000;
pub(crate) const NX_DEVICELCTLKEYMASK: u64 = 0x01;

pub(crate) static TRIGGER_KEY_MASK: AtomicU64 = AtomicU64::new(NX_DEVICERCMDKEYMASK);
pub(crate) static TRIGGER_MODE: AtomicU64 = AtomicU64::new(0);

static RIGHT_CMD_HELD: AtomicBool = AtomicBool::new(false);
static RIGHT_CMD_CHORD_ACTIVE: AtomicBool = AtomicBool::new(false);
static RIGHT_CMD_PRESS_STARTED_MS: AtomicU64 = AtomicU64::new(0);
static LAST_TRIGGER_UP_MS: AtomicU64 = AtomicU64::new(0);
static LAST_SHORTCUT_CONFIG_REFRESH_MS: AtomicU64 = AtomicU64::new(0);

macro_rules! verbose_shortcut_log {
    ($($arg:tt)*) => {
        if VERBOSE.load(Ordering::SeqCst) {
            eprintln!($($arg)*);
        }
    };
}

pub(crate) unsafe extern "C-unwind" fn event_tap_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    _user_info: *mut std::ffi::c_void,
) -> *mut CGEvent {
    let now = now_millis();
    refresh_shortcut_config_if_stale(now);

    if event_type == CGEventType::FlagsChanged {
        let flags = CGEvent::flags(Some(event.as_ref()));
        let device_flags = flags.0 & 0xFFFF;
        let key_mask = TRIGGER_KEY_MASK.load(Ordering::SeqCst);
        let right_cmd_pressed = (device_flags & key_mask) != 0;
        let was_down = RIGHT_CMD_HELD.load(Ordering::SeqCst);
        let mode = TRIGGER_MODE.load(Ordering::SeqCst); // 0=hold, 1=double_tap

        if right_cmd_pressed && !was_down {
            RIGHT_CMD_HELD.store(true, Ordering::SeqCst);
            RIGHT_CMD_CHORD_ACTIVE.store(false, Ordering::SeqCst);
            RIGHT_CMD_PRESS_STARTED_MS.store(now, Ordering::SeqCst);
            if mode == 0 {
                dispatch_action_on_main(MainAction::Start);
            } else if !IS_RECORDING.load(Ordering::SeqCst) {
                // Double-tap mode: eagerly warm up frontier on first press
                dispatch_action_on_main(MainAction::EagerWarmup);
            }
        } else if !right_cmd_pressed && was_down {
            RIGHT_CMD_HELD.store(false, Ordering::SeqCst);
            let chord = RIGHT_CMD_CHORD_ACTIVE.load(Ordering::SeqCst);
            RIGHT_CMD_CHORD_ACTIVE.store(false, Ordering::SeqCst);
            if mode == 0 {
                if IS_RECORDING.load(Ordering::SeqCst) {
                    let hold_ms = now_millis()
                        .saturating_sub(RIGHT_CMD_PRESS_STARTED_MS.load(Ordering::SeqCst));
                    if hold_ms < SHORT_TAP_CANCEL_MS && !VOICE_STARTED.load(Ordering::SeqCst) {
                        verbose_shortcut_log!(
                            "[shuo-engine] short tap detected; cancelling recording hold_ms={}",
                            hold_ms
                        );
                        dispatch_action_on_main(MainAction::Cancel);
                    } else {
                        dispatch_action_on_main(MainAction::Stop);
                    }
                }
            } else if !chord {
                let now = now_millis();
                let last_up = LAST_TRIGGER_UP_MS.load(Ordering::SeqCst);
                let gap = now.saturating_sub(last_up);
                if gap < DOUBLE_TAP_THRESHOLD_MS && last_up > 0 {
                    LAST_TRIGGER_UP_MS.store(0, Ordering::SeqCst);
                    if IS_RECORDING.load(Ordering::SeqCst) {
                        verbose_shortcut_log!("[shuo-engine] double-tap → stop");
                        dispatch_action_on_main(MainAction::Stop);
                    } else {
                        verbose_shortcut_log!("[shuo-engine] double-tap → start");
                        dispatch_action_on_main(MainAction::Start);
                    }
                } else {
                    LAST_TRIGGER_UP_MS.store(now, Ordering::SeqCst);
                }
            }
        }
    } else if event_type == CGEventType::KeyDown && RIGHT_CMD_HELD.load(Ordering::SeqCst) {
        if now_millis() <= SYNTHETIC_INPUT_UNTIL_MS.load(Ordering::SeqCst) {
            return event.as_ptr();
        }
        RIGHT_CMD_CHORD_ACTIVE.store(true, Ordering::SeqCst);
        if IS_RECORDING.load(Ordering::SeqCst) {
            verbose_shortcut_log!(
                "[shuo-engine] right-command chord detected; cancelling dictation"
            );
            dispatch_action_on_main(MainAction::Cancel);
        } else {
            verbose_shortcut_log!(
                "[shuo-engine] right-command chord detected; suppressing dictation"
            );
        }
    }
    event.as_ptr()
}

fn refresh_shortcut_config_if_stale(now_ms: u64) {
    const REFRESH_INTERVAL_MS: u64 = 1_000;
    let last = LAST_SHORTCUT_CONFIG_REFRESH_MS.load(Ordering::SeqCst);
    if now_ms.saturating_sub(last) < REFRESH_INTERVAL_MS {
        return;
    }
    if LAST_SHORTCUT_CONFIG_REFRESH_MS
        .compare_exchange(last, now_ms, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let cfg = load_context_config();
    apply_shortcut_config(&cfg.shortcut);
}
