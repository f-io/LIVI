//! livi-web — the dongle web UI + control API, shared by both dongles (V821B/riscv32 and
//! cpc200/i.MX6). The page and the HTTP server are identical; only the hardware-specific parts
//! differ and come in as `WebCaps` from each dongle's binary: which interfaces to read, whether
//! a controllable LED is present, and how a flash write is actually performed.
//!
//! Routes: GET / (index.html), GET /api/{status,wifi,bt,led,caps,flash/status},
//! POST /api/{led,flash,reboot}. The flash + led routes answer 404 when the caps disable them.

use std::sync::OnceLock;

/// The firmware writes this dongle offers; several can be live at once. Default is none.
#[derive(Default)]
pub struct Flash {
    /// A `.lfwb` bundle written across NOR mtd partitions, magic-checked and CRC-compared.
    pub mtd: bool,
    /// Update the running stack from its gzipped binary — no partition erase.
    pub stack: Option<Stack>,
    /// Restore a full rootfs image (e.g. the stock firmware backup), sha256- and size-gated.
    pub rootfs: Option<Rootfs>,
}

/// Where the stack `.gz` lands on jffs2 and how the launcher is re-run to pick it up.
pub struct Stack {
    pub install_to: String,
    pub restart: String,
}

/// The flash script and the partition it writes.
pub struct Rootfs {
    pub script: String,
    pub partition: String,
}

/// What differs between the dongles the shared server runs on.
pub struct WebCaps {
    /// Human label shown in the Device card, e.g. "CPC200-CCPA" or "V821B + AIC8800D80".
    pub model: String,
    /// The directory under assets/livi-link this dongle's firmware is published in.
    pub target: String,
    pub port: u16,
    /// The AP interface whose SSID/MAC/rates the WiFi card shows (e.g. "wlan0").
    pub wifi_iface: String,
    /// The bridge whose forwarding table names the AP's clients; None counts via nl80211
    /// (the cpc200 bridges with l2fwd and has no br0).
    pub bridge: Option<String>,
    /// The host-facing interface whose MAC stands in as the dongle's address.
    pub host_iface: String,
    /// The Bluetooth controller (e.g. "hci0").
    pub bt: String,
    /// Whether to offer the LED section (a controllable LED daemon is present).
    pub led: bool,
    /// How (or whether) this dongle flashes firmware from the web UI.
    pub flash: Flash,
    /// Where the update channel is kept.
    pub update_conf: String,
}

const VERSION: &str = match option_env!("LIVI_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};
const BUILD: &str = match option_env!("LIVI_BUILD") {
    Some(b) => b,
    None => "dev",
};

static CAPS: OnceLock<WebCaps> = OnceLock::new();
fn caps() -> &'static WebCaps {
    CAPS.get().expect("livi_web::run must be called before any handler")
}

/// Serve the web UI + control API for this dongle until the process is stopped.
pub fn run(caps: WebCaps) -> i32 {
    let _ = CAPS.set(caps);
    match serve_forever() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("[livi-web] {e}");
            1
        }
    }
}

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6, TcpListener, TcpStream};
use std::process::Command;
use std::thread;
use std::time::Duration;

// Kept under web/ (not assets/): the Mac↔build-host sync skips any assets/ directory, which
// would leave the build host compiling a stale or missing page.
const INDEX_HTML: &str = include_str!("../web/index.html");

// mtd slot sizes must match the on-flash partition layout. If a flash payload
// exceeds the slot, refuse — the write would corrupt the neighbouring partition.
const MTD1_SIZE: u64 = 0x0031_0000; // 3.06 MiB — kernel + DTB bootimg
const MTD3_SIZE: u64 = 0x0048_0000; // 4.5  MiB — squashfs rootfs
// mtd0 (u-boot + OpenSBI) is intentionally NOT in this table. We never flash it
// from a running system; a bad write there requires FEL recovery.

fn serve_forever() -> std::io::Result<()> {
    let port = caps().port;
    let addr = SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, port, 0, 0);
    let listener = TcpListener::bind(SocketAddr::V6(addr))?;
    eprintln!("[livi-httpd] listening on [::]:{port} (dual-stack)");

    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN); }

    for conn in listener.incoming() {
        match conn {
            Ok(s) => { thread::spawn(|| handle(s)); }
            Err(e) => eprintln!("[livi-httpd] accept: {e}"),
        }
    }
    Ok(())
}

fn handle(mut sock: TcpStream) {
    let _ = sock.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = sock.set_write_timeout(Some(Duration::from_secs(30)));

    let Some((method, path, content_length, body)) = read_request(&sock) else {
        return;
    };

    let (status, ctype, resp) = route(&method, &path, body.as_deref(), content_length);
    let _ = write_response(&mut sock, status, ctype, &resp);
}

fn read_request(sock: &TcpStream) -> Option<(String, String, u64, Option<Vec<u8>>)> {
    let mut reader = BufReader::new(sock);

    let mut first = String::new();
    reader.read_line(&mut first).ok()?;
    if first.is_empty() { return None; }
    let mut parts = first.split_whitespace();
    let method = parts.next()?.to_string();
    let path   = parts.next()?.to_string();

    let mut content_length: u64 = 0;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).ok()?;
        if n <= 2 { break; } // empty CRLF terminates headers
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
    }

    let body = if content_length > 0 {
        let mut buf = vec![0u8; content_length as usize];
        reader.read_exact(&mut buf).ok()?;
        Some(buf)
    } else { None };

    Some((method, path, content_length, body))
}

