use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::thread;
use std::time::Duration;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSAutoresizingMaskOptions,
    NSBackingStoreType, NSColor, NSFont, NSGlassEffectView, NSGlassEffectViewStyle,
    NSLineBreakMode, NSScreen, NSStatusWindowLevel, NSTextAlignment, NSTextField, NSView, NSWindow,
    NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString};

use crate::input::shared_prefix_chars;
use crate::state::{scale_ui, ui_scale, SHOW_SUBTITLE_OVERLAY, SHUTTING_DOWN};
use crate::ui::{dispatch_main, with_controller_on_main};

const SUBTITLE_HIDE_DELAY_MS: u64 = 600;
const SUBTITLE_BOTTOM_MARGIN: f64 = 34.0;
const SUBTITLE_WIDTH_RATIO: f64 = 0.42;
const SUBTITLE_MIN_WIDTH: f64 = 124.0;
const SUBTITLE_MAX_WIDTH: f64 = 560.0;
const SUBTITLE_CAPSULE_HEIGHT: f64 = 42.0;
const SUBTITLE_HORIZONTAL_PADDING: f64 = 12.0;
const SUBTITLE_VERTICAL_PADDING: f64 = 0.0;
const SUBTITLE_WAVE_BAR_COUNT: usize = 5;
const SUBTITLE_WAVE_WIDTH: f64 = 26.0;
const SUBTITLE_WAVE_HEIGHT: f64 = 18.0;
const SUBTITLE_WAVE_BAR_WIDTH: f64 = 2.2;
const SUBTITLE_WAVE_BAR_GAP: f64 = 2.4;
const SUBTITLE_WAVE_MIN_FRACTION: f64 = 0.04;
pub(crate) const SUBTITLE_WAVE_UPDATE_INTERVAL_MS: u64 = 8;
const SUBTITLE_FADE_IN_MS: u64 = 90;
const SUBTITLE_FADE_IN_STEPS: u64 = 6;
const SUBTITLE_FADE_OUT_MS: u64 = 100;
const SUBTITLE_FADE_OUT_STEPS: u64 = 6;
const SUBTITLE_COLLAPSE_TICK_MS: u64 = 8;
const SUBTITLE_COLLAPSE_STEPS: u64 = 7;
const SUBTITLE_WAVE_ATTACK_FACTOR: f64 = 0.92;
const SUBTITLE_WAVE_RELEASE_FACTOR: f64 = 0.42;
const SUBTITLE_WAVE_SILENCE_LEVEL: f64 = 0.045;
const SUBTITLE_WAVE_PEAK_SCALE: f64 = 5.8;
const SUBTITLE_WAVE_RMS_SCALE: f64 = 30.0;
const SUBTITLE_WAVE_INPUT_GAIN: f64 = 2.2;
const SUBTITLE_WAVE_VISUAL_EXPONENT: f64 = 0.54;
const SUBTITLE_CONTENT_GAP: f64 = 10.0;
const SUBTITLE_FONT_SIZE: f64 = 12.0;
const SUBTITLE_LABEL_HEIGHT: f64 = 15.0;
const SUBTITLE_LINE_HEIGHT: f64 = 15.0;
const SUBTITLE_TEXT_WIDTH_FACTOR: f64 = 1.04;
const SUBTITLE_WRAP_UNIT_FACTOR: f64 = 0.78;
const SUBTITLE_MAX_VISIBLE_LINES: usize = 4;

static SUBTITLE_UPDATE_SEQ: AtomicU64 = AtomicU64::new(0);
static SUBTITLE_LEVEL_BITS: AtomicU64 = AtomicU64::new(0);
static SUBTITLE_LEVEL_SEQ: AtomicU64 = AtomicU64::new(0);
static SUBTITLE_LEVEL_DISPATCH_PENDING: AtomicBool = AtomicBool::new(false);
static SUBTITLE_SESSION_STATE: LazyLock<Mutex<SubtitleSessionState>> =
    LazyLock::new(|| Mutex::new(SubtitleSessionState::default()));

#[derive(Default, Clone)]
struct SubtitleSessionState {
    committed_text: String,
    live_text: String,
}

pub(crate) struct SubtitleTranscriptSnapshot {
    pub(crate) display_text: String,
    pub(crate) commit_text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum SubtitleTheme {
    Light,
    Dark,
}

pub(crate) struct SubtitleOverlay {
    window: Retained<NSWindow>,
    glass: Retained<NSGlassEffectView>,
    content: Retained<NSView>,
    waveform: Retained<NSView>,
    wave_bars: Vec<Retained<NSView>>,
    label: Retained<NSTextField>,
    visible: Cell<bool>,
    smoothed_level: Cell<f64>,
    wave_phase: Cell<f64>,
    fade_seq: Cell<u64>,
    collapse_seq: Cell<u64>,
    collapse_start_width: Cell<f64>,
}

impl SubtitleOverlay {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let frame = subtitle_window_frame(mtm, "");
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        let transparent = NSColor::clearColor();
        window.setBackgroundColor(Some(&transparent));
        window.setOpaque(false);
        window.setHasShadow(false);
        window.setIgnoresMouseEvents(false);
        window.setMovable(false);
        window.setMovableByWindowBackground(true);
        window.setCanHide(false);
        window.setHidesOnDeactivate(false);
        window.setExcludedFromWindowsMenu(true);
        window.setAnimationBehavior(NSWindowAnimationBehavior::None);
        window.setLevel(NSStatusWindowLevel);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        unsafe {
            window.setReleasedWhenClosed(false);
        }

        let glass = NSGlassEffectView::initWithFrame(NSGlassEffectView::alloc(mtm), frame);
        glass.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        glass.setStyle(NSGlassEffectViewStyle::Regular);
        glass.setCornerRadius(subtitle_corner_radius(frame));

        let content = NSView::initWithFrame(NSView::alloc(mtm), subtitle_content_frame(frame));
        content.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        content.setWantsLayer(true);

