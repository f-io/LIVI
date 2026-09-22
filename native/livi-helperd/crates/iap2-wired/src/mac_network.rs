//! macOS only: the iPhone's own CarPlay AV interfaces (enX). macOS binds them itself
//! (AppleUSBNCMData), found by the phone's USB serial, not a fixed USB interface number.

use tokio::process::Command;

fn normalize(serial: &str) -> String {
    serial.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase()
}

/// The `AppleUSBNCMData` interfaces (enX) macOS bound for this iPhone. Usually two, the AV path
/// takes one.
pub async fn ncm_interfaces(udid: &str) -> Result<Vec<String>, String> {
    let want = normalize(udid);
    let out = Command::new("/usr/sbin/ioreg")
        .args(["-r", "-n", "iPhone", "-l", "-w", "0"])
        .output()
        .await
        .map_err(|e| format!("ioreg: {e}"))?;
    if !out.status.success() {
        return Err("ioreg -n iPhone failed".into());
    }
    let text = String::from_utf8_lossy(&out.stdout);

    // ioreg lines carry a "  | | " tree prefix, so pull each quoted value out of the line rather
    // than matching from its start.
    fn quoted_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
        line.split_once(&format!("\"{key}\" = \"")).and_then(|(_, r)| r.split('"').next())
    }

    for block in text.split("+-o iPhone@") {
        let serial = block.lines().find_map(|l| quoted_value(l, "USB Serial Number"));
        if serial.map(normalize).as_deref() != Some(want.as_str()) {
            continue;
        }
        // The BSD name sits below its driver. Take the ones under AppleUSBNCMData, not the plain
        // USB-Ethernet function (AppleUSBEthernetHostAQM).
        let mut ifaces = Vec::new();
        let mut under_ncm = false;
        for l in block.lines() {
            match quoted_value(l, "IOClass") {
                Some("AppleUSBNCMData") => under_ncm = true,
                Some("AppleUSBEthernetHostAQM") => under_ncm = false,
                _ => {}
            }
            if under_ncm && let Some(name) = quoted_value(l, "BSD Name") {
                ifaces.push(name.to_string());
                under_ncm = false;
            }
        }
        return Ok(ifaces);
    }
    Err("iPhone serial not found in the IORegistry".into())
}

/// The first CarPlay NCM interface for this iPhone.
pub async fn discover(udid: &str) -> Result<String, String> {
    ncm_interfaces(udid)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| "no AppleUSBNCMData interface for this iPhone".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_ncm_bsd_names_by_serial_and_skips_plain_ethernet() {
        // Not a real ioreg dump, but the shape the parser keys on.
        let block_serials = normalize("00008120-000924CE2E51A01E");
        assert_eq!(block_serials, "00008120000924ce2e51a01e");
    }
}
