// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use jni::objects::JValue;
use jni::{jni_sig, jni_str};

use crate::activity;

pub struct Volume;

impl livi_core::DeviceVolume for Volume {
    fn get(&self) -> Option<f64> {
        let level = activity::with(|env, activity| {
            env.call_method(activity, jni_str!("volume"), jni_sig!("()F"), &[])?
                .f()
        });
        match level {
            Ok(level) if level >= 0.0 => Some(f64::from(level)),
            Ok(_) => None,
            Err(e) => {
                eprintln!("[android] volume not read: {e}");
                None
            }
        }
    }

    fn set(&self, level: f64) {
        let level = level.clamp(0.0, 1.0) as f32;
        let set = activity::with(|env, activity| {
            env.call_method(
                activity,
                jni_str!("setVolume"),
                jni_sig!("(F)V"),
                &[JValue::Float(level)],
            )
            .map(drop)
        });
        if let Err(e) = set {
            eprintln!("[android] volume not set: {e}");
        }
    }
}
