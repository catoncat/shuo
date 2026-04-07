use std::collections::VecDeque;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSImage, NSSound, NSStatusItem};
use objc2_avf_audio::{AVAudioEngine, AVAudioPCMBuffer, AVAudioTime};
use objc2_foundation::NSString;
use tokio::sync::mpsc::UnboundedSender;

use crate::audio::{
    audio_common_format_name, audio_levels, float_to_i16, pcm_buffer_to_mono_f32, resample_linear,
};
use crate::backend::{queue_backend_command, BackendCommand};
use crate::input::clear_partial_typing_state;
use crate::recording_lifecycle::{pending_flush_utterance_id, spawn_flush_watchdog};
use crate::state::{
    now_millis, AUDIO_CALLBACK_COUNT, AUDIO_UNSUPPORTED_BUFFER_COUNT, BACKEND_READY, IS_RECORDING,
    LAST_FLUSH_SENT_MS, LAST_FLUSH_UTTERANCE_ID, LAST_RECORDING_STARTED_MS, LAST_SPEECH_MS,
    NEXT_UTTERANCE_ID, PARTIAL_RECEIVED_COUNT, PARTIAL_REQUEST_IN_FLIGHT, PARTIAL_SENT_COUNT,
    PARTIAL_SKIPPED_BUSY_COUNT, SENT_SAMPLES, SHOW_SUBTITLE_OVERLAY, SHUTTING_DOWN, VERBOSE,
    VOICE_STARTED,
};
use crate::subtitle::{
    dispatch_subtitle_hide, dispatch_subtitle_level, dispatch_subtitle_show_waveform_only,
    dispatch_subtitle_update, reset_subtitle_level_dispatch, subtitle_audio_meter_level,
    subtitle_session_reset, SubtitleOverlay, SUBTITLE_WAVE_UPDATE_INTERVAL_MS,
};

const TARGET_SAMPLE_RATE: f64 = 16_000.0;
const MIN_UTTERANCE_MS: u64 = 350;
const SPEECH_PEAK_THRESHOLD: f32 = 0.015;
const SPEECH_RMS_THRESHOLD: f32 = 0.004;
const SPEECH_HANGOVER_MS: u64 = 180;
const PRE_SPEECH_ROLL_MS: u64 = 220;

static LAST_SUBTITLE_LEVEL_PUSH_MS: AtomicU64 = AtomicU64::new(0);
static CONTROLLER: AtomicPtr<Controller> = AtomicPtr::new(std::ptr::null_mut());

macro_rules! verbose_ui_log {
    ($($arg:tt)*) => {
        if VERBOSE.load(Ordering::SeqCst) {
            eprintln!($($arg)*);
        }
    };
}

#[derive(Clone, Copy)]
pub(crate) enum MainAction {
    Start,
    Stop,
    Cancel,
    EagerWarmup,
}

pub(crate) struct Controller {
    audio_engine: Retained<AVAudioEngine>,
    status_item: Retained<NSStatusItem>,
    backend_tx: UnboundedSender<BackendCommand>,
    pub(crate) subtitle_overlay: Option<SubtitleOverlay>,
}

impl Controller {
    pub(crate) fn new(
        audio_engine: Retained<AVAudioEngine>,
        status_item: Retained<NSStatusItem>,
        backend_tx: UnboundedSender<BackendCommand>,
        subtitle_overlay: Option<SubtitleOverlay>,
    ) -> Self {
        Self {
            audio_engine,
            status_item,
            backend_tx,
            subtitle_overlay,
        }
    }

    pub(crate) fn update_subtitle_waveform_only(&self, mtm: MainThreadMarker) {
        if let Some(overlay) = &self.subtitle_overlay {
            overlay.show_waveform_only(mtm);
        }
    }

    pub(crate) fn update_subtitle_level(&self, level: f64) {
        if let Some(overlay) = &self.subtitle_overlay {
            overlay.update_level(level);
        }
    }

    pub(crate) fn hide_subtitle(&self) {
        if let Some(overlay) = &self.subtitle_overlay {
            overlay.hide();
        }
    }

