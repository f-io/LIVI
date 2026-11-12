// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#![cfg(target_os = "android")]

mod activity;
mod audio;
mod black;
mod bluetooth;
mod hotspot;
mod logcat;
mod media;
mod planes;
mod update;

use std::io;
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use iap2_usbmux::TunRequest;
use jni::errors::LogErrorAndDefault;
use jni::objects::{JClass, JObject, JString};
use jni::sys::{jboolean, jfloat, jint};
use jni::{EnvUnowned, JValue, jni_sig, jni_str};
use livi_core::{Exit, Helper, Installer, Instance, OutputBackend, Paths, Resources};
use livi_helper::embedded::{Embedded, Host};
use slint::android::AndroidApp;
use slint::android::android_activity::WindowManagerFlags;

const CORE_START: Duration = Duration::from_secs(15);

static CORE: OnceLock<Result<PathBuf, String>> = OnceLock::new();
static CORE_EXIT: OnceLock<bool> = OnceLock::new();

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    if let Err(e) = start(app) {
        eprintln!("[android] {e}");
    }
}

fn start(app: AndroidApp) -> Result<(), Box<dyn std::error::Error>> {
    activity::set(&app);
    app.set_window_flags(
        WindowManagerFlags::KEEP_SCREEN_ON,
        WindowManagerFlags::empty(),
    );
    let socket = CORE.get_or_init(|| core(&app)).clone()?;
    slint::android::init(app.clone())?;
    livi_ui::run(
        || livi_core_client::wait_for_socket(&socket, CORE_START),
        None,
        move |on| {
            if let Err(e) = call(jni_str!("kiosk"), on) {
                eprintln!("[android] kiosk: {e}");
            }
        },
        Some(Box::new(pick)),
    )?;
    if let Some(&restart) = CORE_EXIT.get() {
        call(jni_str!("exit"), restart)?;
    }
    Ok(())
}

fn core(app: &AndroidApp) -> Result<PathBuf, String> {
    if let Err(e) = logcat::capture() {
        logcat::write(&format!("[android] stdout stays unread: {e}"));
    }
    let files = app.internal_data_path().ok_or("no app data dir")?;
    let paths = Paths::within(&files, files.join("run"));
    let socket = paths.socket();
    livi_aaudio::register().map_err(|e| format!("no AAudio elements: {e}"))?;
    livi_aaudio::set_lister(audio::lister());
    livi_video_player::set_views(planes::Planes);
    livi_video_view::set_window(livi_output_proto::SCREENS[0], planes::STAGE);
    livi_video_view::serve(&paths.runtime.join("compositor.ctrl"))
        .map_err(|e| format!("no video control: {e}"))?;
    let host = Host {
        sockets: paths.helper.clone(),
        mfi: files.join("mfi"),
        lockdown: files.join("lockdown"),
        tunnel: iap2_usbmux::Tunnel::new(Arc::new(tun)),
        phones: Arc::new(bluetooth::phones),
        hotspot: Arc::new(hotspot::open),
        drop_phone: Arc::new(|mac| bluetooth::drop(mac).map_err(Into::into)),
        page_phone: Arc::new(bluetooth::page),
        bt_keys: files.join("bluetooth-keys"),
    };
    let res = Resources {
        helper: Some(Helper::Embedded(Arc::new(move |start| {
            Embedded::start(start, host.clone())
                .map(|helper| Box::new(helper) as Box<dyn Send>)
                .map_err(|e| format!("no helper thread: {e}"))
        }))),
        output: Some(OutputBackend::Embedded(Arc::new(media::InProcess))),
        installer: Some(Installer {
            downloads: files.join("update"),
            install: Arc::new(update::install),
        }),
        ..Resources::embedded()
    };
    let instance = Instance::claim(paths)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("no runtime: {e}"))?;
    std::thread::Builder::new()
        .name("core".into())
        .spawn(move || {
            let outcome = runtime.block_on(livi_core::run(instance, res, std::future::pending()));
            let restart = match outcome {
                Ok(exit) => matches!(exit, Exit::Restart(_)),
                Err(e) => {
                    eprintln!("[core] {e}");
                    false
                }
            };
            let _ = CORE_EXIT.set(restart);
            let _ = slint::invoke_from_event_loop(|| {
                let _ = slint::quit_event_loop();
            });
        })
        .map_err(|e| format!("no core thread: {e}"))?;
    Ok(socket)
}

fn call(method: &jni::strings::JNIStr, flag: bool) -> jni::errors::Result<()> {
    activity::with(|env, activity| {
        env.call_method(activity, method, jni_sig!("(Z)V"), &[JValue::Bool(flag)])
            .map(drop)
    })
}

fn pick() {
    let opened = activity::with(|env, activity| {
        env.call_method(activity, jni_str!("pick"), jni_sig!("()V"), &[])
            .map(drop)
    });
    if let Err(e) = opened {
        eprintln!("[android] file picker: {e}");
    }
}

fn tun(request: &TunRequest) -> io::Result<OwnedFd> {
    let fd = activity::with(|env, activity| {
        let addresses = request
            .addresses
            .iter()
            .map(|(address, prefix)| format!("{address}/{prefix}"))
            .collect::<Vec<_>>()
            .join(" ");
        let addresses = env.new_string(addresses)?;
        env.call_method(
            activity,
            jni_str!("tun"),
            jni_sig!("(Ljava/lang/String;I)I"),
            &[
                JValue::Object(&addresses),
                JValue::Int(jint::from(request.mtu)),
            ],
        )?
        .i()
    })
    .map_err(|e| io::Error::other(format!("VPN service: {e}")))?;
    if fd < 0 {
        return Err(io::Error::other("the VPN service opened no interface"));
    }
    // SAFETY: Java detached the tun fd and hands its ownership over.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_fio_livi_LiviActivity_screen<'caller>(
    _env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    width_px: jint,
    height_px: jint,
    width_mm: jfloat,
    height_mm: jfloat,
) {
    let px = |v: jint| usize::try_from(v).unwrap_or(0);
    livi_core::set_screen(
        width_mm.into(),
        height_mm.into(),
        px(width_px),
        px(height_px),
    );
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_fio_livi_LiviActivity_bluetoothAddress<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    address: JString<'caller>,
) {
    let address = env
        .with_env(|env| address.try_to_string(env))
        .resolve::<LogErrorAndDefault>();
    livi_core::set_bt_mac(&address);
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_fio_livi_LiviActivity_picked<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    path: JString<'caller>,
) {
    let path = env
        .with_env(|env| path.try_to_string(env))
        .resolve::<LogErrorAndDefault>();
    if !path.is_empty() {
        let _ = slint::invoke_from_event_loop(move || livi_ui::file_picked(Path::new(&path)));
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_fio_livi_LiviActivity_surface<'caller>(
    env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    plane: jint,
    surface: JObject<'caller>,
) {
    let Ok(plane) = u64::try_from(plane) else {
        return;
    };
    media::plane_surface(env.as_raw(), plane, surface.as_raw());
}

#[unsafe(no_mangle)]
extern "system" fn Java_dev_fio_livi_LiviActivity_key<'caller>(
    _env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    scancode: jint,
    down: jboolean,
    repeat: jboolean,
) {
    let Some(code) = u16::try_from(scancode)
        .ok()
        .and_then(livi_ui_model::scancode::code)
    else {
        return;
    };
    let _ = slint::invoke_from_event_loop(move || livi_ui::key(code, down, repeat));
}
