//! iPhone -> Mac control transport through the system daemon, without claiming USB
//! interfaces or changing configurations. `open` hands the live carkit stream to
//! iAP2; `probe` only checks that it opens. Neither authenticates a CarPlay accessory.

use std::future::Future;
use std::time::Duration;

use idevice::IdeviceError;
use idevice::provider::{IdeviceProvider, UsbmuxdProvider};
use idevice::services::lockdown::LockdownClient;
use idevice::usbmuxd::{Connection, UsbmuxdAddr, UsbmuxdDevice};

use crate::LOCKDOWN_SERVICE;

const STEP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Discovery,
    Lockdown,
    Product,
    PairRecord,
    Session,
    Carkit,
    CarkitTls,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeError {
    pub stage: Stage,
    pub detail: String,
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.stage, self.detail)
    }
}

async fn step<T>(
    stage: Stage,
    future: impl Future<Output = Result<T, IdeviceError>>,
) -> Result<T, ProbeError> {
    match tokio::time::timeout(STEP_TIMEOUT, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => {
            let detail = match std::error::Error::source(&error) {
                Some(cause) => format!("{error}: {cause}"),
                None => error.to_string(),
            };
            Err(ProbeError { stage, detail })
        }
        Err(_) => Err(ProbeError {
            stage,
            detail: "timed out after 5 seconds".into(),
        }),
    }
}

/// Always the local system socket, ignoring remote USB/dongle environment overrides.
pub async fn devices() -> Result<Vec<UsbmuxdDevice>, ProbeError> {
    let mut mux = step(Stage::Discovery, UsbmuxdAddr::default().connect(0)).await?;
    let devices = step(Stage::Discovery, mux.get_devices()).await?;
    Ok(usb_only(devices))
}

fn usb_only(devices: Vec<UsbmuxdDevice>) -> Vec<UsbmuxdDevice> {
    devices
        .into_iter()
        .filter(|d| d.connection_type == Connection::Usb)
        .collect()
}

/// Use the existing macOS trust record in memory, without copying or rewriting it.
/// The caller should ask the user to trust the Mac in Finder if it is unavailable.
pub async fn probe(device: &UsbmuxdDevice) -> Result<(), ProbeError> {
    open(device).await.map(drop)
}

/// Keeps the carkit stream alive for the caller's iAP2 session. Dropping it closes the USB
/// tunnel. The macOS pairing record stays owned by the system daemon.
pub async fn open(device: &UsbmuxdDevice) -> Result<Box<dyn idevice::ReadWrite>, ProbeError> {
    if device.connection_type != Connection::Usb {
        return Err(ProbeError {
            stage: Stage::Discovery,
            detail: "device is not connected by USB".into(),
        });
    }
    open_provider(&UsbmuxdProvider {
        addr: UsbmuxdAddr::default(),
        tag: 0,
        udid: device.udid.clone(),
        device_id: device.device_id,
        label: "livi-usbmuxd".into(),
    })
    .await
}

#[cfg(test)]
async fn probe_provider(provider: &dyn IdeviceProvider) -> Result<(), ProbeError> {
    open_provider(provider).await.map(drop)
}

async fn open_provider(
    provider: &dyn IdeviceProvider,
) -> Result<Box<dyn idevice::ReadWrite>, ProbeError> {
    let connection = step(
        Stage::Lockdown,
        provider.connect(LockdownClient::LOCKDOWND_PORT),
    )
    .await?;
    let mut lockdown = LockdownClient::new(connection);
    let product = step(
        Stage::Product,
        lockdown.get_value(Some("ProductType"), None),
    )
    .await?;
    if !product.as_string().is_some_and(|s| s.starts_with("iPhone")) {
        return Err(ProbeError {
            stage: Stage::Product,
            detail: "USB device is not an iPhone".into(),
        });
    }
    let pairing = step(Stage::PairRecord, provider.get_pairing_file()).await?;
    step(Stage::Session, lockdown.start_session(&pairing)).await?;
    let (port, ssl) = step(Stage::Carkit, lockdown.start_service(LOCKDOWN_SERVICE)).await?;
    let mut channel = step(Stage::Carkit, provider.connect(port)).await?;
    if ssl {
        step(Stage::CarkitTls, channel.start_session(&pairing, false)).await?;
    }
    channel.get_socket().ok_or_else(|| ProbeError {
        stage: Stage::Carkit,
        detail: "carkit stream unavailable after opening the service".into(),
    })
}

#[cfg(test)]
mod tests;
