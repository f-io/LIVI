// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod boot;
pub mod mtd;
pub mod restore;
pub mod shell;

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use shell::Shell;

const REBOOT_TIMEOUT: Duration = Duration::from_secs(180);

pub(crate) const IMX6UL_LFWB: &[u8] =
    include_bytes!("../../../../../../../assets/livi-link/imx6ul_iw416/livi-link-imx6ull.lfwb");
pub(crate) const IMX6UL_RTL8822CS_LFWB: &[u8] = include_bytes!(
    "../../../../../../../assets/livi-link/imx6ul_rtl8822cs/livi-link-imx6ull-rtl8822cs.lfwb"
);
pub(crate) const IMX6UL_RTL8822BS_LFWB: &[u8] = include_bytes!(
    "../../../../../../../assets/livi-link/imx6ul_rtl8822bs/livi-link-imx6ull-rtl8822bs.lfwb"
);
pub(crate) const IMX6UL_BCM4354_LFWB: &[u8] = include_bytes!(
    "../../../../../../../assets/livi-link/imx6ul_bcm4354/livi-link-imx6ull-bcm4354.lfwb"
);

const MODULES: [(&str, &str); 4] = [
    ("0x9159", "imx6ul_iw416"),
    ("0xc822", "imx6ul_rtl8822cs"),
    ("0xb822", "imx6ul_rtl8822bs"),
    ("0x4354", "imx6ul_bcm4354"),
];

pub fn module_target(sh: &Shell) -> Result<Option<&'static str>, String> {
    Ok(target_of(&sh.sh(
        "cat /sys/bus/sdio/devices/*/device 2>/dev/null; true",
    )?))
}

fn target_of(ids: &str) -> Option<&'static str> {
    MODULES
        .iter()
        .find(|(id, _)| ids.lines().any(|found| found.trim() == *id))
        .map(|&(_, target)| target)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Running {
    Vendor,
    Rescue,
    Livi,
}

pub fn running(sh: &Shell) -> Result<Running, String> {
    Ok(running_from(&sh.sh("cat /proc/mounts")?))
}

fn running_from(mounts: &str) -> Running {
    let fs = |t: &str| {
        mounts
            .lines()
            .any(|l| l.split_whitespace().nth(2) == Some(t))
    };
    if fs("jffs2") {
        Running::Vendor
    } else if fs("squashfs") {
        Running::Livi
    } else {
        Running::Rescue
    }
}

pub fn install(
    sh: &Shell,
    bundle: &[u8],
    backup_root: &Path,
    progress: &dyn Fn(&str),
) -> Result<(), String> {
    let bundle = boot::read_bundle(bundle)?;
    let (Some(kernel), Some(rootfs)) = (&bundle.kernel, &bundle.rootfs) else {
        return Err("the bundle has to carry both the kernel and the rootfs".into());
    };
    match running(sh)? {
        Running::Livi => {
            to_rescue(sh, progress)?;
            let plan = boot::prepare(sh, kernel, backup_root, progress)?;
            boot::write_rootfs(sh, rootfs, &plan.backup, progress)?;
            boot::install(sh, &plan, progress)
        }
        Running::Vendor => {
            let dir = mtd::backup(sh, backup_root, progress)?;
            progress(&format!("vendor firmware saved in {}", crate::tilde(&dir)));
            let plan = boot::prepare(sh, kernel, backup_root, progress)?;
            boot::install(sh, &plan, progress)?;
            if running(sh)? != Running::Rescue {
                return Err(
                    "our kernel is in, but the dongle did not come up in the rescue system".into(),
                );
            }
            boot::write_rootfs(sh, rootfs, &plan.backup, progress)?;
            reboot(sh, progress)
        }
        Running::Rescue => {
            let plan = boot::prepare(sh, kernel, backup_root, progress)?;
            boot::write_rootfs(sh, rootfs, &plan.backup, progress)?;
            boot::install(sh, &plan, progress)
        }
    }
}