        let waveform = NSView::initWithFrame(NSView::alloc(mtm), subtitle_waveform_frame(frame));
        waveform.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMaxXMargin);
        waveform.setWantsLayer(true);

        let mut wave_bars = Vec::with_capacity(SUBTITLE_WAVE_BAR_COUNT);
        for _ in 0..SUBTITLE_WAVE_BAR_COUNT {
            let bar = NSView::initWithFrame(NSView::alloc(mtm), subtitle_waveform_frame(frame));
            bar.setWantsLayer(true);
            waveform.addSubview(&bar);
            wave_bars.push(bar);
        }

        let label = NSTextField::wrappingLabelWithString(&NSString::from_str(""), mtm);
        let font: Retained<NSFont> = unsafe {
            objc2::msg_send![objc2::class!(NSFont), systemFontOfSize: scale_ui(SUBTITLE_FONT_SIZE), weight: -0.2f64]
        };
        label.setFrame(subtitle_label_frame(frame, ""));
        label.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        label.setAlignment(NSTextAlignment::Left);
        label.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        label.setMaximumNumberOfLines(SUBTITLE_MAX_VISIBLE_LINES as _);
        label.setAllowsDefaultTighteningForTruncation(false);
        label.setUsesSingleLineMode(false);
        label.setBordered(false);
        label.setBezeled(false);
        label.setEditable(false);
        label.setSelectable(false);
        label.setDrawsBackground(false);
        label.setBackgroundColor(Some(&transparent));
        label.setFont(Some(&font));

        content.addSubview(&waveform);
        content.addSubview(&label);
        glass.setContentView(Some(&content));
        window.setContentView(Some(&glass));
        window.orderOut(None);

