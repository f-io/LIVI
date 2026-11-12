// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::Remote;
use super::arm::imx6ul::shell::md5_hex;
use super::shell::BindShell;

pub use livi_base::lfwb::{image, pack, unpack};

pub fn write_mtd(sh: &mut BindShell, node: &str, data: &[u8]) -> Result<(), String> {
    let cmd = format!(
        "head -c {} 2>/dev/null | dd of={node} bs=64k conv=fsync 2>/dev/null; sync",
        data.len(),
    );
    sh.stream_in(&cmd, data)
}

pub fn raw_node(node: &str) -> String {
    match node.rsplit_once("mtdblock") {
        Some((dir, n)) if n.bytes().all(|b| b.is_ascii_digit()) && !n.is_empty() => {
            format!("{dir}mtd{n}")
        }
        _ => node.to_string(),
    }
}

pub fn write_mtd_verified<R: Remote>(sh: &mut R, node: &str, data: &[u8]) -> Result<(), String> {
    sh.write_mtd(node, data)?;
    let raw = raw_node(node);
    let out = sh.run(&format!(
        "n={raw}; [ -c $n ] || {{ sync; echo 3 > /proc/sys/vm/drop_caches; n={node}; }}; head -c {} $n | md5sum",
        data.len()
    ))?;
    let got = out.split_whitespace().next().unwrap_or("");
    let want = md5_hex(data);
    if got != want {
        return Err(format!(
            "{raw} reads back md5 {got}, wrote {want}: the flash did not take the write"
        ));
    }
    Ok(())
}

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
    let host_md5 = md5_hex(&data);
    println!("selftest: {size} B, host md5 {host_md5}");
    write_mtd(sh, "/tmp/livi-selftest", &data)?;
    let out = sh.run("md5sum /tmp/livi-selftest | awk '{print $1}'; wc -c /tmp/livi-selftest | awk '{print $1}'; rm -f /tmp/livi-selftest")?;
    let mut lines = out.lines();
    let dongle_md5 = lines.next().unwrap_or("").trim().to_string();
    let dongle_size: usize = lines.next().unwrap_or("0").trim().parse().unwrap_or(0);
    println!("selftest: dongle {size} B want {size}, md5 {dongle_md5}");
    if dongle_size != size {
        return Err(format!(
            "selftest size mismatch: got {dongle_size}, want {size}"
        ));
    }
    if dongle_md5 != host_md5 {
        return Err(format!(
            "selftest md5 mismatch: dongle {dongle_md5}, host {host_md5}"
        ));
    }
    Ok(())
}

pub fn stamp() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = t / 86400;
    let secs = t % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    format!("{}_{:02}{:02}", days, h, m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_node_is_read_back_through_its_character_device() {
        assert_eq!(raw_node("/dev/mtdblock6"), "/dev/mtd6");
        assert_eq!(raw_node("/dev/mtdblock12"), "/dev/mtd12");
        assert_eq!(raw_node("/dev/mtd6"), "/dev/mtd6");
        assert_eq!(raw_node("/dev/mtdblock"), "/dev/mtdblock");
    }
}
