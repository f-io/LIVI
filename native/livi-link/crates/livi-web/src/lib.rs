use std::sync::OnceLock;

#[derive(Default)]
pub struct Flash {
    pub mtd: Vec<MtdSlot>,
    /// Shell command after a verified write. The dongle only reboots when it succeeds.
    pub check: Option<String>,
}

/// `size` is the partition's, a larger image would run into the next one.
pub struct MtdSlot {
    pub typ: u8,
    pub node: String,
    pub magic: Vec<u8>,
    pub size: u64,
    /// For a partition whose bootloader does not take the image as it is.
    pub stage: Option<Stage>,
    pub before_write: Option<fn() -> Result<(), String>>,
}

/// Takes the image and what the partition holds now.
pub type Stage = fn(&[u8], &[u8]) -> Result<Vec<u8>, String>;

pub struct WebCaps {
    pub model: String,
    /// The directory under assets/livi-link this dongle's firmware is published in.
    pub target: String,
    pub port: u16,
    pub wifi_iface: String,
    /// Its MAC stands in as the dongle's address.
    pub host_iface: String,
    /// The file where mfid notes the MFi coprocessor it found.
    pub mfi: String,
    pub bt: String,
    pub led: bool,
    pub flash: Flash,
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

use livi_wifid::radio::{self, Radio};

// Lives under web/ because the Mac↔build-host sync skips every assets/ directory.
const INDEX_HTML: &str = include_str!("../web/index.html");

fn serve_forever() -> std::io::Result<()> {
    let port = caps().port;
    let addr = SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, port, 0, 0);
    let listener = TcpListener::bind(SocketAddr::V6(addr))?;
    eprintln!("[livi-httpd] listening on [::]:{port} (dual-stack)");

    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    for conn in listener.incoming() {
        match conn {
            Ok(s) => {
                thread::spawn(|| handle(s));
            }
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
    if first.is_empty() {
        return None;
    }
    let mut parts = first.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();

    let mut content_length: u64 = 0;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).ok()?;
        if n <= 2 {
            break;
        } // empty CRLF terminates headers
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
    }

    let body = if content_length > 0 {
        let mut buf = vec![0u8; content_length as usize];
        reader.read_exact(&mut buf).ok()?;
        Some(buf)
    } else {
        None
    };

    Some((method, path, content_length, body))
}

fn route(
    rmethod: &str,
    rpath: &str,
    body: Option<&[u8]>,
    clen: u64,
) -> (&'static str, &'static str, Vec<u8>) {
    let c = caps();
    let path = rpath.split_once('?').map_or(rpath, |(p, _)| p);
    let method = rmethod;

    if c.led {
        match (method, path) {
            ("GET", "/api/led") => return (S_200, T_JSON, led_json().into_bytes()),
            ("POST", "/api/led") => return set_led(body),
            _ => {}
        }
    }
    if !c.flash.mtd.is_empty() && (method, path) == ("POST", "/api/flash") {
        return flash_dispatch(c, body, clen);
    }

    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => (S_200, T_HTML, INDEX_HTML.as_bytes().to_vec()),
        ("GET", "/api/status") => (S_200, T_JSON, status_json().into_bytes()),
        ("GET", "/api/wifi") => (S_200, T_JSON, wifi_json().into_bytes()),
        ("POST", "/api/wifi") => switch_radio(body, Radio::Wifi),
        ("GET", "/api/bt") => (S_200, T_JSON, bt_json().into_bytes()),
        ("POST", "/api/bt") => switch_radio(body, Radio::Bt),
        ("GET", "/api/caps") => (S_200, T_JSON, caps_json().into_bytes()),
        ("GET", "/api/flash/status") => (S_200, T_JSON, flash_status_json().into_bytes()),
        ("GET", "/api/update") => (S_200, T_JSON, update_json().into_bytes()),
        ("POST", "/api/update") => set_update(body),
        ("POST", "/api/reboot") => reboot_soon(),
        _ => (S_404, T_TEXT, b"not found\n".to_vec()),
    }
}

