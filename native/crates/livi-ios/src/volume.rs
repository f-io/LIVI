// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, extern_class, msg_send};
use objc2_avf_audio::AVAudioSession;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_ui_kit::{UISlider, UIView, UIWindow};

extern_class!(
    #[unsafe(super(UIView))]
    #[thread_kind = MainThreadOnly]
    #[name = "MPVolumeView"]
    struct MPVolumeView;
);

thread_local! {
    static VOLUME_VIEW: RefCell<Option<Retained<MPVolumeView>>> = const { RefCell::new(None) };
}

struct Volume;

impl livi_core::DeviceVolume for Volume {
    fn get(&self) -> Option<f64> {
        // SAFETY: the shared audio session; outputVolume is a readable property.
        let level = unsafe { AVAudioSession::sharedInstance().outputVolume() };
        Some(f64::from(level))
    }

    fn set(&self, level: f64) {
        let level = level.clamp(0.0, 1.0) as f32;
        let _ = slint::invoke_from_event_loop(move || {
            VOLUME_VIEW.with_borrow(|view| {
                let Some(view) = view else { return };
                let slider = view
                    .subviews()
                    .iter()
                    .find_map(|v| v.downcast::<UISlider>().ok());
                if let Some(slider) = slider {
                    slider.setValue(level);
                }
            });
        });
    }
}

pub fn install(window: &UIWindow, mtm: MainThreadMarker) {
    let frame = CGRect::new(CGPoint::new(-2000.0, -2000.0), CGSize::new(100.0, 40.0));
    // SAFETY: initWithFrame on a freshly allocated MPVolumeView, a UIView subclass.
    let view: Retained<MPVolumeView> =
        unsafe { msg_send![MPVolumeView::alloc(mtm), initWithFrame: frame] };
    view.setAlpha(0.01);
    window.addSubview(&view);
    VOLUME_VIEW.with_borrow_mut(|v| *v = Some(view));
    livi_core::set_device_volume(Volume);
}
