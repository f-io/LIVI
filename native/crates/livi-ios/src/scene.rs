// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

// iPadOS ends apps without a scene lifecycle at launch, and winit 0.30 makes its UIWindow
// without a scene. This delegate takes the scene Info.plist declares and moves every window
// into it, the ones open when it connects and the ones shown later.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, Message, define_class, sel};
use objc2_foundation::{NSNotification, NSNotificationCenter};
use objc2_ui_kit::{
    UIApplication, UIResponder, UIScene, UISceneConnectionOptions, UISceneDelegate, UISceneSession,
    UIWindow, UIWindowDidBecomeVisibleNotification, UIWindowScene,
};

thread_local! {
    static SCENE: RefCell<Option<Retained<UIWindowScene>>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(UIResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "LiviSceneDelegate"]
    struct SceneDelegate;

    unsafe impl NSObjectProtocol for SceneDelegate {}

    unsafe impl UISceneDelegate for SceneDelegate {
        #[unsafe(method(scene:willConnectToSession:options:))]
        fn will_connect(
            &self,
            scene: &UIScene,
            _session: &UISceneSession,
            _options: &UISceneConnectionOptions,
        ) {
            let Ok(scene) = scene.retain().downcast::<UIWindowScene>() else {
                return;
            };
            SCENE.with_borrow_mut(|s| *s = Some(scene.clone()));
            // SAFETY: the selector is this class's own method taking the notification.
            unsafe {
                NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                    self,
                    sel!(windowShown:),
                    Some(UIWindowDidBecomeVisibleNotification),
                    None,
                );
            }
            #[allow(deprecated)]
            let windows = UIApplication::sharedApplication(self.mtm()).windows();
            for window in windows.iter() {
                adopt(&window, &scene);
            }
        }
    }

    impl SceneDelegate {
        #[unsafe(method(windowShown:))]
        fn window_shown(&self, note: &NSNotification) {
            let Some(window) = note.object().and_then(|o| o.downcast::<UIWindow>().ok()) else {
                return;
            };
            SCENE.with_borrow(|scene| {
                if let Some(scene) = scene {
                    adopt(&window, scene);
                }
            });
        }
    }
);

fn adopt(window: &UIWindow, scene: &UIWindowScene) {
    if window.windowScene().is_none() {
        window.setWindowScene(Some(scene));
        window.makeKeyAndVisible();
    }
}

pub fn window(_: MainThreadMarker) -> Option<Retained<UIWindow>> {
    SCENE.with_borrow(|scene| scene.as_ref()?.windows().firstObject())
}

// UIKit looks the delegate up by its name, so the class has to exist before UIApplicationMain.
pub fn register(_: MainThreadMarker) {
    let _ = SceneDelegate::class();
}
