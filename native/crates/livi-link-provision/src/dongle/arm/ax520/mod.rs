// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::Path;

use crate::dongle::shell::BindShell;
use crate::dongle::{Remote, lfwb, mtd_is};

pub const BOOT_MTD: u8 = 3;
pub const CUSTOMER_MTD: u8 = 5;
pub const ROOTFS_MTD: u8 = 6;
pub const BOOT_SIZE: u64 = 0x0030_0000;
pub const CUSTOMER_SIZE: u64 = 0x0044_0000;
pub const ROOTFS_SIZE: u64 = 0x0044_0000;

pub(crate) const AX520_LFWB: &[u8] =
    include_bytes!("../../../../../../../assets/livi-link/ax520_aic8800d80/livi-link-ax520.lfwb");

pub struct HardwareInfo {
    pub cpuinfo_head: String,
    pub proc_mtd: String,
}

impl HardwareInfo {
    pub fn looks_like_ax520_aic8800d80(&self) -> bool {
        let cortex_a7 = self.cpuinfo_head.contains("0xc07");
        cortex_a7
            && mtd_is(&self.proc_mtd, BOOT_MTD, "boot", BOOT_SIZE)
            && mtd_is(&self.proc_mtd, ROOTFS_MTD, "rootfs", ROOTFS_SIZE)
    }

    pub fn has_customer(&self) -> bool {
        mtd_is(&self.proc_mtd, CUSTOMER_MTD, "customer", CUSTOMER_SIZE)
    }
}

pub fn verify_hardware<R: Remote>(sh: &mut R) -> Result<HardwareInfo, String> {
    let cpuinfo_head = sh.run("head -20 /proc/cpuinfo")?;
    let proc_mtd = sh.run("cat /proc/mtd")?;
    Ok(HardwareInfo {
        cpuinfo_head,
        proc_mtd,
    })
}

pub fn backup_stock(sh: &mut BindShell, out_dir: &Path) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("mkdir {out_dir:?}: {e}"))?;

    let has_customer = verify_hardware(sh)?.has_customer();

    println!("pulling boot ({} B)…", BOOT_SIZE);
    let boot = sh.stream_out(
        "dd if=/dev/mtdblock3 bs=64k 2>/dev/null; sleep 1",
        BOOT_SIZE,
    )?;
    let customer = if has_customer {
        println!("pulling customer ({} B)…", CUSTOMER_SIZE);
        Some(sh.stream_out(
            "dd if=/dev/mtdblock5 bs=64k 2>/dev/null; sleep 1",
            CUSTOMER_SIZE,
        )?)
    } else {
        None
    };
    println!("pulling rootfs ({} B)…", ROOTFS_SIZE);
    let rootfs = sh.stream_out(
        "dd if=/dev/mtdblock6 bs=64k 2>/dev/null; sleep 1",
        ROOTFS_SIZE,
    )?;

    let out_path = out_dir.join(format!("ax520_stock_{}.lfwb", lfwb::stamp()));
    let mut images: Vec<(u8, &[u8])> = vec![(BOOT_MTD, &boot)];
    if let Some(data) = &customer {
        images.push((CUSTOMER_MTD, data));
    }
    images.push((ROOTFS_MTD, &rootfs));
    let bundle = lfwb::pack(&images);
    std::fs::write(&out_path, &bundle).map_err(|e| format!("write {out_path:?}: {e}"))?;
    println!("wrote {} ({} B)", out_path.display(), bundle.len());
    Ok(out_path)
}

pub fn flash_lfwb(sh: &mut BindShell, lfwb_path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(lfwb_path).map_err(|e| format!("read {lfwb_path:?}: {e}"))?;
    flash_lfwb_bytes(sh, &bytes)
}

pub fn stream_in_selftest(sh: &mut BindShell, size: usize) -> Result<(), String> {
    lfwb::stream_in_selftest(sh, size)
}

pub fn flash_embedded(sh: &mut BindShell) -> Result<(), String> {
    if AX520_LFWB.is_empty() {
        return Err(
            "no LIVI Link firmware baked in — this is a local dev build without CI assets".into(),
        );
    }
    flash_lfwb_bytes(sh, AX520_LFWB)
}

fn flash_lfwb_bytes(sh: &mut BindShell, bytes: &[u8]) -> Result<(), String> {
    write_bundle(sh, bytes)?;
    println!("sync + reboot");
    sh.run("sync")?;
    let _ = sh.run("reboot -f &");
    Ok(())
}

