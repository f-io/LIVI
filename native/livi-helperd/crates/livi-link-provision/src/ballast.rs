//! The CarlinKit userspace our init never starts, and the libraries that belong to it.
//!
//! Binaries go unconditionally. A library only goes when no binary we keep still references it —
//! measured on the device by scanning the kept ELFs for sonames, not assumed from a list.

use std::collections::HashSet;
use std::time::Duration;

use crate::shell::Shell;

/// Vendor daemons; none of them is started by `livi-bringup.sh`.
pub const FILES: [&str; 18] = [
    "/usr/sbin/ARMAndroidAuto",
    "/usr/sbin/AppleCarPlay",
    "/usr/sbin/ARMadb-driver",
    "/usr/sbin/ARMiPhoneIAP2",
    "/usr/sbin/bluetoothDaemon",
    "/usr/sbin/mdnsd",
    "/usr/sbin/hfpd",
    "/usr/sbin/dbus-daemon",
    "/usr/sbin/hcid",
    "/usr/sbin/ARMHiCar",
    "/usr/sbin/ARMandroid_Mirror",
    "/usr/sbin/usbmuxd",
    "/usr/sbin/boxNetworkService",
    "/usr/sbin/AutomaticTest",
    "/usr/sbin/colorLightDaemon",
    "/etc/BoxHelper.apk",
    // The vendor uploader, replaced by our own page.
    "/etc/boa/cgi-bin/upload.cgi",
    // An earlier hand-placed copy; the stack now lives in /script/livi.
    "/usr/sbin/mfid",
];

/// Their shared libraries. Deleted only when nothing we keep needs them.
pub const LIBS: [&str; 20] = [
    "/usr/lib/libxml2.so.2.9.2",
    "/usr/lib/libfdk-aac.so.1.0.0",
    "/usr/lib/libdmsdpplatform.so",
    "/usr/lib/libdmsdp.so",
    "/usr/lib/libHwKeystoreSDK.so",
    "/usr/lib/libHisightSink.so",
    "/usr/lib/libnearby.so",
    "/usr/lib/libHwDeviceAuthSDK.so",
    "/usr/lib/libdmsdpdvaudio.so",
    "/usr/lib/libdmsdpaudiohandler.so",
    "/usr/lib/libauthagent.so",
    "/usr/lib/libhicar.so",
    "/usr/lib/libdmsdpdvcamera.so",
    "/usr/lib/libmanagement.so",
    "/usr/lib/libsecurec.so",
    "/usr/lib/libdmsdpsec.so",
    "/usr/lib/libdbus-1.so.3.2.0",
    "/usr/lib/libusb-1.0.so.0.1.0",
    "/usr/lib/libcrypto.so.1.1",
    "/usr/lib/libssl.so.1.1",
];
// Everything else stays: busybox, boa + /etc/boa (web UI, cgi), hostapd, hciattach/hciconfig/
// hcitool, fw_loader_linux, udhcpd, telnetd, small vendor tools, all modules and firmware.
//
// Never strip, however aggressive this list gets: flash_erase and busybox (the restore stages
// them into tmpfs), and /script/*.sh (init_bluetooth_wifi.sh knows every WiFi module).

/// Matches a soname inside an ELF, e.g. `libxml2.so.2`.
const LIB_RE: &str = r"lib[A-Za-z0-9_+.-]*\.so[.0-9]*";
const SCAN_TIMEOUT: Duration = Duration::from_secs(180);

/// `libxml2.so.2.9.2` and `libxml2.so.2` both reduce to `libxml2.so`, so a file name and the
/// soname an ELF carries compare equal.
pub fn stem(name: &str) -> &str {
    let base = name.rsplit('/').next().unwrap_or(name);
    match base.find(".so") {
        Some(i) => &base[..i + 3],
        None => base,
    }
}

/// The sonames the ELFs we keep reference, followed through kept libraries as well.
pub fn needed_libs(sh: &Shell, excluded: &[String]) -> Result<HashSet<String>, String> {
    let excl = excluded.join(" ");
    let first = format!(
        "for f in /usr/sbin/* /bin/* /sbin/* /usr/bin/* /etc/boa/cgi-bin/*; do \
         [ -f \"$f\" ] && [ ! -L \"$f\" ] || continue; \
         case \" {excl} \" in *\" $f \"*) continue;; esac; \
         grep -a -o -E '{LIB_RE}' \"$f\"; done 2>/dev/null | sort -u"
    );
    let mut needed: HashSet<String> =
        sh.run(&first, SCAN_TIMEOUT)?.split_whitespace().map(str::to_string).collect();

    let libs: Vec<String> = sh
        .sh("ls /usr/lib/*.so* /lib/*.so* 2>/dev/null")?
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let mut scanned: HashSet<String> = HashSet::new();
    loop {
        let want: HashSet<&str> = needed.iter().map(|n| stem(n)).collect();
        let todo: Vec<String> = libs
            .iter()
            .filter(|l| {
                !excluded.contains(l) && !scanned.contains(*l) && want.contains(stem(l))
            })
            .cloned()
            .collect();
        if todo.is_empty() {
            return Ok(needed);
        }
        let more = sh.run(
            &format!(
                "for f in {}; do [ -L \"$f\" ] && continue; grep -a -o -E '{LIB_RE}' \"$f\"; \
                 done 2>/dev/null | sort -u",
                todo.join(" ")
            ),
            SCAN_TIMEOUT,
        )?;
        scanned.extend(todo);
        needed.extend(more.split_whitespace().map(str::to_string));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduces_versioned_names_to_the_soname() {
        assert_eq!(stem("/usr/lib/libxml2.so.2.9.2"), "libxml2.so");
        assert_eq!(stem("libxml2.so.2"), "libxml2.so");
        assert_eq!(stem("/usr/lib/libdmsdp.so"), "libdmsdp.so");
        assert_eq!(stem("/bin/busybox"), "busybox");
    }
}
