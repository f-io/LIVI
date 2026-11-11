// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[cfg(target_os = "linux")]
use std::env;
#[cfg(target_os = "linux")]
use std::path::Path;
use std::process::ExitCode;

#[cfg(target_os = "linux")]
mod bt_up;
#[cfg(target_os = "linux")]
mod btd;
#[cfg(target_os = "linux")]
mod config;
#[cfg(all(target_os = "linux", not(target_arch = "riscv32")))]
mod imx6ul;
#[cfg(target_os = "linux")]
mod ledd;
#[cfg(target_os = "linux")]
mod mfid;
#[cfg(target_os = "linux")]
mod netd;
#[cfg(target_os = "linux")]
mod wifid;

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
fn wifi_standards(target: &str) -> livi_link_ap::server::Standards {
    use livi_link_ap::server::Standards;
    match target {
        "v821b_aic8800d80" | "ax520_aic8800d80" => Standards {
            vht: true,
            he: true,
        },
        "imx6ul_rtl8822cs" | "imx6ul_rtl8822bs" | "imx6ul_bcm4354" => Standards {
            vht: true,
            he: false,
        },
        _ => Standards::default(),
    }
}

#[cfg(target_os = "linux")]
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
    let (led, flash) = match target {
        "ax520_aic8800d80" => (
            true,
            livi_web::Flash {
                mtd: vec![
                    slot(3, "mtdblock3", &[0x56, 0x19, 0x05, 0x27], 0x30_0000),
                    slot(5, "mtdblock5", b"hsqs", 0x44_0000),
                    slot(6, "mtdblock6", b"hsqs", 0x44_0000),
                ],
                check: Some("/usr/sbin/sfc-sr".into()),
            },
        ),
        #[cfg(not(target_arch = "riscv32"))]
        t if t.starts_with("imx6ul_") => (
            false,
            livi_web::Flash {
                mtd: vec![
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

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let arg0 = args.first().map(|s| s.as_str()).unwrap_or("livid");
    let basename = Path::new(arg0)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("livid");

    let (cmd, rest): (&str, Vec<String>) = if basename == "livid" {
        match args.get(1) {
            Some(sub) => (sub.as_str(), args[2..].to_vec()),
            None => {
                eprintln!(
                    "usage: livid <bt-up|bt-probe|btd|config|httpd|ledd|mfid|netd|wifid> [args…]"
                );
                return ExitCode::from(2);
            }
        }
    } else {
        (basename, args[1..].to_vec())
    };

    let rc = match cmd {
        "livi-bt-up" | "bt-up" => bt_up::run(rest),
        "bt-probe" => exit_rc(livi_link_hci::probe()),
        "livi-btd" | "btd" => btd::run(rest),
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

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!("livid runs on Linux");
    ExitCode::FAILURE
}

#[cfg(target_os = "linux")]
fn exit_rc(code: ExitCode) -> i32 {
    if code == ExitCode::SUCCESS { 0 } else { 1 }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use livi_link_ap::server::Standards;

    #[test]
    fn each_module_is_asked_only_for_what_it_speaks() {
        let ac_ax = Standards {
            vht: true,
            he: true,
        };
        assert_eq!(wifi_standards("v821b_aic8800d80"), ac_ax);
        assert_eq!(wifi_standards("ax520_aic8800d80"), ac_ax);
        assert_eq!(
            wifi_standards("imx6ul_rtl8822cs"),
            Standards {
                vht: true,
                he: false
            }
        );
        assert_eq!(
            wifi_standards("imx6ul_rtl8822bs"),
            Standards {
                vht: true,
                he: false
            }
        );
        assert_eq!(
            wifi_standards("imx6ul_bcm4354"),
            Standards {
                vht: true,
                he: false
            }
        );
        assert_eq!(wifi_standards("imx6ul_iw416"), Standards::default());
    }
}