pub fn write_bundle<R: Remote>(sh: &mut R, bytes: &[u8]) -> Result<(), String> {
    let images = lfwb::unpack(bytes)?;
    let boot = lfwb::image(&images, BOOT_MTD).ok_or("no boot payload in bundle")?;
    let rootfs = lfwb::image(&images, ROOTFS_MTD).ok_or("no rootfs payload in bundle")?;
    let customer = lfwb::image(&images, CUSTOMER_MTD);
    let mut slots = vec![("boot", boot, BOOT_SIZE), ("rootfs", rootfs, ROOTFS_SIZE)];
    if let Some(data) = customer {
        slots.push(("customer", data, CUSTOMER_SIZE));
        if !verify_hardware(sh)?.has_customer() {
            return Err("the bundle carries a customer image, but mtd5 of this dongle is not the customer partition".into());
        }
    }
    for (what, data, slot) in slots {
        if data.len() as u64 > slot {
            return Err(format!(
                "{what} image is {} B, its slot holds {slot} B",
                data.len()
            ));
        }
    }

    println!(
        "flashing boot ({} B) → /dev/mtdblock{BOOT_MTD}…",
        boot.len()
    );
    lfwb::write_mtd_verified(sh, &format!("/dev/mtdblock{BOOT_MTD}"), boot)?;
    println!(
        "flashing rootfs ({} B) → /dev/mtdblock{ROOTFS_MTD}…",
        rootfs.len()
    );
    lfwb::write_mtd_verified(sh, &format!("/dev/mtdblock{ROOTFS_MTD}"), rootfs)?;
    if let Some(data) = customer {
        println!(
            "flashing customer ({} B) → /dev/mtdblock{CUSTOMER_MTD}…",
            data.len()
        );
        lfwb::write_mtd_verified(sh, &format!("/dev/mtdblock{CUSTOMER_MTD}"), data)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STOCK_MTD: &str = "dev:    size   erasesize  name
mtd0: 00030000 00010000 \"muboot\"
mtd1: 00010000 00010000 \"env\"
mtd2: 00010000 00010000 \"env-redund\"
mtd3: 00300000 00010000 \"boot\"
mtd4: 00300000 00010000 \"recovery\"
mtd5: 00440000 00010000 \"customer\"
mtd6: 00440000 00010000 \"rootfs\"
mtd7: 00010000 00010000 \"private\"
mtd8: 000b0000 00010000 \"logo\"
mtd9: 00070000 00010000 \"UDISK\"
";
    const CPUINFO: &str = "processor\t: 0\nmodel name\t: ARMv7 Processor rev 5 (v7l)\nCPU implementer\t: 0x41\nCPU architecture: 7\nCPU part\t: 0xc07\n";

    fn info(cpuinfo: &str, mtd: &str) -> HardwareInfo {
        HardwareInfo {
            cpuinfo_head: cpuinfo.into(),
            proc_mtd: mtd.into(),
        }
    }

    #[test]
    fn the_real_stock_table_passes() {
        assert!(info(CPUINFO, STOCK_MTD).looks_like_ax520_aic8800d80());
    }

    #[test]
    fn a_shifted_partition_table_is_refused() {
        let v821b = STOCK_MTD.replace(
            "mtd3: 00300000 00010000 \"boot\"",
            "mtd3: 00480000 00010000 \"rootfs\"",
        );
        assert!(!info(CPUINFO, &v821b).looks_like_ax520_aic8800d80());
    }

    #[test]
    fn a_right_name_with_the_wrong_size_is_refused() {
        let small = STOCK_MTD.replace("mtd6: 00440000", "mtd6: 00400000");
        assert!(!info(CPUINFO, &small).looks_like_ax520_aic8800d80());
    }

    #[test]
    fn the_board_is_told_apart_without_the_customer_partition() {
        let no_customer = STOCK_MTD.replace("\"customer\"", "\"other\"");
        assert!(info(CPUINFO, &no_customer).looks_like_ax520_aic8800d80());
        assert!(!info(CPUINFO, &no_customer).has_customer());
    }

    #[test]
    fn the_real_stock_table_has_the_customer_partition() {
        assert!(info(CPUINFO, STOCK_MTD).has_customer());
    }

    #[test]
    fn a_customer_partition_of_the_wrong_size_is_not_taken_for_one() {
        let small = STOCK_MTD.replace("mtd5: 00440000", "mtd5: 00200000");
        assert!(!info(CPUINFO, &small).has_customer());
    }

    #[test]
    fn a_bundle_may_carry_any_of_the_images() {
        let boot = vec![1u8; 16];
        let customer = vec![2u8; 16];
        let rootfs = vec![3u8; 16];
        let link = lfwb::unpack(&lfwb::pack(&[(BOOT_MTD, &boot), (ROOTFS_MTD, &rootfs)])).unwrap();
        assert!(lfwb::image(&link, CUSTOMER_MTD).is_none());
        let stock = lfwb::unpack(&lfwb::pack(&[
            (BOOT_MTD, &boot),
            (CUSTOMER_MTD, &customer),
            (ROOTFS_MTD, &rootfs),
        ]))
        .unwrap();
        assert_eq!(lfwb::image(&stock, CUSTOMER_MTD), Some(&customer[..]));
        assert_eq!(lfwb::image(&stock, BOOT_MTD), Some(&boot[..]));
        assert_eq!(lfwb::image(&stock, ROOTFS_MTD), Some(&rootfs[..]));
    }

    #[test]
    fn another_cpu_is_refused() {
        assert!(!info("isa\t: rv32imafdc\n", STOCK_MTD).looks_like_ax520_aic8800d80());
    }
}
