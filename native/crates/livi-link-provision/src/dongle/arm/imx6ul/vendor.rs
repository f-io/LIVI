// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io::Read;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::shell::{PUSH_PORT, Shell, md5_hex};

const CHECK_URL: &str = "http://api.paplink.cn/a/upgrade/checkBox";
const DOWN_URL: &str = "http://api.paplink.cn/a/upgrade/down";
const APP_VER: &str = "8.3.0";
const FIRST_YEAR: u32 = 2018;
const MAX_STEPS: usize = 16;
const MAX_IMAGE: u64 = 64 * 1024 * 1024;
const UPDATE_TIMEOUT: Duration = Duration::from_secs(900);
const UPDATER: &str = "/script/update_box_ota.sh";

const ROOTFS: [(&str, &str); 9] = [
    ("0xb822", "rtl8822_rootfs.tar.gz"),
    ("0x4354", "bcm4354_rootfs.tar.gz"),
    ("0x4335", "bcm4354_rootfs.tar.gz"),
    ("0x9149", "sd8987_rootfs.tar.gz"),
    ("0x9141", "sd8987_rootfs.tar.gz"),
    ("0xc822", "rtl8822cs_rootfs.tar.gz"),
    ("0x4358", "bcm4358_rootfs.tar.gz"),
    ("0xaa31", "bcm4358_rootfs.tar.gz"),
    ("0x9159", "iw416_rootfs.tar.gz"),
];

#[derive(Debug, PartialEq, Eq)]
pub struct Unit {
    pub model: String,
    pub version: String,
    pub mfd: String,
    pub serial: String,
    pub sdio: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Offer {
    pub version: String,
    pub size: u64,
    id: String,
    token: String,
    asked: String,
}

pub fn unit(sh: &Shell) -> Result<Unit, String> {
    unit_from(&sh.sh(
        "for f in box_product_type software_version manufacture_date device_serial; do \
         echo \"$f=$(cat /etc/$f 2>/dev/null)\"; done; \
         echo \"sdio=$(cat /sys/bus/sdio/devices/*/device 2>/dev/null | head -n1)\"",
    )?)
}

fn unit_from(out: &str) -> Result<Unit, String> {
    let field = |name: &str| {
        out.lines()
            .find_map(|l| l.trim().strip_prefix(name)?.strip_prefix('='))
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let need = |name: &str| field(name).ok_or_else(|| format!("the dongle has no /etc/{name}"));
    let version = need("software_version")?;
    let mfd = field("manufacture_date").unwrap_or_else(|| {
        version
            .chars()
            .filter(char::is_ascii_digit)
            .take(8)
            .collect()
    });
    Ok(Unit {
        model: need("box_product_type")?,
        mfd,
        serial: need("device_serial")?,
        sdio: field("sdio"),
        version,
    })
}

impl Offer {
    pub fn newer_than(&self, unit: &Unit) -> bool {
        self.version > unit.version
    }

    pub fn older_than(&self, unit: &Unit) -> bool {
        self.version < unit.version
    }
}

fn form(unit: &Unit, ver: &str) -> Vec<(&'static str, String)> {
    vec![
        ("lang", "0".into()),
        ("code", "37".into()),
        ("appVer", APP_VER.into()),
        ("ver", ver.into()),
        ("uuid", unit.serial.clone()),
        ("mfd", unit.mfd.clone()),
        ("fwn", format!("{}_Update.img", unit.model)),
        ("model", unit.model.clone()),
    ]
}

fn check(unit: &Unit, ver: &str) -> Result<Option<Offer>, String> {
    let mut reply = ureq::post(CHECK_URL)
        .config()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .send_form(form(unit, ver))
        .map_err(|e| format!("{CHECK_URL}: {e}"))?;
    let body = reply
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("{CHECK_URL}: {e}"))?;
    offer_from(&body, ver)
}

fn offer_from(body: &str, asked: &str) -> Result<Option<Offer>, String> {
    let reply: Value = serde_json::from_str(body)
        .map_err(|e| format!("the vendor's server answered something else than JSON: {e}"))?;
    if reply["err"].as_i64() != Some(0) {
        let head: String = body.chars().take(200).collect();
        return Err(format!("the vendor's server refused: {head}"));
    }
    let text = |key: &str| match &reply[key] {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    };
    let version = text("ver");
    if version.is_empty() {
        return Ok(None);
    }
    let size = text("size")
        .parse::<u64>()
        .ok()
        .filter(|&s| s > 0 && s <= MAX_IMAGE)
        .ok_or_else(|| format!("the vendor's server offers {version} with no usable size"))?;
    let id = text("id").to_ascii_lowercase();
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "the vendor's server offers {version} without its MD5"
        ));
    }
    let token = text("token");
    if token.is_empty() {
        return Err(format!(
            "the vendor's server offers {version} without a token"
        ));
    }
    Ok(Some(Offer {
        version,
        size,
        id,
        token,
        asked: asked.to_string(),
    }))
}

