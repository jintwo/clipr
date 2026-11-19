use std::path::Path;
use std::process;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, sel};
#[cfg(target_os = "macos")]
use objc2_app_kit::NSMenuDelegate;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSEvent, NSMenu,
    NSMenuItem,
};
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSString, ns_string,
};

use clipr_common::{Command, Config, MAX_LEN, Payload, shorten, Item};
use clipr_daemon::call;

#[derive(Debug, Default)]
struct AppDelegateIvars {}

#[cfg(target_os = "macos")]
define_class!(
    // SAFETY:
    // - The superclass NSObject does not have any subclassing requirements.
    // - `Delegate` does not implement `Drop`.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = AppDelegateIvars]
    struct Delegate;

    // SAFETY: `NSObjectProtocol` has no safety requirements.
    unsafe impl NSObjectProtocol for Delegate {}

    // SAFETY: `NSApplicationDelegate` has no safety requirements.
    unsafe impl NSApplicationDelegate for Delegate {
        // SAFETY: The signature is correct.
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, notification: &NSNotification) {
            // Retrieve the NSApplication.
            let app = notification
                .object()
                .unwrap()
                .downcast::<NSApplication>()
                .unwrap();

            app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

            self.init();
        }

        #[unsafe(method(selectItem:))]
        fn select_item(&self, sender: Option<&NSMenuItem>) {
            if let Some(menu_item) = sender {
                self.handler(menu_item)
            }
            process::exit(0);
        }
    }

    unsafe impl NSMenuDelegate for Delegate {
        #[unsafe(method(menuDidClose:))]
        fn did_close(&self, menu: &NSMenu) {
            process::exit(0);
        }
    }

);

pub fn format_item(item: &Item, short: bool, preview_length: Option<usize>) -> (String, String) {
    let val = if short {
        shorten(&item.value, preview_length)
    } else {
        item.value.clone()
    };

    let tags = match &item.tags {
        Some(tags) => {
            let mut ts = tags.iter().map(|v| v.as_str()).collect::<Vec<&str>>();
            ts.sort();
            ts.join(",")
        }
        None => "".to_string(),
    };

    let max_len = preview_length.unwrap_or(MAX_LEN);

    (format!("{:<max_len$}", val), tags)
}


impl Delegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AppDelegateIvars::default());
        // SAFETY: The signature of `NSObject`'s `init` method is correct.
        unsafe { msg_send![super(this), init] }
    }

    fn init(&self) {
        let mtm = self.mtm();

        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("ClipR"));
        menu.setDelegate(Some(ProtocolObject::from_ref(self)));

        let config = Config::load_config(Path::new(
            "/Users/jin/Documents/dev/projects/clipr/config.toml",
        ))
        .unwrap();

        let cmd = Command::List {
            from_index: Some(0),
            to_index: Some(10),
            preview_length: Some(5),
        };

        if let Ok(result) = call(&config, cmd)
            && let Payload::List {
                value,
                preview_length: _,
            } = result
        {
            for (idx, item) in value {
                unsafe {
                    let (item_value, item_tags) = format_item(&item, true, Some(32));
                    let menu_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(item_value.as_str()),
                        None,
                        &NSString::from_str(format!("{}", (idx + 1) % 10).as_str()),
                    );
                    menu_item.setTarget(Some(self));
                    menu_item.setAction(Some(sel!(selectItem:)));
                    menu_item.setTag(idx.try_into().unwrap());

                    menu_item.setSubtitle(Some(&NSString::from_str(item_tags.as_str())));
                    menu.addItem(&menu_item);
                }
            }
        }

        menu.popUpMenuPositioningItem_atLocation_inView(None, NSEvent::mouseLocation(), None);
    }

    fn handler(&self, sender: &NSMenuItem) {
        let idx: usize = sender.tag().try_into().unwrap();

        let config = Config::load_config(Path::new(
            "/Users/jin/Documents/dev/projects/clipr/config.toml",
        ))
        .unwrap();

        let cmd = Command::Set {
            index: idx
        };

        call(&config, cmd).unwrap();
    }
}

fn main() {
    let mtm = MainThreadMarker::new().unwrap();

    let app = NSApplication::sharedApplication(mtm);
    let delegate = Delegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    app.run();
}
