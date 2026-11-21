// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fs;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use crate::http::{S_200, S_400, S_500, T_JSON, err_json, js, ok_json};
use crate::{Flash, HandOver, MtdSlot, WebCaps};

const HAND_BACK_AFTER: Duration = Duration::from_secs(180);

static TAKEN: AtomicU64 = AtomicU64::new(0);
static FLASHING: AtomicBool = AtomicBool::new(false);
static REBOOTING: AtomicBool = AtomicBool::new(false);

pub(crate) fn take_over(f: &Flash) -> (&'static str, &'static str, Vec<u8>) {
    let Some(hand) = f.hand_over else {
        return (S_200, T_JSON, ok_json("nothing to free"));
    };
    if let Err(e) = (hand.take)() {
        return (S_500, T_JSON, err_json(&e));
    }
    let taken = TAKEN.fetch_add(1, Ordering::SeqCst) + 1;
    thread::spawn(move || {
        thread::sleep(HAND_BACK_AFTER);
        if TAKEN.load(Ordering::SeqCst) == taken {
            give_back(hand);
        }
    });
    (S_200, T_JSON, ok_json("freed for the upload"))
}

pub(crate) fn hand_back(f: &Flash) -> (&'static str, &'static str, Vec<u8>) {
    if let Some(hand) = f.hand_over {
        give_back(hand);
    }
    (S_200, T_JSON, ok_json("given back"))
}

fn give_back(hand: HandOver) {
    if FLASHING.load(Ordering::SeqCst) || REBOOTING.load(Ordering::SeqCst) {
        return;
    }
    TAKEN.fetch_add(1, Ordering::SeqCst);
    (hand.give_back)();
}

pub(crate) fn flash_dispatch(
    c: &WebCaps,
    body: Option<&[u8]>,
    clen: u64,
) -> (&'static str, &'static str, Vec<u8>) {
    FLASHING.store(true, Ordering::SeqCst);
    let out = flash_body(c, body, clen);
    FLASHING.store(false, Ordering::SeqCst);
    if let Some(hand) = c.flash.hand_over {
        give_back(hand);
    }
    out
}

fn flash_body(
    c: &WebCaps,
    body: Option<&[u8]>,
    clen: u64,
) -> (&'static str, &'static str, Vec<u8>) {
    let Some(data) = body else {
        return (S_400, T_JSON, err_json("empty body"));
    };
    if data.starts_with(livi_base::lfwb::MAGIC) {
        return flash_bundle(&c.flash, body, clen);
    }
    (
        S_400,
        T_JSON,
        err_json("not a LIVI Link firmware bundle (.lfwb)"),
    )
}

pub(crate) fn flash_bundle(
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
    let images = match livi_base::lfwb::unpack(data) {
        Ok(images) => images,
        Err(e) => return (S_400, T_JSON, err_json(&e)),
    };
    if images.is_empty() || images.len() > 8 {
        return (
            S_400,
            T_JSON,
            err_json(&format!("bundle image count {} out of range", images.len())),
        );
    }

    let mut staged: Vec<(&MtdSlot, Option<Vec<u8>>)> = Vec::with_capacity(images.len());
    for &(typ, slice) in &images {
        let Some(slot) = f.mtd.iter().find(|s| s.typ == typ) else {
            return (
                S_400,
                T_JSON,
                err_json(&format!(
                    "image type {} is not for this dongle, nothing written",
                    typ
                )),
            );
        };
        if slice.len() as u64 > slot.size {
            return (
                S_400,
                T_JSON,
                err_json(&format!(
                    "image type {} ({}) is {} B, exceeds slot {} B, nothing written",
                    typ,
                    slot.node,
                    slice.len(),
                    slot.size
                )),
            );
        }
        if !slice.starts_with(slot.magic.as_slice()) {
            return (
                S_400,
                T_JSON,
                err_json(&format!(
                    "image type {} payload magic mismatch for {}, nothing written",
                    typ, slot.node
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
                        typ, slot.node
                    )),
                );
            }
        }
    }

    led_flash_running();

    let mut wrote_any = false;
    let mut fed: Option<Fed> = None;
    let mut report: Vec<String> = Vec::new();
    for (&(_, slice), (slot, staged)) in images.iter().zip(&staged) {
        let node = slot.node.as_str();
        let image = staged.as_deref().unwrap_or(slice);
        let len = image.len();
        let path = format!("/dev/{node}");
        let raw = raw_mtd(node);
        write_progress(node, 0, len, "compare");
        let same = flash_matches(&raw, image).unwrap_or(false);
        if same {
            report.push(format!("{} unchanged", node));
            write_progress(node, len, len, "unchanged");
            continue;
        }

        fed.get_or_insert_with(Fed::start);
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
        return (
            S_500,
            T_JSON,
            err_json(&format!("{}, but {e}", report.join(", "))),
        );
    }
    write_progress("bundle", data.len(), data.len(), "done");
    if wrote_any {
        led_flash_done();
        reboot_after(Duration::from_millis(500), fed.and_then(Fed::into_reboot));
        (
            S_200,
            T_JSON,
            ok_json(&format!("{}, rebooting", report.join(", "))),
        )
    } else {
        led_flash_clear();
        (S_200, T_JSON, ok_json("Up-to-date"))
    }
}

pub(crate) fn post_write_check(f: &Flash) -> Result<(), String> {
    let Some(cmd) = &f.check else { return Ok(()) };
    match Command::new("sh").arg("-c").arg(cmd).status() {
        Ok(st) if st.success() => Ok(()),
        _ => {
            led_flash_error();
            Err(format!("{cmd} failed after the write, not rebooting"))
        }
    }
}

