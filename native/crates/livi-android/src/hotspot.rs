// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use jni::objects::{JClass, JObjectArray, JString};
use jni::sys::jint;
use jni::{EnvUnowned, JValue, jni_sig, jni_str};
use livi_helper::wireless::{ApGranted, ApRequest};

pub fn open(request: &ApRequest) -> Result<ApGranted, String> {
    let granted = crate::activity::with(|env, activity| {
        let ssid = env.new_string(&request.ssid)?;
        let passphrase = env.new_string(&request.passphrase)?;
        let granted = env
            .call_method(
                activity,
                jni_str!("hotspot"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;I)[Ljava/lang/String;"),
                &[
                    JValue::Object(&ssid),
                    JValue::Object(&passphrase),
                    JValue::Int(jint::try_from(request.frequency).unwrap_or_default()),
                ],
            )?
            .l()?;
        let granted = JObjectArray::<JString>::cast_local(env, granted)?;
        if granted.is_null() {
            return Ok(None);
        }
        let mut fields = Vec::new();
        for i in 0..granted.len(env)? {
            fields.push(granted.get_element(env, i)?.try_to_string(env)?);
        }
        Ok(Some(fields))
    })
    .map_err(|e| format!("Wi-Fi: {e}"))?
    .ok_or_else(|| format!("Android opened no access point {}", request.ssid))?;
    let [iface, ssid, passphrase, frequency] =
        <[String; 4]>::try_from(granted).map_err(|fields| {
            format!(
                "Android described its access point with {} fields",
                fields.len()
            )
        })?;
    Ok(ApGranted {
        iface,
        ssid,
        passphrase,
        frequency: frequency.parse().unwrap_or_default(),
    })
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_fio_livi_LiviHotspot_stopped<'caller>(
    _env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) {
    livi_helper::wireless::off_air();
}