fn newest_from(unit: &Unit, ver: &str) -> Result<Option<Offer>, String> {
    let mut best: Option<Offer> = None;
    let mut asked = ver.to_string();
    for _ in 0..MAX_STEPS {
        match check(unit, &asked)? {
            Some(next) if next.version > asked => {
                asked = next.version.clone();
                best = Some(next);
            }
            _ => break,
        }
    }
    Ok(best)
}

fn seeds(version: &str) -> Vec<String> {
    let year = version
        .get(..4)
        .and_then(|y| y.parse().ok())
        .unwrap_or(FIRST_YEAR);
    (FIRST_YEAR..=year)
        .rev()
        .flat_map(|y| [format!("{y}.07.01.0000"), format!("{y}.01.01.0000")])
        .filter(|seed| seed.as_str() < version)
        .collect()
}

pub fn find(unit: &Unit) -> Result<Offer, String> {
    if let Some(newer) = newest_from(unit, &unit.version)? {
        return Ok(newer);
    }
    for seed in seeds(&unit.version) {
        if let Some(found) = newest_from(unit, &seed)? {
            return Ok(found);
        }
    }
    Err(format!(
        "the vendor's server has no firmware for this {} (serial {})",
        unit.model, unit.serial
    ))
}

pub fn download(
    unit: &Unit,
    offer: &Offer,
    cache: &Path,
    progress: &dyn Fn(&str),
) -> Result<Vec<u8>, String> {
    let saved = cache.join(format!("{}_{}_Update.img", unit.model, offer.version));
    if let Ok(bytes) = std::fs::read(&saved)
        && md5_hex(&bytes) == offer.id
    {
        progress(&format!(
            "using the saved download {}",
            crate::tilde(&saved)
        ));
        return Ok(bytes);
    }
    progress(&format!(
        "downloading {} {} ({} bytes)",
        unit.model, offer.version, offer.size
    ));
    let mut fields = form(unit, &offer.asked);
    fields.push(("id", offer.id.clone()));
    let reply = ureq::post(DOWN_URL)
        .header("Authorization", &offer.token)
        .header("User-Agent", "LIVI")
        .config()
        .timeout_global(Some(Duration::from_secs(900)))
        .build()
        .send_form(fields)
        .map_err(|e| format!("{DOWN_URL}: {e}"))?;
    let mut bytes = Vec::new();
    reply
        .into_body()
        .into_reader()
        .take(MAX_IMAGE)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{DOWN_URL}: {e}"))?;
    if bytes.len() as u64 != offer.size {
        return Err(format!(
            "the download is {} bytes, the vendor's server named {}",
            bytes.len(),
            offer.size
        ));
    }
    if md5_hex(&bytes) != offer.id {
        return Err("the download does not match the MD5 the vendor's server named".into());
    }
    if let Err(e) = std::fs::create_dir_all(cache).and_then(|()| std::fs::write(&saved, &bytes)) {
        progress(&format!(
            "could not keep the download for the next run: {e}"
        ));
    }
    Ok(bytes)
}

fn entry<'a>(listing: &'a str, name: &str) -> Option<&'a str> {
    listing
        .lines()
        .map(str::trim)
        .find(|e| e.trim_start_matches("./") == name)
}

fn packs(listing: &str) -> Vec<&str> {
    listing
        .lines()
        .map(|e| e.trim().trim_start_matches("./"))
        .filter(|e| !e.contains('/') && e.ends_with("_rootfs.tar.gz"))
        .collect()
}