    pub(crate) fn advance_subtitle_fade(&self, seq: u64, step: u64, fade_in: bool) {
        if let Some(overlay) = &self.subtitle_overlay {
            overlay.advance_fade(seq, step, fade_in);
        }
    }

    pub(crate) fn advance_subtitle_collapse(&self, mtm: MainThreadMarker, seq: u64, step: u64) {
        if let Some(overlay) = &self.subtitle_overlay {
            overlay.advance_collapse(mtm, seq, step);
        }
    }

    pub(crate) fn start_recording(&self, mtm: MainThreadMarker) {
        if IS_RECORDING.load(Ordering::SeqCst) {
            return;
        }
        if !BACKEND_READY.load(Ordering::SeqCst) {
            eprintln!("[shuo-engine] backend not ready yet");
            return;
        }
        if let Some(utterance_id) = pending_flush_utterance_id() {
            eprintln!(
                "[shuo-engine] previous utterance still processing; wait for final text (utterance_id={})",
                utterance_id
            );
            if SHOW_SUBTITLE_OVERLAY.load(Ordering::SeqCst) {
                dispatch_subtitle_update("正在处理上一句…".to_string(), false);
            }
            return;
        }

        verbose_ui_log!("[shuo-engine] recording started...");
        IS_RECORDING.store(true, Ordering::SeqCst);

        // Play Siri-style start sound
        play_system_sound(SFX_BEGIN);
        VOICE_STARTED.store(false, Ordering::SeqCst);
        SENT_SAMPLES.store(0, Ordering::SeqCst);
        AUDIO_CALLBACK_COUNT.store(0, Ordering::SeqCst);
        AUDIO_UNSUPPORTED_BUFFER_COUNT.store(0, Ordering::SeqCst);
        PARTIAL_REQUEST_IN_FLIGHT.store(false, Ordering::SeqCst);
        PARTIAL_SENT_COUNT.store(0, Ordering::SeqCst);
        PARTIAL_SKIPPED_BUSY_COUNT.store(0, Ordering::SeqCst);
        PARTIAL_RECEIVED_COUNT.store(0, Ordering::SeqCst);
        LAST_SUBTITLE_LEVEL_PUSH_MS.store(0, Ordering::SeqCst);
        reset_subtitle_level_dispatch();
        LAST_SPEECH_MS.store(now_millis(), Ordering::SeqCst);
        LAST_RECORDING_STARTED_MS.store(now_millis(), Ordering::SeqCst);
        clear_partial_typing_state();
        subtitle_session_reset();
        if SHOW_SUBTITLE_OVERLAY.load(Ordering::SeqCst) {
            dispatch_subtitle_show_waveform_only();
        }
        set_status_icon(&self.status_item, true, mtm);
        if !queue_backend_command(&self.backend_tx, BackendCommand::Reset, "reset")
            || !queue_backend_command(
                &self.backend_tx,
                BackendCommand::CaptureContext { reason: "start" },
                "capture_context",
            )
            || !queue_backend_command(
                &self.backend_tx,
                BackendCommand::Warmup { force: false },
                "warmup",
            )
        {
            IS_RECORDING.store(false, Ordering::SeqCst);
            set_status_icon(&self.status_item, false, mtm);
            dispatch_subtitle_hide();
            return;
        }

        if unsafe { self.audio_engine.isRunning() } {
            verbose_ui_log!(
                "[shuo-engine] audio engine still running before start; forcing reset"
            );
            unsafe {
                self.audio_engine.stop();
                self.audio_engine.reset();
            }
        }

        let microphone = unsafe { self.audio_engine.inputNode() };
        let backend_audio_tx = self.backend_tx.clone();
        let native_format = unsafe { microphone.outputFormatForBus(0) };
        let native_sample_rate = unsafe { native_format.sampleRate() as u32 };
        let native_common_format = unsafe { native_format.commonFormat() };
        let native_channels = unsafe { native_format.channelCount() };
        let native_interleaved = unsafe { native_format.isInterleaved() };
        let preroll_limit = (TARGET_SAMPLE_RATE as usize * PRE_SPEECH_ROLL_MS as usize) / 1000;
        let preroll_buffer = std::sync::Mutex::new(VecDeque::<i16>::with_capacity(preroll_limit));
        verbose_ui_log!(
            "[shuo-engine] native sample rate: {}Hz format={} channels={} interleaved={}",
            native_sample_rate,
            audio_common_format_name(native_common_format),
            native_channels,
            native_interleaved
        );
        let tap_block = RcBlock::new(
            move |buffer: NonNull<AVAudioPCMBuffer>, _time: NonNull<AVAudioTime>| {
                if !IS_RECORDING.load(Ordering::SeqCst) {
                    return;
                }
                AUDIO_CALLBACK_COUNT.fetch_add(1, Ordering::SeqCst);
                let buffer = unsafe { buffer.as_ref() };
                let Some(samples) = pcm_buffer_to_mono_f32(buffer) else {
                    let unsupported_count =
                        AUDIO_UNSUPPORTED_BUFFER_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
                    if unsupported_count == 1 {
                        let format = unsafe { buffer.format() };
                        let common = unsafe { format.commonFormat() };
                        let channels = unsafe { format.channelCount() };
                        let interleaved = unsafe { format.isInterleaved() };
                        let stride = unsafe { buffer.stride() };
                        eprintln!(
                            "[shuo-engine] unsupported input buffer format={} channels={} interleaved={} stride={}",
                            audio_common_format_name(common),
                            channels,
                            interleaved,
                            stride
                        );
                    }
                    return;
                };
                let resampled =
                    resample_linear(&samples, native_sample_rate, TARGET_SAMPLE_RATE as u32);
                let (peak, rms) = audio_levels(&resampled);
                let is_speech = peak >= SPEECH_PEAK_THRESHOLD || rms >= SPEECH_RMS_THRESHOLD;
                let now = now_millis();
                if SHOW_SUBTITLE_OVERLAY.load(Ordering::SeqCst) {
                    let last_push = LAST_SUBTITLE_LEVEL_PUSH_MS.load(Ordering::SeqCst);
                    if now.saturating_sub(last_push) >= SUBTITLE_WAVE_UPDATE_INTERVAL_MS {
                        LAST_SUBTITLE_LEVEL_PUSH_MS.store(now, Ordering::SeqCst);
                        dispatch_subtitle_level(subtitle_audio_meter_level(peak, rms));
                    }
                }
                let pcm = float_to_i16(&resampled);
                let voice_started = VOICE_STARTED.load(Ordering::SeqCst);

                if !voice_started {
                    let mut preroll = preroll_buffer.lock().expect("preroll mutex poisoned");
                    preroll.extend(pcm.iter().copied());
                    while preroll.len() > preroll_limit {
                        let _ = preroll.pop_front();
                    }

                    if is_speech {
                        VOICE_STARTED.store(true, Ordering::SeqCst);
                        LAST_SPEECH_MS.store(now, Ordering::SeqCst);
                        let initial_pcm: Vec<i16> = preroll.drain(..).collect();
                        let initial_len = initial_pcm.len();
                        drop(preroll);
                        verbose_ui_log!(
                            "[shuo-engine] voice detected; sending preroll_ms={} peak={:.4} rms={:.4}",
                            (initial_len as u64 * 1000) / TARGET_SAMPLE_RATE as u64,
                            peak,
                            rms
                        );
                        if queue_backend_command(
                            &backend_audio_tx,
                            BackendCommand::Audio(initial_pcm),
                            "audio_preroll",
                        ) {
                            SENT_SAMPLES.fetch_add(initial_len, Ordering::SeqCst);
                        }
                    }
                    return;
                }

                if is_speech {
                    LAST_SPEECH_MS.store(now, Ordering::SeqCst);
                }

                let in_hangover = voice_started
                    && now.saturating_sub(LAST_SPEECH_MS.load(Ordering::SeqCst))
                        <= SPEECH_HANGOVER_MS;

                if voice_started && (is_speech || in_hangover) {
                    let pcm_len = pcm.len();
                    if queue_backend_command(
                        &backend_audio_tx,
                        BackendCommand::Audio(pcm),
                        "audio_chunk",
                    ) {
                        SENT_SAMPLES.fetch_add(pcm_len, Ordering::SeqCst);
                    }
                }
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
        let engine_started_at_ms = now_millis();
        if let Err(error) = unsafe { self.audio_engine.startAndReturnError() } {
            eprintln!("[shuo-engine] audio engine start error: {:?}", error);
            IS_RECORDING.store(false, Ordering::SeqCst);
            set_status_icon(&self.status_item, false, mtm);
            unsafe { microphone.removeTapOnBus(0) };
        } else {
            verbose_ui_log!(
                "[shuo-engine] engine_start_ms={}",
                now_millis().saturating_sub(engine_started_at_ms)
            );
        }
    }

    fn finish_recording(&self, mtm: MainThreadMarker, flush_result: bool) {
        if !IS_RECORDING.swap(false, Ordering::SeqCst) {
            return;
        }

        if flush_result {
            verbose_ui_log!("[shuo-engine] recording stopped");
            // Play Siri-style stop sound
            play_system_sound(SFX_CONFIRM);
        } else {
            verbose_ui_log!("[shuo-engine] recording cancelled");
        }
        set_status_icon(&self.status_item, false, mtm);

        let microphone = unsafe { self.audio_engine.inputNode() };
        unsafe { microphone.removeTapOnBus(0) };
        unsafe { self.audio_engine.stop() };
        unsafe { self.audio_engine.reset() };

        if !flush_result {
            PARTIAL_REQUEST_IN_FLIGHT.store(false, Ordering::SeqCst);
            clear_partial_typing_state();
            subtitle_session_reset();
            dispatch_subtitle_hide();
            let _ = self.backend_tx.send(BackendCommand::Reset);
            return;
        }

        let sent_samples = SENT_SAMPLES.load(Ordering::SeqCst);
        let audio_callbacks = AUDIO_CALLBACK_COUNT.load(Ordering::SeqCst);
        let unsupported_buffers = AUDIO_UNSUPPORTED_BUFFER_COUNT.load(Ordering::SeqCst);
        let min_samples = (TARGET_SAMPLE_RATE as usize * MIN_UTTERANCE_MS as usize) / 1000;
        let voice_started = VOICE_STARTED.load(Ordering::SeqCst);
        verbose_ui_log!(
            "[shuo-engine] finish_recording flush_result={} voice_started={} sent_samples={} callbacks={} unsupported_buffers={} min_samples={}",
            flush_result,
            voice_started,
            sent_samples,
            audio_callbacks,
            unsupported_buffers,
            min_samples
        );

        if !voice_started || sent_samples < min_samples {
            if audio_callbacks == 0 {
                eprintln!("[shuo-engine] no audio callbacks received from AVAudioEngine");
            } else if unsupported_buffers > 0 {
                eprintln!(
                    "[shuo-engine] dropped {} audio buffers due to unsupported input format",
                    unsupported_buffers
                );
            }
            eprintln!("[shuo-engine] discarded short/quiet utterance");
            PARTIAL_REQUEST_IN_FLIGHT.store(false, Ordering::SeqCst);
            clear_partial_typing_state();
            subtitle_session_reset();
            dispatch_subtitle_hide();
            let _ = queue_backend_command(&self.backend_tx, BackendCommand::Reset, "reset");
        } else {
            let utterance_id = NEXT_UTTERANCE_ID.fetch_add(1, Ordering::SeqCst);
            if queue_backend_command(
                &self.backend_tx,
                BackendCommand::Flush { utterance_id },
                "flush",
            ) {
                LAST_FLUSH_UTTERANCE_ID.store(utterance_id, Ordering::SeqCst);
                LAST_FLUSH_SENT_MS.store(now_millis(), Ordering::SeqCst);
                verbose_ui_log!(
                    "[shuo-engine] flush queued utterance_id={} sent_samples={}",
                    utterance_id,
                    sent_samples
                );
                spawn_flush_watchdog(utterance_id);
            } else {
                clear_partial_typing_state();
                dispatch_subtitle_hide();
            }
        }
    }

    pub(crate) fn stop_recording(&self, mtm: MainThreadMarker) {
        self.finish_recording(mtm, true);
    }

    pub(crate) fn cancel_recording(&self, mtm: MainThreadMarker) {
        self.finish_recording(mtm, false);
    }

    /// Pre-warm frontier connection without starting recording.
    /// Called on first key press in double-tap mode to overlap
    /// connection setup with the double-tap wait window.
    pub(crate) fn eager_warmup(&self) {
        if IS_RECORDING.load(Ordering::SeqCst) {
            return;
        }
        let _ = queue_backend_command(
            &self.backend_tx,
            BackendCommand::CaptureContext { reason: "eager" },
            "eager_capture_context",
        );
        let _ = queue_backend_command(
            &self.backend_tx,
            BackendCommand::Warmup { force: false },
            "eager_warmup",
        );
    }
}

pub(crate) fn dispatch_main<F>(work: F)
where
    F: FnOnce() + Send + 'static,
{
    dispatch2::Queue::main().exec_async(work);
}

pub(crate) fn install_controller(controller: Box<Controller>) {
    let controller = Box::into_raw(controller);
    CONTROLLER.store(controller, Ordering::SeqCst);
}

pub(crate) fn uninstall_controller() -> Option<Box<Controller>> {
    let controller = CONTROLLER.swap(std::ptr::null_mut(), Ordering::SeqCst);
    if controller.is_null() {
        None
    } else {
        Some(unsafe { Box::from_raw(controller) })
    }
}

fn current_controller() -> Option<&'static Controller> {
    let controller = CONTROLLER.load(Ordering::SeqCst);
    if controller.is_null() {
        None
    } else {
        Some(unsafe { &*controller })
    }
}

pub(crate) fn with_controller_on_main<R>(
    work: impl FnOnce(&Controller, MainThreadMarker) -> R,
) -> Option<R> {
    let controller = current_controller()?;
    let mtm = MainThreadMarker::new().expect("main thread marker");
    Some(work(controller, mtm))
}

pub(crate) fn set_status_icon(item: &NSStatusItem, recording: bool, mtm: MainThreadMarker) {
    let name = if recording { "mic.fill" } else { "mic" };
    if let Some(button) = item.button(mtm) {
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(name),
            Some(&NSString::from_str("Vox Dictation")),
        ) {
            image.setTemplate(true);
            button.setImage(Some(&image));
        } else {
            button.setTitle(&NSString::from_str(if recording { "●" } else { "🎤" }));
        }
    }
}

