// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use iap2_mfi::{AuthCoprocessor, NoCoprocessor};
use livi_core_proto::helper::{HelperSockets, HelperStart};
use livi_runtime::aa_sock::{self, AaSockDeps};
use livi_runtime::ident::Transport;
use livi_runtime::livi_sock::{self, Broadcaster, LiviSockConfig};
use livi_runtime::mfi_async::SharedCoprocessor;
use livi_runtime::state::HelperState;
use tokio::sync::{oneshot, watch};

use crate::link::LinkPresence;
use crate::wired::{Dongle, Wired};

const SHUTDOWN: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct Host {
    pub sockets: HelperSockets,
    pub mfi: PathBuf,
    pub lockdown: PathBuf,
    #[cfg(target_os = "android")]
    pub tun: iap2_usbmux::OpenTun,
}

pub struct Embedded {
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Embedded {
    pub fn start(start: HelperStart, host: Host) -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("helper")
            .build()?;
        let (stop, stopped) = oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("helper".into())
            .spawn(move || {
                runtime.block_on(async move {
                    let _bluez = serve(start, host);
                    let _ = stopped.await;
                });
                runtime.shutdown_timeout(SHUTDOWN);
            })?;
        Ok(Self {
            stop: Some(stop),
            thread: Some(thread),
        })
    }
}

impl Drop for Embedded {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(start: HelperStart, host: Host) -> watch::Sender<Option<livi_sock::Bluez>> {
    let files = crate::mfi::open_in(&host.mfi);
    let link = if files.is_some() {
        LinkPresence::always()
    } else {
        println!(
            "[helper] no MFi files in {}, CarPlay waits for them",
            host.mfi.display()
        );
        LinkPresence::new()
    };
    let chip: Box<dyn AuthCoprocessor + Send> = match files {
        Some(files) => Box::new(files),
        None => Box::new(NoCoprocessor),
    };
    let auth = SharedCoprocessor::new(chip);
    let (identity, cp) = crate::carplay(&start, Transport::Wired);
    let bcast = Broadcaster::default();
    let state = Arc::new(HelperState::default());
    let (bluez, bluez_later) = watch::channel(None);
    serve_android_auto(&host.sockets);
    let sock = LiviSockConfig {
        path: host.sockets.carplay,
        identity: identity.clone(),
        cp: cp.clone(),
        disconnect: None,
        targets: None,
        cp_live: None,
    };
    {
        let (auth, bcast, state) = (auth.clone(), bcast.clone(), state.clone());
        tokio::spawn(async move {
            if let Err(e) = livi_sock::serve(sock, auth, bluez_later, bcast, state).await {
                eprintln!("[helper] CarPlay socket ended: {e}");
            }
        });
    }
    #[cfg(target_os = "android")]
    let airplay_txt = livi_runtime::bonjour::txt_records(
        &livi_runtime::bringup::accessory_id(&cp).unwrap_or_default(),
        &cp.source_version,
        &start.public_key,
        &start.pairing_id,
    );
    let wired = Wired {
        auth,
        identity,
        cp,
        dongle: Dongle::default(),
        bcast,
        state,
        link,
        lockdown: host.lockdown,
        #[cfg(target_os = "android")]
        tun: host.tun,
        #[cfg(target_os = "android")]
        airplay_txt,
    };
    #[cfg(any(target_os = "linux", target_os = "android"))]
    tokio::spawn(crate::wired::watch(wired));
    #[cfg(target_os = "macos")]
    tokio::spawn(crate::wired::watch_usbmuxd(wired));
    bluez
}

fn serve_android_auto(sockets: &HelperSockets) {
    let events = Broadcaster::default();
    let usb = livi_aa::usb::Control::default();
    let deps = AaSockDeps {
        set_wired_phones: Box::new(|_| {}),
        events: events.clone(),
        set_sco_sink: Box::new(|_| {}),
        restart_usb: Box::new({
            let usb = usb.clone();
            move |serial| usb.restart(serial)
        }),
    };
    let path = sockets.android_auto.clone();
    tokio::spawn(async move {
        if let Err(e) = aa_sock::serve(&path, deps).await {
            eprintln!("[helper] Android Auto socket ended: {e}");
        }
    });
    tokio::spawn(crate::aa::watch_usb(usb, sockets.sessions.clone(), events));
}
