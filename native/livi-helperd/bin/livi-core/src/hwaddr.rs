use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

const BT_SYSFS: &str = "/sys/class/bluetooth";
const NET_SYSFS: &str = "/sys/class/net";
pub const FALLBACK: &str = "AA:BB:CC:DD:EE:FF";

fn is_mac(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    parts.len() == 6
        && parts.iter().all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn mac_in(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let raw = raw.trim();
    is_mac(raw).then(|| raw.to_uppercase())
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// The LIVI Link's controller sits on vhci.
fn tunnelled(dir: &Path, hci: &str) -> bool {
    fs::canonicalize(dir.join(hci)).is_ok_and(|p| p.to_string_lossy().contains("/devices/virtual/"))
}

fn bt_mac_in(dir: &Path, adapter: &str) -> Result<Option<String>, ()> {
    let adapter = if adapter == livi_link_host::link::CHOICE {
        match entries(dir).into_iter().find(|n| tunnelled(dir, n)) {
            Some(hci) => hci,
            None => return Err(()),
        }
    } else {
        adapter.to_string()
    };
    let candidates = if adapter.is_empty() {
        entries(dir).into_iter().filter(|n| n.starts_with("hci")).collect()
    } else {
        vec![adapter]
    };
    Ok(candidates.iter().find_map(|name| mac_in(&dir.join(name).join("address"))))
}

/// BlueZ knows the address where sysfs does not show it.
fn busctl(adapter: &str) -> Option<String> {
    let out = Command::new("busctl")
        .args(["--system", "get-property", "org.bluez"])
        .arg(format!("/org/bluez/{adapter}"))
        .args(["org.bluez.Adapter1", "Address"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let quoted = text.split('"').nth(1)?;
    is_mac(quoted).then(|| quoted.to_uppercase())
}

pub fn bt_mac(adapter: &str) -> Option<String> {
    if let Ok(mac) = std::env::var("AA_BT_MAC") {
        return Some(mac);
    }
    // Without a tunnelled controller the dongle names its own.
    if adapter == livi_link_host::link::CHOICE && !cfg!(target_os = "linux") {
        return livi_link_host::ap::bt_mac()
            .map(|b| b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(":"));
    }
    match bt_mac_in(Path::new(BT_SYSFS), adapter) {
        Ok(Some(mac)) => Some(mac),
        Ok(None) => busctl(if adapter.is_empty() { "hci0" } else { adapter }),
        Err(()) => None,
    }
}

fn wifi_mac_in(dir: &Path, iface: &str) -> Option<String> {
    let candidates = if iface.is_empty() {
        entries(dir).into_iter().filter(|n| n.starts_with("wlan")).collect()
    } else {
        vec![iface.to_string()]
    };
    candidates.iter().find_map(|name| mac_in(&dir.join(name).join("address")))
}

fn wifi_interfaces_in(dir: &Path) -> Vec<String> {
    entries(dir).into_iter().filter(|n| dir.join(n).join("wireless").exists()).collect()
}

pub fn wifi_interfaces() -> Vec<String> {
    wifi_interfaces_in(Path::new(NET_SYSFS))
}

fn bt_adapters_in(dir: &Path) -> Vec<String> {
    entries(dir)
        .into_iter()
        .filter(|n| {
            n.strip_prefix("hci")
                .is_some_and(|i| !i.is_empty() && i.bytes().all(|b| b.is_ascii_digit()))
                && !tunnelled(dir, n)
        })
        .collect()
}

pub fn bt_adapters() -> Vec<String> {
    bt_adapters_in(Path::new(BT_SYSFS))
}

/// The chips LIVI meets, by the IDs their devices report, named as the kernel's driver tables name
/// them. MediaTek Wi-Fi on PCIe first, then the Bluetooth half of the same cards on USB.
const CHIPS: [(u16, u16, &str); 11] = [
    (0x14c3, 0x7961, "MT7921"),
    (0x14c3, 0x0608, "MT7921"),
    (0x14c3, 0x7922, "MT7922"),
    (0x14c3, 0x0616, "MT7922"),
    (0x14c3, 0x7920, "MT7920"),
    (0x14c3, 0x7925, "MT7925"),
    (0x14c3, 0x0717, "MT7925"),
    (0x14c3, 0x7927, "MT7927"),
    (0x0489, 0xe0e2, "MT7922"),
    (0x13d3, 0x3602, "MT7925"),
    (0x0e8d, 0x7925, "MT7925"),
];

fn hex_in(path: &Path) -> Option<u16> {
    let text = fs::read_to_string(path).ok()?;
    u16::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()
}

/// The chip behind an interface's device, else its maker, else its driver.
fn model_of(device: &Path) -> Option<String> {
    let device = fs::canonicalize(device).ok()?;
    let parent = device.parent()?;
    // A PCI or SDIO function names its IDs itself, a USB interface leaves them to its device.
    let ids = hex_in(&device.join("vendor"))
        .zip(hex_in(&device.join("device")))
        .or_else(|| hex_in(&parent.join("idVendor")).zip(hex_in(&parent.join("idProduct"))));
    if let Some((_, _, chip)) = ids.and_then(|ids| CHIPS.iter().find(|(v, d, _)| (*v, *d) == ids)) {
        return Some((*chip).to_string());
    }
    // On USB the driver alone would only say btusb.
    let maker = fs::read_to_string(parent.join("manufacturer")).ok();
    if let Some(maker) = maker.as_deref().and_then(|m| m.split_whitespace().next()) {
        return Some(maker.trim_end_matches([',', '.']).to_string());
    }
    let driver = fs::read_link(device.join("driver")).ok()?;
    Some(driver.file_name()?.to_string_lossy().into_owned())
}

fn interface_models_in(net: &Path, bt: &Path, names: &[String]) -> BTreeMap<String, String> {
    names
        .iter()
        .filter_map(|name| {
            let dir = if name.starts_with("hci") { bt } else { net };
            model_of(&dir.join(name).join("device")).map(|model| (name.clone(), model))
        })
        .collect()
}

pub fn interface_models(names: &[String]) -> BTreeMap<String, String> {
    interface_models_in(Path::new(NET_SYSFS), Path::new(BT_SYSFS), names)
}

/// Asking the dongle blocks for up to its timeout.
pub fn accessory_id(wifi_interface: &str) -> Option<String> {
    if let Ok(mac) = std::env::var("AA_WIFI_BSSID") {
        return Some(mac);
    }
    if wifi_interface == livi_link_host::link::CHOICE {
        return livi_link_host::ap::mac().map(|m| m.to_uppercase());
    }
    wifi_mac_in(Path::new(NET_SYSFS), wifi_interface)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::tests::TempDir;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn macs_are_six_hex_pairs() {
        assert!(is_mac("aa:bb:cc:dd:ee:ff"));
        assert!(!is_mac("aa:bb:cc:dd:ee"));
        assert!(!is_mac("aa:bb:cc:dd:ee:fg"));
        assert!(!is_mac("aab:b:cc:dd:ee:ff"));
    }

    #[test]
    fn bluetooth_comes_from_the_named_or_the_first_adapter() {
        let dir = TempDir::new();
        write(&dir.0.join("hci1/address"), "11:22:33:44:55:66\n");
        write(&dir.0.join("hci0/address"), "garbage");
        assert_eq!(bt_mac_in(&dir.0, "hci1"), Ok(Some("11:22:33:44:55:66".into())));
        assert_eq!(bt_mac_in(&dir.0, ""), Ok(Some("11:22:33:44:55:66".into())));
        assert_eq!(bt_mac_in(&dir.0, "hci9"), Ok(None));
        assert_eq!(bt_mac_in(&dir.0, livi_link_host::link::CHOICE), Err(()));
    }

    #[test]
    fn the_lists_hold_the_wifi_interfaces_and_the_local_controllers() {
        let dir = TempDir::new();
        let net = dir.0.join("net");
        fs::create_dir_all(net.join("wlan0/wireless")).unwrap();
        fs::create_dir_all(net.join("eth0")).unwrap();
        assert_eq!(wifi_interfaces_in(&net), ["wlan0"]);

        let bt = dir.0.join("bt");
        fs::create_dir_all(bt.join("hci1")).unwrap();
        fs::create_dir_all(bt.join("hci0:1")).unwrap();
        fs::create_dir_all(bt.join("hci")).unwrap();
        assert_eq!(bt_adapters_in(&bt), ["hci1"]);
    }

    #[test]
    fn each_interface_names_its_chip_its_maker_or_its_driver() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new();
        let (net, bt, devices) = (dir.0.join("net"), dir.0.join("bt"), dir.0.join("devices"));
        let place = |class: &Path, name: &str, device: &Path| {
            fs::create_dir_all(class.join(name)).unwrap();
            fs::create_dir_all(device).unwrap();
            symlink(device, class.join(name).join("device")).unwrap();
        };
        let card = devices.join("pci/0000:68:00.0");
        place(&net, "wlp104s0", &card);
        write(&card.join("vendor"), "0x14c3\n");
        write(&card.join("device"), "0x7925\n");

        let sdio = devices.join("mmc1:0001:1");
        place(&net, "wlan0", &sdio);
        write(&sdio.join("vendor"), "0x02d0\n");
        write(&sdio.join("device"), "0xa9a6\n");
        fs::create_dir_all(devices.join("drivers/brcmfmac")).unwrap();
        symlink(devices.join("drivers/brcmfmac"), sdio.join("driver")).unwrap();

        let known = devices.join("usb5/5-7/5-7:1.0");
        place(&bt, "hci1", &known);
        write(&devices.join("usb5/5-7/idVendor"), "13d3\n");
        write(&devices.join("usb5/5-7/idProduct"), "3602\n");

        let other = devices.join("usb3/3-6/3-6:1.0");
        place(&bt, "hci0", &other);
        write(&devices.join("usb3/3-6/idVendor"), "0e8d\n");
        write(&devices.join("usb3/3-6/idProduct"), "0616\n");
        write(&devices.join("usb3/3-6/manufacturer"), "MediaTek Inc.\n");

        let names: Vec<String> =
            ["wlp104s0", "wlan0", "hci1", "hci0", "hci7", "livi-link"].map(String::from).to_vec();
        let models = interface_models_in(&net, &bt, &names);
        assert_eq!(models.get("wlp104s0").map(String::as_str), Some("MT7925"));
        assert_eq!(models.get("wlan0").map(String::as_str), Some("brcmfmac"));
        assert_eq!(models.get("hci1").map(String::as_str), Some("MT7925"));
        assert_eq!(models.get("hci0").map(String::as_str), Some("MediaTek"));
        assert_eq!(models.len(), 4);
    }

    #[test]
    fn wifi_comes_from_the_named_or_the_first_wlan() {
        let dir = TempDir::new();
        write(&dir.0.join("wlan1/address"), "aa:bb:cc:dd:ee:01");
        write(&dir.0.join("eth0/address"), "aa:bb:cc:dd:ee:02");
        assert_eq!(wifi_mac_in(&dir.0, ""), Some("AA:BB:CC:DD:EE:01".into()));
        assert_eq!(wifi_mac_in(&dir.0, "eth0"), Some("AA:BB:CC:DD:EE:02".into()));
        assert_eq!(wifi_mac_in(&dir.0, "wlan7"), None);
    }
}
