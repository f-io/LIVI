//! The Ingenic X1600 dongle sold as "Mini Ultra 3". Its web stack is not the `index.cgi` one: the
//! model answers `getversion.cgi` as `v851se-yunlian-cp`, and the update goes through
//! `downfile.cgi` and `submition.cgi`. The OTA writes the bank that is not running and flips the
//! pointer when it is done, a bank that does not boot falls back on its own, so there is no raw
//! flash write here.

pub mod image;
pub mod shell;

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::dongle::shell::BindShell;
use crate::dongle::{DONGLE_HOST, lfwb, mtd_is};

pub const MODEL: &str = "v851se-yunlian-cp";

const CHUNK: usize = 10 * 1024;
const OK_MARKER: &str = "ota update ok";

/// `/proc/mtd` of the stock firmware, also what the backup reads.
const MTDS: [(u8, &str, u64); 7] = [
    (0, "uboot", 0x0010_0000),
    (1, "kernel", 0x0040_0000),
    (2, "rootfs", 0x0100_0000),
    (3, "kernel2", 0x0040_0000),
    (4, "rootfs2", 0x0100_0000),
    (5, "ota", 0x0010_0000),
    (6, "userdata", 0x0530_0000),
];

#[derive(Debug, PartialEq, Eq)]
pub struct Version {
    pub model: String,
    pub platform: Option<String>,
    pub custom: Option<String>,
    pub system_version: Option<String>,
}