        let overlay = Self {
            window,
            glass,
            content,
            waveform,
            wave_bars,
            label,
            visible: Cell::new(false),
            smoothed_level: Cell::new(0.0),
            wave_phase: Cell::new(0.0),
            fade_seq: Cell::new(0),
            collapse_seq: Cell::new(0),
            collapse_start_width: Cell::new(0.0),
        };
        overlay.apply_visual_style(frame, "");
        overlay.apply_waveform(frame, 0.0);
        overlay
    }

    pub(crate) fn show_waveform_only(&self, mtm: MainThreadMarker) {
        let was_visible = self.visible.replace(true);
        if !was_visible {
            self.smoothed_level.set(0.0);
            self.wave_phase.set(0.0);
            let frame = subtitle_waveform_only_frame(mtm);
            self.window.setFrame_display(frame, false);
            self.apply_visual_style(frame, "");
            self.apply_waveform(frame, 0.0);
            self.label.setStringValue(&NSString::from_str(""));
            let seq = self.fade_seq.get().wrapping_add(1);
            self.fade_seq.set(seq);
            self.window.setAlphaValue(0.0);
            self.window.orderFrontRegardless();
            self.window.displayIfNeeded();
            spawn_subtitle_fade_tick(seq, 0, true);
        }
    }

    pub(crate) fn hide(&self) {
        if !self.visible.replace(false) {
            return;
        }
        self.fade_seq.set(self.fade_seq.get().wrapping_add(1));
        self.collapse_start_width
            .set(self.window.frame().size.width);
        let seq = self.collapse_seq.get().wrapping_add(1);
        self.collapse_seq.set(seq);
        spawn_subtitle_collapse_tick(seq, 0);
    }

    pub(crate) fn advance_fade(&self, seq: u64, step: u64, fade_in: bool) {
        if self.fade_seq.get() != seq {
            return;
        }
        let total = if fade_in {
            SUBTITLE_FADE_IN_STEPS
        } else {
            SUBTITLE_FADE_OUT_STEPS
        };
        let t = ((step + 1) as f64 / total as f64).clamp(0.0, 1.0);
        let alpha = if fade_in {
            1.0 - (1.0 - t).powi(3)
        } else {
            (1.0 - t).powi(2)
        };
        self.window.setAlphaValue(alpha);
        if step + 1 < total {
            spawn_subtitle_fade_tick(seq, step + 1, fade_in);
        } else if !fade_in {
            self.apply_waveform(self.window.frame(), 0.0);
            self.window.orderOut(None);
            self.window.setAlphaValue(1.0);
        }
    }

    pub(crate) fn advance_collapse(&self, _mtm: MainThreadMarker, seq: u64, step: u64) {
        if self.collapse_seq.get() != seq {
            return;
        }
        let t = ((step + 1) as f64 / SUBTITLE_COLLAPSE_STEPS as f64).clamp(0.0, 1.0);

        let text_alpha = (1.0 - t / 0.25).clamp(0.0, 1.0);
        self.label.setAlphaValue(text_alpha);
        let wave_level = self.smoothed_level.get() * (1.0 - t / 0.3).clamp(0.0, 1.0).max(0.0);
        self.smoothed_level.set(wave_level);

        let collapse_t = (t / 0.75).clamp(0.0, 1.0);
        let spring = spring_overshoot(collapse_t);
        let target_w = scale_ui(SUBTITLE_CAPSULE_HEIGHT);
        let start_w = self.collapse_start_width.get();
        let width = start_w + (target_w - start_w) * spring;

        let alpha = if t > 0.75 {
            ((1.0 - t) / 0.25).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.window.setAlphaValue(alpha);

        let current = self.window.frame();
        let height = current.size.height;
        let center_x = current.origin.x + current.size.width / 2.0;
        let new_frame = NSRect::new(
            NSPoint::new(center_x - width / 2.0, current.origin.y),
            NSSize::new(width, height),
        );
        self.window.setFrame_display(new_frame, false);
        self.apply_visual_style(new_frame, "");
        self.apply_waveform(new_frame, wave_level);
        self.window.displayIfNeeded();

        if step + 1 < SUBTITLE_COLLAPSE_STEPS {
            spawn_subtitle_collapse_tick(seq, step + 1);
        } else {
            self.smoothed_level.set(0.0);
            self.wave_phase.set(0.0);
            self.label.setAlphaValue(1.0);
            self.window.orderOut(None);
            self.window.setAlphaValue(1.0);
        }
    }

    pub(crate) fn update_level(&self, level: f64) {
        if !self.visible.get() {
            return;
        }
        let current = self.smoothed_level.get();
        let next = if level <= SUBTITLE_WAVE_SILENCE_LEVEL {
            current * 0.42
        } else {
            let factor = if level > current {
                SUBTITLE_WAVE_ATTACK_FACTOR
            } else {
                SUBTITLE_WAVE_RELEASE_FACTOR
            };
            current + (level - current) * factor
        };
        let next = if next < 0.01 {
            0.0
        } else {
            next.clamp(0.0, 1.0)
        };
        self.smoothed_level.set(next);
        if next > 0.0 {
            self.wave_phase
                .set(self.wave_phase.get() + 0.70 + next * 1.15);
        }
        self.apply_waveform(self.window.frame(), next);
    }

    fn apply_visual_style(&self, frame: NSRect, text: &str) {
        let radius = subtitle_corner_radius(frame);
        self.glass.setFrame(subtitle_content_frame(frame));
        self.glass.setCornerRadius(radius);
        self.content.setFrame(subtitle_content_frame(frame));
        self.waveform.setFrame(subtitle_waveform_frame(frame));
        self.label.setFrame(subtitle_label_frame(frame, text));
        let theme = subtitle_theme();
        if subtitle_should_reduce_transparency() {
            self.glass.setStyle(NSGlassEffectViewStyle::Regular);
            self.glass.setAlphaValue(0.96);
            let (_wave, wave_glow, foreground, fill, border, shadow) = match theme {
                SubtitleTheme::Light => (
                    NSColor::colorWithCalibratedWhite_alpha(0.12, 0.82).CGColor(),
                    NSColor::colorWithCalibratedWhite_alpha(0.0, 0.04).CGColor(),
                    NSColor::colorWithCalibratedWhite_alpha(0.18, 0.78),
                    NSColor::colorWithCalibratedRed_green_blue_alpha(0.97, 0.97, 0.955, 0.80)
                        .CGColor(),
                    NSColor::colorWithCalibratedWhite_alpha(1.0, 0.12).CGColor(),
                    NSColor::colorWithCalibratedWhite_alpha(0.0, 0.05).CGColor(),
                ),
                SubtitleTheme::Dark => (
                    NSColor::colorWithCalibratedWhite_alpha(0.96, 0.86).CGColor(),
                    NSColor::colorWithCalibratedWhite_alpha(1.0, 0.08).CGColor(),
                    NSColor::colorWithCalibratedWhite_alpha(0.96, 0.82),
                    NSColor::colorWithCalibratedRed_green_blue_alpha(0.12, 0.13, 0.15, 0.76)
                        .CGColor(),
                    NSColor::colorWithCalibratedWhite_alpha(1.0, 0.08).CGColor(),
                    NSColor::colorWithCalibratedWhite_alpha(0.0, 0.14).CGColor(),
                ),
            };
            self.label.setTextColor(Some(&foreground));
            if let Some(layer) = self.content.layer() {
                layer.setMasksToBounds(false);
                layer.setBackgroundColor(Some(&fill));
                layer.setCornerRadius(radius);
                layer.setBorderWidth(scale_ui(0.6));
                layer.setBorderColor(Some(&border));
                layer.setShadowColor(Some(&shadow));
                layer.setShadowOpacity(match theme {
                    SubtitleTheme::Light => 0.18,
                    SubtitleTheme::Dark => 0.32,
                });
                layer.setShadowRadius(match theme {
                    SubtitleTheme::Light => scale_ui(8.0),
                    SubtitleTheme::Dark => scale_ui(12.0),
                });
            }
            for (idx, bar) in self.wave_bars.iter().enumerate() {
                if let Some(layer) = bar.layer() {
                    let fill = match theme {
                        SubtitleTheme::Light => {
                            if idx % 2 == 0 {
                                NSColor::colorWithCalibratedRed_green_blue_alpha(
                                    0.15, 0.45, 0.95, 0.82,
                                )
                                .CGColor()
                            } else {
                                NSColor::colorWithCalibratedRed_green_blue_alpha(
                                    0.15, 0.45, 0.95, 0.42,
                                )
                                .CGColor()
                            }
                        }
                        SubtitleTheme::Dark => {
                            if idx % 2 == 0 {
                                NSColor::colorWithCalibratedRed_green_blue_alpha(
                                    0.40, 0.68, 1.0, 0.86,
                                )
                                .CGColor()
                            } else {
                                NSColor::colorWithCalibratedRed_green_blue_alpha(
                                    0.40, 0.68, 1.0, 0.48,
                                )
                                .CGColor()
                            }
                        }
                    };
                    layer.setBackgroundColor(Some(&fill));
                    layer.setCornerRadius(scale_ui(SUBTITLE_WAVE_BAR_WIDTH) / 2.0);
                    layer.setShadowColor(Some(&wave_glow));
                    layer.setShadowOpacity(match theme {
                        SubtitleTheme::Light => 0.12,
                        SubtitleTheme::Dark => 0.28,
                    });
                    layer.setShadowRadius(match theme {
                        SubtitleTheme::Light => scale_ui(2.0),
                        SubtitleTheme::Dark => scale_ui(4.0),
                    });
                }
            }
            return;
        }

        self.glass.setStyle(NSGlassEffectViewStyle::Clear);
        self.glass.setAlphaValue(0.92);
        let (_wave, wave_glow, foreground, fill, border, shadow, tint) = match theme {
            SubtitleTheme::Light => (
                NSColor::colorWithCalibratedWhite_alpha(0.12, 0.74).CGColor(),
                NSColor::colorWithCalibratedWhite_alpha(0.0, 0.04).CGColor(),
                NSColor::colorWithCalibratedWhite_alpha(0.10, 0.88),
                NSColor::colorWithCalibratedWhite_alpha(1.0, 0.52).CGColor(),
                NSColor::colorWithCalibratedWhite_alpha(0.0, 0.06).CGColor(),
                NSColor::colorWithCalibratedWhite_alpha(0.0, 0.10).CGColor(),
                NSColor::colorWithCalibratedWhite_alpha(1.0, 0.12),
            ),
            SubtitleTheme::Dark => (
                NSColor::colorWithCalibratedWhite_alpha(0.96, 0.78).CGColor(),
                NSColor::colorWithCalibratedWhite_alpha(1.0, 0.10).CGColor(),
                NSColor::colorWithCalibratedWhite_alpha(0.97, 0.90),
                NSColor::colorWithCalibratedWhite_alpha(0.04, 0.48).CGColor(),
                NSColor::colorWithCalibratedWhite_alpha(1.0, 0.08).CGColor(),
                NSColor::colorWithCalibratedWhite_alpha(0.0, 0.14).CGColor(),
                NSColor::colorWithCalibratedWhite_alpha(0.0, 0.10),
            ),
        };
        self.glass.setTintColor(Some(&tint));
        self.label.setTextColor(Some(&foreground));
        if let Some(layer) = self.content.layer() {
            layer.setMasksToBounds(false);
            layer.setBackgroundColor(Some(&fill));
            layer.setCornerRadius(radius);
            layer.setBorderWidth(scale_ui(0.55));
            layer.setBorderColor(Some(&border));
            layer.setShadowColor(Some(&shadow));
            layer.setShadowOpacity(match theme {
                SubtitleTheme::Light => 0.22,
                SubtitleTheme::Dark => 0.32,
            });
            layer.setShadowRadius(match theme {
                SubtitleTheme::Light => scale_ui(12.0),
                SubtitleTheme::Dark => scale_ui(14.0),
            });
        }
        for (idx, bar) in self.wave_bars.iter().enumerate() {
            if let Some(layer) = bar.layer() {
                let fill = match theme {
                    SubtitleTheme::Light => {
                        if idx % 2 == 0 {
                            NSColor::colorWithCalibratedRed_green_blue_alpha(0.15, 0.45, 0.95, 0.86)
                                .CGColor()
                        } else {
                            NSColor::colorWithCalibratedRed_green_blue_alpha(0.15, 0.45, 0.95, 0.44)
                                .CGColor()
                        }
                    }
                    SubtitleTheme::Dark => {
                        if idx % 2 == 0 {
                            NSColor::colorWithCalibratedRed_green_blue_alpha(0.42, 0.70, 1.0, 0.88)
                                .CGColor()
                        } else {
                            NSColor::colorWithCalibratedRed_green_blue_alpha(0.42, 0.70, 1.0, 0.50)
                                .CGColor()
                        }
                    }
                };
                layer.setBackgroundColor(Some(&fill));
                layer.setCornerRadius(scale_ui(SUBTITLE_WAVE_BAR_WIDTH) / 2.0);
                layer.setShadowColor(Some(&wave_glow));
                layer.setShadowOpacity(match theme {
                    SubtitleTheme::Light => 0.10,
                    SubtitleTheme::Dark => 0.24,
                });
                layer.setShadowRadius(match theme {
                    SubtitleTheme::Light => scale_ui(2.0),
                    SubtitleTheme::Dark => scale_ui(4.0),
                });
            }
        }
    }

    fn apply_waveform(&self, frame: NSRect, level: f64) {
        let wave_frame = subtitle_waveform_frame(frame);
        self.waveform.setFrame(wave_frame);
        let bounds = wave_frame.size;
        let bar_widths = [scale_ui(SUBTITLE_WAVE_BAR_WIDTH); SUBTITLE_WAVE_BAR_COUNT];
        let total_width = bar_widths.iter().sum::<f64>()
            + scale_ui(SUBTITLE_WAVE_BAR_GAP) * (self.wave_bars.len().saturating_sub(1)) as f64;
        let start_x = ((bounds.width - total_width) / 2.0).max(0.0);
        let phase = self.wave_phase.get();
        let energy = level.clamp(0.0, 1.0);
        let idle_pattern: [f64; SUBTITLE_WAVE_BAR_COUNT] = [0.16, 0.30, 0.22, 0.40, 0.16];
        let profile: [f64; SUBTITLE_WAVE_BAR_COUNT] = [0.30, 0.72, 0.48, 0.86, 0.28];
        let offsets: [f64; SUBTITLE_WAVE_BAR_COUNT] = [0.0, 1.257, 2.513, 0.628, 1.885];

        let mut x = start_x;
        for (idx, bar) in self.wave_bars.iter().enumerate() {
            let width = bar_widths[idx];
            let fraction = if energy <= 0.001 {
                idle_pattern[idx].max(SUBTITLE_WAVE_MIN_FRACTION)
            } else {
                let motion = (0.26 + energy * 1.10).clamp(0.26, 1.0);
                let pulse = ((phase * 1.75 + offsets[idx]).sin() * 0.5 + 0.5).powf(0.66);
                let sway = ((phase * 2.45 + offsets[idx] * 1.12).sin() * 0.5 + 0.5).powf(0.90);
                let bounce = ((phase * 3.10 + offsets[idx] * 0.54).cos() * 0.5 + 0.5).powf(1.08);
                let base = idle_pattern[idx] * (0.50 + motion * 0.24);
                let lift = motion * profile[idx] * (0.26 + 0.88 * pulse);
                let accent = motion * (0.14 * sway + 0.10 * bounce);
                (base + lift + accent).clamp(SUBTITLE_WAVE_MIN_FRACTION, 0.96)
            };
            let height = (bounds.height * fraction).clamp(scale_ui(6.0), bounds.height);
            let y = (bounds.height - height) / 2.0;
            bar.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)));
            if let Some(layer) = bar.layer() {
                layer.setCornerRadius(width / 2.0);
            }
            x += width + scale_ui(SUBTITLE_WAVE_BAR_GAP);
        }
    }
}

