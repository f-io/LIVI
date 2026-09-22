//! The `.lfwb` firmware bundle every flashable dongle ships as, and the mtd writes that consume
//! it. An image's type byte is its MTD index on the dongle.
//!
//! ```text
//! Header (8 B):     "LFWB" | version:u8=1 | count:u8 | reserved:u16
//! Descriptor (12 B) type:u8 | flags:u8 | reserved:u16 | length:u32 le | crc32:u32 le   (per image)
//! Payload:          the images back to back, in descriptor order
//! ```

use md5::{Digest, Md5};

use super::shell::BindShell;

const MAGIC: &[u8; 4] = b"LFWB";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 8;
const DESC_LEN: usize = 12;

pub fn pack(images: &[(u8, &[u8])]) -> Vec<u8> {
    let payload: usize = images.iter().map(|(_, data)| data.len()).sum();
    let mut out = Vec::with_capacity(HEADER_LEN + images.len() * DESC_LEN + payload);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(images.len() as u8);
    out.extend_from_slice(&[0, 0]);
    for (typ, data) in images {
        out.push(*typ);
        out.push(0); // flags
        out.extend_from_slice(&[0, 0]); // reserved
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&crc32(data).to_le_bytes());
    }
    for (_, data) in images {
        out.extend_from_slice(data);
    }
    out
}

/// Every image with its type, each one checked against the crc the bundle carries for it.
pub fn unpack(bytes: &[u8]) -> Result<Vec<(u8, Vec<u8>)>, String> {
    if bytes.len() < HEADER_LEN || &bytes[..4] != MAGIC {
        return Err("not an LFWB bundle".into());
    }
    if bytes[4] != VERSION {
        return Err(format!("unsupported LFWB version {}", bytes[4]));
    }
    let count = bytes[5] as usize;
    let mut off = HEADER_LEN + count * DESC_LEN;
    if bytes.len() < off {
        return Err("LFWB bundle is truncated in its descriptor table".into());
    }
    let mut images = Vec::with_capacity(count);
    for i in 0..count {
        let desc = &bytes[HEADER_LEN + i * DESC_LEN..][..DESC_LEN];
        let typ = desc[0];
        let len = u32::from_le_bytes(desc[4..8].try_into().unwrap()) as usize;
        let crc = u32::from_le_bytes(desc[8..12].try_into().unwrap());
        let data = bytes
            .get(off..off + len)
            .ok_or_else(|| format!("LFWB bundle is truncated in image type {typ}"))?;
        if crc32(data) != crc {
            return Err(format!("LFWB image type {typ} does not match its crc32"));
        }
        images.push((typ, data.to_vec()));
        off += len;
    }
    Ok(images)
}

/// The image of one type out of `unpack`'s result.
pub fn image(images: &[(u8, Vec<u8>)], typ: u8) -> Option<&[u8]> {
    images.iter().find(|(t, _)| *t == typ).map(|(_, data)| data.as_slice())
}

pub fn crc32(data: &[u8]) -> u32 {
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

fn md5_hex(data: &[u8]) -> String {
    Md5::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn write_mtd(sh: &mut BindShell, node: &str, data: &[u8]) -> Result<(), String> {
    // `head -c SIZE` reads exactly SIZE bytes from the bindshell socket, dd flushes them to the
    // mtd. No base64, no tmpfile — the stock firmware has neither `base64` nor much /tmp room.
    let cmd = format!(
        "head -c {} 2>/dev/null | dd of={node} bs=64k conv=fsync 2>/dev/null; sync",
        data.len(),
    );
    sh.stream_in(&cmd, data)
}

/// `write_mtd`, then reads the partition back and compares. For flash that can accept a write
/// command and silently drop it (a chip whose write protection is set answers the same way).
pub fn write_mtd_verified(sh: &mut BindShell, node: &str, data: &[u8]) -> Result<(), String> {
    write_mtd(sh, node, data)?;
    let out = sh.run(&format!("head -c {} {node} | md5sum", data.len()))?;
    let got = out.split_whitespace().next().unwrap_or("");
    let want = md5_hex(data);
    if got != want {
        return Err(format!(
            "{node} reads back md5 {got}, wrote {want}: the flash did not take the write"
        ));
    }
    Ok(())
}

/// Exercises the whole `stream_in` path without touching an mtd.
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
        return Err(format!("selftest size mismatch: got {dongle_size}, want {size}"));
    }
    if dongle_md5 != host_md5 {
        return Err(format!("selftest md5 mismatch: dongle {dongle_md5}, host {host_md5}"));
    }
    Ok(())
}

/// A stamp for a backup file name.
pub fn stamp() -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_is_the_standard_one() {
        // The check value of CRC-32/ISO-HDLC, what gzip's trailer (and so pack-bundle.sh) carries.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn a_packed_bundle_unpacks_to_the_same_images() {
        let boot = vec![7u8; 1000];
        let rootfs: Vec<u8> = (0..3000u32).map(|i| i as u8).collect();
        let bundle = pack(&[(3, &boot), (6, &rootfs)]);
        let images = unpack(&bundle).unwrap();
        assert_eq!(image(&images, 3), Some(boot.as_slice()));
        assert_eq!(image(&images, 6), Some(rootfs.as_slice()));
        assert_eq!(image(&images, 1), None);
    }

    #[test]
    fn a_damaged_bundle_is_refused_not_flashed() {
        let mut bundle = pack(&[(3, &[1, 2, 3, 4])]);
        assert!(unpack(&bundle[..bundle.len() - 1]).is_err(), "truncated");
        *bundle.last_mut().unwrap() ^= 1;
        assert!(unpack(&bundle).is_err(), "flipped bit");
        assert!(unpack(b"nope").is_err(), "not a bundle");
    }
}
