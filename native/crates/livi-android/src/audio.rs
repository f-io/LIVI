// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use jni::objects::{JObject, JObjectArray, JString};
use jni::{Env, JValue, jni_sig, jni_str};
use livi_aaudio::Device;

const GET_DEVICES_ALL: i32 = 3;

fn text(env: &mut Env, obj: JObject) -> jni::errors::Result<String> {
    if obj.is_null() {
        return Ok(String::new());
    }
    JString::cast_local(env, obj)?.try_to_string(env)
}

fn device(env: &mut Env, info: &JObject) -> jni::errors::Result<Device> {
    let id = env
        .call_method(info, jni_str!("getId"), jni_sig!("()I"), &[])?
        .i()?;
    let kind = env
        .call_method(info, jni_str!("getType"), jni_sig!("()I"), &[])?
        .i()?;
    let sink = env
        .call_method(info, jni_str!("isSink"), jni_sig!("()Z"), &[])?
        .z()?;
    let address = env
        .call_method(
            info,
            jni_str!("getAddress"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    let address = text(env, address)?;
    let product = env
        .call_method(
            info,
            jni_str!("getProductName"),
            jni_sig!("()Ljava/lang/CharSequence;"),
            &[],
        )?
        .l()?;
    let product = if product.is_null() {
        product
    } else {
        env.call_method(
            &product,
            jni_str!("toString"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?
    };
    let product = text(env, product)?;
    Ok(Device {
        id,
        kind,
        sink,
        address,
        product,
    })
}

fn list() -> jni::errors::Result<Vec<Device>> {
    crate::activity::with(|env, activity| {
        let name = env.new_string("audio")?;
        let manager = env
            .call_method(
                activity,
                jni_str!("getSystemService"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                &[JValue::Object(&name)],
            )?
            .l()?;
        let infos = env
            .call_method(
                &manager,
                jni_str!("getDevices"),
                jni_sig!("(I)[Landroid/media/AudioDeviceInfo;"),
                &[JValue::Int(GET_DEVICES_ALL)],
            )?
            .l()?;
        let infos = JObjectArray::<JObject>::cast_local(env, infos)?;
        let mut devices = Vec::new();
        for i in 0..infos.len(env)? {
            let info = infos.get_element(env, i)?;
            devices.push(device(env, &info)?);
        }
        Ok(devices)
    })
}

pub fn lister() -> impl Fn() -> Vec<Device> + Send + Sync + 'static {
    move || {
        list()
            .inspect_err(|e| eprintln!("[android] audio devices: {e}"))
            .unwrap_or_default()
    }
}