pub(crate) fn subtitle_session_reset() {
    let mut state = SUBTITLE_SESSION_STATE
        .lock()
        .expect("subtitle session mutex poisoned");
    state.committed_text.clear();
    state.live_text.clear();
}

pub(crate) fn reset_subtitle_level_dispatch() {
    SUBTITLE_LEVEL_BITS.store(0.0f64.to_bits(), Ordering::SeqCst);
    SUBTITLE_LEVEL_SEQ.store(0, Ordering::SeqCst);
    SUBTITLE_LEVEL_DISPATCH_PENDING.store(false, Ordering::SeqCst);
}

fn subtitle_char_count(text: &str) -> usize {
    text.chars().count()
}

fn subtitle_is_sentence_break(ch: char) -> bool {
    matches!(ch, '。' | '！' | '？' | '!' | '?' | '；' | ';')
}

fn subtitle_display_text(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    for ch in trimmed.chars() {
        current.push(ch);
        if subtitle_is_sentence_break(ch) {
            let line = current.trim();
            if !line.is_empty() {
                lines.push(line.to_string());
            }
            current.clear();
        }
    }
    let tail = current.trim();
    if !tail.is_empty() {
        lines.push(tail.to_string());
    }
    if lines.is_empty() {
        return trimmed.to_string();
    }
    let visible = lines.len().min(SUBTITLE_MAX_VISIBLE_LINES);
    lines[lines.len() - visible..]
        .iter()
        .fold(String::new(), |acc, line| subtitle_join_text(&acc, line))
}

