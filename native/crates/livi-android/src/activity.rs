// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::Mutex;

use jni::errors::Error;
use jni::objects::JObject;
use jni::{Env, JavaVM};
use slint::android::AndroidApp;

static CURRENT: Mutex<Option<AndroidApp>> = Mutex::new(None);

pub fn set(app: &AndroidApp) {
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(app.clone());
}

pub fn with<R>(
    call: impl FnOnce(&mut Env, &JObject) -> jni::errors::Result<R>,
) -> jni::errors::Result<R> {
    let app = CURRENT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or(Error::NullPtr("activity"))?;
    JavaVM::singleton()?.attach_current_thread(|env| {
        let activity = unsafe { JObject::from_raw(env, app.activity_as_ptr().cast()) };
        call(env, &activity)
    })
}
