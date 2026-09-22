// Flashing LIVI Link onto the Allwinner V821B+AIC8800D80 dongle specifically. Getting a root
// shell on it (any project, any hardware) is `crate::dongle::hook`; this is what we do with it once we
// know it's this one board.

use std::path::Path;

use crate::dongle::lfwb;
use crate::dongle::shell::BindShell;

pub const PROJECT: &str = "ly6238";

pub const MTD1_SIZE: u64 = 0x0031_0000;
pub const MTD3_SIZE: u64 = 0x0048_0000;

const V821B_LFWB: &[u8] =
    include_bytes!("../../../../../../../assets/livi-link/v821b_aic8800d80/livi-link-v821b.lfwb");

pub struct HardwareInfo {
    pub cpuinfo_head: String,
    pub proc_mtd: String,
    pub aic_modules: String,
}

impl HardwareInfo {
    /// Gates flashing `V821B_LFWB`. Other hardware (e.g. AX520) failing this is correct — it
    /// stops at the bind-shell instead of getting the wrong image.
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
    let mtd1 = sh.stream_out("dd if=/dev/mtdblock1 bs=64k 2>/dev/null; sleep 1", MTD1_SIZE)?;
    println!("pulling mtd3 ({} B)…", MTD3_SIZE);
    let mtd3 = sh.stream_out("dd if=/dev/mtdblock3 bs=64k 2>/dev/null; sleep 1", MTD3_SIZE)?;

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

/// Self-test that exercises the full stream_in path without touching an mtd.
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
    let images = lfwb::unpack(bytes)?;
    let mtd1 = lfwb::image(&images, 1).ok_or("no mtd1 payload in bundle")?;
    let mtd3 = lfwb::image(&images, 3).ok_or("no mtd3 payload in bundle")?;

    println!("flashing mtd1 ({} B) → /dev/mtdblock1…", mtd1.len());
    lfwb::write_mtd(sh, "/dev/mtdblock1", mtd1)?;
    println!("flashing mtd3 ({} B) → /dev/mtdblock3…", mtd3.len());
    lfwb::write_mtd(sh, "/dev/mtdblock3", mtd3)?;
    println!("sync + reboot");
    sh.run("sync")?;
    // fire-and-forget; the dongle drops the shell as it goes down
    let _ = sh.run("reboot -f &");
    Ok(())
}