fn route(rmethod: &str, rpath: &str, body: Option<&[u8]>, clen: u64)
    -> (&'static str, &'static str, Vec<u8>)
{
    let c = caps();
    // The path may carry a query (?sha=… for the rootfs flash); match on the path, keep the query.
    let (path, query) = rpath.split_once('?').unwrap_or((rpath, ""));
    let method = rmethod;

    // LED section only when a controllable LED daemon is present.
    if c.led {
        match (method, path) {
            ("GET", "/api/led") => return (S_200, T_JSON, led_json().into_bytes()),
            ("POST", "/api/led") => return set_led(body),
            _ => {}
        }
    }
    // One upload endpoint; the dongle picks the write from what it offers and what the file is.
    let has_flash = c.flash.mtd || c.flash.stack.is_some() || c.flash.rootfs.is_some();
    if has_flash && (method, path) == ("POST", "/api/flash") {
        return flash_dispatch(c, query, body, clen);
    }
    if c.flash.mtd {
        match (method, path) {
            ("POST", "/api/flash/mtd1") => return flash("mtdblock1", MTD1_SIZE, b"ANDROID!", body, clen),
            ("POST", "/api/flash/mtd3") => return flash("mtdblock3", MTD3_SIZE, b"hsqs", body, clen),
            _ => {}
        }
    }

    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => (S_200, T_HTML, INDEX_HTML.as_bytes().to_vec()),
        ("GET", "/api/status") => (S_200, T_JSON, status_json().into_bytes()),
        ("GET", "/api/wifi")   => (S_200, T_JSON, wifi_json().into_bytes()),
        ("GET", "/api/bt")     => (S_200, T_JSON, bt_json().into_bytes()),
        ("GET", "/api/caps")   => (S_200, T_JSON, caps_json().into_bytes()),
        ("GET", "/api/flash/status") => (S_200, T_JSON, flash_status_json().into_bytes()),
        ("GET", "/api/update") => (S_200, T_JSON, update_json().into_bytes()),
        ("POST", "/api/update") => set_update(body),
        ("POST", "/api/reboot")    => reboot_soon(),
        _ => (S_404, T_TEXT, b"not found\n".to_vec()),
    }
}

/// What the page shows: which sections are live and which firmware writes this dongle offers.
fn caps_json() -> String {
    let f = &caps().flash;
    format!(
        r#"{{"flash":{{"mtd":{},"stack":{},"rootfs":{}}},"led":{}}}"#,
        f.mtd, f.stack.is_some(), f.rootfs.is_some(), caps().led
    )
}

/// cpc200/i.MX6 flash: write the uploaded rootfs image to a staging file, then hand it to
/// flash-image.sh with the client-supplied sha256, which re-checks size + hash before it erases.
/// No FEL here, so the script is the guard — we never write the partition ourselves.
fn flash_rootfs(script: &str, partition: &str, query: &str, body: Option<&[u8]>, clen: u64)
    -> (&'static str, &'static str, Vec<u8>)
{
    let Some(data) = body else { return (S_400, T_JSON, err_json("empty body")); };
    if clen == 0 || (data.len() as u64) != clen {
        return (S_400, T_JSON, err_json("content-length mismatch"));
    }
    let sha = query.split('&').find_map(|kv| kv.strip_prefix("sha=")).unwrap_or("");
    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return (S_400, T_JSON, err_json("missing/!64-hex ?sha= — refusing an unguarded flash"));
    }
    let img = "/tmp/restore.img";
    if let Err(e) = fs::write(img, data) {
        return (S_500, T_JSON, err_json(&format!("stage {img}: {e}")));
    }
    let _ = fs::create_dir_all("/tmp/livi/led");
    let _ = fs::write("/tmp/livi/led/flash-mode", b"");
    // flash-image.sh stages its own tools to tmpfs, blinks, writes, verifies and reboots.
    match Command::new("/bin/sh").arg(script).arg(partition).arg(sha).arg(img).spawn() {
        Ok(_) => (S_200, T_JSON, ok_json(&format!(
            "flashing {partition} via {script}; the dongle reboots on success"
        ))),
        Err(e) => {
            let _ = fs::remove_file("/tmp/livi/led/flash-mode");
            (S_500, T_JSON, err_json(&format!("spawn {script}: {e}")))
        }
    }
}

