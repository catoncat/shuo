use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::thread;
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::NSPasteboard;
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
};
use objc2_foundation::{NSArray, NSCopying, NSData, NSString};

use crate::state::now_millis;

const KEYCODE_DELETE: u16 = 0x33;
const SYNTHETIC_INPUT_GRACE_MS: u64 = 250;

pub(crate) static SYNTHETIC_INPUT_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
static LAST_TYPED_PARTIAL: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new(String::new()));

enum PasteboardPayload {
    Data(Retained<NSData>),
    String(Retained<NSString>),
}

struct PasteboardEntry {
    data_type: Retained<objc2_app_kit::NSPasteboardType>,
    payload: PasteboardPayload,
}

type PasteboardSnapshot = Vec<Vec<PasteboardEntry>>;

pub(crate) fn clear_partial_typing_state() {
    let mut state = LAST_TYPED_PARTIAL
        .lock()
        .expect("partial typing mutex poisoned");
    state.clear();
}

fn extend_synthetic_input_window(extra_ms: u64) {
    let deadline = now_millis().saturating_add(extra_ms);
    let mut current = SYNTHETIC_INPUT_UNTIL_MS.load(Ordering::SeqCst);
    while deadline > current {
        match SYNTHETIC_INPUT_UNTIL_MS.compare_exchange(
            current,
            deadline,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
}

pub(crate) fn type_text(text: &str) {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState);
    let utf16: Vec<u16> = text.encode_utf16().collect();
    let total_chunks = (utf16.len() as u64).div_ceil(20);
    extend_synthetic_input_window(
        total_chunks
            .saturating_mul(20)
            .saturating_add(SYNTHETIC_INPUT_GRACE_MS),
    );
    for chunk in utf16.chunks(20) {
        let down = CGEvent::new_keyboard_event(source.as_deref(), 0, true);
        if let Some(ref ev) = down {
            CGEvent::set_flags(Some(ev), CGEventFlags(0));
            unsafe {
                CGEvent::keyboard_set_unicode_string(Some(ev), chunk.len() as _, chunk.as_ptr());
            }
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(ev));
        }
        thread::sleep(Duration::from_millis(8));
        let up = CGEvent::new_keyboard_event(source.as_deref(), 0, false);
        if let Some(ref ev) = up {
            CGEvent::set_flags(Some(ev), CGEventFlags(0));
            unsafe {
                CGEvent::keyboard_set_unicode_string(Some(ev), chunk.len() as _, chunk.as_ptr());
            }
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(ev));
        }
        thread::sleep(Duration::from_millis(12));
    }
}

fn press_backspace(count: usize) {
    if count == 0 {
        return;
    }
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState);
    extend_synthetic_input_window(
        (count as u64)
            .saturating_mul(12)
            .saturating_add(SYNTHETIC_INPUT_GRACE_MS),
    );
    for _ in 0..count {
        let down = CGEvent::new_keyboard_event(source.as_deref(), KEYCODE_DELETE, true);
        if let Some(ref ev) = down {
            CGEvent::set_flags(Some(ev), CGEventFlags(0));
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(ev));
        }
        thread::sleep(Duration::from_millis(5));
        let up = CGEvent::new_keyboard_event(source.as_deref(), KEYCODE_DELETE, false);
        if let Some(ref ev) = up {
            CGEvent::set_flags(Some(ev), CGEventFlags(0));
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(ev));
        }
        thread::sleep(Duration::from_millis(8));
    }
}

/// Paste text via the system clipboard (NSPasteboard) + Cmd+V.
/// This is much more reliable than synthesising individual key events for
/// each character, especially for long sentences.
fn paste_text(text: &str) {
    let pb = NSPasteboard::generalPasteboard();
    let snapshot = capture_pasteboard_snapshot(&pb);

    // 1. Write to clipboard
    pb.clearContents();
    let ns_text = NSString::from_str(text);
    pb.setString_forType(&ns_text, unsafe { objc2_app_kit::NSPasteboardTypeString });

    // 2. Synthesise Cmd+V
    extend_synthetic_input_window(SYNTHETIC_INPUT_GRACE_MS + 50);
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState);
    // keycode 9 = 'V'
    let down = CGEvent::new_keyboard_event(source.as_deref(), 9, true);
    if let Some(ref ev) = down {
        CGEvent::set_flags(Some(ev), CGEventFlags(CGEventFlags::MaskCommand.0));
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(ev));
    }
    thread::sleep(Duration::from_millis(8));
    let up = CGEvent::new_keyboard_event(source.as_deref(), 9, false);
    if let Some(ref ev) = up {
        CGEvent::set_flags(Some(ev), CGEventFlags(CGEventFlags::MaskCommand.0));
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(ev));
    }
    thread::sleep(Duration::from_millis(30));
    restore_pasteboard_snapshot(&pb, snapshot);
}

