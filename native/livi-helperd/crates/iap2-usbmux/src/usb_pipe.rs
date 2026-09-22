// Local phone: the mux pipes are the CarPlay bulk endpoints on usbfs.

use std::time::Duration;

use nusb::transfer::{Bulk, Buffer, In, Out, TransferError};
use nusb::{Endpoint, Interface, MaybeFuture};

use crate::linux::{find_iphones, open_by_address};
use crate::pipe::{MuxPipes, MuxReader, MuxWriter};
use crate::{EP_IN, EP_OUT};

const USBMUX_IFACE: u8 = 1;
const READ_CHUNK: usize = 65536;

pub struct UsbWriter(Endpoint<Bulk, Out>);
pub struct UsbReader(Endpoint<Bulk, In>);

impl MuxWriter for UsbWriter {
    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        let mut buf = Buffer::new(data.len());
        buf.extend_from_slice(data);
        let completion = self.0.transfer_blocking(buf, Duration::from_millis(2000));
        completion.status.map_err(|e| format!("bulk write: {e}"))
    }
}

impl MuxReader for UsbReader {
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, String> {
        let completion = self.0.transfer_blocking(Buffer::new(READ_CHUNK), timeout);
        match completion.status {
            Ok(()) => Ok(completion.buffer.into_vec()),
            // a timeout is an empty read the caller retries; anything else ends the pipe
            Err(TransferError::Cancelled) => Ok(Vec::new()),
            Err(e) => Err(format!("bulk read: {e}")),
        }
    }
}

/// Claims the usbmux interface and hands back its bulk pipes.
pub fn open_pipes(serial: &str) -> Result<MuxPipes, String> {
    let dev = find_iphones()
        .into_iter()
        .find(|d| d.serial == serial)
        .ok_or_else(|| format!("iphone {serial} not found"))?;
    let device = open_by_address(dev.bus, dev.address)?;

    // The phone needs a moment after the config switch before it hands out the interface.
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