/// Stack update: the uploaded gzip is the relay-stack binary. Write it to its home on jffs2, then
/// re-run the launcher so it unpacks and relinks — no partition erase, no full reboot.
fn flash_stack(install_to: &str, restart: &str, query: &str, body: Option<&[u8]>, clen: u64)
    -> (&'static str, &'static str, Vec<u8>)
{
    let Some(data) = body else { return (S_400, T_JSON, err_json("empty body")); };
    if clen == 0 || (data.len() as u64) != clen {
        return (S_400, T_JSON, err_json("content-length mismatch"));
    }
    if data.len() < 512 || !data.starts_with(&[0x1f, 0x8b]) {
        return (S_400, T_JSON, err_json("not a gzip — expected the stack .gz"));
    }
    let _ = fs::create_dir_all("/tmp/livi/led");
    let _ = fs::write("/tmp/livi/led/flash-mode", b"");
    // Write beside the target and rename, so the launcher never sees a half-written file.
    let tmp = format!("{install_to}.new");
    if let Err(e) = write_sync(&tmp, data) {
        let _ = fs::remove_file("/tmp/livi/led/flash-mode");
        return (S_500, T_JSON, err_json(&format!("write {tmp}: {e}")));
    }
    // A stack that does not unpack leaves a dongle with no web UI to fix it from.
    if let Some(want) = query.split('&').find_map(|kv| kv.strip_prefix("sha=")) {
        let got = Command::new("sha256sum").arg(&tmp).output().ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.split_whitespace().next().map(str::to_owned));
        if got.as_deref() != Some(want) {
            let _ = fs::remove_file(&tmp);
            let _ = fs::remove_file("/tmp/livi/led/flash-mode");
            return (S_400, T_JSON, err_json("sha256 mismatch, nothing installed"));
        }
    }
    if let Err(e) = fs::rename(&tmp, install_to) {
        let _ = fs::remove_file("/tmp/livi/led/flash-mode");
        return (S_500, T_JSON, err_json(&format!("install {install_to}: {e}")));
    }
    // The restart takes this httpd down with it, so the response must go out first.
    restart_after(Duration::from_millis(500), restart.to_string());
    (S_200, T_JSON, ok_json(&format!("installed {} B; restarting the stack", data.len())))
}

/// The single upload endpoint: routes by what this dongle offers and what the file is — a `.lfwb`
/// bundle ("LFWB"), a gzipped stack (1f 8b), else a full rootfs image.
fn flash_dispatch(c: &WebCaps, query: &str, body: Option<&[u8]>, clen: u64)
    -> (&'static str, &'static str, Vec<u8>)
{
    let Some(data) = body else { return (S_400, T_JSON, err_json("empty body")); };
    if c.flash.mtd && data.starts_with(BUNDLE_MAGIC) {
        return flash_bundle(body, clen);
    }
    if let Some(s) = c.flash.stack.as_ref().filter(|_| data.starts_with(&[0x1f, 0x8b])) {
        return flash_stack(&s.install_to, &s.restart, query, body, clen);
    }
    if let Some(r) = &c.flash.rootfs {
        return flash_rootfs(&r.script, &r.partition, query, body, clen);
    }
    (S_400, T_JSON, err_json("this upload matches no firmware this dongle can write"))
}

fn flash(node: &str, slot_size: u64, magic: &[u8], body: Option<&[u8]>, clen: u64)
    -> (&'static str, &'static str, Vec<u8>)
{
    let Some(data) = body else {
        return (S_400, T_JSON, err_json("empty body"));
    };
    if clen == 0 || (data.len() as u64) != clen {
        return (S_400, T_JSON, err_json("content-length mismatch"));
    }
    if data.len() as u64 > slot_size {
        return (S_400, T_JSON, err_json(&format!(
            "payload {} B exceeds {} slot ({} B)", data.len(), node, slot_size
        )));
    }
    if data.len() < 512 {
        return (S_400, T_JSON, err_json("payload absurdly small — refusing"));
    }

    // Pre-flight: refuse anything whose header magic does not match the
    // partition type. This is the guard that prevents a wrong file from
    // bricking mtd1 — the flash write only starts if the payload looks
    // structurally plausible.
    if !data.starts_with(magic) {
        let want = String::from_utf8_lossy(magic).to_string();
        let got: String = data.iter().take(magic.len())
            .map(|b| if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '.' })
            .collect();
        return (S_400, T_JSON, err_json(&format!(
            "payload magic mismatch for /dev/{node}: expected {:?}, got {:?} — refusing",
            want, got
        )));
    }

    // Tell livi-ledd we're flashing → red/blue alternating blink.
    // Cleared on any failure below; reboot itself clears the whole tmpfs.
    let _ = fs::create_dir_all("/tmp/livi/led");
    let _ = fs::write("/tmp/livi/led/flash-mode", b"");

    // Write, then fsync so the block driver commits the NOR erase+program.
    let path = format!("/dev/{node}");
    write_progress(node, 0, data.len(), "write");
    if let Err(e) = write_chunked(&path, data, node) {
        let _ = fs::remove_file("/tmp/livi/led/flash-mode");
        write_progress(node, 0, data.len(), "error");
        return (S_500, T_JSON, err_json(&format!("write /dev/{node}: {e}")));
    }

    // Post-flight: read what we just wrote back off flash and byte-compare.
    // Only reboot on a verified-good write. If verify fails we leave the
    // partition in whatever state it is in (bricked), but at least we do
    // NOT reboot into it — the caller learns and can FEL-recover instead.
    write_progress(node, data.len(), data.len(), "verify");
    match verify_flash(&path, data) {
        Ok(()) => {
            write_progress(node, data.len(), data.len(), "done");
            reboot_after(Duration::from_millis(500));
            (S_200, T_JSON, ok_json(&format!(
                "wrote {} B to /dev/{node}, verified, rebooting", data.len()
            )))
        }
        Err(e) => {
            let _ = fs::remove_file("/tmp/livi/led/flash-mode");
            write_progress(node, data.len(), data.len(), "error");
            (S_500, T_JSON, err_json(&format!(
                "verify /dev/{node} failed after write: {e} — DO NOT reboot, use FEL to restore"
            )))
        }
    }
}