/// The value of `key` in whatever `getversion.cgi` answers with: JSON, `key=value` or `key: value`.
fn value_of(body: &str, key: &str) -> Option<String> {
    let rest = &body[body.find(key)? + key.len()..];
    let rest = rest.trim_start_matches(['"', '\'']).trim_start();
    let rest = rest.strip_prefix([':', '=']).unwrap_or(rest).trim_start();
    let rest = rest.strip_prefix(['"', '\'']).unwrap_or(rest);
    let end = rest.find(['"', '\'', ',', '\n', '\r', '}', '<']).unwrap_or(rest.len());
    let value = rest[..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

pub fn parse_version(body: &str) -> Option<Version> {
    Some(Version {
        model: value_of(body, "product_Model")?,
        platform: value_of(body, "platform"),
        custom: value_of(body, "custom"),
        system_version: value_of(body, "system_version"),
    })
}

fn url(host: &str, path: &str) -> String {
    format!("http://{host}/cgi-bin/{path}")
}

pub fn version(host: &str) -> Result<Version, String> {
    let mut resp = ureq::get(&url(host, "getversion.cgi"))
        .config()
        .timeout_global(Some(Duration::from_secs(3)))
        .build()
        .call()
        .map_err(|e| format!("getversion.cgi: {e}"))?;
    let body = resp.body_mut().read_to_string().map_err(|e| format!("getversion.cgi: {e}"))?;
    parse_version(&body).ok_or_else(|| "getversion.cgi has no product_Model".into())
}

/// The dongle this module is for, or none.
pub fn detect(host: &str) -> Option<Version> {
    version(host).ok().filter(|v| v.model == MODEL)
}

pub const UPDATE_URL: &str = "http://120.79.59.57:8080/device-web/upgrade/downLoad";
const MAX_DOWNLOAD: u64 = 64 * 1024 * 1024;

/// The body the dongle's own updater sends. The version is `ax_` and the digits of the system
/// version, and a `C` on that version goes onto `custom`. Both rules are read off one dongle.
pub fn update_request(v: &Version) -> serde_json::Value {
    let digits: String = v.system_version.as_deref().unwrap_or("").chars().filter(char::is_ascii_digit).collect();
    let marked = v.system_version.as_deref().is_some_and(|s| s.ends_with('C'));
    let custom = v.custom.as_deref().unwrap_or("yunlian");
    serde_json::json!({
        "version": format!("ax_{digits}"),
        "platform": v.platform.as_deref().unwrap_or("AX1800"),
        "custom": if marked { format!("{custom}C") } else { custom.to_string() },
    })
}

/// The vendor's current OTA for this dongle, checked like any image before it is returned. The
/// server sends the image itself, unmasked and unsigned.
pub fn fetch(v: &Version) -> Result<Vec<u8>, String> {
    let mut resp = ureq::post(UPDATE_URL)
        .config()
        .timeout_global(Some(Duration::from_secs(300)))
        .build()
        .send_json(update_request(v))
        .map_err(|e| format!("vendor update server: {e}"))?;
    let body = resp
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD)
        .read_to_vec()
        .map_err(|e| format!("vendor update download: {e}"))?;
    image::parse(&body).map_err(|e| format!("the server did not send an OTA image: {e}"))?;
    Ok(body)
}

pub fn is_x1600(proc_mtd: &str, cpuinfo: &str) -> bool {
    cpuinfo.contains("XBurst") && MTDS.iter().all(|&(i, name, size)| mtd_is(proc_mtd, i, name, size))
}

/// The role is in the part's file name: `<name>.0` first, `<name>.1` between, `<name>:<md5>.2` last.
pub fn part_name(name: &str, index: usize, count: usize, whole_md5: &str) -> String {
    if index == 0 {
        format!("{name}.0")
    } else if index + 1 == count {
        format!("{name}:{whole_md5}.2")
    } else {
        format!("{name}.1")
    }
}

fn post_chunk(host: &str, part: &str, data: &[u8]) -> Result<String, String> {
    let boundary = format!("----livi{}", std::process::id());
    let mut body = Vec::with_capacity(data.len() + 256);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"chunk\"; filename=\"{part}\"\r\n\
             Content-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let mut resp = ureq::post(&url(host, "downfile.cgi"))
        .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
        .header("Referer", format!("http://{host}/"))
        .config()
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .send(body.as_slice())
        .map_err(|e| format!("downfile.cgi: {e}"))?;
    resp.body_mut().read_to_string().map_err(|e| format!("downfile.cgi: {e}"))
}

/// `ax_<14-digit UTC timestamp>`, the shape of every name the vendor's own catalog hands out (see
/// `queryDeviceVersion`'s `fileName`/`version` fields). The apply step on the device appears to
/// key off this pattern — a name with anything else in it (tried `ax_livi<unix time>`) uploaded
/// and reassembled fine but the apply step silently did nothing, no "ota update ok" and no error.
fn staging_name() -> String {
    format!("ax_{}", civil_timestamp(time::OffsetDateTime::now_utc()))
}

/// `YYYYMMDDHHMM`, matching `ax_202510270944` from the catalog.
fn civil_timestamp(t: time::OffsetDateTime) -> String {
    format!("{:04}{:02}{:02}{:02}{:02}", t.year(), u8::from(t.month()), t.day(), t.hour(), t.minute())
}

/// Uploads the image and applies it to the inactive bank. The apply answers when it is finished,
/// and the connection is held until then: letting go early leaves the bank pointer unflipped.
pub fn flash(host: &str, ota: &[u8], report: &dyn Fn(&str)) -> Result<(), String> {
    image::parse(ota)?;
    let seen = version(host)?;
    if seen.model != MODEL {
        return Err(format!("this is {}, not {MODEL}", seen.model));
    }
    let name = staging_name();
    let whole = lfwb_md5(ota);
    let count = ota.len().div_ceil(CHUNK);
    report(&format!("uploading {} B in {count} chunks as {name}", ota.len()));
    for (i, chunk) in ota.chunks(CHUNK).enumerate() {
        let reply = post_chunk(host, &part_name(&name, i, count, &whole), chunk)?;
        if !reply.contains("success") {
            return Err(format!("chunk {i} refused: {}", reply.trim()));
        }
        if i + 1 == count && !reply.contains("file check success") {
            return Err(format!("the dongle's check of the whole file failed: {}", reply.trim()));
        }
        if i % 200 == 0 {
            report(&format!("  {}%", (i + 1) * 100 / count));
        }
    }
    report("writing the inactive bank (erase, write, verify), this takes minutes, do not unplug");
    let mut resp = ureq::get(&format!("{}?filename={name}&", url(host, "submition.cgi")))
        .header("Referer", format!("http://{host}/"))
        .config()
        .timeout_global(Some(Duration::from_secs(1200)))
        .build()
        .call()
        .map_err(|e| format!("submition.cgi: {e}"))?;
    let out = resp.body_mut().read_to_string().map_err(|e| format!("submition.cgi: {e}"))?;
    if out.to_lowercase().contains(OK_MARKER) {
        Ok(())
    } else {
        let tail: String = out.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
        Err(format!("the update did not report \"{OK_MARKER}\", the bank was not switched:\n{tail}"))
    }
}

pub fn reboot(host: &str) {
    // The dongle goes down while it answers.
    let _ = ureq::get(&url(host, "reboot.cgi"))
        .config()
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .call();
}

fn lfwb_md5(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    Md5::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// The bank the `ota` partition names as running: 1 (kernel, rootfs) or 2 (kernel2, rootfs2).
pub fn active_bank(ota_mtd_head: &[u8]) -> u8 {
    let text = String::from_utf8_lossy(ota_mtd_head);
    if text.contains("ota:kernel2") { 2 } else { 1 }
}

/// Pulls one bank (1 or 2) and returns it as an OTA image the dongle itself accepts. Every pull is
/// hashed on the dongle first, so a flaky transfer is caught here, not after it is written back.
fn pull_bank(sh: &mut BindShell, bank: u8) -> Result<Vec<u8>, String> {
    let (kmtd, rmtd) = if bank == 1 { (1, 2) } else { (3, 4) };
    println!("bank {bank}: mtd{kmtd} kernel, mtd{rmtd} rootfs");

    let mut pulled = Vec::new();
    for mtd in [kmtd, rmtd] {
        let size = MTDS[mtd as usize].2;
        let node = format!("/dev/mtdblock{mtd}");
        println!("pulling {node} ({size} B)…");
        let want = sh.run(&format!("md5sum {node}"))?;
        let want = want.split_whitespace().next().unwrap_or("").to_string();
        let data = sh.stream_out(&format!("dd if={node} bs=64k 2>/dev/null; sleep 1"), size)?;
        if lfwb_md5(&data) != want {
            return Err(format!("{node} arrived with another md5 than the dongle read, try again"));
        }
        pulled.push(data);
    }
    let kernel = &pulled[0][..image::uimage_len(&pulled[0])?];
    let rootfs = &pulled[1][..image::squashfs_len(&pulled[1])?];
    let ota = image::build(kernel, rootfs);
    image::parse(&ota)?;
    Ok(ota)
}

fn write_backup(ota: &[u8], out_dir: &Path, prefix: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("mkdir {out_dir:?}: {e}"))?;
    let path = out_dir.join(format!("{prefix}_{}.ota.img", lfwb::stamp()));
    std::fs::write(&path, ota).map_err(|e| format!("write {path:?}: {e}"))?;
    println!("wrote {} ({} B, md5 {})", path.display(), ota.len(), lfwb_md5(ota));
    Ok(path)
}

fn check_dongle(sh: &mut BindShell) -> Result<(), String> {
    let proc_mtd = sh.run("cat /proc/mtd")?;
    let cpuinfo = sh.run("head -20 /proc/cpuinfo")?;
    if !is_x1600(&proc_mtd, &cpuinfo) {
        return Err("this is not an X1600 dongle with the stock partition table".into());
    }
    Ok(())
}

/// Pulls the running bank off a dongle with a shell on 2323 and saves it as an OTA image the
/// dongle itself accepts, which is the way back to stock — or, if the running bank is one `flash`
/// or `install-shell` just wrote, a reusable copy of exactly what is now on the device.
pub fn backup_stock(sh: &mut BindShell, out_dir: &Path) -> Result<PathBuf, String> {
    check_dongle(sh)?;
    let head = sh.stream_out("dd if=/dev/mtd5 bs=4096 count=1 2>/dev/null", 4096)?;
    let bank = active_bank(&head);
    println!("running bank is {bank}");
    write_backup(&pull_bank(sh, bank)?, out_dir, "x1600_stock")
}

/// Pulls a specific bank (1 or 2) regardless of which one is running — e.g. to see what is in the
/// bank you are *not* currently booted into before you overwrite it.
pub fn backup_bank(sh: &mut BindShell, bank: u8, out_dir: &Path) -> Result<PathBuf, String> {
    if bank != 1 && bank != 2 {
        return Err(format!("bank must be 1 or 2, not {bank}"));
    }
    check_dongle(sh)?;
    write_backup(&pull_bank(sh, bank)?, out_dir, &format!("x1600_bank{bank}"))
}

pub fn default_host() -> &'static str {
    DONGLE_HOST
}

