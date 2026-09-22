// Enumerate USB devices, open one, list every configuration with its interfaces, optionally
// claim each interface of the active one.
// Cross-platform nusb groundwork for talking to a CDC device (the STM bridge).
//   usb-probe               list every device
//   usb-probe cafe          only VID cafe, dump its interfaces
//   usb-probe cafe --claim  also claim/release each interface

use nusb::{DeviceInfo, MaybeFuture};
use std::process::ExitCode;

fn main() -> ExitCode {
    let vid = std::env::args().nth(1).and_then(|a| u16::from_str_radix(a.trim_start_matches("0x"), 16).ok());
    let claim = std::env::args().any(|a| a == "--claim");

    let Ok(list) = nusb::list_devices().wait() else {
        eprintln!("[usb-probe] list_devices failed");
        return ExitCode::FAILURE;
    };
    let devices: Vec<DeviceInfo> = list.filter(|d| vid.is_none_or(|v| d.vendor_id() == v)).collect();
    if devices.is_empty() {
        eprintln!("[usb-probe] no matching device");
        return ExitCode::FAILURE;
    }

    for info in devices {
        println!(
            "[usb-probe] {:04x}:{:04x} {}",
            info.vendor_id(),
            info.product_id(),
            info.product_string().unwrap_or("?")
        );
        if vid.is_none() {
            continue;
        }
        let Ok(device) = info.open().wait() else {
            println!("[usb-probe]   open failed");
            continue;
        };
        let Ok(cfg) = device.active_configuration() else { continue };
        let mut ifaces: Vec<u8> = cfg.interface_alt_settings().map(|d| d.interface_number()).collect();
        ifaces.sort_unstable();
        ifaces.dedup();
        let active = cfg.configuration_value();
        for cfg in device.configurations() {
            let value = cfg.configuration_value();
            let mark = if value == active { " (active)" } else { "" };
            println!("[usb-probe]   config {value}{mark}");
            for desc in cfg.interface_alt_settings() {
                println!(
                    "[usb-probe]     iface {} alt {} class {:02x}/{:02x}/{:02x}",
                    desc.interface_number(),
                    desc.alternate_setting(),
                    desc.class(),
                    desc.subclass(),
                    desc.protocol()
                );
            }
        }
        if claim {
            for n in ifaces {
                match device.claim_interface(n).wait() {
                    Ok(i) => {
                        println!("[usb-probe]   claim {n} OK");
                        drop(i);
                    }
                    Err(e) => println!("[usb-probe]   claim {n} denied: {e}"),
                }
            }
        }
    }
    ExitCode::SUCCESS
}