// ---------------------------------------------------------------------------
// Firmware bundle (.lfwb): a single file carrying mtd1 + mtd3 (and future
// slots). On upload the dongle keeps the whole thing in RAM, then for each
// image compares CRC32 against what's currently on that partition and only
// writes if it actually differs. See scripts/livi-link/pack-bundle.sh.
// ---------------------------------------------------------------------------

const BUNDLE_MAGIC: &[u8; 4] = b"LFWB";
const BUNDLE_VERSION: u8 = 1;
const BUNDLE_HDR_LEN: usize = 8;
const BUNDLE_DESC_LEN: usize = 12;

struct ImageDesc {
    typ: u8,
    _flags: u8,
    length: u32,
    crc32: u32,
    payload_offset: usize,
}

fn flash_bundle(body: Option<&[u8]>, clen: u64) -> (&'static str, &'static str, Vec<u8>) {
    let Some(data) = body else {
        return (S_400, T_JSON, err_json("empty body"));
    };
    if clen == 0 || (data.len() as u64) != clen {
        return (S_400, T_JSON, err_json("content-length mismatch"));
    }
    if data.len() < BUNDLE_HDR_LEN {
        return (S_400, T_JSON, err_json("bundle too small for header"));
    }
    if &data[0..4] != BUNDLE_MAGIC {
        return (S_400, T_JSON, err_json("bundle magic mismatch — expected LFWB"));
    }
    if data[4] != BUNDLE_VERSION {
        return (S_400, T_JSON, err_json(&format!("bundle version {} not supported", data[4])));
    }
    let count = data[5] as usize;
    if count == 0 || count > 8 {
        return (S_400, T_JSON, err_json(&format!("bundle image count {} out of range", count)));
    }
    let descs_end = BUNDLE_HDR_LEN + count * BUNDLE_DESC_LEN;
    if data.len() < descs_end {
        return (S_400, T_JSON, err_json("bundle truncated in descriptor table"));
    }

    // Parse descriptors + reserve payload slices.
    let mut descs: Vec<ImageDesc> = Vec::with_capacity(count);
    let mut cursor = descs_end;
    for i in 0..count {
        let off = BUNDLE_HDR_LEN + i * BUNDLE_DESC_LEN;
        let typ = data[off];
        let flags = data[off + 1];
        let length = u32::from_le_bytes([data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);
        let crc = u32::from_le_bytes([data[off + 8], data[off + 9], data[off + 10], data[off + 11]]);
        if cursor + length as usize > data.len() {
            return (S_400, T_JSON, err_json(&format!(
                "bundle payload short for image {} (type {})", i, typ
            )));
        }
        descs.push(ImageDesc { typ, _flags: flags, length, crc32: crc, payload_offset: cursor });
        cursor += length as usize;
    }

    // Verify payload CRCs match what the descriptors promise.
    for (i, d) in descs.iter().enumerate() {
        let slice = &data[d.payload_offset .. d.payload_offset + d.length as usize];
        if crc32(slice) != d.crc32 {
            return (S_400, T_JSON, err_json(&format!(
                "bundle image {} (type {}) CRC mismatch — corrupt upload", i, d.typ
            )));
        }
    }

    // Signal LED flash-mode across the whole operation.
    let _ = fs::create_dir_all("/tmp/livi/led");
    let _ = fs::write("/tmp/livi/led/flash-mode", b"");

    // Walk images: skip if the on-flash content already matches, else write+verify.
    let mut wrote_any = false;
    let mut report: Vec<String> = Vec::new();
    for d in descs.iter() {
        let (node, magic, slot_size) = match d.typ {
            1 => ("mtdblock1", &b"ANDROID!"[..], MTD1_SIZE),
            3 => ("mtdblock3", &b"hsqs"[..],     MTD3_SIZE),
            other => {
                let _ = fs::remove_file("/tmp/livi/led/flash-mode");
                return (S_400, T_JSON, err_json(&format!("unknown image type {}", other)));
            }
        };
        let slice = &data[d.payload_offset .. d.payload_offset + d.length as usize];

        if (d.length as u64) > slot_size {
            let _ = fs::remove_file("/tmp/livi/led/flash-mode");
            return (S_400, T_JSON, err_json(&format!(
                "image type {} ({}) is {} B, exceeds slot {} B", d.typ, node, d.length, slot_size
            )));
        }
        if !slice.starts_with(magic) {
            let _ = fs::remove_file("/tmp/livi/led/flash-mode");
            return (S_400, T_JSON, err_json(&format!(
                "image type {} payload magic mismatch for {}", d.typ, node
            )));
        }

        let path = format!("/dev/{node}");
        // Compare against what's already on flash.
        write_progress(node, 0, d.length as usize, "compare");
        let same = flash_matches(&path, slice).unwrap_or(false);
        if same {
            report.push(format!("{} unchanged", node));
            write_progress(node, d.length as usize, d.length as usize, "unchanged");
            continue;
        }

        // Different — write + verify.
        write_progress(node, 0, d.length as usize, "write");
        if let Err(e) = write_chunked(&path, slice, node) {
            let _ = fs::remove_file("/tmp/livi/led/flash-mode");
            write_progress(node, 0, d.length as usize, "error");
            return (S_500, T_JSON, err_json(&format!("write /dev/{node}: {e}")));
        }
        write_progress(node, d.length as usize, d.length as usize, "verify");
        if let Err(e) = verify_flash(&path, slice) {
            let _ = fs::remove_file("/tmp/livi/led/flash-mode");
            write_progress(node, d.length as usize, d.length as usize, "error");
            return (S_500, T_JSON, err_json(&format!(
                "verify /dev/{node} failed: {e} — DO NOT reboot, use FEL to restore"
            )));
        }
        wrote_any = true;
        report.push(format!("{} written", node));
    }

    write_progress("bundle", data.len(), data.len(), "done");
    if wrote_any {
        reboot_after(Duration::from_millis(500));
        (S_200, T_JSON, ok_json(&format!("{}, rebooting", report.join(", "))))
    } else {
        let _ = fs::remove_file("/tmp/livi/led/flash-mode");
        (S_200, T_JSON, ok_json("Up-to-date"))
    }
}

/// Read `expected.len()` bytes from `path` and compare CRC32.
fn flash_matches(path: &str, expected: &[u8]) -> std::io::Result<bool> {
    let mut f = fs::File::open(path)?;
    let mut buf = vec![0u8; expected.len()];
    f.read_exact(&mut buf)?;
    Ok(crc32(&buf) == crc32(expected))
}

/// IEEE CRC-32 (poly 0xEDB88320) — matches Python zlib.crc32.
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

fn write_chunked(path: &str, data: &[u8], node: &str) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_SYNC)
        .open(path)?;
    // 64 KiB chunks give ~50 progress updates for a 3 MiB payload — enough
    // to feel live, few enough to keep the /tmp/livi/flash-progress writes
    // negligible next to the actual NOR erase+program cost.
    let chunk = 64 * 1024;
    let mut written = 0usize;
    for slice in data.chunks(chunk) {
        f.write_all(slice)?;
        written += slice.len();
        write_progress(node, written, data.len(), "write");
    }
    f.sync_all()?;
    unsafe { libc::sync(); }
    Ok(())
}

