use std::fs;
use std::io::{Read, Write};
use std::path::Path;

const MTD_DEV: &str = "/dev/mtdblock4";
const MAGIC: &[u8; 4] = b"LVCF";
const VERSION_V1: u16 = 1;
const VERSION_V2: u16 = 2;
const HEADER_LEN: usize = 16;
const V2_ENTRY_LEN: usize = 40;
const MAX_PAYLOAD: usize = 32 * 1024;
const MAX_ENTRY: usize = 8 * 1024;

const TMPFS_LED: &str = "/tmp/livi/led.toml";
const TMPFS_HOSTAPD: &str = "/tmp/livi/hostapd.conf.saved";
const TMPFS_BT_KEYS: &str = "/tmp/livi/bt-keys";
const DEFAULT_LED: &str = "/etc/livi/led.toml";
const DEFAULT_HOSTAPD: &str = "/etc/hostapd.conf";

const ENTRY_LED: &str = "led.toml";
const ENTRY_HOSTAPD: &str = "hostapd.conf.saved";
const ENTRY_BT_KEYS: &str = "bt-keys";

pub fn run(args: Vec<String>) -> i32 {
    match args.first().map(|s| s.as_str()).unwrap_or("") {
        "load" => cmd_load(),
        "save" => cmd_save(),
        other => {
            eprintln!("usage: livid config <load|save>  (got {:?})", other);
            2
        }
    }
}

struct Entry {
    name: String,
    data: Vec<u8>,
}

fn cmd_load() -> i32 {
    let _ = fs::create_dir_all("/tmp/livi");
    match read_blob() {
        Ok(entries) => {
            for e in entries {
                let path = match e.name.as_str() {
                    ENTRY_LED => TMPFS_LED,
                    ENTRY_HOSTAPD => TMPFS_HOSTAPD,
                    ENTRY_BT_KEYS => TMPFS_BT_KEYS,
                    _ => {
                        eprintln!("[livid config load] skipping unknown entry {:?}", e.name);
                        continue;
                    }
                };
                if let Err(err) = fs::write(path, &e.data) {
                    eprintln!("[livid config load] write {path}: {err}");
                    return 1;
                }
            }
            eprintln!("[livid config load] restored from {MTD_DEV}");
        }
        Err(err) => {
            eprintln!("[livid config load] {MTD_DEV} not usable ({err}); seeding defaults");
        }
    }
    seed_if_missing(TMPFS_LED, DEFAULT_LED);
    seed_if_missing(TMPFS_HOSTAPD, DEFAULT_HOSTAPD);
    ensure_setting(TMPFS_HOSTAPD, "bridge", "br0");
    0
}

fn ensure_setting(path: &str, key: &str, value: &str) {
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let present = text.lines().any(|line| {
        let line = line.trim();
        !line.starts_with('#') && line.split_once('=').is_some_and(|(k, _)| k.trim() == key)
    });
    if present {
        return;
    }
    let mut out = text;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!("{key}={value}\n"));
    let _ = fs::write(path, out);
}

fn seed_if_missing(target: &str, default: &str) {
    if Path::new(target).exists() {
        return;
    }
    if let Ok(v) = fs::read(default) {
        let _ = fs::write(target, v);
    }
}

fn cmd_save() -> i32 {
    let mut entries: Vec<Entry> = Vec::new();
    for (name, path) in [
        (ENTRY_LED, TMPFS_LED),
        (ENTRY_HOSTAPD, TMPFS_HOSTAPD),
        (ENTRY_BT_KEYS, TMPFS_BT_KEYS),
    ] {
        match fs::read(path) {
            Ok(data) if !data.is_empty() && data.len() <= MAX_ENTRY => {
                entries.push(Entry { name: name.to_string(), data });
            }
            Ok(_) => {}
            Err(_) => {}
        }
    }
    if entries.is_empty() {
        eprintln!("[livid config save] nothing to persist");
        return 0;
    }
    let blob = match pack_v2(&entries) {
        Ok(b) => b,
        Err(e) => { eprintln!("[livid config save] pack: {e}"); return 1; }
    };
    if let Err(e) = write_blob(&blob) {
        eprintln!("[livid config save] write {MTD_DEV}: {e}");
        return 1;
    }
    match read_blob() {
        Ok(back) => {
            let same = back.len() == entries.len()
                && back.iter().zip(&entries).all(|(a, b)| a.name == b.name && a.data == b.data);
            if !same {
                eprintln!("[livid config save] verify: read-back differs");
                return 2;
            }
            eprintln!("[livid config save] {} entries persisted to {MTD_DEV}", entries.len());
            0
        }
        Err(e) => { eprintln!("[livid config save] verify: {e}"); 2 }
    }
}

