// One multi-call binary saves more than 3 MiB of runtime. common/build-rootfs.sh makes the
// symlinks whose names pick the module.

use std::env;
use std::path::Path;
use std::process::ExitCode;

mod accessoryd;
mod bt_up;
mod btd;
mod config;
#[cfg(not(target_arch = "riscv32"))]
mod imx6ul;
mod ledd;
mod mfid;
mod netd;
mod wifid;

/// One livid build serves every board of a CPU arch, so the board comes from its devicetree.
fn board() -> (&'static str, &'static str) {
    let compatible = std::fs::read("/sys/firmware/devicetree/base/compatible").unwrap_or_default();
    let has = |c: &[u8]| compatible.windows(c.len()).any(|w| w == c);
    if has(b"axera,ax520") {
        return ("AX520 + AIC8800D80", "ax520_aic8800d80");
    }
    #[cfg(not(target_arch = "riscv32"))]
    if has(b"livi,link-imx6ull") {
        return imx6ul::module();
    }
    ("V821B + AIC8800D80", "v821b_aic8800d80")
}

fn wifi_standards(target: &str) -> livi_wifid::server::Standards {
    use livi_wifid::server::Standards;
    match target {
        "v821b_aic8800d80" | "ax520_aic8800d80" => Standards { vht: true, he: true },
        "imx6ul_rtl8822cs" | "imx6ul_rtl8822bs" => Standards { vht: true, he: false },
        _ => Standards::default(),
    }
}

fn web_caps() -> livi_web::WebCaps {
    let (model, target) = board();
    let slot = |typ, node: &str, magic: &[u8], size| livi_web::MtdSlot {
        typ,
        node: node.into(),
        magic: magic.to_vec(),
        size,
        stage: None,
        before_write: None,
    };
    // The bundle types are the MTD numbers. The bootloader partition is never in the table, a bad
    // write there needs the flash off the board (AX520, i.MX6UL) or FEL (V821B).
    let (led, flash) = match target {
        "ax520_aic8800d80" => (
            true,
            livi_web::Flash {
                mtd: vec![
                    slot(3, "mtdblock3", &[0x56, 0x19, 0x05, 0x27], 0x30_0000), // little-endian uImage
                    // The vendor userspace, which also holds the config. Only a rollback bundle
                    // to stock writes it.
                    slot(5, "mtdblock5", b"hsqs", 0x44_0000),
                    slot(6, "mtdblock6", b"hsqs", 0x44_0000),
                ],
                // The loader after the bootrom reads the flash in quad mode and needs QE set.
                check: Some("/usr/sbin/sfc-sr".into()),
            },
        ),
        #[cfg(not(target_arch = "riscv32"))]
        t if t.starts_with("imx6ul_") => (
            false,
            livi_web::Flash {
                mtd: vec![
                    // The vendor U-Boot decrypts the kernel head with a key only the SoC holds,
                    // so the zImage goes in as staging blocks it converts on its next start.
                    livi_web::MtdSlot {
                        stage: Some(imx6ul::stage_kernel),
                        before_write: Some(imx6ul::erase_env),
                        ..slot(2, "mtdblock2", b"", 0x34_0000)
                    },
                    slot(3, "mtdblock3", b"hsqs", 0xc6_0000),
                ],
                ..Default::default()
            },
        ),
        _ => (
            true,
            livi_web::Flash {
                mtd: vec![
                    slot(1, "mtdblock1", b"ANDROID!", 0x31_0000),
                    slot(3, "mtdblock3", b"hsqs", 0x48_0000),
                ],
                ..Default::default()
            },
        ),
    };
    livi_web::WebCaps {
        model: model.into(),
        target: target.into(),
        port: 80,
        wifi_iface: "wlan0".into(),
        host_iface: "usb0".into(),
        mfi: mfid::STATE.into(),
        bt: "hci0".into(),
        led,
        flash,
        update_conf: "/tmp/livi/update.conf".into(),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let arg0 = args.first().map(|s| s.as_str()).unwrap_or("livid");
    let basename = Path::new(arg0).file_name().and_then(|s| s.to_str()).unwrap_or("livid");

    let (cmd, rest): (&str, Vec<String>) = if basename == "livid" {
        match args.get(1) {
            Some(sub) => (sub.as_str(), args[2..].to_vec()),
            None => {
                eprintln!(
                    "usage: livid <accessoryd|bt-up|bt-mgmt|bt-probe|btd|config|httpd|ledd|mfid|netd|wifid> [args…]"
                );
                return ExitCode::from(2);
            }
        }
    } else {
        (basename, args[1..].to_vec())
    };

    let rc = match cmd {
        "livi-bt-up" | "bt-up" => bt_up::run(rest),
        "bt-mgmt" => exit_rc(livi_accessoryd::mgmt::probe()),
        "bt-probe" => exit_rc(livi_btd::probe()),
        "livi-btd" | "btd" => btd::run(rest),
        "livi-accessoryd" | "accessoryd" => accessoryd::run(rest),
        "livi-httpd" | "httpd" => livi_web::run(web_caps()),
        "livi-ledd" | "ledd" => ledd::run(rest),
        "livi-netd" | "netd" => netd::run(rest),
        "config" => config::run(rest),
        "livi-mfid" | "mfid" => mfid::run(rest),
        "livi-wifid" | "wifid" => wifid::run(rest),
        _ => {
            eprintln!("livid: unknown command '{}'", cmd);
            2
        }
    };
    ExitCode::from(rc.clamp(0, 255) as u8)
}

fn exit_rc(code: ExitCode) -> i32 {
    if code == ExitCode::SUCCESS { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use livi_wifid::server::Standards;

    #[test]
    fn each_module_is_asked_only_for_what_it_speaks() {
        let ac_ax = Standards { vht: true, he: true };
        assert_eq!(wifi_standards("v821b_aic8800d80"), ac_ax);
        assert_eq!(wifi_standards("ax520_aic8800d80"), ac_ax);
        assert_eq!(wifi_standards("imx6ul_rtl8822cs"), Standards { vht: true, he: false });
        assert_eq!(wifi_standards("imx6ul_rtl8822bs"), Standards { vht: true, he: false });
        assert_eq!(wifi_standards("imx6ul_iw416"), Standards::default());
    }
}
