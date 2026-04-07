use std::ptr;
use std::sync::atomic::Ordering;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSStatusBar};
use objc2_avf_audio::AVAudioEngine;
use objc2_core_foundation::{kCFRunLoopCommonModes, CFMachPort, CFRunLoop};
use objc2_core_graphics::{
    CGEvent, CGEventMask, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
};

use crate::backend::{spawn_backend_worker, BackendCommand, TYPE_PARTIAL};
use crate::config::{apply_shortcut_config, load_context_config};
use crate::shortcut::event_tap_callback;
use crate::state::{SHOW_SUBTITLE_OVERLAY, SHUTTING_DOWN, UI_SCALE_BITS, VERBOSE};
use crate::status_menu::install_status_menu;
use crate::subtitle::{subtitle_should_reduce_transparency, SubtitleOverlay};
use crate::ui::{install_controller, set_status_icon, uninstall_controller, Controller};
use crate::{Args, HELPER_VERSION};

pub(crate) fn run_app(args: Args) {
    let ui_scale = if args.ui_scale.is_finite() {
        args.ui_scale.clamp(0.5, 6.0)
    } else {
        1.0
    };
    UI_SCALE_BITS.store(ui_scale.to_bits(), Ordering::SeqCst);
    VERBOSE.store(args.verbose, Ordering::SeqCst);
    TYPE_PARTIAL.store(args.type_partial, Ordering::SeqCst);
    SHOW_SUBTITLE_OVERLAY.store(args.subtitle_overlay, Ordering::SeqCst);
    {
        let cfg = load_context_config();
        apply_shortcut_config(&cfg.shortcut);
    }

    let mtm = MainThreadMarker::new().expect("must run on main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let backend_tx = spawn_backend_worker(args.server_url.clone(), args.partial_interval_ms);

    let audio_engine = unsafe { AVAudioEngine::new() };
    let event_mask: CGEventMask =
        (1 << CGEventType::FlagsChanged.0) | (1 << CGEventType::KeyDown.0);
    let tap = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::HIDEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            event_mask,
            Some(event_tap_callback),
            ptr::null_mut(),
        )
    }
    .expect("failed to create event tap — grant Accessibility permission");

    let run_loop_source = CFMachPort::new_run_loop_source(None, Some(&tap), 0)
        .expect("failed to create run loop source");
    unsafe {
        let run_loop = CFRunLoop::current().expect("no current run loop");
        run_loop.add_source(Some(&run_loop_source), kCFRunLoopCommonModes);
    }

    let status_bar = NSStatusBar::systemStatusBar();
    let status_item = status_bar.statusItemWithLength(-1.0);
    set_status_icon(&status_item, false, mtm);
    let _menu_delegate = install_status_menu(mtm, &status_item);

    let controller = Box::new(Controller::new(
        audio_engine,
        status_item,
        backend_tx.clone(),
        if args.subtitle_overlay {
            Some(SubtitleOverlay::new(mtm))
        } else {
            None
        },
    ));
    install_controller(controller);

    if args.subtitle_overlay {
        if subtitle_should_reduce_transparency() {
            eprintln!(
                "[shuo-engine] subtitle overlay enabled (fallback style: macOS Reduce Transparency is on, scale={ui_scale:.2})"
            );
        } else {
            eprintln!("[shuo-engine] subtitle overlay enabled (glass style, scale={ui_scale:.2})");
        }
    }

    eprintln!(
        "[shuo-engine] ready v{} - hold right Command alone to dictate, release to type{}",
        HELPER_VERSION,
        if args.subtitle_overlay {
            "; live subtitles appear at the bottom"
        } else {
            ""
        },
    );

    app.run();

    SHUTTING_DOWN.store(true, Ordering::SeqCst);
    let _ = backend_tx.send(BackendCommand::Close);
    drop(uninstall_controller());
}