/// Write a whole file and flush it to storage.
fn write_sync(path: &str, data: &[u8]) -> std::io::Result<()> {
    let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).open(path)?;
    f.write_all(data)?;
    f.sync_all()?;
    unsafe { libc::sync(); }
    Ok(())
}

fn write_progress(node: &str, written: usize, total: usize, phase: &str) {
    let _ = fs::create_dir_all("/tmp/livi");
    let _ = fs::write(
        "/tmp/livi/flash-progress",
        format!("{node}:{written}:{total}:{phase}\n"),
    );
}

fn flash_status_json() -> String {
    let raw = fs::read_to_string("/tmp/livi/flash-progress").unwrap_or_default();
    // Format: node:written:total:phase
    let parts: Vec<&str> = raw.trim().split(':').collect();
    if parts.len() != 4 {
        return r#"{"phase":"idle","node":"","written":0,"total":0}"#.to_string();
    }
    let node = parts[0];
    let written: u64 = parts[1].parse().unwrap_or(0);
    let total:   u64 = parts[2].parse().unwrap_or(0);
    let phase = parts[3];
    format!(
        r#"{{"phase":"{}","node":"{}","written":{},"total":{}}}"#,
        js(phase), js(node), written, total
    )
}

fn verify_flash(path: &str, expected: &[u8]) -> std::io::Result<()> {
    let mut f = fs::OpenOptions::new().read(true).open(path)?;
    let mut got = vec![0u8; expected.len()];
    f.read_exact(&mut got)?;
    if got == expected {
        Ok(())
    } else {
        let mut diff_at = 0;
        for (i, (a, b)) in got.iter().zip(expected.iter()).enumerate() {
            if a != b { diff_at = i; break; }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("read-back mismatch at byte {diff_at}")
        ))
    }
}

fn reboot_soon() -> (&'static str, &'static str, Vec<u8>) {
    reboot_after(Duration::from_millis(500));
    (S_200, T_JSON, ok_json("rebooting"))
}

fn reboot_after(delay: Duration) {
    thread::spawn(move || {
        thread::sleep(delay);
        unsafe { libc::sync(); }
        // Prefer busybox reboot (userspace-friendly), fall back to the syscall.
        let _ = Command::new("/sbin/reboot").arg("-f").status();
        unsafe {
            libc::reboot(libc::LINUX_REBOOT_CMD_RESTART);
        }
    });
}

/// Re-runs the stack launcher detached, after the response has been sent — it takes this httpd
/// with it, so it must outlive the process.
fn restart_after(delay: Duration, cmd: String) {
    thread::spawn(move || {
        thread::sleep(delay);
        unsafe { libc::sync(); }
        let _ = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("setsid {cmd} </dev/null >/tmp/livi/stack-restart.log 2>&1 &"))
            .status();
    });
}