fn caps_json() -> String {
    format!(r#"{{"flash":{{"mtd":{}}},"led":{}}}"#, !caps().flash.mtd.is_empty(), caps().led)
}

fn flash_dispatch(
    c: &WebCaps,
    body: Option<&[u8]>,
    clen: u64,
) -> (&'static str, &'static str, Vec<u8>) {
    let Some(data) = body else {
        return (S_400, T_JSON, err_json("empty body"));
    };
    if data.starts_with(BUNDLE_MAGIC) {
        return flash_bundle(&c.flash, body, clen);
    }
    (S_400, T_JSON, err_json("not a LIVI Link firmware bundle (.lfwb)"))
}

// The .lfwb layout is defined by scripts/livi-link/common/pack-bundle.sh.
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

fn flash_bundle(
    f: &Flash,
    body: Option<&[u8]>,
    clen: u64,
) -> (&'static str, &'static str, Vec<u8>) {
    let Some(data) = body else {
        return (S_400, T_JSON, err_json("empty body"));
    };
    if clen == 0 || (data.len() as u64) != clen {
        return (S_400, T_JSON, err_json("content-length mismatch"));
    }
    if data.len() < BUNDLE_HDR_LEN {
        return (S_400, T_JSON, err_json("bundle too small for header"));
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

    let mut descs: Vec<ImageDesc> = Vec::with_capacity(count);
    let mut cursor = descs_end;
    for i in 0..count {
        let off = BUNDLE_HDR_LEN + i * BUNDLE_DESC_LEN;
        let typ = data[off];
        let flags = data[off + 1];
        let length =
            u32::from_le_bytes([data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);
        let crc =
            u32::from_le_bytes([data[off + 8], data[off + 9], data[off + 10], data[off + 11]]);
        if cursor + length as usize > data.len() {
            return (
                S_400,
                T_JSON,
                err_json(&format!("bundle payload short for image {} (type {})", i, typ)),
            );
        }
        descs.push(ImageDesc { typ, _flags: flags, length, crc32: crc, payload_offset: cursor });
        cursor += length as usize;
    }

    for (i, d) in descs.iter().enumerate() {
        let slice = &data[d.payload_offset..d.payload_offset + d.length as usize];
        if crc32(slice) != d.crc32 {
            return (
                S_400,
                T_JSON,
                err_json(&format!(
                    "bundle image {} (type {}) CRC mismatch — corrupt upload",
                    i, d.typ
                )),
            );
        }
    }

    // Every image is checked before the first write, so a bundle this dongle cannot take in full
    // never leaves the flash half rewritten.
    let mut staged: Vec<(&MtdSlot, Option<Vec<u8>>)> = Vec::with_capacity(descs.len());
    for d in descs.iter() {
        let Some(slot) = f.mtd.iter().find(|s| s.typ == d.typ) else {
            return (
                S_400,
                T_JSON,
                err_json(&format!("image type {} is not for this dongle, nothing written", d.typ)),
            );
        };
        let slice = &data[d.payload_offset..d.payload_offset + d.length as usize];
        if (d.length as u64) > slot.size {
            return (
                S_400,
                T_JSON,
                err_json(&format!(
                    "image type {} ({}) is {} B, exceeds slot {} B, nothing written",
                    d.typ, slot.node, d.length, slot.size
                )),
            );
        }
        if !slice.starts_with(slot.magic.as_slice()) {
            return (
                S_400,
                T_JSON,
                err_json(&format!(
                    "image type {} payload magic mismatch for {}, nothing written",
                    d.typ, slot.node
                )),
            );
        }
        let Some(stage) = slot.stage else {
            staged.push((slot, None));
            continue;
        };
        let raw = raw_mtd(&slot.node);
        match fs::read(&raw)
            .map_err(|e| format!("read {raw}: {e}"))
            .and_then(|now| stage(slice, &now))
        {
            Ok(part) => staged.push((slot, Some(part))),
            Err(e) => {
                return (
                    S_400,
                    T_JSON,
                    err_json(&format!(
                        "image type {} for {}: {e}, nothing written",
                        d.typ, slot.node
                    )),
                );
            }
        }
    }

    led_flash_running();

    let mut wrote_any = false;
    let mut report: Vec<String> = Vec::new();
    for (d, (slot, staged)) in descs.iter().zip(&staged) {
        let node = slot.node.as_str();
        let slice = &data[d.payload_offset..d.payload_offset + d.length as usize];
        let image = staged.as_deref().unwrap_or(slice);
        let len = image.len();
        let path = format!("/dev/{node}");
        // Read back through the character device, the block device answers from the page cache.
        let raw = raw_mtd(node);
        write_progress(node, 0, len, "compare");
        let same = flash_matches(&raw, image).unwrap_or(false);
        if same {
            report.push(format!("{} unchanged", node));
            write_progress(node, len, len, "unchanged");
            continue;
        }

        if let Some(before) = slot.before_write
            && let Err(e) = before()
        {
            led_flash_error();
            write_progress(node, 0, len, "error");
            report.push(format!("{node}: {e}, not written"));
            return (S_500, T_JSON, err_json(&report.join(", ")));
        }

        write_progress(node, 0, len, "write");
        if let Err(e) = write_chunked(&path, image, node) {
            led_flash_error();
            write_progress(node, 0, len, "error");
            return (S_500, T_JSON, err_json(&format!("write /dev/{node}: {e}")));
        }
        write_progress(node, len, len, "verify");
        if let Err(e) = verify_flash(&raw, image) {
            led_flash_error();
            write_progress(node, len, len, "error");
            return (
                S_500,
                T_JSON,
                err_json(&format!(
                    "verify {raw} failed: {e} — DO NOT reboot, do not unplug, the chip does not hold what was written"
                )),
            );
        }
        wrote_any = true;
        report.push(format!("{} written", node));
    }

    if wrote_any && let Err(e) = post_write_check(f) {
        write_progress("bundle", data.len(), data.len(), "error");
        return (S_500, T_JSON, err_json(&format!("{}, but {e}", report.join(", "))));
    }
    write_progress("bundle", data.len(), data.len(), "done");
    if wrote_any {
        // Green only once every image is verified and the check has passed.
        led_flash_done();
        reboot_after(Duration::from_millis(500));
        (S_200, T_JSON, ok_json(&format!("{}, rebooting", report.join(", "))))
    } else {
        led_flash_clear();
        (S_200, T_JSON, ok_json("Up-to-date"))
    }
}

fn post_write_check(f: &Flash) -> Result<(), String> {
    let Some(cmd) = &f.check else { return Ok(()) };
    match Command::new("sh").arg("-c").arg(cmd).status() {
        Ok(st) if st.success() => Ok(()),
        _ => {
            led_flash_error();
            Err(format!("{cmd} failed after the write, not rebooting"))
        }
    }
}

const LED_DIR: &str = "/tmp/livi/led";

// LED states during a flash, one file each under /tmp/livi/led:
//   flash-mode  red/blue alternating until the last image is verified
//   flash-done  steady green, all verified, safe to unplug or reboot
//   flash-error steady red, a write or its check failed, do not unplug or reboot
// A reboot clears the tmpfs. On a board that cannot reboot yet, the state stays until unplugged.
fn led_flash_set(state: Option<&str>) {
    let _ = fs::create_dir_all(LED_DIR);
    for f in ["flash-mode", "flash-done", "flash-error"] {
        let _ = fs::remove_file(format!("{LED_DIR}/{f}"));
    }
    if let Some(name) = state {
        let _ = fs::write(format!("{LED_DIR}/{name}"), b"");
    }
}
fn led_flash_running() {
    led_flash_set(Some("flash-mode"))
}
fn led_flash_done() {
    led_flash_set(Some("flash-done"))
}
fn led_flash_error() {
    led_flash_set(Some("flash-error"))
}
fn led_flash_clear() {
    led_flash_set(None)
}

fn raw_mtd(node: &str) -> String {
    match node.strip_prefix("mtdblock") {
        Some(n) => format!("/dev/mtd{n}"),
        None => format!("/dev/{node}"),
    }
}

fn flash_matches(path: &str, expected: &[u8]) -> std::io::Result<bool> {
    let mut f = fs::File::open(path)?;
    let mut buf = vec![0u8; expected.len()];
    f.read_exact(&mut buf)?;
    Ok(crc32(&buf) == crc32(expected))
}

/// IEEE CRC-32 (poly 0xEDB88320), the one zlib computes.
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

fn write_chunked(path: &str, data: &[u8], node: &str) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new().write(true).custom_flags(libc::O_SYNC).open(path)?;
    // 64 KiB gives about 50 progress updates for 3 MiB, cheap next to the NOR erase and program.
    let chunk = 64 * 1024;
    let mut written = 0usize;
    for slice in data.chunks(chunk) {
        f.write_all(slice)?;
        written += slice.len();
        write_progress(node, written, data.len(), "write");
    }
    f.sync_all()?;
    unsafe {
        libc::sync();
    }
    Ok(())
}

fn write_progress(node: &str, written: usize, total: usize, phase: &str) {
    let _ = fs::create_dir_all("/tmp/livi");
    let _ = fs::write("/tmp/livi/flash-progress", format!("{node}:{written}:{total}:{phase}\n"));
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
    let total: u64 = parts[2].parse().unwrap_or(0);
    let phase = parts[3];
    format!(
        r#"{{"phase":"{}","node":"{}","written":{},"total":{}}}"#,
        js(phase),
        js(node),
        written,
        total
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
            if a != b {
                diff_at = i;
                break;
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("read-back mismatch at byte {diff_at}"),
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
        unsafe {
            libc::sync();
        }
        let _ = Command::new("/sbin/reboot").arg("-f").status();
        unsafe {
            libc::reboot(libc::LINUX_REBOOT_CMD_RESTART);
        }
    });
}

fn bt_json() -> String {
    // The AIC driver has no sysfs `address` file, so the MAC falls back to what livi-bt-up
    // cached in /tmp/livi/bt-mac.
    let bt = caps().bt.as_str();
    let mut mac = read_trim(&format!("/sys/class/bluetooth/{bt}/address"));
    if mac.is_empty() {
        mac = read_trim("/tmp/livi/bt-mac");
    }
    let mut name = read_trim(&format!("/sys/class/bluetooth/{bt}/name"));
    // accessoryd gives the controller the AP name, so the hostapd config has it when sysfs does not.
    if name.is_empty() {
        name = livi_wifid::server::ap_name_from(
            std::path::Path::new(HOSTAPD_BASE),
            &[std::path::Path::new(HOSTAPD_LIVE[0]), std::path::Path::new(HOSTAPD_LIVE[1])],
        )
        .unwrap_or_default();
    }
    let present = std::path::Path::new(&format!("/sys/class/bluetooth/{bt}")).exists();
    let enabled = radio::enabled(Radio::Bt);
    let state = if !enabled {
        "switched off"
    } else if !present {
        "not present"
    } else if mac.is_empty() {
        "up, MAC unknown"
    } else {
        "up"
    };
    format!(
        r#"{{"enabled":{enabled},"name":"{}","mac":"{}","state":"{}"}}"#,
        js(&name),
        js(&mac),
        js(state)
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
        ch = ap.channel.number.to_string();
        width = ap.width;
        band = ap.channel.band.to_string();
    }
    if ssid.is_empty() {
        let cfg = livi_wifid::server::ap_config_from(
            std::path::Path::new(HOSTAPD_BASE),
            &[std::path::Path::new(HOSTAPD_LIVE[0]), std::path::Path::new(HOSTAPD_LIVE[1])],
        );
        if let Some(cfg) = cfg {
            for line in cfg.lines() {
                let l = line.trim();
                if let Some(v) = l.strip_prefix("ssid=") {
                    ssid = v.to_string();
                } else if let Some(v) = l.strip_prefix("channel=") {
                    ch = v.to_string();
                    if let Ok(number) = v.parse() {
                        band = livi_wifi::Channel::of_number(number).band.to_string();
                    }
                }
            }
        }
    }
    let mac = read_trim(&format!("/sys/class/net/{iface}/address"));
    let stations = livi_wifi::stations(iface);
    let clients = stations.count;
    let (downrate, uprate) = stations.rates.unwrap_or((0, 0));
    let downbytes = read_trim(&format!("/sys/class/net/{iface}/statistics/rx_bytes"))
        .parse::<u64>()
        .unwrap_or(0);
    let upbytes = read_trim(&format!("/sys/class/net/{iface}/statistics/tx_bytes"))
        .parse::<u64>()
        .unwrap_or(0);
    format!(
        r#"{{"enabled":{},"ssid":"{}","mac":"{}","band":"{}","channel":"{}","width":{},"clients":{},"downrate":{},"uprate":{},"downbytes":{},"upbytes":{}}}"#,
        radio::enabled(Radio::Wifi),
        js(&ssid),
        js(&mac),
        js(&band),
        js(&ch),
        width,
        clients,
        downrate,
        uprate,
        downbytes,
        upbytes
    )
}

fn status_json() -> String {
    let kernel = read_trim("/proc/sys/kernel/osrelease");
    let uptime = fmt_uptime(&read_trim("/proc/uptime"));
    let load = read_trim("/proc/loadavg");
    let mem = fmt_meminfo();
    let mac = read_trim(&format!("/sys/class/net/{}/address", caps().host_iface));
    let mut cp = read_trim(&caps().mfi);
    if cp.is_empty() {
        cp = "not found".into();
    }
    format!(
        r#"{{"model":"{}","target":"{}","version":"{}","build":"{}","kernel":"{}","uptime":"{}","load":"{}","mem":"{}","mac":"{}","cp":"{}"}}"#,
        js(&caps().model),
        js(&caps().target),
        js(VERSION),
        js(BUILD),
        js(&kernel),
        js(&uptime),
        js(&load),
        js(&mem),
        js(&mac),
        js(&cp)
    )
}

fn ok_json(msg: &str) -> Vec<u8> {
    format!(r#"{{"ok":true,"message":"{}"}}"#, js(msg)).into_bytes()
}
fn err_json(msg: &str) -> Vec<u8> {
    format!(r#"{{"ok":false,"error":"{}"}}"#, js(msg)).into_bytes()
}

// /etc is read-only. livid seeds this tmpfs copy from /etc/livi/led.toml at boot.
const LED_CFG: &str = "/tmp/livi/led.toml";
const LED_PID: &str = "/tmp/livi/livi-ledd.pid";

fn led_json() -> String {
    // livi-ledd's default, the web UI accent #4dd0e1.
    let mut r = 0x4du8;
    let mut g = 0xd0u8;
    let mut b = 0xe1u8;
    let mut brightness = 20u8; // 0-100 %
    if let Ok(s) = fs::read_to_string(LED_CFG) {
        for line in s.lines() {
            let line = line.trim();
            // Only whole-line comments, a `#` inside a value is part of it.
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim().trim_matches('"');
            match k.trim() {
                "status_color" => {
                    if let Some((rr, gg, bb)) = parse_rgb(v) {
                        r = rr;
                        g = gg;
                        b = bb;
                    }
                }
                "brightness" => {
                    if let Ok(n) = v.parse::<u8>() {
                        brightness = n.min(100);
                    }
                }
                _ => {}
            }
        }
    }
    format!(r##"{{"status_color":"#{:02x}{:02x}{:02x}","brightness":{}}}"##, r, g, b, brightness)
}

fn set_led(body: Option<&[u8]>) -> (&'static str, &'static str, Vec<u8>) {
    let Some(data) = body else {
        return (S_400, T_JSON, err_json("empty body"));
    };
    let Ok(s) = std::str::from_utf8(data) else {
        return (S_400, T_JSON, err_json("body not UTF-8"));
    };

    // Form encoding (status_color=%23aabbcc&brightness=50) or a small JSON subset.
    let mut status: Option<(u8, u8, u8)> = None;
    let mut brightness: Option<u8> = None;

    let trimmed = s.trim();
    if trimmed.starts_with('{') {
        if let Some(v) = json_str(trimmed, "status_color") {
            status = parse_rgb(&v);
        }
        if let Some(v) = json_num(trimmed, "brightness") {
            brightness = v.parse::<u8>().ok().map(|n| n.min(100));
        }
    } else {
        for kv in trimmed.split('&') {
            let Some((k, v)) = kv.split_once('=') else {
                continue;
            };
            let v = url_decode(v);
            match k {
                "status_color" => status = parse_rgb(&v),
                "brightness" => brightness = v.parse::<u8>().ok().map(|n| n.min(100)),
                _ => {}
            }
        }
    }

    let existing = led_json();
    let cur_status = json_str(&existing, "status_color")
        .and_then(|v| parse_rgb(&v))
        .unwrap_or((0x4d, 0xd0, 0xe1));
    let cur_bri =
        json_num(&existing, "brightness").and_then(|v| v.parse::<u8>().ok()).unwrap_or(20);

    let (r, g, b) = status.unwrap_or(cur_status);
    let bri = brightness.unwrap_or(cur_bri);

    // The page never sends white_balance, a hand-tuned value must survive a save.
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
    // The page debounces, so these flash writes stay rare.
    persist_config();
    (S_200, T_JSON, ok_json("led config updated"))
}

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

/// Switches through wifid, so the page and a host take the same path.
fn switch_radio(body: Option<&[u8]>, radio: Radio) -> (&'static str, &'static str, Vec<u8>) {
    let Some(s) = body.and_then(|b| std::str::from_utf8(b).ok()) else {
        return (S_400, T_JSON, err_json("empty body"));
    };
    let on = match json_num(s.trim(), "enabled").as_deref() {
        Some("1") => true,
        Some("0") => false,
        _ => return (S_400, T_JSON, err_json("enabled must be 0 or 1")),
    };
    let command = match (radio, on) {
        (Radio::Wifi, true) => "on",
        (Radio::Wifi, false) => "off",
        (Radio::Bt, true) => "bt on",
        (Radio::Bt, false) => "bt off",
    };
    if let Err(e) = wifid(command) {
        return (S_500, T_JSON, err_json(&e));
    }
    let json = match radio {
        Radio::Wifi => wifi_json(),
        Radio::Bt => bt_json(),
    };
    (S_200, T_JSON, json.into_bytes())
}

fn wifid(command: &str) -> Result<(), String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], livi_net::port::CONTROL));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .map_err(|e| format!("wifid: {e}"))?;
    // Bringing the AP or hci0 up takes a while.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    stream.write_all(format!("{command}\n").as_bytes()).map_err(|e| format!("wifid: {e}"))?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).map_err(|e| format!("wifid: {e}"))?;
    match line.trim() {
        "ok" => Ok(()),
        other => Err(other.strip_prefix("error ").unwrap_or(other).to_string()),
    }
}

fn kick_ledd() {
    let Ok(s) = fs::read_to_string(LED_PID) else {
        return;
    };
    let Ok(pid) = s.trim().parse::<i32>() else {
        return;
    };
    unsafe {
        libc::kill(pid, libc::SIGHUP);
    }
}

fn persist_config() {
    // Not awaited, the flash write is slow.
    let _ = Command::new("/usr/bin/livid")
        .arg("config")
        .arg("save")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn parse_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        return Some((r, g, b));
    }
    let parts: Vec<_> = s.split(',').map(|p| p.trim()).collect();
    if parts.len() != 3 {
        return None;
    }
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
    if end == 0 {
        return None;
    }
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
    let secs =
        s.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) as u64;
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if d > 0 { format!("{d}d {h:02}:{m:02}:{s:02}") } else { format!("{h:02}:{m:02}:{s:02}") }
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
    mem_text(total_kb, avail_kb)
}

fn mem_text(total_kb: u64, avail_kb: u64) -> String {
    let mb = |kb: u64| (kb + 512) / 1024;
    format!("{} MB used / {} MB total", mb(total_kb.saturating_sub(avail_kb)), mb(total_kb))
}

fn js(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

fn write_response(
    w: &mut TcpStream,
    status: &str,
    ctype: &str,
    body: &[u8],
) -> std::io::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::{mem_text, raw_mtd};

    #[test]
    fn a_block_node_is_read_back_through_its_character_device() {
        assert_eq!(raw_mtd("mtdblock6"), "/dev/mtd6");
        assert_eq!(raw_mtd("mtdblock12"), "/dev/mtd12");
        assert_eq!(raw_mtd("mtd6"), "/dev/mtd6");
    }

    #[test]
    fn memory_reads_in_whole_megabytes() {
        assert_eq!(mem_text(123_940, 112_276), "11 MB used / 121 MB total");
    }
}
