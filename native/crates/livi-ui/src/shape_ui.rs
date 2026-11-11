// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::Cell;
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{msg_send, sel};
use objc2_app_kit::{NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use slint::ComponentHandle;

use crate::AppWindow;

thread_local! {
    static WANTED: Cell<Option<(u32, u32)>> = const { Cell::new(None) };
    static MAIN: Cell<usize> = const { Cell::new(0) };
    static HOOKED: Cell<bool> = const { Cell::new(false) };
}

pub fn follow(ui: &AppWindow, width: u32, height: u32) {
    WANTED.set(Some((width, height)));
    let weak = ui.as_weak();
    slint::Timer::single_shot(Duration::ZERO, move || {
        if let Some(ui) = weak.upgrade() {
            apply(&ui);
        }
    });
}

pub fn apply(ui: &AppWindow) {
    let (Some((w, h)), Some(window)) = (WANTED.get(), crate::dock_ui::ns_window(ui)) else {
        return;
    };
    MAIN.set(Retained::as_ptr(&window) as usize);
    hook(&window);
    if window.styleMask().contains(NSWindowStyleMask::FullScreen) {
        return;
    }
    let (w, h) = (f64::from(w), f64::from(h));
    let frame = window.frame();
    let content = window.contentRectForFrameRect(frame);
    let chrome = frame.size.height - content.size.height;
    let room = window.screen().map_or(frame, |s| s.visibleFrame());
    let scale = (content.size.width / w)
        .min(room.size.width / w)
        .min((room.size.height - chrome) / h);
    let size = NSSize::new(w * scale, h * scale);
    let top = frame.origin.y + frame.size.height;
    let origin = NSPoint::new(content.origin.x, top - chrome - size.height);
    let wanted = window.frameRectForContentRect(NSRect::new(origin, size));
    let right = room.origin.x + room.size.width - wanted.size.width;
    let upper = room.origin.y + room.size.height - wanted.size.height;
    let x = wanted.origin.x.min(right).max(room.origin.x);
    let y = wanted.origin.y.min(upper).max(room.origin.y);
    window.setFrame_display(NSRect::new(NSPoint::new(x, y), wanted.size), true);
}

fn hook(window: &NSWindow) {
    if HOOKED.get() {
        return;
    }
    // SAFETY: delegate is an NSWindow getter returning an optional object.
    let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![window, delegate] };
    let Some(delegate) = delegate else {
        return;
    };
    let class = delegate.class() as *const AnyClass as *mut AnyClass;
    let resize: extern "C-unwind" fn(&AnyObject, Sel, &NSWindow, NSSize) -> NSSize = will_resize;
    // SAFETY: Imp is the untyped form of a method implementation; the runtime calls it with this signature.
    let imp = unsafe {
        std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, &NSWindow, NSSize) -> NSSize, Imp>(
            resize,
        )
    };
    // SAFETY: the type encoding matches will_resize, and class is the delegate's live class.
    let added = unsafe {
        objc2::ffi::class_addMethod(
            class,
            sel!(windowWillResize:toSize:),
            imp,
            c"{CGSize=dd}@:@{CGSize=dd}".as_ptr(),
        )
    };
    if !added.as_bool() {
        eprintln!("[ui] the window delegate already answers resizes, the aspect ratio stays free");
    }
    HOOKED.set(true);
}

extern "C-unwind" fn will_resize(
    _delegate: &AnyObject,
    _sel: Sel,
    window: &NSWindow,
    size: NSSize,
) -> NSSize {
    let Some((w, h)) = WANTED.get() else {
        return size;
    };
    if window as *const NSWindow as usize != MAIN.get()
        || window.styleMask().contains(NSWindowStyleMask::FullScreen)
    {
        return size;
    }
    let ratio = f64::from(w) / f64::from(h);
    let now = window.frame().size;
    let content = window.contentRectForFrameRect(NSRect::new(NSPoint::new(0.0, 0.0), size));
    let chrome = size.height - content.size.height;
    if (size.width - now.width).abs() >= (size.height - now.height).abs() {
        NSSize::new(size.width, size.width / ratio + chrome)
    } else {
        NSSize::new((size.height - chrome) * ratio, size.height)
    }
}