// ---------------------------------------------------------------------------
// Status / small helpers
// ---------------------------------------------------------------------------

fn bt_json() -> String {
    // AIC vendor driver ships its own hci_register_dev and skips the bt-core sysfs
    // group, so /sys has no 'address' file. livi-bt-up caches the MAC (via
    // HCIGETDEVINFO after HCIDEVUP) in /tmp/livi/bt-mac; we prefer sysfs when
    // available (upstream drivers) and fall back to the cached file otherwise.
    let bt = caps().bt.as_str();
    let mut mac = read_trim(&format!("/sys/class/bluetooth/{bt}/address"));
    if mac.is_empty() {
        mac = read_trim("/tmp/livi/bt-mac");
    }
    // Advertised (friendly) name — the string a phone would see when scanning.
    // Populated by whichever routine sets HCI Write_Local_Name; empty until
    // that lands. Prefer sysfs (upstream drivers expose it) then our cached
    // file, then fall back to no value at all.
    let mut name = read_trim(&format!("/sys/class/bluetooth/{bt}/name"));
    if name.is_empty() {
        name = read_trim("/tmp/livi/bt-name");
    }
    // iapd sets the controller name to the AP name (see livi-iapd), and the AIC driver exposes
    // no sysfs name file — so read it from the same hostapd config the AP name comes from.
    if name.is_empty() {
        name = livi_wifi::server::ap_name_from(
            std::path::Path::new(HOSTAPD_BASE),
            &[std::path::Path::new(HOSTAPD_LIVE[0]), std::path::Path::new(HOSTAPD_LIVE[1])],
        )
        .unwrap_or_default();
    }
    let present = std::path::Path::new(&format!("/sys/class/bluetooth/{bt}")).exists();
    let state = if !present { "not present" }
                else if mac.is_empty() { "up, MAC unknown" }
                else { "up" };
    format!(
        r#"{{"name":"{}","mac":"{}","state":"{}"}}"#,
        js(&name), js(&mac), js(state)
    )
}

const HOSTAPD_BASE: &str = "/tmp/livi/hostapd.conf.saved";
const HOSTAPD_LIVE: [&str; 2] = ["/tmp/livi/hostapd.conf", "/tmp/livi/hostapd.alt"];

fn wifi_json() -> String {
    let iface = caps().wifi_iface.as_str();
    let mut ssid = String::new();
    let mut ch = String::new();
    let mut band = String::new();
    let mut width = 0;
    if let Some(ap) = livi_wifi::ap_state(iface) {
        ssid = ap.ssid;
        ch = ap.channel.to_string();
        width = ap.width;
        band = if ap.channel <= 14 { "2.4 GHz" } else { "5 GHz" }.to_string();
    }
    // Fall back to the managed hostapd.conf when the AP is not up yet.
    if ssid.is_empty() {
        let cfg = livi_wifi::server::ap_config_from(
            std::path::Path::new(HOSTAPD_BASE),
            &[std::path::Path::new(HOSTAPD_LIVE[0]), std::path::Path::new(HOSTAPD_LIVE[1])],
        );
        if let Some(cfg) = cfg {
            for line in cfg.lines() {
                let l = line.trim();
                if let Some(v) = l.strip_prefix("ssid=")     { ssid = v.to_string(); }
                else if let Some(v) = l.strip_prefix("channel=")  { ch = v.to_string(); }
                else if let Some(v) = l.strip_prefix("hw_mode=")  { band = match v { "a" => "5 GHz", "g" => "2.4 GHz", "b" => "2.4 GHz", _ => v }.to_string(); }
            }
        }
    }
    let mac = read_trim(&format!("/sys/class/net/{iface}/address"));
    // Clients from the bridge forwarding table where there is a bridge (V821B/br0), else via an
    // nl80211 station dump (cpc200 bridges with l2fwd, no br0).
    let clients = match caps().bridge.as_deref() {
        Some(br) => bridge_port_clients(br, iface),
        None => livi_wifi::station_count(iface),
    };
    let (downrate, uprate) = livi_wifi::station_rates(iface).unwrap_or((0, 0));
    let downbytes = read_trim(&format!("/sys/class/net/{iface}/statistics/rx_bytes")).parse::<u64>().unwrap_or(0);
    let upbytes = read_trim(&format!("/sys/class/net/{iface}/statistics/tx_bytes")).parse::<u64>().unwrap_or(0);
    format!(
        r#"{{"ssid":"{}","mac":"{}","band":"{}","channel":"{}","width":{},"clients":{},"downrate":{},"uprate":{},"downbytes":{},"upbytes":{}}}"#,
        js(&ssid), js(&mac), js(&band), js(&ch), width, clients, downrate, uprate, downbytes, upbytes
    )
}