pub(crate) fn dispatch_action_on_main(action: MainAction) {
    dispatch_main(move || {
        let _ = with_controller_on_main(|controller, mtm| match action {
            MainAction::Start => controller.start_recording(mtm),
            MainAction::Stop => controller.stop_recording(mtm),
            MainAction::Cancel => controller.cancel_recording(mtm),
            MainAction::EagerWarmup => controller.eager_warmup(),
        });
    });
}

pub(crate) fn request_app_shutdown() {
    if SHUTTING_DOWN.swap(true, Ordering::SeqCst) {
        return;
    }

    dispatch_main(move || {
        if IS_RECORDING.load(Ordering::SeqCst) {
            let _ = with_controller_on_main(|controller, mtm| {
                controller.cancel_recording(mtm);
            });
        }

        let mtm = MainThreadMarker::new().expect("main thread marker");
        let app = NSApplication::sharedApplication(mtm);
        app.terminate(None);
    });
}

const SFX_DIR: &str =
    "/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/siri";
const SFX_BEGIN: &str = "jbl_begin_short.caf";
const SFX_CONFIRM: &str = "jbl_confirm.caf";

fn play_system_sound(name: &str) {
    let path = format!("{}/{}", SFX_DIR, name);
    let ns_path = NSString::from_str(&path);
    let url = objc2_foundation::NSURL::fileURLWithPath(&ns_path);
    let sound: Option<Retained<NSSound>> = unsafe {
        objc2::msg_send![NSSound::alloc(), initWithContentsOfURL: &*url, byReference: true]
    };
    if let Some(s) = sound {
        s.play();
    }
}