#[cfg(test)]
mod tests {
    use super::*;

    const MTD: &str = "dev:    size   erasesize  name
mtd0: 00100000 00020000 \"uboot\"
mtd1: 00400000 00020000 \"kernel\"
mtd2: 01000000 00020000 \"rootfs\"
mtd3: 00400000 00020000 \"kernel2\"
mtd4: 01000000 00020000 \"rootfs2\"
mtd5: 00100000 00020000 \"ota\"
mtd6: 05300000 00020000 \"userdata\"
";

    #[test]
    fn the_civil_timestamp_matches_a_known_unix_time() {
        // 2025-10-27 09:44 UTC, same shape as the catalog's real "ax_202510270944".
        let t = time::OffsetDateTime::from_unix_timestamp(1_761_558_240).unwrap();
        assert_eq!(civil_timestamp(t), "202510270944");
        assert_eq!(civil_timestamp(time::OffsetDateTime::UNIX_EPOCH), "197001010000");
    }

    #[test]
    fn the_version_is_read_from_json_or_key_value() {
        let json = r#"{"product_Model":"v851se-yunlian-cp","platform":"AX1800","custom":"yunlian","system_version":"20251027xxxxC"}"#;
        let v = parse_version(json).unwrap();
        assert_eq!(v.model, MODEL);
        assert_eq!(v.platform.as_deref(), Some("AX1800"));
        assert_eq!(v.custom.as_deref(), Some("yunlian"));
        let kv = parse_version("product_Model=v851se-yunlian-cp\nplatform=AX1800\n").unwrap();
        assert_eq!(kv.model, MODEL);
        assert!(parse_version("<html>404</html>").is_none());
    }