fn fits(listing: &str, sdio: Option<&str>) -> Result<(), String> {
    for part in ["tmp/once.sh", "tmp/upg"] {
        if entry(listing, part).is_none() {
            return Err(format!(
                "that update carries no {part}, it would not rewrite this dongle's U-Boot block"
            ));
        }
    }
    let packs = packs(listing);
    if packs.is_empty() {
        return Ok(());
    }
    let want = sdio.and_then(|id| ROOTFS.iter().find(|(m, _)| *m == id).map(|&(_, p)| p));
    match want {
        Some(pack) if packs.contains(&pack) => Ok(()),
        _ => Err(format!(
            "that update has Wi-Fi files for {} but none for this dongle's module {}",
            packs.join(", "),
            sdio.unwrap_or("(unknown)")
        )),
    }
}

pub fn reinstall(
    sh: &Shell,
    unit: &Unit,
    image: &[u8],
    progress: &dyn Fn(&str),
) -> Result<(), String> {
    let name = format!("{}_Update.img", unit.model);
    let path = format!("/tmp/{name}");
    let want = md5_hex(image);
    if sh
        .sh(&format!(
            "[ -e {UPDATER} ] && command -v ARMimg_maker >/dev/null && echo yes"
        ))?
        .trim()
        != "yes"
    {
        return Err(format!("the dongle has no {UPDATER} or ARMimg_maker"));
    }
    sh.sh(
        "cd /tmp && rm -rf update update.tar.gz *_Update.img; echo 3 > /proc/sys/vm/drop_caches",
    )?;
    progress("copying the vendor firmware to the dongle");
    sh.land(image, &path, PUSH_PORT, &want)?;

    progress("unpacking it on the dongle to check it");
    let listing = sh.run(
        &format!("cd /tmp && ARMimg_maker ./{name} >/dev/null 2>&1; tar -tzf update.tar.gz"),
        Duration::from_secs(180),
    )?;
    let checked = fits(&listing, unit.sdio.as_deref()).and_then(|()| {
        let Some(e) = entry(&listing, "etc/box_product_type") else {
            return Ok(());
        };
        let model = sh.sh(&format!("cd /tmp && tar -xzOf update.tar.gz {e}"))?;
        if model.trim() != unit.model {
            return Err(format!(
                "that update is for {}, this dongle is {}",
                model.trim(),
                unit.model
            ));
        }
        Ok(())
    });
    sh.sh("rm -f /tmp/update.tar.gz")?;
    if let Err(e) = checked {
        let _ = sh.sh(&format!("rm -f {path}"));
        return Err(e);
    }
    if sh.md5(&path).as_deref() != Some(want.as_str()) {
        return Err(format!("{path} changed while it was checked"));
    }

    progress("the vendor's updater runs now, do not unplug the dongle");
    sh.sh(&format!(
        "cd /tmp && rm -f update_status update_progress && \
         setsid {UPDATER} </dev/null >/dev/null 2>&1 &"
    ))?;
    watch(sh, progress)
}

