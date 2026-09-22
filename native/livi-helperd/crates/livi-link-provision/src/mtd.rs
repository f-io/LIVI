//! Partition images. The original rootfs exists exactly once, on the device, so `apply` reads it
//! off before it strips anything.

use std::io::Read;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::shell::{PUSH_PORT, Shell};

/// Read in 64 KiB from the socket at a time; the link does about 1 MB/s.
const READ_SLICE: Duration = Duration::from_secs(30);
const HASH_TIMEOUT: Duration = Duration::from_secs(240);

pub struct Partition {
    pub name: String,
    pub device: String,
    pub bytes: u64,
}

/// What `/proc/mtd` reports. Names, not numbers: the numbering differs between dongle generations.
pub fn partitions(sh: &Shell) -> Result<Vec<Partition>, String> {
    let out = sh.sh("cat /proc/mtd")?;
    let mut parts = Vec::new();
    for line in out.lines().skip(1) {
        // mtd2: 00c80000 00010000 "rootfs"
        let mut fields = line.split_whitespace();
        let (Some(dev), Some(size), Some(_erase), Some(name)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let Ok(bytes) = u64::from_str_radix(size, 16) else { continue };
        parts.push(Partition {
            name: name.trim_matches('"').to_string(),
            device: format!("/dev/{}", dev.trim_end_matches(':')),
            bytes,
        });
    }
    if parts.is_empty() {
        return Err("could not read /proc/mtd".into());
    }
    Ok(parts)
}

/// Pulls one partition off the device and checks it against the hash the dongle computes itself,
/// so a truncated read cannot pass as a backup.
pub fn pull(sh: &Shell, part: &Partition, progress: &dyn Fn(&str)) -> Result<Vec<u8>, String> {
    let port = PUSH_PORT;
    sh.sh(&format!(
        "pkill -f 'nc -l -p {port}' 2>/dev/null; \
         setsid sh -c 'nc -l -p {port} < {}' >/dev/null 2>&1 &",
        part.device
    ))?;
    sleep(Duration::from_secs(1));

    let addr = sh.socket_addr(port)?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(8))
        .map_err(|e| format!("connect {}:{port}: {e}", sh.host()))?;
    stream.set_read_timeout(Some(READ_SLICE)).map_err(|e| e.to_string())?;

    let mut image = Vec::with_capacity(part.bytes as usize);
    let mut buf = vec![0u8; 65536];
    let started = Instant::now();
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => image.extend_from_slice(&buf[..n]),
            Err(e) => return Err(format!("read {}: {e}", part.name)),
        }
        if image.len() as u64 >= part.bytes {
            break;
        }
    }
    if image.len() as u64 != part.bytes {
        return Err(format!(
            "{}: read {} of {} bytes",
            part.name,
            image.len(),
            part.bytes
        ));
    }
    progress(&format!(
        "read {} ({} bytes in {:.0}s), verifying",
        part.name,
        part.bytes,
        started.elapsed().as_secs_f32()
    ));

    let here = sha256_hex(&image);
    let there = sh.run(&format!("sha256sum {}", part.device), HASH_TIMEOUT)?;
    let there = there.split_whitespace().next().unwrap_or("");
    if there != here {
        return Err(format!("{}: dongle says {there}, we read {here}", part.name));
    }
    Ok(image)
}

/// Saves every partition under `root/<model>-<firmware>-<stamp>/`, with a manifest naming the
/// hashes. Returns that directory.
pub fn backup(sh: &Shell, root: &Path, progress: &dyn Fn(&str)) -> Result<PathBuf, String> {
    let model = sh.sh("cat /etc/box_product_type 2>/dev/null")?.trim().to_string();
    let firmware = sh.sh("cat /etc/software_version 2>/dev/null")?.trim().to_string();
    let model = if model.is_empty() { "dongle".to_string() } else { model };
    let firmware = if firmware.is_empty() { "unknown".to_string() } else { firmware };

    let state = match sh.sh(&format!(
        "grep -q '{}' {} 2>/dev/null && echo livi-link || echo stock",
        crate::payload::BRINGUP_MARKER,
        crate::payload::BRINGUP_REMOTE
    )) {
        Ok(s) if s.trim() == "livi-link" => "livi-link",
        _ => "stock",
    };
    let dir = root.join(format!("{model}-{firmware}-{state}-{}", stamp()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", crate::tilde(&dir)))?;

    let mut entries = Vec::new();
    for part in partitions(sh)? {
        progress(&format!("reading {} ({} bytes)", part.name, part.bytes));
        let image = pull(sh, &part, progress)?;
        let file = dir.join(format!("{}.img", part.name));
        std::fs::write(&file, &image).map_err(|e| format!("{}: {e}", crate::tilde(&file)))?;
        entries.push(format!(
            "    {{ \"name\": \"{}\", \"device\": \"{}\", \"bytes\": {}, \"sha256\": \"{}\", \"file\": \"{}.img\" }}",
            part.name,
            part.device,
            part.bytes,
            sha256_hex(&image),
            part.name
        ));
        progress(&format!("saved {}", crate::tilde(&file)));
    }

    let manifest = format!(
        "{{\n  \"model\": \"{model}\",\n  \"firmware\": \"{firmware}\",\n  \"state\": \"{state}\",\n  \"taken\": \"{}\",\n  \"partitions\": [\n{}\n  ]\n}}\n",
        stamp(),
        entries.join(",\n")
    );
    let path = dir.join("manifest.json");
    std::fs::write(&path, manifest).map_err(|e| format!("{}: {e}", crate::tilde(&path)))?;
    Ok(dir)
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// UTC timestamp for the directory name; falls back to seconds since the epoch.
fn stamp() -> String {
    std::process::Command::new("date")
        .args(["-u", "+%Y%m%dT%H%M%SZ"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_else(|_| "unknown".into())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_like_sha256sum() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
