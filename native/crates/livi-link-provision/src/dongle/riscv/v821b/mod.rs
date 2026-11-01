// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::Path;

use crate::dongle::shell::BindShell;
use crate::dongle::{Remote, lfwb};

pub const MTD1_SIZE: u64 = 0x0031_0000;
pub const MTD3_SIZE: u64 = 0x0048_0000;

pub(crate) const V821B_LFWB: &[u8] =
    include_bytes!("../../../../../../../assets/livi-link/v821b_aic8800d80/livi-link-v821b.lfwb");

pub struct HardwareInfo {
    pub cpuinfo_head: String,
    pub proc_mtd: String,
    pub aic_modules: String,
}

impl HardwareInfo {
    pub fn looks_like_v821b_aic8800d80(&self) -> bool {
        let rv32 = self.cpuinfo_head.contains("rv32");
        let mtd_layout = self.proc_mtd.matches("mtd").count() >= 8;
        let aic = self.aic_modules.contains("aic8800");
        rv32 && mtd_layout && aic
    }
}

pub fn verify_hardware(sh: &mut BindShell) -> Result<HardwareInfo, String> {
    let cpuinfo_head = sh.run("head -20 /proc/cpuinfo")?;
    let proc_mtd = sh.run("cat /proc/mtd")?;
    let aic_modules = sh.run("ls /sys/module 2>/dev/null | grep -i aic8800 || true")?;
    Ok(HardwareInfo {
        cpuinfo_head,
        proc_mtd,
        aic_modules,
    })
}

pub fn backup_stock(sh: &mut BindShell, out_dir: &Path) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("mkdir {out_dir:?}: {e}"))?;

    println!("pulling mtd1 ({} B)…", MTD1_SIZE);
    let mtd1 = sh.stream_out(
        "dd if=/dev/mtdblock1 bs=64k 2>/dev/null; sleep 1",
        MTD1_SIZE,
    )?;
    println!("pulling mtd3 ({} B)…", MTD3_SIZE);
    let mtd3 = sh.stream_out(
        "dd if=/dev/mtdblock3 bs=64k 2>/dev/null; sleep 1",
        MTD3_SIZE,
    )?;

    let out_path = out_dir.join(format!("v821b_stock_{}.lfwb", lfwb::stamp()));
    let bundle = lfwb::pack(&[(1, &mtd1), (3, &mtd3)]);
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
    if V821B_LFWB.is_empty() {
        return Err(
            "no LIVI Link firmware baked in — this is a local dev build without CI assets".into(),
        );
    }
    flash_lfwb_bytes(sh, V821B_LFWB)
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
    let mtd1 = lfwb::image(&images, 1).ok_or("no mtd1 payload in bundle")?;
    let mtd3 = lfwb::image(&images, 3).ok_or("no mtd3 payload in bundle")?;
    for (what, data, slot) in [("mtd1", mtd1, MTD1_SIZE), ("mtd3", mtd3, MTD3_SIZE)] {
        if data.len() as u64 > slot {
            return Err(format!(
                "{what} image is {} B, its slot holds {slot} B",
                data.len()
            ));
        }
    }

    println!("flashing mtd1 ({} B) → /dev/mtdblock1…", mtd1.len());
    lfwb::write_mtd_verified(sh, "/dev/mtdblock1", mtd1)?;
    println!("flashing mtd3 ({} B) → /dev/mtdblock3…", mtd3.len());
    lfwb::write_mtd_verified(sh, "/dev/mtdblock3", mtd3)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dongle::arm::imx6ul::shell::md5_hex;

    #[derive(Default)]
    struct Fake {
        written: Vec<(String, Vec<u8>)>,
        lost: bool,
    }

    impl Remote for Fake {
        fn run(&mut self, cmd: &str) -> Result<String, String> {
            let (_, data) = self.written.last().ok_or("read before any write")?;
            assert!(cmd.contains("md5sum"), "unexpected command {cmd}");
            Ok(format!(
                "{}  -",
                if self.lost {
                    md5_hex(b"")
                } else {
                    md5_hex(data)
                }
            ))
        }

        fn write_mtd(&mut self, node: &str, data: &[u8]) -> Result<(), String> {
            self.written.push((node.to_string(), data.to_vec()));
            Ok(())
        }
    }

    #[test]
    fn the_kernel_goes_first_then_the_rootfs() {
        let mut sh = Fake::default();
        write_bundle(&mut sh, &lfwb::pack(&[(1, b"kernel"), (3, b"rootfs")])).unwrap();
        let nodes: Vec<&str> = sh.written.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(nodes, ["/dev/mtdblock1", "/dev/mtdblock3"]);
    }

    #[test]
    fn an_image_too_big_for_its_slot_writes_nothing() {
        let mut sh = Fake::default();
        let rootfs = vec![0u8; MTD3_SIZE as usize + 1];
        let err = write_bundle(&mut sh, &lfwb::pack(&[(1, b"kernel"), (3, &rootfs)])).unwrap_err();
        assert!(err.contains("mtd3"), "{err}");
        assert!(sh.written.is_empty());
    }

    #[test]
    fn a_write_the_chip_dropped_is_an_error() {
        let mut sh = Fake {
            lost: true,
            ..Fake::default()
        };
        let err =
            write_bundle(&mut sh, &lfwb::pack(&[(1, b"kernel"), (3, b"rootfs")])).unwrap_err();
        assert!(err.contains("did not take the write"), "{err}");
    }
}
