// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use tokio::process::Command;

const NCM_DATA: [&str; 2] = ["AppleUSBNCMData", "AppleUSBNCM11Data"];

fn normalize(serial: &str) -> String {
    serial
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

pub async fn ncm_interfaces(udid: &str) -> Result<Vec<String>, String> {
    let out = Command::new("/usr/sbin/ioreg")
        .args(["-r", "-n", "iPhone", "-l", "-w", "0"])
        .output()
        .await
        .map_err(|e| format!("ioreg: {e}"))?;
    if !out.status.success() {
        return Err("ioreg -n iPhone failed".into());
    }
    ncm_interfaces_in(&String::from_utf8_lossy(&out.stdout), udid)
}

fn quoted_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_once(&format!("\"{key}\" = \""))
        .and_then(|(_, r)| r.split('"').next())
}

fn node_class(line: &str) -> Option<&str> {
    line.contains("+-o ").then_some(())?;
    line.split_once("<class ")?.1.split([',', '>']).next()
}

fn ncm_interfaces_in(ioreg: &str, udid: &str) -> Result<Vec<String>, String> {
    let want = normalize(udid);
    for block in ioreg.split("+-o iPhone@") {
        let serial = block
            .lines()
            .find_map(|l| quoted_value(l, "USB Serial Number"));
        if serial.map(normalize).as_deref() != Some(want.as_str()) {
            continue;
        }
        let mut ifaces = Vec::new();
        let mut under_ncm = false;
        let mut restricted = false;
        for l in block.lines() {
            if let Some(class) = node_class(l) {
                restricted = class.contains("Restricted");
            }
            match quoted_value(l, "IOClass") {
                Some(class) if NCM_DATA.contains(&class) => under_ncm = true,
                Some("AppleUSBEthernetHostAQM") => under_ncm = false,
                _ => {}
            }
            if under_ncm && let Some(name) = quoted_value(l, "BSD Name") {
                if !restricted {
                    ifaces.push(name.to_string());
                }
                under_ncm = false;
            }
        }
        return Ok(ifaces);
    }
    Err("iPhone serial not found in the IORegistry".into())
}

pub async fn discover(udid: &str) -> Result<String, String> {
    ncm_interfaces(udid)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| "no NCM data interface for this iPhone".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NCM11: &str = r#"
+-o iPhone@01100000  <class IOUSBHostDevice, id 0x100009fd5, registered, matched, active>
  |   "USB Serial Number" = "0000814000012345ABCDEF01"
  +-o Apple USB Multiplexor@1  <class IOUSBHostInterface, id 0x100009fe0, registered>
  | +-o usbmuxd  <class AppleUSBHostInterfaceUserClient, id 0x100009fed, !registered>
  +-o NCM Control@2  <class IOUSBHostInterface, id 0x100009fe2, registered, matched>
  | +-o AppleUSBNCM11Control  <class AppleUSBNCM11Control, id 0x100009fe6, registered>
  |   |   "IOClass" = "AppleUSBNCM11Control"
  |   +-o AppleUSBNCM11Data  <class AppleUSBNCM11Data, id 0x100009ff3, registered>
  |     |   "IOClass" = "AppleUSBNCM11Data"
  |     +-o en13  <class IOEthernetInterface, id 0x100009ff6, registered, matched>
  |       |   "BSD Name" = "en13"
  |       +-o IONetworkStack  <class IONetworkStack, id 0x1000004ce, registered>
  |         |   "IOClass" = "IONetworkStack"
  +-o NCM Control@4  <class IOUSBHostInterface, id 0x100009fe4, registered, matched>
  | +-o AppleUSBNCM11Control  <class AppleUSBNCM11Control, id 0x100009fe8, registered>
  |   |   "IOClass" = "AppleUSBNCM11Control"
  |   +-o AppleUSBNCM11Data  <class AppleUSBNCM11Data, id 0x100009fef, registered>
  |     |   "IOClass" = "AppleUSBNCM11Data"
  |     +-o anri0  <class AppleUSBHostNCMRestrictedEthernetInterface, id 0x100009ff2, registered>
  |       |   "BSD Name" = "anri0"
"#;

    #[test]
    fn serials_compare_without_dashes_or_case() {
        assert_eq!(
            normalize("00008120-000924CE2E51A01E"),
            "00008120000924ce2e51a01e"
        );
    }

    #[test]
    fn ncm_1_1_gives_its_ethernet_interface_and_skips_the_restricted_one() {
        let ifaces = ncm_interfaces_in(NCM11, "00008140-00012345ABCDEF01").unwrap();
        assert_eq!(ifaces, ["en13"]);
    }

    #[test]
    fn ncm_1_0_is_found_as_before() {
        let ncm10 = NCM11.replace("NCM11", "NCM");
        assert_eq!(
            ncm_interfaces_in(&ncm10, "00008140-00012345ABCDEF01").unwrap(),
            ["en13"]
        );
    }

    #[test]
    fn another_iphone_is_not_taken() {
        assert!(ncm_interfaces_in(NCM11, "00008120-000924CE2E51A01E").is_err());
    }
}
