// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use idevice::pairing_file::PairingFile;
use idevice::services::lockdown::LockdownClient;
use idevice::{Idevice, IdeviceError};

use iap2_usbmux::{LOCKDOWN_PORT, MuxDevice, MuxStream, short};

use crate::Error;

pub const LOCKDOWN_SERVICE: &str = "com.apple.carkit.service";
pub(crate) const STEP_TIMEOUT: Duration = Duration::from_secs(5);

pub fn pair_record_path(store: &Path, serial: &str) -> Option<PathBuf> {
    let dashed = if serial.len() == 24 && !serial.contains('-') {
        format!("{}-{}", &serial[..8], &serial[8..])
    } else {
        serial.to_string()
    };
    for name in [
        dashed.clone(),
        dashed.to_uppercase(),
        serial.to_string(),
        serial.to_uppercase(),
    ] {
        let p = store.join(format!("{name}.plist"));
        if p.exists() {
            return Some(p);
        }
    }
    None
}

pub struct CarkitChannel {
    idevice: Idevice,
}

impl CarkitChannel {
    pub async fn send(&mut self, data: &[u8]) -> Result<(), Error> {
        self.idevice.send_raw(data).await.map_err(Error::Channel)
    }

    pub async fn recv(&mut self, max: u32) -> Result<Vec<u8>, Error> {
        self.idevice.read_any(max).await.map_err(Error::Channel)
    }

    pub fn into_stream(self) -> Option<Box<dyn idevice::ReadWrite>> {
        self.idevice.get_socket()
    }
}

fn record_file(store: &Path, serial: &str) -> PathBuf {
    let name = if serial.len() == 24 && !serial.contains('-') {
        format!("{}-{}", &serial[..8], &serial[8..])
    } else {
        serial.to_string()
    };
    store.join(format!("{name}.plist"))
}

fn system_buid(store: &Path) -> String {
    let path = store.join("SystemConfiguration.plist");
    plist::Value::from_file(&path)
        .ok()
        .and_then(|v| v.as_dictionary().and_then(|d| d.get("SystemBUID").cloned()))
        .and_then(|v| v.into_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string().to_uppercase())
}

fn save_pair_record(store: &Path, serial: &str, pairing: &PairingFile) -> Result<(), Error> {
    let bytes = pairing
        .clone()
        .serialize()
        .map_err(Error::lockdown("serialize pair record"))?;
    let path = record_file(store, serial);
    std::fs::create_dir_all(store).map_err(Error::Store)?;
    std::fs::write(&path, bytes).map_err(|source| Error::Write {
        path: path.clone(),
        source,
    })?;
    println!("[wired] saved pair record {}", path.display());
    Ok(())
}

async fn connect(dev: &Arc<MuxDevice>, port: u16) -> Result<MuxStream, Error> {
    Ok(dev.host.connect(port).await?)
}

async fn new_lockdown(dev: &Arc<MuxDevice>) -> Result<LockdownClient, Error> {
    let stream = connect(dev, LOCKDOWN_PORT).await?;
    Ok(LockdownClient::new(Idevice::new(
        Box::new(stream),
        "livi-helperd",
    )))
}

pub async fn pair_device(dev: &Arc<MuxDevice>, store: &Path) -> Result<PairingFile, Error> {
    let mut lockdown = new_lockdown(dev).await?;
    let host_id = uuid::Uuid::new_v4().to_string().to_uppercase();
    println!(
        "[wired] {}: pairing — confirm the trust dialog on the phone",
        short(&dev.serial)
    );
    let pairing = lockdown
        .pair(host_id, system_buid(store), Some("LIVI"))
        .await
        .map_err(Error::lockdown("pair"))?;
    save_pair_record(store, &dev.serial, &pairing)?;
    Ok(pairing)
}

async fn within<T>(
    what: &str,
    future: impl Future<Output = Result<T, IdeviceError>>,
) -> Result<T, Error> {
    match tokio::time::timeout(STEP_TIMEOUT, future).await {
        Ok(done) => done.map_err(Error::lockdown(what)),
        Err(_) => Err(Error::Timeout(what.into())),
    }
}

async fn session(
    dev: &Arc<MuxDevice>,
    store: &Path,
) -> Result<(PairingFile, LockdownClient), Error> {
    let stored =
        pair_record_path(store, &dev.serial).and_then(|p| PairingFile::read_from_file(&p).ok());
    if let Some(pairing) = stored {
        let mut lockdown = new_lockdown(dev).await?;
        match within("lockdown session", lockdown.start_session(&pairing)).await {
            Ok(_) => return Ok((pairing, lockdown)),
            Err(e @ Error::Lockdown { .. }) => println!(
                "[wired] {}: stored pair record rejected ({e}), re-pairing",
                short(&dev.serial)
            ),
            Err(e) => return Err(e),
        }
    }
    let pairing = pair_device(dev, store).await?;
    let mut lockdown = new_lockdown(dev).await?;
    within("lockdown session", lockdown.start_session(&pairing)).await?;
    Ok((pairing, lockdown))
}

pub async fn open_carkit(dev: &Arc<MuxDevice>, store: &Path) -> Result<CarkitChannel, Error> {
    let (pairing, mut lockdown) = session(dev, store).await?;
    let (port, ssl) = within(
        &format!("start {LOCKDOWN_SERVICE}"),
        lockdown.start_service(LOCKDOWN_SERVICE),
    )
    .await?;

    let stream = connect(dev, port).await?;
    let mut idevice = Idevice::new(Box::new(stream), "livi-carkit");
    if ssl {
        within("carkit tls", idevice.start_session(&pairing, false)).await?;
    }
    Ok(CarkitChannel { idevice })
}
