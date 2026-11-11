// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::os::fd::{FromRawFd, OwnedFd};
use std::sync::Mutex;

use jni::errors::LogErrorAndDefault;
use jni::objects::{JClass, JString};
use jni::sys::jint;
use jni::{EnvUnowned, JValue, jni_sig, jni_str};
use livi_helper::wireless::{Phone, Service};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

static PHONES: Mutex<Option<UnboundedSender<Phone>>> = Mutex::new(None);

pub fn phones(
    name: &str,
    carplay: bool,
    android_auto: bool,
) -> Result<UnboundedReceiver<Phone>, String> {
    let (tx, rx) = unbounded_channel();
    *PHONES.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    let listening = crate::activity::with(|env, activity| {
        let name = env.new_string(name)?;
        env.call_method(
            activity,
            jni_str!("bluetooth"),
            jni_sig!("(Ljava/lang/String;ZZ)Z"),
            &[
                JValue::Object(&name),
                JValue::Bool(carplay),
                JValue::Bool(android_auto),
            ],
        )?
        .z()
    })
    .map_err(|e| format!("Bluetooth: {e}"))?;
    if listening {
        Ok(rx)
    } else {
        Err("Android gave LIVI no Bluetooth".into())
    }
}

pub fn drop(mac: String) -> Result<(), String> {
    let dropped = crate::activity::with(|env, activity| {
        let peer = env.new_string(&mac)?;
        env.call_method(
            activity,
            jni_str!("dropPhone"),
            jni_sig!("(Ljava/lang/String;)Z"),
            &[JValue::Object(&peer)],
        )?
        .z()
    })
    .map_err(|e| format!("Bluetooth: {e}"))?;
    if dropped {
        Ok(())
    } else {
        Err(format!("no Bluetooth link to {mac}"))
    }
}

pub fn page(mac: &str, uuid: &str) -> Result<(), String> {
    let failed = crate::activity::with(|env, activity| {
        let peer = env.new_string(mac)?;
        let uuid = env.new_string(uuid)?;
        let failed = env
            .call_method(
                activity,
                jni_str!("pagePhone"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;"),
                &[JValue::Object(&peer), JValue::Object(&uuid)],
            )?
            .l()?;
        let failed = JString::cast_local(env, failed)?;
        if failed.is_null() {
            return Ok(None);
        }
        failed.try_to_string(env).map(Some)
    })
    .map_err(|e| format!("Bluetooth: {e}"))?;
    failed.map_or(Ok(()), Err)
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_fio_livi_LiviBluetooth_phone<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    fd: jint,
    peer: JString<'caller>,
    service: jint,
) {
    if fd < 0 {
        return;
    }
    let service = match service {
        1 => Service::HandsFree,
        2 => Service::AndroidAuto,
        _ => Service::Iap,
    };
    // SAFETY: Java detached this socket fd and hands its ownership over.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let peer = env
        .with_env(|env| peer.try_to_string(env))
        .resolve::<LogErrorAndDefault>();
    let phones = PHONES.lock().unwrap_or_else(|e| e.into_inner());
    let taken = phones
        .as_ref()
        .is_some_and(|tx| tx.send(Phone { fd, peer, service }).is_ok());
    if !taken {
        eprintln!("[bt] nothing waits for phones, the link closes");
    }
}