fn subtitle_join_text(prefix: &str, suffix: &str) -> String {
    let left = prefix.trim();
    let right = suffix.trim();
    if left.is_empty() {
        return right.to_string();
    }
    if right.is_empty() {
        return left.to_string();
    }
    let need_space = left
        .chars()
        .last()
        .zip(right.chars().next())
        .map(|(a, b)| a.is_ascii_alphanumeric() && b.is_ascii_alphanumeric())
        .unwrap_or(false);
    if need_space {
        format!("{left} {right}")
    } else {
        format!("{left}{right}")
    }
}

fn subtitle_split_committed_tail(text: &str) -> (String, String) {
    let normalized = text.trim();
    if normalized.is_empty() {
        return (String::new(), String::new());
    }
    let mut last_break_end = 0usize;
    for (idx, ch) in normalized.char_indices() {
        if subtitle_is_sentence_break(ch) {
            last_break_end = idx + ch.len_utf8();
        }
    }
    if last_break_end == 0 {
        return (String::new(), normalized.to_string());
    }
    let committed = normalized[..last_break_end].trim().to_string();
    let tail = normalized[last_break_end..].trim().to_string();
    (committed, tail)
}

fn subtitle_state_full_text(state: &SubtitleSessionState) -> String {
    subtitle_join_text(&state.committed_text, &state.live_text)
}

fn subtitle_strip_sentence_breaks(text: &str) -> String {
    text.chars()
        .filter(|ch| !subtitle_is_sentence_break(*ch))
        .collect()
}

fn subtitle_prefix_looks_like_full_transcript(prefix: &str, full: &str) -> bool {
    let prefix_trimmed = prefix.trim();
    let full_trimmed = full.trim();
    if prefix_trimmed.is_empty() || full_trimmed.is_empty() {
        return false;
    }
    if full_trimmed.starts_with(prefix_trimmed) {
        return true;
    }
    // Strip sentence-break punctuation before comparing so that twopass
    // refinement (e.g. "你好吗？" vs "你好吗看看世界") doesn't cause a
    // prefix mismatch at the punctuation point.
    let prefix_stripped = subtitle_strip_sentence_breaks(prefix_trimmed);
    let full_stripped = subtitle_strip_sentence_breaks(full_trimmed);
    if !prefix_stripped.is_empty() && full_stripped.starts_with(&prefix_stripped) {
        return true;
    }
    let shared = shared_prefix_chars(&prefix_stripped, &full_stripped);
    let prefix_len = subtitle_char_count(&prefix_stripped);
    let full_len = subtitle_char_count(&full_stripped);
    shared >= 3 && shared.saturating_mul(2) >= prefix_len.min(full_len).max(2)
}

fn subtitle_session_snapshot(state: &SubtitleSessionState) -> SubtitleTranscriptSnapshot {
    let commit_text = subtitle_state_full_text(state);
    let display_text = subtitle_display_text(&commit_text);
    SubtitleTranscriptSnapshot {
        display_text,
        commit_text,
    }
}

fn subtitle_state_apply_partial(state: &mut SubtitleSessionState, normalized: &str) {
    if normalized.is_empty() {
        return;
    }
    let previous_full = subtitle_state_full_text(state);
    let check_committed =
        subtitle_prefix_looks_like_full_transcript(&state.committed_text, normalized);
    let check_prev = subtitle_prefix_looks_like_full_transcript(&previous_full, normalized);
    let prefers_full_transcript =
        state.committed_text.trim().is_empty() || check_committed || check_prev;
    let next_full = if prefers_full_transcript {
        normalized.to_string()
    } else {
        subtitle_join_text(&state.committed_text, normalized)
    };
    let (committed_text, live_text) = subtitle_split_committed_tail(&next_full);
    state.committed_text = committed_text;
    state.live_text = live_text;
}

pub(crate) fn subtitle_session_apply_partial(text: &str) -> SubtitleTranscriptSnapshot {
    let normalized = text.trim();
    let mut state = SUBTITLE_SESSION_STATE
        .lock()
        .expect("subtitle session mutex poisoned");
    if normalized.is_empty() {
        return subtitle_session_snapshot(&state);
    }
    subtitle_state_apply_partial(&mut state, normalized);
    subtitle_session_snapshot(&state)
}

