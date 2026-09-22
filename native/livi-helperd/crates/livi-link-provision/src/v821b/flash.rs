use std::path::Path;

use md5::{Digest, Md5};

use super::shell::{self, BindShell};
use super::web;

pub const MTD1_SIZE: u64 = 0x0031_0000;
pub const MTD3_SIZE: u64 = 0x0048_0000;

const UPDATE_SHELL_IMG: &[u8] = include_bytes!("../../assets/v821b/update.shell.img");
const V821B_LFWB: &[u8] =
    include_bytes!("../../../../../../assets/livi-link/v821b_aic8800d80/livi-link-v821b.lfwb");

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

pub fn install_bindshell() -> Result<(), String> {
    if shell::is_up() {
        println!("bind-shell already listening on 2323 — skipping OTA upload");
        return Ok(());
    }
    let info = web::host()?;
    if info.update != 0 {
        return Err(format!(
            "dongle is not idle (update={}); reboot and retry",
            info.update
        ));
    }
    println!("dongle: {} appver={}", info.name, info.sys.appver);

    println!("uploading update.shell.img ({} B)…", UPDATE_SHELL_IMG.len());
    web::upload(UPDATE_SHELL_IMG)?;
    println!("waiting for update to complete + dongle to reboot…");
    web::wait_for_update_complete(300)?;
    Ok(())
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

    let ts = chrono_now();
    let out_path = out_dir.join(format!("v821b_stock_{ts}.lfwb"));
    let bundle = pack_lfwb(&mtd1, &mtd3);
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
    use std::time::SystemTime;
    let mut data = vec![0u8; size];
    let seed = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x1234_5678_9abc_def0);
    let mut s = seed | 1;
    for byte in data.iter_mut() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        *byte = s as u8;
    }
    let host_md5: String = Md5::digest(&data).iter().map(|b| format!("{b:02x}")).collect();
    println!("selftest: {size} B, host md5 {host_md5}");
    write_mtd(sh, "/tmp/livi-selftest", &data)?;
    let out = sh.run("md5sum /tmp/livi-selftest | awk '{print $1}'; wc -c /tmp/livi-selftest | awk '{print $1}'; rm -f /tmp/livi-selftest")?;
    let mut lines = out.lines();
    let dongle_md5 = lines.next().unwrap_or("").trim().to_string();
    let dongle_size: usize = lines.next().unwrap_or("0").trim().parse().unwrap_or(0);
    println!("selftest: dongle {size} B want {size}, md5 {dongle_md5}");
    if dongle_size != size {
        return Err(format!("selftest size mismatch: got {dongle_size}, want {size}"));
    }
    if dongle_md5 != host_md5 {
        return Err(format!(
            "selftest md5 mismatch: dongle {dongle_md5}, host {host_md5}"
        ));
    }
    Ok(())
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
    let (mtd1, mtd3) = unpack_lfwb(bytes)?;

    println!("flashing mtd1 ({} B) → /dev/mtdblock1…", mtd1.len());
    write_mtd(sh, "/dev/mtdblock1", &mtd1)?;
    println!("flashing mtd3 ({} B) → /dev/mtdblock3…", mtd3.len());
    write_mtd(sh, "/dev/mtdblock3", &mtd3)?;
    println!("sync + reboot");
    sh.run("sync")?;
    // fire-and-forget; the dongle drops the shell as it goes down
    let _ = sh.run("reboot -f &");
    Ok(())
}

fn write_mtd(sh: &mut BindShell, node: &str, data: &[u8]) -> Result<(), String> {
    // `head -c SIZE` reads exactly SIZE bytes from the bindshell socket, dd flushes them to the
    // mtd. No base64, no tmpfile — the stock firmware has neither `base64` nor much /tmp room.
    let cmd = format!(
        "head -c {} 2>/dev/null | dd of={node} bs=64k conv=fsync 2>/dev/null; sync",
        data.len(),
    );
    sh.stream_in(&cmd, data)
}

fn pack_lfwb(mtd1: &[u8], mtd3: &[u8]) -> Vec<u8> {
    let mut hdr = Vec::with_capacity(8 + 24 + mtd1.len() + mtd3.len());
    hdr.extend_from_slice(b"LFWB");
    hdr.push(1); // version
    hdr.push(2); // count
    hdr.extend_from_slice(&[0, 0]);
    for (typ, data) in [(1u8, mtd1), (3u8, mtd3)] {
        let crc = crc32(data);
        hdr.push(typ);
        hdr.push(0); // flags
        hdr.extend_from_slice(&[0, 0]); // reserved
        hdr.extend_from_slice(&(data.len() as u32).to_le_bytes());
        hdr.extend_from_slice(&crc.to_le_bytes());
    }
    hdr.extend_from_slice(mtd1);
    hdr.extend_from_slice(mtd3);
    hdr
}

fn unpack_lfwb(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    if bytes.len() < 8 || &bytes[..4] != b"LFWB" {
        return Err("not an LFWB bundle".into());
    }
    let count = bytes[5];
    let mut off = 8;
    let mut mtd1 = None;
    let mut mtd3 = None;
    let mut descs = Vec::new();
    for _ in 0..count {
        let typ = bytes[off];
        let len = u32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap()) as usize;
        descs.push((typ, len));
        off += 12;
    }
    for (typ, len) in descs {
        let data = &bytes[off..off + len];
        off += len;
        match typ {
            1 => mtd1 = Some(data.to_vec()),
            3 => mtd3 = Some(data.to_vec()),
            _ => {}
        }
    }
    Ok((
        mtd1.ok_or("no mtd1 payload in bundle")?,
        mtd3.ok_or("no mtd3 payload in bundle")?,
    ))
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        let mut byte = b as u32;
        for _ in 0..8 {
            let mix = (crc ^ byte) & 1;
            crc >>= 1;
            if mix != 0 {
                crc ^= 0xEDB8_8320;
            }
            byte >>= 1;
        }
    }
    !crc
}

fn chrono_now() -> String {
    // Cheap ISO-ish stamp without a chrono dep — good enough for a filename.
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = t / 86400;
    let secs = t % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    // approximate — good enough for uniqueness in a filename
    format!("{}_{:02}{:02}", days, h, m)
}