/// Stations behind one bridge port, from the bridge's forwarding table: with wlan0 inside br0
/// the ARP table names br0, not the port. brforward is 16-byte entries — mac[6], port_no,
/// is_local, then ageing and padding.
fn bridge_port_clients(bridge: &str, port: &str) -> usize {
    let port_no = read_trim(&format!("/sys/class/net/{port}/brport/port_no"));
    let Ok(port_no) = u8::from_str_radix(port_no.trim_start_matches("0x"), 16) else {
        return 0;
    };
    let Ok(fdb) = fs::read(format!("/sys/class/net/{bridge}/brforward")) else {
        return 0;
    };
    fdb.as_chunks::<16>()
        .0
        .iter()
        .filter(|entry| entry[6] == port_no && entry[7] == 0)
        .count()
}

fn status_json() -> String {
    let kernel = read_trim("/proc/sys/kernel/osrelease");
    let uptime = fmt_uptime(&read_trim("/proc/uptime"));
    let load   = read_trim("/proc/loadavg");
    let mem    = fmt_meminfo();
    let mac    = read_trim(&format!("/sys/class/net/{}/address", caps().host_iface));
    format!(
        r#"{{"model":"{}","target":"{}","version":"{}","build":"{}","kernel":"{}","uptime":"{}","load":"{}","mem":"{}","mac":"{}"}}"#,
        js(&caps().model), js(&caps().target), js(VERSION), js(BUILD),
        js(&kernel), js(&uptime), js(&load), js(&mem), js(&mac)
    )
}

