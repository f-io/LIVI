use std::path::PathBuf;
use std::sync::Arc;

use idevice::pairing_file::PairingFile;
use idevice::services::lockdown::LockdownClient;
use idevice::Idevice;

use iap2_usbmux::{AsyncMuxStream, MuxDevice, LOCKDOWN_PORT};

pub const LOCKDOWN_SERVICE: &str = "com.apple.carkit.service";

/// Pair record store: usbmuxd's `/var/lib/lockdown` on Linux, the LIVI user folder on macOS;
/// LIVI_LOCKDOWN_STORE overrides both.
fn lockdown_store() -> PathBuf {
    if let Some(dir) = std::env::var_os("LIVI_LOCKDOWN_STORE") {
        return PathBuf::from(dir);
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join("Library/Application Support/LIVI/lockdown");
    }
    PathBuf::from("/var/lib/lockdown")
}

/// The lockdown pair record for a device. The serial from sysfs has no dashes, while the
/// stored record uses the dashed UDID form, so both spellings are tried.
pub fn pair_record_path(serial: &str) -> Option<PathBuf> {
    let dashed = if serial.len() == 24 && !serial.contains('-') {
        format!("{}-{}", &serial[..8], &serial[8..])
    } else {
        serial.to_string()
    };
    for name in [dashed.clone(), dashed.to_uppercase(), serial.to_string(), serial.to_uppercase()] {
        let p = lockdown_store().join(format!("{name}.plist"));
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// The carkit TLS channel: raw iAP2 bytes in both directions.
pub struct CarkitChannel {
    idevice: Idevice,
}

impl CarkitChannel {
    pub async fn send(&mut self, data: &[u8]) -> Result<(), String> {
        self.idevice.send_raw(data).await.map_err(|e| e.to_string())
    }

    pub async fn recv(&mut self, max: u32) -> Result<Vec<u8>, String> {
        self.idevice.read_any(max).await.map_err(|e| e.to_string())
    }

    /// Hands out the TLS stream so the iAP2 link layer can drive it directly.
    pub fn into_stream(self) -> Option<Box<dyn idevice::ReadWrite>> {
        self.idevice.get_socket()
    }
}

fn record_file(serial: &str) -> PathBuf {
    let name = if serial.len() == 24 && !serial.contains('-') {
        format!("{}-{}", &serial[..8], &serial[8..])
    } else {
        serial.to_string()
    };
    lockdown_store().join(format!("{name}.plist"))
}

fn system_buid() -> String {
    let path = lockdown_store().join("SystemConfiguration.plist");
    plist::Value::from_file(&path)
        .ok()
        .and_then(|v| v.as_dictionary().and_then(|d| d.get("SystemBUID").cloned()))
        .and_then(|v| v.into_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string().to_uppercase())
}

fn save_pair_record(serial: &str, pairing: &PairingFile) -> Result<(), String> {
    let bytes = pairing.clone().serialize().map_err(|e| format!("serialize pair record: {e}"))?;
    let path = record_file(serial);
    std::fs::create_dir_all(lockdown_store()).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    println!("[wired] saved pair record {}", path.display());
    Ok(())
}

async fn new_lockdown(dev: &Arc<MuxDevice>) -> Result<LockdownClient, String> {
    let conn = dev.host.connect(LOCKDOWN_PORT)?;
    let stream = AsyncMuxStream::new(conn);
    Ok(LockdownClient::new(Idevice::new(Box::new(stream), "livi-helperd")))
}

/// Pairs with the phone, which shows the trust dialog; the device must be unlocked.
pub async fn pair_device(dev: &Arc<MuxDevice>) -> Result<PairingFile, String> {
    let mut lockdown = new_lockdown(dev).await?;
    let host_id = uuid::Uuid::new_v4().to_string().to_uppercase();
    println!("[wired] {}: pairing — confirm the trust dialog on the phone", short(&dev.serial));
    let pairing = lockdown
        .pair(host_id, system_buid(), Some("LIVI"))
        .await
        .map_err(|e| format!("pair: {e}"))?;
    save_pair_record(&dev.serial, &pairing)?;
    Ok(pairing)
}

fn short(serial: &str) -> &str {
    &serial[..8.min(serial.len())]
}

/// Loads the stored pair record, pairing first when there is none or the device no longer
/// accepts it.
pub async fn ensure_pairing(dev: &Arc<MuxDevice>) -> Result<PairingFile, String> {
    let existing = pair_record_path(&dev.serial)
        .and_then(|p| PairingFile::read_from_file(&p).ok());

    let Some(pairing) = existing else {
        return pair_device(dev).await;
    };

    let mut lockdown = new_lockdown(dev).await?;
    match lockdown.start_session(&pairing).await {
        Ok(_) => Ok(pairing),
        Err(e) => {
            println!("[wired] {}: stored pair record rejected ({e}), re-pairing", short(&dev.serial));
            pair_device(dev).await
        }
    }
}

/// Opens com.apple.carkit.service on a phone that already has its usbmux transport up.
pub async fn open_carkit(dev: &Arc<MuxDevice>) -> Result<CarkitChannel, String> {
    let pairing = ensure_pairing(dev).await?;

    let mut lockdown = new_lockdown(dev).await?;
    lockdown.start_session(&pairing).await.map_err(|e| format!("lockdown session: {e}"))?;
    let (port, ssl) = lockdown
        .start_service(LOCKDOWN_SERVICE)
        .await
        .map_err(|e| format!("start {LOCKDOWN_SERVICE}: {e}"))?;

    let conn = dev.host.connect(port)?;
    let stream = AsyncMuxStream::new(conn);
    let mut idevice = Idevice::new(Box::new(stream), "livi-carkit");
    if ssl {
        idevice
            .start_session(&pairing, false)
            .await
            .map_err(|e| format!("carkit tls: {e}"))?;
    }
    Ok(CarkitChannel { idevice })
}
