use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSEvent, NSMenu,
    NSMenuItem,
};
use objc2_foundation::{
    ns_string, MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSString,
};

use clipr_common::{Command, Config, Payload};
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
            let mtm = self.mtm();

            // Retrieve the NSApplication.
            let app = notification
                .object()
                .unwrap()
                .downcast::<NSApplication>()
                .unwrap();

            app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

            let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("ClipR"));

            let config = Config::load_config(Path::new("/Users/jin/Documents/dev/projects/clipr/config.toml")).unwrap();

            let cmd = Command::List {
                from_index: Some(0),
                to_index: Some(5),
                preview_length: Some(5)
            };

            if let Ok(result) = call(&config, cmd) &&
               let Payload::List {value, preview_length: _} = result {
                for (idx, item) in value {
                    unsafe {
                        let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                            NSMenuItem::alloc(mtm),
                            &NSString::from_str(item.value.as_str()),
                            Some(sel!(terminate:)),
                            &NSString::from_str(format!("{}", idx).as_str())
                        );
                        menu.addItem(&item);
                    }
                }
            }

            menu.popUpMenuPositioningItem_atLocation_inView(
                None,
                NSEvent::mouseLocation(),
                None,
            );
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AppDelegateIvars::default());
        // SAFETY: The signature of `NSObject`'s `init` method is correct.
        unsafe { msg_send![super(this), init] }
    }
}

fn main() {
    let mtm = MainThreadMarker::new().unwrap();

    let app = NSApplication::sharedApplication(mtm);
    let delegate = Delegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    app.run();
}
