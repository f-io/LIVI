// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::thread::sleep;
use std::time::{Duration, Instant};

use nusb::transfer::{Buffer, Bulk, ControlIn, ControlType, In, Out, Recipient, TransferError};
use nusb::{Device, Endpoint, Interface, MaybeFuture};

#[cfg(target_os = "android")]
use crate::android::open;
#[cfg(target_os = "linux")]
use crate::linux::open;
use crate::pipe::{MuxPipes, MuxReader, MuxWriter};
use crate::{EP_IN, EP_OUT, short};

const USBMUX_IFACE: u8 = 1;
const READ_CHUNK: usize = 65536;
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

pub struct UsbWriter(Endpoint<Bulk, Out>);
pub struct UsbReader(Endpoint<Bulk, In>);

impl MuxWriter for UsbWriter {
    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        let mut buf = Buffer::new(data.len());
        buf.extend_from_slice(data);
        let completion = self.0.transfer_blocking(buf, Duration::from_millis(2000));
        completion.status.map_err(|e| format!("bulk write: {e}"))?;
        if ends_on_packet(data.len(), self.0.max_packet_size()) {
            let zlp = self
                .0
                .transfer_blocking(Buffer::new(0), Duration::from_millis(2000));
            zlp.status
                .map_err(|e| format!("bulk write, empty packet: {e}"))?;
        }
        Ok(())
    }
}

fn ends_on_packet(len: usize, max_packet: usize) -> bool {
    len != 0 && len.is_multiple_of(max_packet)
}

impl MuxReader for UsbReader {
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, String> {
        let completion = self.0.transfer_blocking(Buffer::new(READ_CHUNK), timeout);
        match completion.status {
            Ok(()) => Ok(completion.buffer.into_vec()),
            Err(TransferError::Cancelled) => Ok(Vec::new()),
            Err(e) => Err(format!("bulk read: {e}")),
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

pub fn open_pipes(serial: &str) -> Result<MuxPipes, String> {
    let device = open(serial)?;

    let mut last = String::new();
    let mut iface: Option<Interface> = None;
    for _ in 0..12 {
        match device.claim_interface(USBMUX_IFACE).wait() {
            Ok(i) => {
                iface = Some(i);
                break;
            }
            Err(e) => {
                last = e.to_string();
                std::thread::sleep(Duration::from_millis(300));
            }
        }
    }
    let iface = iface.ok_or_else(|| format!("claim usbmux interface: {last}"))?;

    let ep_out = iface
        .endpoint::<Bulk, Out>(EP_OUT)
        .map_err(|e| format!("usbmux out endpoint: {e}"))?;
    let ep_in = iface
        .endpoint::<Bulk, In>(EP_IN)
        .map_err(|e| format!("usbmux in endpoint: {e}"))?;
    Ok((Box::new(UsbWriter(ep_out)), Box::new(UsbReader(ep_in))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_transfer_that_fills_its_last_packet_needs_an_empty_one() {
        assert!(ends_on_packet(512, 512));
        assert!(ends_on_packet(1024, 512));
        assert!(!ends_on_packet(476, 512));
        assert!(!ends_on_packet(513, 512));
        assert!(!ends_on_packet(0, 512));
        assert!(!ends_on_packet(512, 0));
    }
}
