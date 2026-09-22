// Ported from bin/livi-bt-up/src/main.rs — see livid dispatcher in main.rs.
pub fn run(_args: Vec<String>) -> i32 {
    // ExitCode is opaque; the module either loops forever (returning implicitly)
    // or calls std::process::exit on setup failure. Treat any normal return as 0.
    let _ = livid_main();
    0
}

// livi-bt-up — ioctl(HCIDEVUP) then ioctl(HCIGETDEVINFO) so hci0 opens and we can
// extract the MAC even when the vendor driver skips the standard bluetooth-core
// sysfs attribute group. Writes MAC to /tmp/livi/bt-mac for livi-httpd to read.

use std::fs;
use std::io::Write;

const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_HCI: libc::c_int = 1;
// HCIDEVUP = _IOW('H', 201, int)          = 0x400448c9
// HCIGETDEVINFO = _IOR('H', 211, int)     = 0x800448d3 (with a struct hci_dev_info buffer)
const HCIDEVUP: libc::c_ulong = 0x400448c9;
const HCIGETDEVINFO: libc::c_ulong = 0x800448d3;

// struct hci_dev_info from <net/bluetooth/hci.h>. Only the first ~28 bytes matter
// for our purposes; we pass a zeroed buffer with dev_id set to the target index.
#[repr(C)]
struct HciDevInfo {
    dev_id: u16,
    name: [u8; 8],
    bdaddr: [u8; 6],
    flags: u32,
    dev_type: u8,
    _pad: [u8; 7],
    features: [u8; 8],
    _rest: [u8; 128],
}

fn livid_main() -> std::process::ExitCode {
    let dev: libc::c_int = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let sock = unsafe { libc::socket(AF_BLUETOOTH, libc::SOCK_RAW, BTPROTO_HCI) };
    if sock < 0 {
        eprintln!("livi-bt-up: socket(AF_BLUETOOTH): {}", std::io::Error::last_os_error());
        return 1.into();
    }

    // 1) Bring hci up (idempotent — EALREADY is fine).
    let r = unsafe { libc::ioctl(sock, HCIDEVUP as _, dev) };
    if r < 0 {
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::EALREADY) {
            eprintln!("livi-bt-up: HCIDEVUP hci{dev}: {e}");
            unsafe { libc::close(sock); }
            return 1.into();
        }
    }

    // 2) Ask the driver for device info; use the returned MAC.
    let mut info: HciDevInfo = unsafe { std::mem::zeroed() };
    info.dev_id = dev as u16;
    let r = unsafe { libc::ioctl(sock, HCIGETDEVINFO as _, &mut info as *mut _) };
    unsafe { libc::close(sock); }
    if r < 0 {
        eprintln!("livi-bt-up: HCIGETDEVINFO hci{dev}: {}", std::io::Error::last_os_error());
        return 2.into();
    }

    // bdaddr is stored little-endian; MAC address strings are printed big-endian.
    let mac = format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        info.bdaddr[5], info.bdaddr[4], info.bdaddr[3],
        info.bdaddr[2], info.bdaddr[1], info.bdaddr[0]
    );
    // The kernel packs bus (low nibble) and dev_type (bits 4-5) into one byte; an "amp"
    // here is why mgmt refuses the controller — it only manages primary ones.
    let bus = info.dev_type & 0x0f;
    let kind = match (info.dev_type >> 4) & 0x03 {
        0 => "primary",
        1 => "amp",
        _ => "?",
    };
    println!("livi-bt-up: hci{dev} up, MAC {mac}, bus {bus}, type {kind}");

    // Persist so livi-httpd can display it (vendor driver skips the standard sysfs group).
    let _ = fs::create_dir_all("/tmp/livi");
    if let Ok(mut f) = fs::File::create("/tmp/livi/bt-mac") {
        let _ = writeln!(f, "{mac}");
    }
    0.into()
}
