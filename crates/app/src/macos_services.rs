use std::{cell::RefCell, ffi::CStr, sync::OnceLock};

use cocoa::{
    base::{id, nil},
    foundation::NSString,
};
use objc::{
    class,
    declare::ClassDecl,
    msg_send,
    rc::StrongPtr,
    runtime::{Object, Sel},
    sel, sel_impl,
};

static REQUESTS: OnceLock<async_channel::Sender<String>> = OnceLock::new();
thread_local! {
    static PROVIDER: RefCell<Option<StrongPtr>> = const { RefCell::new(None) };
}

/// Finder's Services menu calls this provider for the selected folder.
pub(crate) fn register(sender: async_channel::Sender<String>) {
    if REQUESTS.set(sender).is_err() {
        return;
    }
    unsafe {
        let mut provider = ClassDecl::new("QrateFolderService", class!(NSObject))
            .expect("qrate's folder service class is registered once");
        provider.add_method(
            sel!(openWithQrate:userData:error:),
            open_folder as extern "C" fn(&Object, Sel, id, id, *mut id),
        );
        let object: id = msg_send![provider.register(), new];
        let app: id = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, setServicesProvider: object];
        PROVIDER.with(|provider| *provider.borrow_mut() = Some(StrongPtr::new(object)));
    }
}

extern "C" fn open_folder(_: &Object, _: Sel, pasteboard: id, _: id, error: *mut id) {
    unsafe {
        let classes: id = msg_send![class!(NSArray), arrayWithObject: class!(NSURL)];
        let urls: id = msg_send![pasteboard, readObjectsForClasses: classes options: nil];
        if !urls.is_null() {
            let count: usize = msg_send![urls, count];
            for index in 0..count {
                let url: id = msg_send![urls, objectAtIndex: index];
                let text: id = msg_send![url, absoluteString];
                let utf8: *const std::ffi::c_char = msg_send![text, UTF8String];
                if !utf8.is_null()
                    && let Ok(value) = CStr::from_ptr(utf8).to_str()
                    && matches!(
                        crate::open_target::OpenTarget::parse(value.as_ref()),
                        Some(crate::open_target::OpenTarget::Folder(_))
                    )
                    && let Some(sender) = REQUESTS.get()
                    && sender.try_send(value.to_owned()).is_ok()
                {
                    return;
                }
            }
        }
        log::warn!("Finder's Open with qrate service received no readable folder");
        if !error.is_null() {
            let message =
                NSString::alloc(nil).init_str("Choose a folder to start a qrate project.");
            *error = msg_send![message, autorelease];
        }
    }
}