fn subtitle_state_apply_final(state: &mut SubtitleSessionState, normalized: &str) {
    let resolved = if normalized.is_empty() {
        subtitle_state_full_text(state)
    } else {
        normalized.to_string()
    };
    let (committed_text, live_text) = subtitle_split_committed_tail(&resolved);
    state.committed_text = committed_text;
    state.live_text = live_text;
}

pub(crate) fn subtitle_session_apply_final(text: &str) -> SubtitleTranscriptSnapshot {
    let normalized = text.trim();
    let mut state = SUBTITLE_SESSION_STATE
        .lock()
        .expect("subtitle session mutex poisoned");

    subtitle_state_apply_final(&mut state, normalized);
    subtitle_session_snapshot(&state)
}

fn subtitle_text_units(text: &str) -> f64 {
    let units = text
        .chars()
        .map(|ch| {
            if ch.is_ascii_whitespace() {
                0.35
            } else if ch.is_ascii() {
                0.58
            } else {
                1.0
            }
        })
        .sum::<f64>();
    units.max(1.0)
}

pub(crate) fn subtitle_should_reduce_transparency() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency()
}

fn subtitle_theme() -> SubtitleTheme {
    let mtm = MainThreadMarker::new().expect("main thread marker");
    let app = NSApplication::sharedApplication(mtm);
    let appearance = app.effectiveAppearance();
    let choices = NSArray::from_slice(&[unsafe { NSAppearanceNameAqua }, unsafe {
        NSAppearanceNameDarkAqua
    }]);
    let Some(best_match) = appearance.bestMatchFromAppearancesWithNames(&choices) else {
        return SubtitleTheme::Dark;
    };
    if best_match.isEqualToString(unsafe { NSAppearanceNameDarkAqua }) {
        SubtitleTheme::Dark
    } else {
        SubtitleTheme::Light
    }
}

fn subtitle_waveform_only_frame(mtm: MainThreadMarker) -> NSRect {
    let size = scale_ui(SUBTITLE_CAPSULE_HEIGHT);
    let default_rect = NSRect::new(
        NSPoint::new(scale_ui(120.0), scale_ui(SUBTITLE_BOTTOM_MARGIN)),
        NSSize::new(size, size),
    );
    let Some(screen) = NSScreen::mainScreen(mtm) else {
        return default_rect;
    };
    let visible = screen.visibleFrame();
    let x = visible.origin.x + ((visible.size.width - size).max(0.0) / 2.0);
    let y = visible.origin.y + scale_ui(SUBTITLE_BOTTOM_MARGIN);
    NSRect::new(NSPoint::new(x, y), NSSize::new(size, size))
}

fn subtitle_window_frame(mtm: MainThreadMarker, text: &str) -> NSRect {
    let text_units = subtitle_text_units(text);
    let preferred_text_width = ((text_units + 1.2)
        * (scale_ui(SUBTITLE_FONT_SIZE) * SUBTITLE_TEXT_WIDTH_FACTOR))
        .max(scale_ui(84.0));
    let width = (preferred_text_width
        + scale_ui(SUBTITLE_HORIZONTAL_PADDING) * 2.0
        + scale_ui(SUBTITLE_WAVE_WIDTH)
        + scale_ui(SUBTITLE_CONTENT_GAP))
    .max(scale_ui(SUBTITLE_MIN_WIDTH));
    let label_width = (width
        - scale_ui(SUBTITLE_HORIZONTAL_PADDING) * 2.0
        - scale_ui(SUBTITLE_WAVE_WIDTH)
        - scale_ui(SUBTITLE_CONTENT_GAP))
    .max(scale_ui(40.0));
    let line_count = subtitle_wrapped_line_count(text, label_width);
    let label_height =
        (line_count as f64 * scale_ui(SUBTITLE_LINE_HEIGHT)).max(scale_ui(SUBTITLE_LABEL_HEIGHT));
    let height = (label_height + scale_ui(SUBTITLE_VERTICAL_PADDING) * 2.0 + scale_ui(14.0))
        .max(scale_ui(SUBTITLE_CAPSULE_HEIGHT));
    subtitle_window_frame_for_size(mtm, width, height, None)
}

fn subtitle_window_frame_for_size(
    mtm: MainThreadMarker,
    width: f64,
    height: f64,
    origin_override: Option<NSPoint>,
) -> NSRect {
    let default_rect = NSRect::new(
        NSPoint::new(scale_ui(120.0), scale_ui(SUBTITLE_BOTTOM_MARGIN)),
        NSSize::new(
            scale_ui(SUBTITLE_MIN_WIDTH),
            scale_ui(SUBTITLE_CAPSULE_HEIGHT),
        ),
    );
    let Some(screen) = NSScreen::mainScreen(mtm) else {
        return default_rect;
    };
    let visible = screen.visibleFrame();
    let max_available_width = (visible.size.width - scale_ui(40.0)).max(scale_ui(200.0));
    let max_width = (visible.size.width * SUBTITLE_WIDTH_RATIO * ui_scale())
        .max(scale_ui(SUBTITLE_MIN_WIDTH))
        .min(scale_ui(SUBTITLE_MAX_WIDTH))
        .min(max_available_width);
    let width = width.max(scale_ui(SUBTITLE_MIN_WIDTH)).min(max_width);
    let height = height
        .max(scale_ui(SUBTITLE_CAPSULE_HEIGHT))
        .min(visible.size.height - scale_ui(SUBTITLE_BOTTOM_MARGIN) - scale_ui(16.0));
    let max_x = visible.origin.x + visible.size.width - width;
    let max_y = visible.origin.y + visible.size.height - height;
    let default_x = visible.origin.x + ((visible.size.width - width).max(0.0) / 2.0);
    let default_y = visible.origin.y + scale_ui(SUBTITLE_BOTTOM_MARGIN);
    let x = origin_override
        .map(|origin| {
            origin
                .x
                .clamp(visible.origin.x, max_x.max(visible.origin.x))
        })
        .unwrap_or(default_x);
    let y = origin_override
        .map(|origin| {
            origin
                .y
                .clamp(visible.origin.y, max_y.max(visible.origin.y))
        })
        .unwrap_or(default_y);
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

fn subtitle_content_frame(window_frame: NSRect) -> NSRect {
    NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(window_frame.size.width, window_frame.size.height),
    )
}