fn watch(sh: &Shell, progress: &dyn Fn(&str)) -> Result<(), String> {
    let start = Instant::now();
    let (mut last, mut lost) = (String::new(), 0);
    while start.elapsed() < UPDATE_TIMEOUT {
        sleep(Duration::from_secs(2));
        let Ok(out) = sh.sh("cat /tmp/update_status /tmp/update_progress 2>/dev/null") else {
            lost += 1;
            if lost >= 15 {
                progress("the dongle went away while its updater ran, it restarts");
                return Ok(());
            }
            continue;
        };
        lost = 0;
        let mut words = out.split_whitespace();
        let (status, step) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
        match status {
            "7" => {
                return Err(format!(
                    "the vendor's updater failed at step {step}, the dongle stays as it is, do not unplug it yet"
                ));
            }
            "6" => {
                progress("the vendor's updater is done, the dongle restarts");
                return Ok(());
            }
            _ if out != last && !status.is_empty() => {
                progress(&format!("vendor updater at step {step}"));
                last = out;
            }
            _ => {}
        }
    }
    Err(format!(
        "the vendor's updater did not finish within {}s",
        UPDATE_TIMEOUT.as_secs()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unit_is_read_off_its_etc() {
        let out = "box_product_type=Auto_Box\nsoftware_version=2025.04.17.1054\nmanufacture_date=20250417\n\
                   device_serial=9698c67b764e6bdb00a6d666f4836d3f\nsdio=0x9159\n";
        assert_eq!(
            unit_from(out).unwrap(),
            Unit {
                model: "Auto_Box".into(),
                version: "2025.04.17.1054".into(),
                mfd: "20250417".into(),
                serial: "9698c67b764e6bdb00a6d666f4836d3f".into(),
                sdio: Some("0x9159".into()),
            }
        );
        let bare = "box_product_type=Auto_Box\nsoftware_version=2024.09.27.1710\nmanufacture_date=\n\
                    device_serial=00ff\nsdio=\n";
        let u = unit_from(bare).unwrap();
        assert_eq!(u.mfd, "20240927");
        assert_eq!(u.sdio, None);
        assert!(unit_from("box_product_type=Auto_Box\n").is_err());
    }

    #[test]
    fn the_offer_is_read_off_the_reply() {
        let reply = r#"{"err":0,"token":"t.k","ver":"2024.09.27.1710","size":"14684421","id":"FF22CC348656C79A3AE6659EA3E37015","notes":"1、x","forced":0}"#;
        let offer = offer_from(reply, "2024.01.01.0000").unwrap().unwrap();
        assert_eq!(offer.version, "2024.09.27.1710");
        assert_eq!(offer.size, 14684421);
        assert_eq!(offer.id, "ff22cc348656c79a3ae6659ea3e37015");
        assert_eq!(offer.asked, "2024.01.01.0000");
        let none = r#"{"err":0,"token":"t","ver":"","size":0,"id":"","notes":"","forced":0}"#;
        assert_eq!(offer_from(none, "x").unwrap(), None);
        assert!(offer_from(r#"{"err":1}"#, "x").is_err());
        assert!(
            offer_from(
                r#"{"err":0,"token":"t","ver":"1","size":"9","id":"zz"}"#,
                "x"
            )
            .is_err()
        );
    }

    #[test]
    fn versions_compare_by_date() {
        let unit = unit_from(
            "box_product_type=Auto_Box\nsoftware_version=2025.04.17.1054\ndevice_serial=1\n",
        )
        .unwrap();
        let offer = |v: &str| Offer {
            version: v.into(),
            size: 1,
            id: String::new(),
            token: String::new(),
            asked: String::new(),
        };
        assert!(offer("2024.09.27.1710").older_than(&unit));
        assert!(offer("2025.10.01.0900").newer_than(&unit));
        assert!(!offer("2025.04.17.1054").newer_than(&unit));
        assert!(!offer("2025.04.17.1054").older_than(&unit));
    }

    #[test]
    fn the_package_has_to_rewrite_uboot_and_carry_our_wifi() {
        let full = "./\n./tmp/\n./tmp/once.sh\n./tmp/upg\n./tmp/AutoBox.img\n./iw416_rootfs.tar.gz\n./bcm4354_rootfs.tar.gz\n";
        assert!(fits(full, Some("0x9159")).is_ok());
        assert!(fits(full, Some("0x4335")).is_ok());
        assert!(fits(full, Some("0xc822")).is_err());
        assert!(fits(full, None).is_err());
        assert!(fits("tmp/once.sh\ntmp/upg\n", None).is_ok());
        assert!(fits("./tmp/once.sh\n./etc/profile\n", Some("0x9159")).is_err());
        assert_eq!(entry(full, "tmp/upg"), Some("./tmp/upg"));
    }

    #[test]
    fn the_seeds_walk_back_from_the_installed_year() {
        let s = seeds("2025.04.17.1054");
        assert_eq!(
            s[..3],
            ["2025.01.01.0000", "2024.07.01.0000", "2024.01.01.0000"]
        );
        assert_eq!(s.last().map(String::as_str), Some("2018.01.01.0000"));
        assert!(seeds("2018.01.01.0000").is_empty());
    }

    #[test]
    #[ignore = "asks the vendor's server"]
    fn the_server_offers_the_end_of_its_chain() {
        let unit = unit_from(
            "box_product_type=Auto_Box\nsoftware_version=2025.04.17.1054\nmanufacture_date=20250417\n\
             device_serial=9698c67b764e6bdb00a6d666f4836d3f\n",
        )
        .unwrap();
        let offer = find(&unit).unwrap();
        assert_eq!(offer.version, "2024.09.27.1710");
        assert_eq!(offer.id, "ff22cc348656c79a3ae6659ea3e37015");
    }
}
