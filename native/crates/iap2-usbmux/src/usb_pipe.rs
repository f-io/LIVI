// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::thread::sleep;
use std::time::{Duration, Instant};

use livi_session_io::usb::UsbStream;
use nusb::transfer::{ControlIn, ControlType, Recipient};
use nusb::{Device, MaybeFuture};

#[cfg(target_os = "android")]
use crate::android::open;
#[cfg(target_os = "linux")]
use crate::linux::open;
use crate::{EP_IN, EP_OUT, short};

const USBMUX_IFACE: u8 = 1;
const CLAIM_TRIES: usize = 12;
const CLAIM_RETRY: Duration = Duration::from_millis(300);
const CONFIGS_WAIT: Duration = Duration::from_secs(5);
const CONFIGS_POLL: Duration = Duration::from_millis(200);

pub(crate) fn await_carplay_configs(
    serial: &str,
    exposed: impl Fn() -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + CONFIGS_WAIT;
    loop {
        sleep(CONFIGS_POLL);
        if exposed() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "iphone {} did not expose CarPlay configs",
                short(serial)
            ));
        }
    }
}

pub fn request_carplay_configs(device: &Device) -> Result<(), String> {
    device
        .control_in(
            ControlIn {
                control_type: ControlType::Vendor,
                recipient: Recipient::Device,
                request: 0x52,
                value: 0x0000,
                index: 0x0004,
                length: 1,
            },
            Duration::from_secs(1),
        )
        .wait()
        .map_err(|e| format!("vendor request 0x52: {e}"))?;
    Ok(())
}

pub async fn open_stream(serial: &str) -> Result<UsbStream, String> {
    let device = open(serial)?;
    let pipe = livi_session_io::usb::claim(
        &device,
        USBMUX_IFACE,
        EP_IN,
        EP_OUT,
        CLAIM_TRIES,
        CLAIM_RETRY,
    )
    .await
    .map_err(|e| format!("usbmux interface: {e}"))?;
    Ok(UsbStream::new(pipe.with_zlp()))
}
