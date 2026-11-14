// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::RefCell;
use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSArray, NSURL};
use objc2_ui_kit::{UIDocumentPickerDelegate, UIDocumentPickerViewController};
use objc2_uniform_type_identifiers::UTTypeItem;

thread_local! {
    // The picker holds its delegate weakly.
    static DELEGATE: RefCell<Option<Retained<Delegate>>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "LiviPickerDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UIDocumentPickerDelegate for Delegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn picked(&self, _picker: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
            let Some(path) = urls.firstObject().and_then(|url| url.path()) else {
                return;
            };
            livi_ui::file_picked(&PathBuf::from(path.to_string()));
        }
    }
);

fn delegate(mtm: MainThreadMarker) -> Retained<Delegate> {
    DELEGATE.with_borrow_mut(|d| {
        d.get_or_insert_with(|| {
            // SAFETY: init is NSObject's designated initializer, Delegate adds no state.
            unsafe { msg_send![Delegate::alloc(mtm), init] }
        })
        .clone()
    })
}

pub fn pick() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(root) = crate::scene::window(mtm).and_then(|w| w.rootViewController()) else {
        eprintln!("[ios] no view controller to show the document picker on");
        return;
    };
    // SAFETY: UTTypeItem is an immutable framework constant.
    let types = NSArray::from_slice(&[unsafe { UTTypeItem }]);
    let picker = UIDocumentPickerViewController::initForOpeningContentTypes_asCopy(
        UIDocumentPickerViewController::alloc(mtm),
        &types,
        true,
    );
    let delegate = delegate(mtm);
    picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    root.presentViewController_animated_completion(&picker, true, None);
}