fn read_blob() -> std::io::Result<Vec<Entry>> {
    let mut f = fs::File::open(MTD_DEV)?;
    let mut hdr = [0u8; HEADER_LEN];
    f.read_exact(&mut hdr)?;
    if &hdr[0..4] != MAGIC {
        return Err(io_err("magic mismatch"));
    }
    let version = u16::from_le_bytes([hdr[4], hdr[5]]);
    let length = u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]) as usize;
    let crc = u32::from_le_bytes([hdr[12], hdr[13], hdr[14], hdr[15]]);
    if length == 0 || length > MAX_PAYLOAD {
        return Err(io_err(&format!("length {length} out of range")));
    }
    let mut payload = vec![0u8; length];
    f.read_exact(&mut payload)?;
    if crc32(&payload) != crc {
        return Err(io_err("crc mismatch"));
    }
    match version {
        VERSION_V1 => Ok(vec![Entry { name: ENTRY_LED.into(), data: payload }]),
        VERSION_V2 => unpack_v2(&payload),
        _ => Err(io_err(&format!("unknown version {version}"))),
    }
}

fn unpack_v2(payload: &[u8]) -> std::io::Result<Vec<Entry>> {
    if payload.len() < 4 {
        return Err(io_err("v2 payload too short"));
    }
    let count = payload[0] as usize;
    if count == 0 || count > 32 {
        return Err(io_err(&format!("v2 entry count {count} out of range")));
    }
    let table_end = 4 + count * V2_ENTRY_LEN;
    if payload.len() < table_end {
        return Err(io_err("v2 table truncated"));
    }
    let mut entries = Vec::with_capacity(count);
    let mut cursor = table_end;
    for i in 0..count {
        let off = 4 + i * V2_ENTRY_LEN;
        let name_end = payload[off..off + 32].iter().position(|&b| b == 0).unwrap_or(32);
        let name = std::str::from_utf8(&payload[off..off + name_end])
            .map_err(|_| io_err("v2 non-utf8 name"))?
            .to_string();
        let len = u32::from_le_bytes([
            payload[off + 32], payload[off + 33], payload[off + 34], payload[off + 35],
        ]) as usize;
        if cursor + len > payload.len() {
            return Err(io_err(&format!("v2 entry {i} extends past payload")));
        }
        let data = payload[cursor..cursor + len].to_vec();
        cursor += len;
        entries.push(Entry { name, data });
    }
    Ok(entries)
}

fn pack_v2(entries: &[Entry]) -> Result<Vec<u8>, String> {
    if entries.is_empty() || entries.len() > 32 {
        return Err(format!("bad entry count {}", entries.len()));
    }
    let total_len: usize = entries.iter().map(|e| e.data.len()).sum();
    let payload_len = 4 + entries.len() * V2_ENTRY_LEN + total_len;
    if payload_len > MAX_PAYLOAD {
        return Err(format!("payload {payload_len} exceeds cap {MAX_PAYLOAD}"));
    }
    let mut payload = Vec::with_capacity(payload_len);
    payload.push(entries.len() as u8);
    payload.extend_from_slice(&[0, 0, 0]);
    for e in entries {
        let mut name = [0u8; 32];
        let bytes = e.name.as_bytes();
        if bytes.len() > 32 {
            return Err(format!("name too long: {}", e.name));
        }
        name[..bytes.len()].copy_from_slice(bytes);
        payload.extend_from_slice(&name);
        payload.extend_from_slice(&(e.data.len() as u32).to_le_bytes());
        payload.extend_from_slice(&[0, 0, 0, 0]);
    }
    for e in entries {
        payload.extend_from_slice(&e.data);
    }
    let crc = crc32(&payload);
    let mut blob = Vec::with_capacity(HEADER_LEN + payload.len());
    blob.extend_from_slice(MAGIC);
    blob.extend_from_slice(&VERSION_V2.to_le_bytes());
    blob.extend_from_slice(&0u16.to_le_bytes());
    blob.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    blob.extend_from_slice(&crc.to_le_bytes());
    blob.extend_from_slice(&payload);
    Ok(blob)
}

fn write_blob(blob: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_SYNC)
        .open(MTD_DEV)?;
    f.write_all(blob)?;
    f.sync_all()?;
    unsafe { libc::sync(); }
    Ok(())
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        let mut byte = b as u32;
        for _ in 0..8 {
            let mix = (crc ^ byte) & 1;
            crc >>= 1;
            if mix != 0 { crc ^= 0xEDB8_8320; }
            byte >>= 1;
        }
    }
    !crc
}

fn io_err(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())
}
