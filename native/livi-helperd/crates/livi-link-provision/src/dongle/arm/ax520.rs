// Flashing LIVI Link onto the Axera AX520 (Cortex-A7, ARMv7) + AIC8800D80 dongle specifically.
// Getting a root shell on it (any project, any hardware) is `crate::dongle::hook`; this is what
// we do with it once we know it's this one board.

use std::path::Path;

use crate::dongle::lfwb;
use crate::dongle::shell::BindShell;

pub const PROJECT: &str = "ly7129";

/// The MTD indices and sizes of the stock partition table (`/proc/mtd`), also the type byte
/// each image carries in the bundle.
pub const BOOT_MTD: u8 = 3;
pub const ROOTFS_MTD: u8 = 6;
pub const BOOT_SIZE: u64 = 0x0030_0000;
pub const ROOTFS_SIZE: u64 = 0x0044_0000;

const AX520_LFWB: &[u8] =
    include_bytes!("../../../../../../../assets/livi-link/ax520_aic8800d80/livi-link-ax520.lfwb");

pub struct HardwareInfo {
    pub cpuinfo_head: String,
    pub proc_mtd: String,
}

impl HardwareInfo {
    /// Gates flashing `AX520_LFWB`. The writes go by MTD index, so what has to hold is that
    /// those two indices really are the boot and rootfs partitions of this board, by name and
    /// size. Anything else stops at the bind-shell instead of getting the wrong image.
    pub fn looks_like_ax520_aic8800d80(&self) -> bool {
        let cortex_a7 = self.cpuinfo_head.contains("0xc07");
        cortex_a7
            && mtd_is(&self.proc_mtd, BOOT_MTD, "boot", BOOT_SIZE)
            && mtd_is(&self.proc_mtd, ROOTFS_MTD, "rootfs", ROOTFS_SIZE)
    }
}

/// Whether `/proc/mtd` lists partition `index` under `name` with exactly `size` bytes:
/// `mtd3: 00300000 00010000 "boot"`.
fn mtd_is(proc_mtd: &str, index: u8, name: &str, size: u64) -> bool {
    let prefix = format!("mtd{index}:");
    proc_mtd.lines().any(|line| {
        let Some(rest) = line.trim().strip_prefix(&prefix) else {
            return false;
        };
        let mut fields = rest.split_whitespace();
        let listed_size = fields.next().and_then(|s| u64::from_str_radix(s, 16).ok());
        let listed_name = fields.nth(1).map(|s| s.trim_matches('"'));
        listed_size == Some(size) && listed_name == Some(name)
    })
}

pub fn verify_hardware(sh: &mut BindShell) -> Result<HardwareInfo, String> {
    let cpuinfo_head = sh.run("head -20 /proc/cpuinfo")?;
    let proc_mtd = sh.run("cat /proc/mtd")?;
    Ok(HardwareInfo {
        cpuinfo_head,
        proc_mtd,
    })
}

pub fn backup_stock(sh: &mut BindShell, out_dir: &Path) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("mkdir {out_dir:?}: {e}"))?;

    println!("pulling boot ({} B)…", BOOT_SIZE);
    let boot = sh.stream_out("dd if=/dev/mtdblock3 bs=64k 2>/dev/null; sleep 1", BOOT_SIZE)?;
    println!("pulling rootfs ({} B)…", ROOTFS_SIZE);
    let rootfs = sh.stream_out("dd if=/dev/mtdblock6 bs=64k 2>/dev/null; sleep 1", ROOTFS_SIZE)?;

    let out_path = out_dir.join(format!("ax520_stock_{}.lfwb", lfwb::stamp()));
    let bundle = lfwb::pack(&[(BOOT_MTD, &boot), (ROOTFS_MTD, &rootfs)]);
    std::fs::write(&out_path, &bundle).map_err(|e| format!("write {out_path:?}: {e}"))?;
    println!("wrote {} ({} B)", out_path.display(), bundle.len());
    Ok(out_path)
}

pub fn flash_lfwb(sh: &mut BindShell, lfwb_path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(lfwb_path).map_err(|e| format!("read {lfwb_path:?}: {e}"))?;
    flash_lfwb_bytes(sh, &bytes)
}

/// Self-test that exercises the full stream_in path without touching an mtd.
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
    let images = lfwb::unpack(bytes)?;
    let boot = lfwb::image(&images, BOOT_MTD).ok_or("no boot payload in bundle")?;
    let rootfs = lfwb::image(&images, ROOTFS_MTD).ok_or("no rootfs payload in bundle")?;
    for (what, data, slot) in [("boot", boot, BOOT_SIZE), ("rootfs", rootfs, ROOTFS_SIZE)] {
        if data.len() as u64 > slot {
            return Err(format!("{what} image is {} B, its slot holds {slot} B", data.len()));
        }
    }

    // Kernel first. If the power goes between the two writes, the new kernel finds no LIVI
    // rootfs and stays in its initramfs recovery (USB-NCM + telnet), the other way round would
    // pair the old kernel with a rootfs it cannot run.
    println!("flashing boot ({} B) → /dev/mtdblock{BOOT_MTD}…", boot.len());
    lfwb::write_mtd_verified(sh, &format!("/dev/mtdblock{BOOT_MTD}"), boot)?;
    println!("flashing rootfs ({} B) → /dev/mtdblock{ROOTFS_MTD}…", rootfs.len());
    lfwb::write_mtd_verified(sh, &format!("/dev/mtdblock{ROOTFS_MTD}"), rootfs)?;
    println!("sync + reboot");
    sh.run("sync")?;
    // fire-and-forget; the dongle drops the shell as it goes down
    let _ = sh.run("reboot -f &");
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
        // The V821B layout: boot is mtd1 there, mtd3 is its rootfs.
        let v821b = STOCK_MTD.replace("mtd3: 00300000 00010000 \"boot\"", "mtd3: 00480000 00010000 \"rootfs\"");
        assert!(!info(CPUINFO, &v821b).looks_like_ax520_aic8800d80());
    }

    #[test]
    fn a_right_name_with_the_wrong_size_is_refused() {
        let small = STOCK_MTD.replace("mtd6: 00440000", "mtd6: 00400000");
        assert!(!info(CPUINFO, &small).looks_like_ax520_aic8800d80());
    }

    #[test]
    fn another_cpu_is_refused() {
        assert!(!info("isa\t: rv32imafdc\n", STOCK_MTD).looks_like_ax520_aic8800d80());
    }
}