fn capture_pasteboard_snapshot(pb: &NSPasteboard) -> PasteboardSnapshot {
    let mut snapshot = Vec::new();
    let Some(items) = pb.pasteboardItems() else {
        return snapshot;
    };

    for item in items.iter() {
        let mut entries = Vec::new();
        let types = item.types();
        for data_type in types.iter() {
            let data_type = data_type.copy();
            if let Some(data) = item.dataForType(&data_type) {
                entries.push(PasteboardEntry {
                    data_type,
                    payload: PasteboardPayload::Data(data),
                });
            } else if let Some(string) = item.stringForType(&data_type) {
                entries.push(PasteboardEntry {
                    data_type,
                    payload: PasteboardPayload::String(string),
                });
            }
        }
        if !entries.is_empty() {
            snapshot.push(entries);
        }
    }

    snapshot
}

fn restore_pasteboard_snapshot(pb: &NSPasteboard, snapshot: PasteboardSnapshot) {
    pb.clearContents();
    if snapshot.is_empty() {
        return;
    }

    let writers: Vec<Retained<ProtocolObject<dyn objc2_app_kit::NSPasteboardWriting>>> = snapshot
        .into_iter()
        .filter_map(|entries| {
            let item = objc2_app_kit::NSPasteboardItem::new();
            let mut wrote_any = false;
            for entry in entries {
                let wrote = match entry.payload {
                    PasteboardPayload::Data(data) => item.setData_forType(&data, &entry.data_type),
                    PasteboardPayload::String(string) => {
                        item.setString_forType(&string, &entry.data_type)
                    }
                };
                wrote_any |= wrote;
            }
            wrote_any.then(|| ProtocolObject::from_retained(item))
        })
        .collect();

    if writers.is_empty() {
        return;
    }

    let objects = NSArray::from_retained_slice(&writers);
    let _ = pb.writeObjects(&objects);
}

pub(crate) fn shared_prefix_chars(left: &str, right: &str) -> usize {
    left.chars()
        .zip(right.chars())
        .take_while(|(a, b)| a == b)
        .count()
}

pub(crate) fn sync_partial_text(text: &str) -> (usize, usize, usize) {
    let normalized = text.trim();
    let mut state = LAST_TYPED_PARTIAL
        .lock()
        .expect("partial typing mutex poisoned");
    let prefix_chars = shared_prefix_chars(state.as_str(), normalized);
    let deleted_chars = state.chars().count().saturating_sub(prefix_chars);
    let appended_text: String = normalized.chars().skip(prefix_chars).collect();
    let appended_chars = appended_text.chars().count();

    if deleted_chars > 0 {
        press_backspace(deleted_chars);
    }
    if !appended_text.is_empty() {
        type_text(&appended_text);
    }

    state.clear();
    state.push_str(normalized);
    (prefix_chars, deleted_chars, appended_chars)
}

pub(crate) fn commit_partial_text(final_text: &str) -> (usize, usize, usize) {
    let normalized = final_text.trim();
    let mut state = LAST_TYPED_PARTIAL
        .lock()
        .expect("partial typing mutex poisoned");

    let total_typed = state.chars().count();
    let prefix_chars = shared_prefix_chars(state.as_str(), normalized);

    if total_typed > 0 && prefix_chars < total_typed {
        // Delete everything we typed, settle, then paste the final text via
        // the system clipboard + Cmd+V.  This is far more reliable than
        // synthesising hundreds of keystrokes for long sentences.
        press_backspace(total_typed);
        thread::sleep(Duration::from_millis(30)); // let the app finish processing deletes
        if !normalized.is_empty() {
            paste_text(normalized);
        }
        let deleted_chars = total_typed;
        let appended_chars = normalized.chars().count();
        state.clear();
        (0, deleted_chars, appended_chars)
    } else {
        // Only need to append (final is a prefix extension of what's typed).
        let appended_text: String = normalized.chars().skip(prefix_chars).collect();
        let appended_chars = appended_text.chars().count();
        if !appended_text.is_empty() {
            type_text(&appended_text);
        }
        state.clear();
        (prefix_chars, 0, appended_chars)
    }
}