fn subtitle_corner_radius(window_frame: NSRect) -> f64 {
    (window_frame.size.height * 0.5).clamp(scale_ui(18.0), scale_ui(22.0))
}

fn spring_overshoot(t: f64) -> f64 {
    if t >= 1.0 {
        return 1.0;
    }
    1.0 - (-8.0 * t).exp() * (std::f64::consts::PI * 2.0 * t).cos()
}

fn subtitle_waveform_frame(window_frame: NSRect) -> NSRect {
    let wave_w = scale_ui(SUBTITLE_WAVE_WIDTH);
    let wave_h = scale_ui(SUBTITLE_WAVE_HEIGHT);
    let x = if window_frame.size.width <= scale_ui(SUBTITLE_CAPSULE_HEIGHT) * 1.2 {
        (window_frame.size.width - wave_w) / 2.0
    } else {
        scale_ui(SUBTITLE_HORIZONTAL_PADDING)
    };
    NSRect::new(
        NSPoint::new(x, ((window_frame.size.height - wave_h) / 2.0).max(0.0)),
        NSSize::new(wave_w, wave_h),
    )
}

fn subtitle_wrapped_line_count(text: &str, label_width: f64) -> usize {
    let units_per_line =
        (label_width / (scale_ui(SUBTITLE_FONT_SIZE) * SUBTITLE_WRAP_UNIT_FACTOR)).max(6.0);
    let mut total_lines = 0usize;
    for raw_line in text.lines() {
        let units = subtitle_text_units(raw_line);
        let wrapped = (units / units_per_line).ceil() as usize;
        total_lines = total_lines.saturating_add(wrapped.max(1));
    }
    total_lines.clamp(1, SUBTITLE_MAX_VISIBLE_LINES)
}

fn subtitle_label_frame(window_frame: NSRect, text: &str) -> NSRect {
    let label_width = (window_frame.size.width
        - scale_ui(SUBTITLE_HORIZONTAL_PADDING) * 2.0
        - scale_ui(SUBTITLE_WAVE_WIDTH)
        - scale_ui(SUBTITLE_CONTENT_GAP))
    .max(scale_ui(40.0));
    let line_count = subtitle_wrapped_line_count(text, label_width);
    let label_height = (line_count as f64 * scale_ui(SUBTITLE_LINE_HEIGHT))
        .max(scale_ui(SUBTITLE_LABEL_HEIGHT))
        .min(window_frame.size.height);
    NSRect::new(
        NSPoint::new(
            scale_ui(SUBTITLE_HORIZONTAL_PADDING)
                + scale_ui(SUBTITLE_WAVE_WIDTH)
                + scale_ui(SUBTITLE_CONTENT_GAP),
            ((window_frame.size.height - label_height) / 2.0)
                .max(scale_ui(SUBTITLE_VERTICAL_PADDING)),
        ),
        NSSize::new(label_width, label_height),
    )
}

fn next_subtitle_sequence() -> u64 {
    SUBTITLE_UPDATE_SEQ
        .fetch_add(1, Ordering::SeqCst)
        .saturating_add(1)
}

fn dispatch_subtitle_on_main<F>(work: F)
where
    F: FnOnce() + Send + 'static,
{
    dispatch_main(work);
}

pub(crate) fn dispatch_subtitle_update(text: String, final_result: bool) {
    if !SHOW_SUBTITLE_OVERLAY.load(Ordering::SeqCst) {
        return;
    }
    let sequence = next_subtitle_sequence();

    dispatch_subtitle_on_main(move || {
        let _ = with_controller_on_main(|controller, mtm| {
            if let Some(overlay) = &controller.subtitle_overlay {
                if !overlay.visible.get() {
                    overlay.visible.set(true);
                    overlay.window.setAlphaValue(1.0);
                }
                let frame = subtitle_window_frame(mtm, &text);
                overlay.window.setFrame_display(frame, false);
                overlay
                    .label
                    .setStringValue(&NSString::from_str(text.trim()));
                overlay.window.orderFrontRegardless();
                overlay.window.display();
                objc2_quartz_core::CATransaction::flush();
            }
        });
    });

    if final_result {
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(SUBTITLE_HIDE_DELAY_MS));
            if SHUTTING_DOWN.load(Ordering::SeqCst) {
                return;
            }
            if SUBTITLE_UPDATE_SEQ.load(Ordering::SeqCst) != sequence {
                return;
            }
            dispatch_subtitle_hide_if_current(sequence);
        });
    }
}

pub(crate) fn dispatch_subtitle_show_waveform_only() {
    if !SHOW_SUBTITLE_OVERLAY.load(Ordering::SeqCst) {
        return;
    }
    dispatch_subtitle_on_main(move || {
        let _ = with_controller_on_main(|controller, mtm| {
            controller.update_subtitle_waveform_only(mtm);
        });
    });
}

pub(crate) fn dispatch_subtitle_level(level: f64) {
    if !SHOW_SUBTITLE_OVERLAY.load(Ordering::SeqCst) {
        return;
    }
    SUBTITLE_LEVEL_BITS.store(level.to_bits(), Ordering::SeqCst);
    let scheduled_seq = SUBTITLE_LEVEL_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    if SUBTITLE_LEVEL_DISPATCH_PENDING.swap(true, Ordering::SeqCst) {
        return;
    }
    dispatch_subtitle_on_main(move || {
        let level = f64::from_bits(SUBTITLE_LEVEL_BITS.load(Ordering::SeqCst));
        let observed_seq = SUBTITLE_LEVEL_SEQ.load(Ordering::SeqCst);
        SUBTITLE_LEVEL_DISPATCH_PENDING.store(false, Ordering::SeqCst);
        let _ = with_controller_on_main(|controller, _mtm| {
            controller.update_subtitle_level(level);
        });
        if observed_seq != scheduled_seq {
            dispatch_subtitle_level(f64::from_bits(SUBTITLE_LEVEL_BITS.load(Ordering::SeqCst)));
        }
    });
}