fn ok_json(msg: &str)  -> Vec<u8> { format!(r#"{{"ok":true,"message":"{}"}}"#,  js(msg)).into_bytes() }
fn err_json(msg: &str) -> Vec<u8> { format!(r#"{{"ok":false,"error":"{}"}}"#, js(msg)).into_bytes() }

// ---------------------------------------------------------------------------
// LED config API (paired with livi-ledd)
// ---------------------------------------------------------------------------

// /etc/ is read-only squashfs; runtime config lives on tmpfs and is
// seeded from /etc/livi/led.toml by rcS at boot.
const LED_CFG: &str = "/tmp/livi/led.toml";
const LED_PID: &str = "/tmp/livi/livi-ledd.pid";

fn led_json() -> String {
    // Default matches livi-ledd's Config::default() = web-UI accent #4dd0e1.
    let mut r = 0x4du8; let mut g = 0xd0u8; let mut b = 0xe1u8;
    let mut brightness = 20u8; // 0-100 %
    if let Ok(s) = fs::read_to_string(LED_CFG) {
        for line in s.lines() {
            let line = line.trim();
            // Only whole-line comments — a `#` mid-value belongs to the value.
            if line.is_empty() || line.starts_with('#') { continue; }
            let Some((k, v)) = line.split_once('=') else { continue; };
            let v = v.trim().trim_matches('"');
            match k.trim() {
                "status_color" => if let Some((rr, gg, bb)) = parse_rgb(v) { r = rr; g = gg; b = bb; },
                "brightness"   => if let Ok(n) = v.parse::<u8>() { brightness = n.min(100); },
                _ => {}
            }
        }
    }
    format!(
        r##"{{"status_color":"#{:02x}{:02x}{:02x}","brightness":{}}}"##,
        r, g, b, brightness
    )
}

fn set_led(body: Option<&[u8]>) -> (&'static str, &'static str, Vec<u8>) {
    let Some(data) = body else { return (S_400, T_JSON, err_json("empty body")); };
    let Ok(s) = std::str::from_utf8(data) else {
        return (S_400, T_JSON, err_json("body not UTF-8"));
    };

    // Accept a tiny form encoding: status_color=%23aabbcc&brightness=50
    // or a JSON-ish subset. We keep it dumb — no serde.
    let mut status: Option<(u8, u8, u8)> = None;
    let mut brightness: Option<u8> = None;

    let trimmed = s.trim();
    if trimmed.starts_with('{') {
        // Very small JSON subset: pick out "status_color":"…" and "brightness":N
        if let Some(v) = json_str(trimmed, "status_color") {
            status = parse_rgb(&v);
        }
        if let Some(v) = json_num(trimmed, "brightness") {
            brightness = v.parse::<u8>().ok().map(|n| n.min(100));
        }
    } else {
        for kv in trimmed.split('&') {
            let Some((k, v)) = kv.split_once('=') else { continue; };
            let v = url_decode(v);
            match k {
                "status_color" => status = parse_rgb(&v),
                "brightness"   => brightness = v.parse::<u8>().ok().map(|n| n.min(100)),
                _ => {}
            }
        }
    }

    // Merge with existing config so partial updates don't nuke fields.
    let existing = led_json();
    let cur_status = json_str(&existing, "status_color").and_then(|v| parse_rgb(&v)).unwrap_or((0x4d,0xd0,0xe1));
    let cur_bri    = json_num(&existing, "brightness").and_then(|v| v.parse::<u8>().ok()).unwrap_or(20);

    let (r, g, b) = status.unwrap_or(cur_status);
    let bri = brightness.unwrap_or(cur_bri);

    // Preserve the white-balance line the web form never sends, so a colour/brightness
    // save does not wipe a hand-tuned value. Falls back to the seeded default.
    let cur_wb = fs::read_to_string(LED_CFG)
        .ok()
        .and_then(|s| {
            s.lines().find_map(|l| {
                let l = l.trim();
                if l.starts_with('#') {
                    return None;
                }
                let (k, v) = l.split_once('=')?;
                (k.trim() == "white_balance").then(|| v.trim().trim_matches('"').to_string())
            })
        })
        .unwrap_or_else(|| "#ffbe82".to_string());

    let contents = format!(
        "# LIVI-Link LED config (managed by livi-httpd). brightness is 0-100 %.\n\
         status_color = \"#{:02x}{:02x}{:02x}\"\n\
         brightness = {}\n\
         white_balance = \"{}\"\n",
        r, g, b, bri, cur_wb
    );

    let _ = fs::create_dir_all("/tmp/livi");
    if let Err(e) = fs::write(LED_CFG, contents) {
        return (S_500, T_JSON, err_json(&format!("write {LED_CFG}: {e}")));
    }
    kick_ledd();
    // Persist to mtd4 in the background so the response returns promptly.
    // Wear is not a concern at the debounced pace at which the UI sends
    // updates (150 ms per user gesture, one write per real change).
    persist_config();
    (S_200, T_JSON, ok_json("led config updated"))
}

// ---------------------------------------------------------------------------
// Update channel
// ---------------------------------------------------------------------------

fn update_nightly() -> bool {
    fs::read_to_string(&caps().update_conf)
        .is_ok_and(|s| s.lines().any(|l| l.trim() == "nightly=1"))
}

fn update_json() -> String {
    format!(r#"{{"nightly":{}}}"#, update_nightly())
}

fn set_update(body: Option<&[u8]>) -> (&'static str, &'static str, Vec<u8>) {
    let Some(s) = body.and_then(|b| std::str::from_utf8(b).ok()) else {
        return (S_400, T_JSON, err_json("empty body"));
    };
    let nightly = match json_num(s.trim(), "nightly").as_deref() {
        Some("1") => true,
        Some("0") => false,
        _ => return (S_400, T_JSON, err_json("nightly must be 0 or 1")),
    };
    let path = &caps().update_conf;
    if let Some(dir) = std::path::Path::new(path).parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Err(e) = fs::write(path, format!("nightly={}\n", u8::from(nightly))) {
        return (S_500, T_JSON, err_json(&format!("{path}: {e}")));
    }
    persist_config();
    (S_200, T_JSON, ok_json("update channel saved"))
}

fn kick_ledd() {
    let Ok(s) = fs::read_to_string(LED_PID) else { return; };
    let Ok(pid) = s.trim().parse::<i32>() else { return; };
    unsafe { libc::kill(pid, libc::SIGHUP); }
}

fn persist_config() {
    // Fire off `livid config save` — non-blocking, no waiting on flash I/O.
    let _ = Command::new("/usr/bin/livid")
        .arg("config").arg("save")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn parse_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() != 6 { return None; }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        return Some((r, g, b));
    }
    let parts: Vec<_> = s.split(',').map(|p| p.trim()).collect();
    if parts.len() != 3 { return None; }
    Some((parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?))
}

fn json_str(hay: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let i = hay.find(&needle)?;
    let after = &hay[i + needle.len()..];
    let colon = after.find(':')?;
    let rest = &after[colon + 1..];
    let start = rest.find('"')?;
    let rest = &rest[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn json_num(hay: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let i = hay.find(&needle)?;
    let after = &hay[i + needle.len()..];
    let colon = after.find(':')?;
    let rest = after[colon + 1..].trim_start();
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    if end == 0 { return None; }
    Some(rest[..end].to_string())
}

fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.bytes();
    while let Some(b) = it.next() {
        match b {
            b'+' => out.push(' '),
            b'%' => {
                let h = it.next().unwrap_or(b'0') as char;
                let l = it.next().unwrap_or(b'0') as char;
                if let Ok(v) = u8::from_str_radix(&format!("{h}{l}"), 16) {
                    out.push(v as char);
                }
            }
            _ => out.push(b as char),
        }
    }
    out
}

fn read_trim(path: &str) -> String {
    fs::read_to_string(path).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn fmt_uptime(s: &str) -> String {
    let secs = s.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) as u64;
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if d > 0 { format!("{d}d {h:02}:{m:02}:{s:02}") }
    else     { format!("{h:02}:{m:02}:{s:02}") }
}

fn fmt_meminfo() -> String {
    let mut total_kb = 0u64;
    let mut avail_kb = 0u64;
    if let Ok(s) = fs::read_to_string("/proc/meminfo") {
        for line in s.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                total_kb = rest.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0);
            } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
                avail_kb = rest.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0);
            }
        }
    }
    format!("{} KiB used / {} KiB total", total_kb.saturating_sub(avail_kb), total_kb)
}

fn js(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' | '\\' => { out.push('\\'); out.push(c); }
            '\n' | '\r' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

fn write_response(w: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        body.len()
    );
    w.write_all(header.as_bytes())?;
    w.write_all(body)?;
    Ok(())
}

const S_200: &str = "200 OK";
const S_400: &str = "400 Bad Request";
const S_404: &str = "404 Not Found";
const S_500: &str = "500 Internal Server Error";
const T_HTML: &str = "text/html; charset=utf-8";
const T_JSON: &str = "application/json";
const T_TEXT: &str = "text/plain; charset=utf-8";
