use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{define_class, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSMenu, NSMenuItem, NSStatusItem};
use objc2_foundation::NSString;

use crate::settings_launcher::dispatch_open_settings;
use crate::ui::request_app_shutdown;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "VoxDictationMenuDelegate"]
    #[derive(Debug, PartialEq)]
    pub(crate) struct MenuDelegate;

    #[allow(non_snake_case)]
    impl MenuDelegate {
        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: &AnyObject) {
            dispatch_open_settings();
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &AnyObject) {
            request_app_shutdown();
        }
    }
);

pub(crate) fn install_status_menu(
    mtm: MainThreadMarker,
    status_item: &NSStatusItem,
) -> Retained<MenuDelegate> {
    let delegate: Retained<MenuDelegate> =
        unsafe { objc2::msg_send![MenuDelegate::alloc(mtm), init] };
    let menu = NSMenu::new(mtm);
    let settings_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Settings…"),
            Some(sel!(openSettings:)),
            &NSString::from_str(","),
        )
    };
    unsafe { settings_item.setTarget(Some(&delegate)) };
    menu.addItem(&settings_item);
    menu.addItem(&NSMenuItem::separatorItem(mtm));

    let quit_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Quit"),
            Some(sel!(quit:)),
            &NSString::from_str("q"),
        )
    };
    unsafe { quit_item.setTarget(Some(&delegate)) };
    menu.addItem(&quit_item);
    status_item.setMenu(Some(&menu));
    delegate
}