    #[test]
    fn the_update_request_follows_the_dongles_own() {
        let v = Version {
            model: MODEL.into(),
            platform: Some("AX1800".into()),
            custom: Some("yunlian".into()),
            system_version: Some("202510270944C".into()),
        };
        assert_eq!(
            update_request(&v),
            serde_json::json!({"version":"ax_202510270944","platform":"AX1800","custom":"yunlianC"})
        );
    }

    #[test]
    fn the_stock_table_and_cpu_pass_and_others_do_not() {
        assert!(is_x1600(MTD, "system type\t: XBurst-Based\n"));
        assert!(!is_x1600(MTD, "CPU part\t: 0xc07\n"));
        assert!(!is_x1600(&MTD.replace("01000000", "00800000"), "XBurst"));
    }

    #[test]
    fn the_chunk_role_is_in_the_name() {
        assert_eq!(part_name("n", 0, 3, "m"), "n.0");
        assert_eq!(part_name("n", 1, 3, "m"), "n.1");
        assert_eq!(part_name("n", 2, 3, "m"), "n:m.2");
        assert_eq!(part_name("n", 1, 2, "m"), "n:m.2");
    }

    #[test]
    fn the_running_bank_is_named_by_the_ota_partition() {
        assert_eq!(active_bank(b"ota:kernel\0\0"), 1);
        assert_eq!(active_bank(b"ota:kernel2\0"), 2);
        assert_eq!(active_bank(&[0xff; 64]), 1);
    }
}
