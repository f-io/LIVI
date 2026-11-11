// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::RefCell;
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSView, NSWindow};
use objc2_foundation::NSAppleEventManager;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{CloseRequestResponse, ComponentHandle, Weak};

use crate::AppWindow;

const CORE_EVENTS: u32 = u32::from_be_bytes(*b"aevt");
const REOPEN: u32 = u32::from_be_bytes(*b"rapp");

thread_local! {
    static MAIN: RefCell<Option<Weak<AppWindow>>> = const { RefCell::new(None) };
    static HANDLER: RefCell<Option<Retained<Reopen>>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "LIVIDockReopen"]
    struct Reopen;

    impl Reopen {
        #[unsafe(method(handleReopen:withReply:))]
        fn handle_reopen(&self, _event: &AnyObject, _reply: &AnyObject) {
            let ui = MAIN.with_borrow(|m| m.as_ref().and_then(Weak::upgrade));
            if let Some(w) = ui.as_ref().and_then(ns_window) {
                if w.isMiniaturized() {
                    w.deminiaturize(None);
                }
                w.makeKeyAndOrderFront(None);
            }
        }
    }
);

pub fn ns_window(ui: &AppWindow) -> Option<Retained<NSWindow>> {
    let window = ui.window().window_handle();
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(h) = handle.as_raw() else {
        return None;
    };
    // SAFETY: an AppKit handle carries a valid NSView that lives as long as the window.
    let view: &NSView = unsafe { h.ns_view.cast().as_ref() };
    view.window()
}

pub fn keep(ui: &AppWindow) {
    MAIN.set(Some(ui.as_weak()));
    let weak = ui.as_weak();
    ui.window()
        .on_close_requested(move || match weak.upgrade().as_ref().and_then(ns_window) {
            Some(w) => {
                w.orderOut(None);
                CloseRequestResponse::KeepWindowShown
            }
            None => CloseRequestResponse::HideWindow,
        });
    slint::Timer::single_shot(Duration::ZERO, listen);
}

fn listen() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    // SAFETY: init is NSObject's initializer, Reopen adds no ivars that need setting.
    let handler: Retained<Reopen> = unsafe { msg_send![Reopen::alloc(mtm), init] };
    let events = NSAppleEventManager::sharedAppleEventManager();
    // SAFETY: the selector matches Reopen's method, and HANDLER keeps the handler alive.
    let _: () = unsafe {
        msg_send![
            &*events,
            setEventHandler: &*handler,
            andSelector: sel!(handleReopen:withReply:),
            forEventClass: CORE_EVENTS,
            andEventID: REOPEN
        ]
    };
    HANDLER.set(Some(handler));
}
