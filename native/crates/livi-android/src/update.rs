// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::Path;

use jni::objects::JString;
use jni::{JValue, jni_sig, jni_str};

use crate::activity;

pub fn install(apk: &Path) -> Result<(), String> {
    let refused = activity::with(|env, activity| {
        let path = env.new_string(apk.to_string_lossy())?;
        let refused = env
            .call_method(
                activity,
                jni_str!("install"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                &[JValue::Object(&path)],
            )?
            .l()?;
        if refused.is_null() {
            return Ok(None);
        }
        JString::cast_local(env, refused)?
            .try_to_string(env)
            .map(Some)
    })
    .map_err(|e| format!("package installer: {e}"))?;
    refused.map_or(Ok(()), Err)
}