fn spawn_subtitle_fade_tick(seq: u64, step: u64, fade_in: bool) {
    let interval = if fade_in {
        SUBTITLE_FADE_IN_MS / SUBTITLE_FADE_IN_STEPS
    } else {
        SUBTITLE_FADE_OUT_MS / SUBTITLE_FADE_OUT_STEPS
    };
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(interval));
        if SHUTTING_DOWN.load(Ordering::SeqCst) {
            return;
        }
        dispatch_subtitle_fade_tick(seq, step, fade_in);
    });
}

fn dispatch_subtitle_fade_tick(seq: u64, step: u64, fade_in: bool) {
    if !SHOW_SUBTITLE_OVERLAY.load(Ordering::SeqCst) {
        return;
    }
    dispatch_subtitle_on_main(move || {
        let _ = with_controller_on_main(|controller, _mtm| {
            controller.advance_subtitle_fade(seq, step, fade_in);
        });
    });
}

fn spawn_subtitle_collapse_tick(seq: u64, step: u64) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(SUBTITLE_COLLAPSE_TICK_MS));
        if SHUTTING_DOWN.load(Ordering::SeqCst) {
            return;
        }
        dispatch_subtitle_collapse_tick(seq, step);
    });
}

fn dispatch_subtitle_collapse_tick(seq: u64, step: u64) {
    if !SHOW_SUBTITLE_OVERLAY.load(Ordering::SeqCst) {
        return;
    }
    dispatch_subtitle_on_main(move || {
        let _ = with_controller_on_main(|controller, mtm| {
            controller.advance_subtitle_collapse(mtm, seq, step);
        });
    });
}

pub(crate) fn dispatch_subtitle_hide() {
    if !SHOW_SUBTITLE_OVERLAY.load(Ordering::SeqCst) {
        return;
    }
    let sequence = next_subtitle_sequence();
    dispatch_subtitle_hide_if_current(sequence);
}

fn dispatch_subtitle_hide_if_current(sequence: u64) {
    dispatch_subtitle_on_main(move || {
        if SUBTITLE_UPDATE_SEQ.load(Ordering::SeqCst) != sequence {
            return;
        }
        let _ = with_controller_on_main(|controller, _mtm| {
            controller.hide_subtitle();
        });
    });
}

pub(crate) fn subtitle_audio_meter_level(peak: f32, rms: f32) -> f64 {
    let raw = ((peak.max(0.0) as f64) * SUBTITLE_WAVE_PEAK_SCALE)
        .max((rms.max(0.0) as f64) * SUBTITLE_WAVE_RMS_SCALE)
        .clamp(0.0, 1.0);
    if raw <= SUBTITLE_WAVE_SILENCE_LEVEL {
        return 0.0;
    }
    let normalized =
        ((raw - SUBTITLE_WAVE_SILENCE_LEVEL) / (1.0 - SUBTITLE_WAVE_SILENCE_LEVEL)).clamp(0.0, 1.0);
    (normalized * SUBTITLE_WAVE_INPUT_GAIN)
        .clamp(0.0, 1.0)
        .powf(SUBTITLE_WAVE_VISUAL_EXPONENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitle_keeps_committed_sentence_when_next_partial_is_tail_only() {
        let mut state = SubtitleSessionState::default();

        subtitle_state_apply_partial(&mut state, "锄禾日当午，汗滴禾下土。");
        subtitle_state_apply_partial(&mut state, "谁知盘中餐");

        let snapshot = subtitle_session_snapshot(&state);
        assert_eq!(snapshot.commit_text, "锄禾日当午，汗滴禾下土。谁知盘中餐");
        assert_eq!(snapshot.display_text, "锄禾日当午，汗滴禾下土。谁知盘中餐");
    }

    #[test]
    fn subtitle_accepts_full_transcript_after_tail_only_partial() {
        let mut state = SubtitleSessionState::default();

        subtitle_state_apply_partial(&mut state, "锄禾日当午，汗滴禾下土。");
        subtitle_state_apply_partial(&mut state, "谁知盘中餐");
        subtitle_state_apply_partial(&mut state, "锄禾日当午，汗滴禾下土。谁知盘中餐，粒粒皆辛苦");

        let snapshot = subtitle_session_snapshot(&state);
        assert_eq!(
            snapshot.commit_text,
            "锄禾日当午，汗滴禾下土。谁知盘中餐，粒粒皆辛苦"
        );
        assert_eq!(
            snapshot.display_text,
            "锄禾日当午，汗滴禾下土。谁知盘中餐，粒粒皆辛苦"
        );
    }

    #[test]
    fn subtitle_final_keeps_full_text() {
        let mut state = SubtitleSessionState::default();

        subtitle_state_apply_partial(&mut state, "锄禾日当午，汗滴禾下土。");
        subtitle_state_apply_partial(&mut state, "谁知盘中餐");
        subtitle_state_apply_final(
            &mut state,
            "锄禾日当午，汗滴禾下土。谁知盘中餐，粒粒皆辛苦。",
        );

        let snapshot = subtitle_session_snapshot(&state);
        assert_eq!(
            snapshot.commit_text,
            "锄禾日当午，汗滴禾下土。谁知盘中餐，粒粒皆辛苦。"
        );
        assert_eq!(
            snapshot.display_text,
            "锄禾日当午，汗滴禾下土。谁知盘中餐，粒粒皆辛苦。"
        );
    }
}