pub fn back_to_stock(
    sh: &Shell,
    stock: &restore::Stock,
    progress: &dyn Fn(&str),
) -> Result<(), String> {
    match running(sh)? {
        Running::Vendor => return Err("the dongle runs its vendor firmware already".into()),
        Running::Livi => to_rescue(sh, progress)?,
        Running::Rescue => {}
    }
    restore::write(sh, stock, progress)?;
    progress("restarting into the vendor firmware");
    sh.sh("(sleep 1; sync; reboot; sleep 5; reboot -f) >/dev/null 2>&1 &")?;
    Ok(())
}

fn to_rescue(sh: &Shell, progress: &dyn Fn(&str)) -> Result<(), String> {
    let parts = mtd::partitions(sh)?;
    let state = parts
        .iter()
        .find(|p| p.name == "bootstate")
        .ok_or("no bootstate partition in /proc/mtd")?;
    let block = state.device.replacen("/dev/mtd", "/dev/mtdblock", 1);
    sh.sh(&format!("printf LIVIBOOT > {block} && sync"))?;
    progress("restarting into the rescue system, it writes the rootfs");
    sh.sh("(sleep 1; sync; reboot; sleep 5; reboot -f) >/dev/null 2>&1 &")?;
    let gone = Instant::now();
    while sh.reachable() && gone.elapsed() < Duration::from_secs(30) {
        sleep(Duration::from_secs(1));
    }
    wait_for_dongle(sh, progress)?;
    match running(sh)? {
        Running::Rescue => Ok(()),
        other => Err(format!(
            "the dongle came back as {other:?}, not in its rescue system"
        )),
    }
}

fn reboot(sh: &Shell, progress: &dyn Fn(&str)) -> Result<(), String> {
    progress("rebooting into LIVI Link");
    sh.sh("(sleep 1; sync; reboot; sleep 5; reboot -f) >/dev/null 2>&1 &")?;
    Ok(())
}

fn wait_for_dongle(sh: &Shell, progress: &dyn Fn(&str)) -> Result<(), String> {
    let usb = Shell::new(shell::DEFAULT_HOST);
    let start = Instant::now();
    while start.elapsed() < REBOOT_TIMEOUT {
        if sh.reachable() {
            progress(&format!("dongle back after {}s", start.elapsed().as_secs()));
            return Ok(());
        }
        if sh.host() != shell::DEFAULT_HOST && usb.reachable() {
            sh.move_to(shell::DEFAULT_HOST);
            progress(&format!(
                "dongle back on USB at {} after {}s",
                shell::DEFAULT_HOST,
                start.elapsed().as_secs()
            ));
            return Ok(());
        }
        sleep(Duration::from_secs(3));
    }
    Err(format!(
        "dongle did not come back within {}s",
        REBOOT_TIMEOUT.as_secs()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mounts_tell_which_system_runs() {
        let vendor = "rootfs / rootfs rw 0 0\n/dev/mtdblock2 / jffs2 rw,relatime 0 0\nproc /proc proc rw 0 0";
        assert_eq!(running_from(vendor), Running::Vendor);
        let ours = "/dev/root / squashfs ro,relatime 0 0\ntmpfs /tmp tmpfs rw 0 0";
        assert_eq!(running_from(ours), Running::Livi);
        assert_eq!(
            running_from("none / rootfs rw 0 0\nproc /proc proc rw 0 0"),
            Running::Rescue
        );
    }

    #[test]
    fn the_module_picks_the_target() {
        assert_eq!(target_of("0x9159\n"), Some("imx6ul_iw416"));
        assert_eq!(target_of("0xc822\r\n"), Some("imx6ul_rtl8822cs"));
        assert_eq!(target_of("0xb822\n"), Some("imx6ul_rtl8822bs"));
        assert_eq!(target_of("0x4354\n"), Some("imx6ul_bcm4354"));
        assert_eq!(target_of("0x4330\n"), None);
        assert_eq!(target_of(""), None);
    }
}
