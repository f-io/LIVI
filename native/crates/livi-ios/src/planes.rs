// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use dispatch2::run_on_main;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_ui_kit::{UIColor, UIView, UIViewAutoresizing};

pub const STAGE: usize = 1;

struct Plane {
    rank: i32,
    clip: Retained<UIView>,
    gl: Retained<UIView>,
    region: [f64; 6],
}

thread_local! {
    static HOST: RefCell<Option<Retained<UIView>>> = const { RefCell::new(None) };
    static PLANES: RefCell<HashMap<u64, Plane>> = RefCell::new(HashMap::new());
}

static NEXT: AtomicU64 = AtomicU64::new(1);

pub struct Planes;

fn rect(x: f64, y: f64, w: f64, h: f64) -> CGRect {
    CGRect::new(CGPoint::new(x, y), CGSize::new(w, h))
}

fn view(mtm: MainThreadMarker, frame: CGRect) -> Retained<UIView> {
    UIView::initWithFrame(UIView::alloc(mtm), frame)
}

fn host(mtm: MainThreadMarker) -> Option<Retained<UIView>> {
    if let Some(host) = HOST.with_borrow(Clone::clone) {
        return Some(host);
    }
    let window = crate::scene::window(mtm)?;
    let ui = window.rootViewController()?.view()?;
    // UIKit nests the root view in its own container views, the host goes next to it in there.
    let parent = ui.superview()?;
    let host = view(mtm, ui.frame());
    host.setAutoresizingMask(
        UIViewAutoresizing::FlexibleWidth | UIViewAutoresizing::FlexibleHeight,
    );
    host.setUserInteractionEnabled(false);
    host.setBackgroundColor(Some(&UIColor::blackColor()));
    parent.insertSubview_belowSubview(&host, &ui);
    ui.setBackgroundColor(Some(&UIColor::clearColor()));
    ui.layer().setOpaque(false);
    HOST.with_borrow_mut(|h| *h = Some(host.clone()));
    Some(host)
}

fn layout(host: &UIView, plane: &Plane) {
    let size = host.bounds().size;
    let (ww, wh) = (size.width, size.height);
    let [crop_l, crop_t, vis_w, vis_h, tier_w, tier_h] = plane.region;
    if ww <= 0.0 || wh <= 0.0 || vis_w <= 0.0 || vis_h <= 0.0 || tier_w <= 0.0 || tier_h <= 0.0 {
        plane.clip.setFrame(host.bounds());
        plane.gl.setFrame(rect(0.0, 0.0, ww, wh));
        return;
    }
    let scale = (ww / vis_w).min(wh / vis_h);
    plane.clip.setFrame(rect(
        (ww - vis_w * scale) / 2.0,
        (wh - vis_h * scale) / 2.0,
        vis_w * scale,
        vis_h * scale,
    ));
    plane.gl.setFrame(rect(
        -crop_l * scale,
        -crop_t * scale,
        tier_w * scale,
        tier_h * scale,
    ));
}

fn open(mtm: MainThreadMarker, id: u64, rank: i32) -> Option<usize> {
    let host = host(mtm)?;
    let clip = view(mtm, host.bounds());
    clip.setClipsToBounds(true);
    clip.setBackgroundColor(Some(&UIColor::blackColor()));
    let gl = view(mtm, clip.bounds());
    clip.addSubview(&gl);
    let above = PLANES.with_borrow(|planes| {
        planes
            .values()
            .filter(|p| p.rank <= rank)
            .max_by_key(|p| p.rank)
            .map(|p| p.clip.clone())
    });
    match above {
        Some(above) => host.insertSubview_aboveSubview(&clip, &above),
        None => host.insertSubview_atIndex(&clip, 0),
    }
    let plane = Plane {
        rank,
        clip,
        gl,
        region: [0.0; 6],
    };
    layout(&host, &plane);
    let handle = Retained::as_ptr(&plane.gl) as usize;
    PLANES.with_borrow_mut(|planes| planes.insert(id, plane));
    Some(handle)
}

fn with_plane(view: u64, f: impl FnOnce(&UIView, &mut Plane) + Send) {
    run_on_main(move |_| {
        let Some(host) = HOST.with_borrow(Clone::clone) else {
            return;
        };
        PLANES.with_borrow_mut(|planes| {
            if let Some(plane) = planes.get_mut(&view) {
                f(&host, plane);
            }
        });
    });
}

impl livi_video_player::Views for Planes {
    fn open(&self, rank: i32) -> Option<u64> {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let handle = run_on_main(move |mtm| open(mtm, id, rank));
        let Some(handle) = handle else {
            eprintln!("[ios] no window to put video plane {id} in");
            return None;
        };
        livi_video_player::set_surface(id, handle);
        Some(id)
    }

    fn show(&self, view: u64, shown: bool) {
        with_plane(view, move |_, plane| plane.clip.setHidden(!shown));
    }

    fn place(&self, view: u64, region: [f64; 6]) {
        with_plane(view, move |host, plane| {
            plane.region = region;
            layout(host, plane);
        });
    }

    fn close(&self, view: u64) {
        run_on_main(move |_| {
            if let Some(plane) = PLANES.with_borrow_mut(|planes| planes.remove(&view)) {
                plane.clip.removeFromSuperview();
            }
        });
    }
}