pub(crate) const LED_DIR: &str = "/tmp/livi/led";

pub(crate) fn led_flash_set(state: Option<&str>) {
    let _ = fs::create_dir_all(LED_DIR);
    for f in ["flash-mode", "flash-done", "flash-error"] {
        let _ = fs::remove_file(format!("{LED_DIR}/{f}"));
    }
    if let Some(name) = state {
        let _ = fs::write(format!("{LED_DIR}/{name}"), b"");
    }
}

pub(crate) fn led_flash_running() {
    led_flash_set(Some("flash-mode"))
}

pub(crate) fn led_flash_done() {
    led_flash_set(Some("flash-done"))
}

pub(crate) fn led_flash_error() {
    led_flash_set(Some("flash-error"))
}

pub(crate) fn led_flash_clear() {
    led_flash_set(None)
}

pub(crate) fn raw_mtd(node: &str) -> String {
    match node.strip_prefix("mtdblock") {
        Some(n) => format!("/dev/mtd{n}"),
        None => format!("/dev/{node}"),
    }
}

pub(crate) fn flash_matches(path: &str, expected: &[u8]) -> std::io::Result<bool> {
    Ok(first_difference(path, expected)?.is_none())
}

fn first_difference(path: &str, expected: &[u8]) -> std::io::Result<Option<usize>> {
    let mut f = fs::File::open(path)?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut at = 0;
    for want in expected.chunks(buf.len()) {
        let got = &mut buf[..want.len()];
        f.read_exact(got)?;
        if let Some(i) = got.iter().zip(want).position(|(a, b)| a != b) {
            return Ok(Some(at + i));
        }
        at += want.len();
    }
    Ok(None)
}

pub(crate) fn write_chunked(path: &str, data: &[u8], node: &str) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_SYNC)
        .open(path)?;
    let chunk = 64 * 1024;
    let mut written = 0usize;
    for slice in data.chunks(chunk) {
        f.write_all(slice)?;
        written += slice.len();
        write_progress(node, written, data.len(), "write");
    }
    f.sync_all()?;
    // SAFETY: sync has no preconditions.
    unsafe {
        libc::sync();
    }
    Ok(())
}

pub(crate) fn write_progress(node: &str, written: usize, total: usize, phase: &str) {
    let _ = fs::create_dir_all("/tmp/livi");
    let _ = fs::write(
        "/tmp/livi/flash-progress",
        format!("{node}:{written}:{total}:{phase}\n"),
    );
}

pub(crate) fn flash_status_json() -> String {
    let raw = fs::read_to_string("/tmp/livi/flash-progress").unwrap_or_default();
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

pub(crate) fn verify_flash(path: &str, expected: &[u8]) -> std::io::Result<()> {
    match first_difference(path, expected)? {
        None => Ok(()),
        Some(diff_at) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("read-back mismatch at byte {diff_at}"),
        )),
    }
}

pub(crate) fn reboot_soon() -> (&'static str, &'static str, Vec<u8>) {
    reboot_after(Duration::from_millis(500), None);
    (S_200, T_JSON, ok_json("rebooting"))
}

fn arm_watchdog() -> Option<fs::File> {
    const WDIOC_SETTIMEOUT: u32 = 0xc004_5706;
    const TIMEOUT_S: libc::c_int = 10;
    let dog = fs::OpenOptions::new()
        .write(true)
        .open("/dev/watchdog")
        .ok()?;
    let mut secs = TIMEOUT_S;
    // SAFETY: the ioctl reads and writes one int, secs outlives the call.
    unsafe { libc::ioctl(dog.as_raw_fd(), WDIOC_SETTIMEOUT as _, &mut secs) };
    Some(dog)
}

/// Fed from its own thread while a flash writes: the process runs from the rootfs it overwrites
/// and can die on any page it reads back, the unfed watchdog then resets the board.
struct Fed {
    dog: Option<Arc<fs::File>>,
    stop: Arc<AtomicBool>,
    feeder: Option<thread::JoinHandle<()>>,
}

impl Fed {
    fn start() -> Self {
        let dog = arm_watchdog().map(Arc::new);
        let stop = Arc::new(AtomicBool::new(false));
        let feeder = dog.clone().map(|dog| {
            let stop = stop.clone();
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let _ = (&*dog).write_all(b"k");
                    thread::sleep(Duration::from_secs(2));
                }
            })
        });
        Self { dog, stop, feeder }
    }

    fn unfed(&mut self) -> Option<fs::File> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(feeder) = self.feeder.take() {
            let _ = feeder.join();
        }
        self.dog.take().and_then(|dog| Arc::try_unwrap(dog).ok())
    }

    fn into_reboot(mut self) -> Option<fs::File> {
        self.unfed()
    }
}

impl Drop for Fed {
    fn drop(&mut self) {
        // A flash that ends without a reboot leaves the board running: the magic close stops it.
        if let Some(mut dog) = self.unfed() {
            let _ = dog.write_all(b"V");
        }
    }
}

pub(crate) fn reboot_after(delay: Duration, dog: Option<fs::File>) {
    REBOOTING.store(true, Ordering::SeqCst);
    thread::spawn(move || {
        let dog = dog.or_else(arm_watchdog);
        thread::sleep(delay);
        // SAFETY: sync has no preconditions.
        unsafe {
            libc::sync();
        }
        // SAFETY: reboot takes a plain command value and touches no memory of ours.
        unsafe {
            libc::reboot(libc::LINUX_REBOOT_CMD_RESTART);
        }
        let _ = Command::new("/sbin/reboot").arg("-f").status();
        // Kept open: depending on the driver, closing stops the watchdog.
        std::mem::forget(dog);
    });
}
