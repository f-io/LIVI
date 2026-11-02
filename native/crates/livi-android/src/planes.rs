// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use jni::objects::JObject;
use jni::{Env, JValue, jni_sig, jni_str};

pub const STAGE: usize = 1;

pub struct Planes;

impl Planes {
    fn on_activity<R>(
        &self,
        what: &str,
        call: impl FnOnce(&mut Env, &JObject) -> jni::errors::Result<R>,
    ) -> Option<R> {
        crate::activity::with(call)
            .inspect_err(|e| eprintln!("[android] {what}: {e}"))
            .ok()
    }
}

fn id(view: u64) -> JValue<'static> {
    JValue::Int(i32::try_from(view).unwrap_or_default())
}

impl livi_video_player::Views for Planes {
    fn open(&self, rank: i32) -> Option<u64> {
        let id = self.on_activity("plane", |env, activity| {
            env.call_method(
                activity,
                jni_str!("plane"),
                jni_sig!("(I)I"),
                &[JValue::Int(rank)],
            )?
            .i()
        })?;
        u64::try_from(id).ok()
    }

    fn show(&self, view: u64, shown: bool) {
        self.on_activity("planeShown", |env, activity| {
            env.call_method(
                activity,
                jni_str!("planeShown"),
                jni_sig!("(IZ)V"),
                &[id(view), JValue::Bool(shown)],
            )
            .map(drop)
        });
    }

    fn place(&self, view: u64, [crop_l, crop_t, vis_w, vis_h, tier_w, tier_h]: [f64; 6]) {
        self.on_activity("planePlace", |env, activity| {
            env.call_method(
                activity,
                jni_str!("planePlace"),
                jni_sig!("(IDDDDDD)V"),
                &[
                    id(view),
                    JValue::Double(crop_l),
                    JValue::Double(crop_t),
                    JValue::Double(vis_w),
                    JValue::Double(vis_h),
                    JValue::Double(tier_w),
                    JValue::Double(tier_h),
                ],
            )
            .map(drop)
        });
    }

    fn close(&self, view: u64) {
        self.on_activity("planeGone", |env, activity| {
            env.call_method(
                activity,
                jni_str!("planeGone"),
                jni_sig!("(I)V"),
                &[id(view)],
            )
            .map(drop)
        });
    }
}
