// Ported from bin/livi-ledd/src/main.rs — see livid dispatcher in main.rs.
pub fn run(_args: Vec<String>) -> i32 {
    match livid_main() {
        Ok(()) => 0,
        Err(e) => { eprintln!("[livi-ledd] {e}"); 1 }
    }
}

// livi-ledd — RGB LED driver for the LIVI-Link dongles.
//
// Drives WS2812-style chips via /dev/spidev1.0 (4-bit-per-bit encoding at
// ~3.1 MHz). One pixel on V821B, a chain of three on AX520 (the count comes from the
// board's device tree). State inputs are file existence under /tmp/livi/led/. Config
// (WLAN color + brightness) lives in /etc/livi/led.toml.
//
// Wifi and bluetooth share the one pixel the way two LEDs would, their colours added:
//   wifi client     → wlan-color solid   (a station is associated to the AP)
//   waiting         → wlan-color blinking (no client on the AP yet)
//   bt-connected    → blue solid, on top of the wifi state
//   bt-paging       → pulsing blue, over the wifi state
// Ahead of both:
//   flash-mode      → red+blue alternating (~2 Hz)
//   iap2-active     → off
//
// Brightness = 0 turns the LED off entirely (no separate toggle needed).

use std::fs;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const SPI_DEV: &str = "/dev/spidev1.0";
// Chain length, a big-endian u32 on the spidev node. Boards without the property have one LED.
const LED_COUNT_PROP: &str = "/sys/bus/spi/devices/spi1.0/of_node/livi,led-count";
// /etc/ is on read-only squashfs; the runtime config lives on tmpfs.
// rcS seeds it from /etc/livi/led.toml at boot; changes made via the web
// UI are lost on reboot until we add a writable partition.
const CONFIG_PATH: &str = "/tmp/livi/led.toml";
const PID_PATH: &str = "/tmp/livi/livi-ledd.pid";
const STATE_DIR: &str = "/tmp/livi/led";

// WS2812 timing: bit-0 = 0b1000, bit-1 = 0b1110 → 4 SPI bits per 1 LED bit.
// At 3.2 MHz SPI clock: each SPI bit = 312 ns → 4 bits = 1.25 µs (WS2812 spec).
//   bit-0: T0H = 312 ns  (spec 220-380 ns)  T0L = 937 ns  (spec 580-1000 ns)
//   bit-1: T1H = 937 ns  (spec 580-1000 ns) T1L = 312 ns  (spec 220-420 ns)
// The earlier 3-bit / 2.4 MHz variant had T0H = 417 ns which some WS2812
// clones latch as bit-1 → the G channel stayed permanently on because every
// "0" bit in the G byte was read as "1". GRB byte order (WS2812 native).
const SPI_HZ: u32 = 3_200_000;
const TICK_HZ: u64 = 50;
const TICK: Duration = Duration::from_millis(1000 / TICK_HZ);

// SPI ioctls (Linux spi/spidev.h) — reproduced here to avoid pulling in a crate.
const SPI_IOC_MAGIC: u8 = b'k';
const SPI_IOC_WR_MODE: u32 = _iow(SPI_IOC_MAGIC, 1, 1);
const SPI_IOC_WR_BITS_PER_WORD: u32 = _iow(SPI_IOC_MAGIC, 3, 1);
const SPI_IOC_WR_MAX_SPEED_HZ: u32 = _iow(SPI_IOC_MAGIC, 4, 4);

const fn _iow(magic: u8, nr: u8, size: u32) -> u32 {
    (1u32 << 30) | ((size & 0x3fff) << 16) | ((magic as u32) << 8) | (nr as u32)
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Config {
    status: Rgb,
    brightness_pct: u8, // 0-100 %; 0 = LED completely off
    // Per-channel gain (0-255) applied on top of brightness. The WS2812's blue and green
    // dies are perceptually brighter than red at equal PWM, so full-scale blue swamps a
    // dimmed red status. This tones each channel so equal values read as neutral white.
    wb: Rgb,
}

impl Default for Config {
    fn default() -> Self {
        // Matches the "● online" accent (--acc: #4dd0e1) in the web UI.
        Self {
            status: Rgb(0x4d, 0xd0, 0xe1),
            brightness_pct: 20,
            wb: Rgb(255, 190, 130),
        }
    }
}

impl Config {
    fn load() -> Self {
        let Ok(s) = fs::read_to_string(CONFIG_PATH) else { return Self::default(); };
        let mut cfg = Self::default();
        for line in s.lines() {
            let line = line.trim();
            // Only whole-line comments — a `#` mid-value belongs to the value
            // (e.g. status_color = "#00ff00" — else we'd chop the colour).
            if line.is_empty() || line.starts_with('#') { continue; }
            let Some((k, v)) = line.split_once('=') else { continue; };
            let k = k.trim();
            let v = v.trim().trim_matches('"');
            match k {
                "status_color" => {
                    if let Some(rgb) = parse_rgb(v) { cfg.status = rgb; }
                }
                "brightness" => {
                    if let Ok(n) = v.parse::<u8>() { cfg.brightness_pct = n.min(100); }
                }
                "white_balance" => {
                    if let Some(rgb) = parse_rgb(v) { cfg.wb = rgb; }
                }
                _ => {}
            }
        }
        cfg
    }
}

fn parse_rgb(s: &str) -> Option<Rgb> {
    // Accept "r,g,b" or "#rrggbb"
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() != 6 { return None; }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        return Some(Rgb(r, g, b));
    }
    let parts: Vec<_> = s.split(',').map(|p| p.trim()).collect();
    if parts.len() != 3 { return None; }
    Some(Rgb(parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?))
}

// ---------------------------------------------------------------------------
// State (from /tmp/livi/led/*)
// ---------------------------------------------------------------------------

#[derive(Default, Clone, Copy)]
struct State {
    client: bool,
    bt_paging: bool,
    bt_connected: bool,
    iap2_active: bool,
    flash_mode: bool,
    // `touch /tmp/livi/led/wbtest` forces full white so all three channels light at once —
    // the only way to eyeball the white balance. `rm` it to return to normal.
    wbtest: bool,
}

impl State {
    fn read() -> Self {
        Self {
            client:       wifi_client(),
            bt_paging:    exists("bt-paging"),
            bt_connected: exists("bt-connected"),
            iap2_active:  exists("iap2-active"),
            flash_mode:   exists("flash-mode"),
            wbtest:       exists("wbtest"),
        }
    }
}

fn exists(name: &str) -> bool {
    Path::new(&format!("{STATE_DIR}/{name}")).exists()
}

/// True while a wifi station is associated to the access point, read from the bridge forwarding
/// table (wlan0's stations show up behind its br0 port). Same source as the web UI's client
/// count, so the LED and the page agree; a cheap /sys read, no netlink.
fn wifi_client() -> bool {
    let Ok(port) = fs::read_to_string("/sys/class/net/wlan0/brport/port_no") else {
        return false;
    };
    let Ok(port_no) = u8::from_str_radix(port.trim().trim_start_matches("0x"), 16) else {
        return false;
    };
    let Ok(fdb) = fs::read("/sys/class/net/br0/brforward") else {
        return false;
    };
    // 16-byte entries: mac[6], port_no @6, is_local @7. A non-local mac on wlan0's port = a station.
    fdb.as_chunks::<16>()
        .0
        .iter()
        .any(|e| e[6] == port_no && e[7] == 0)
}

// ---------------------------------------------------------------------------
// Render — decide pixel color for a given tick
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Rgb(u8, u8, u8);
const OFF: Rgb = Rgb(0, 0, 0);
const BLUE: Rgb = Rgb(0, 0, 255);

const WHITE: Rgb = Rgb(255, 255, 255);

fn render(state: &State, cfg: &Config, tick: u64) -> Rgb {
    if cfg.brightness_pct == 0 { return OFF; }

    // WB test: full white, so brightness + white-balance are the only things shaping it.
    if state.wbtest {
        let gain = ((cfg.brightness_pct as u16 * 255) / 100) as u8;
        return balance(scale(WHITE, gain), cfg.wb);
    }

    // 2 Hz blink for red/blue flash-mode and for wlan-not-up.
    let slow_on = (tick / (TICK_HZ / 4)).is_multiple_of(2); // TICK_HZ/4 = 12 → ~2 Hz
    // Fast blitz (bt-paging): 5 Hz, on ~80 ms so the blue pulse reads as a blink, not a flicker.
    let blitz_on = (tick % (TICK_HZ / 5)) < (TICK_HZ / 12);

    // Steady once a station is on the AP, blinking while it waits for one.
    let wifi = if state.client || slow_on { cfg.status } else { OFF };

    let base = if state.flash_mode {
        // Alternating red / blue every ~250 ms
        if slow_on { Rgb(255, 0, 0) } else { BLUE }
    } else if state.iap2_active {
        OFF
    } else if state.bt_connected {
        add(wifi, BLUE)
    } else if state.bt_paging {
        // The pulse replaces the colour rather than adding to it: a status colour with
        // blue in it would swallow an added pulse.
        if blitz_on { BLUE } else { wifi }
    } else {
        wifi
    };

    // Convert 0-100 % to a u8 gain factor (0-255) for scale().
    let gain = ((cfg.brightness_pct as u16 * 255) / 100) as u8;
    balance(scale(base, gain), cfg.wb)
}

fn add(a: Rgb, b: Rgb) -> Rgb {
    Rgb(a.0.saturating_add(b.0), a.1.saturating_add(b.1), a.2.saturating_add(b.2))
}

/// Per-channel white-balance: multiply each channel by its gain/255.
fn balance(c: Rgb, wb: Rgb) -> Rgb {
    Rgb(
        ((c.0 as u16 * wb.0 as u16) / 255) as u8,
        ((c.1 as u16 * wb.1 as u16) / 255) as u8,
        ((c.2 as u16 * wb.2 as u16) / 255) as u8,
    )
}

fn scale(c: Rgb, brightness: u8) -> Rgb {
    let s = brightness as u16;
    Rgb(
        ((c.0 as u16 * s) / 255) as u8,
        ((c.1 as u16 * s) / 255) as u8,
        ((c.2 as u16 * s) / 255) as u8,
    )
}

// ---------------------------------------------------------------------------
// WS2812 SPI encoder
// ---------------------------------------------------------------------------

/// Encode one byte into 4 SPI bytes (32 SPI bits) with bit-0=1000, bit-1=1110.
fn encode_byte(b: u8, out: &mut [u8; 4]) {
    // Pack 32 output bits MSB-first across four bytes.
    let mut acc: u32 = 0;
    for i in (0..8).rev() {
        let bit = (b >> i) & 1;
        let pat = if bit == 1 { 0b1110u32 } else { 0b1000u32 };
        acc = (acc << 4) | pat;
    }
    out[0] = ((acc >> 24) & 0xff) as u8;
    out[1] = ((acc >> 16) & 0xff) as u8;
    out[2] = ((acc >>  8) & 0xff) as u8;
    out[3] = ( acc        & 0xff) as u8;
}

/// Encode one pixel (GRB) into 12 SPI bytes. WS2812B native byte order.
fn encode_pixel(c: Rgb, out: &mut [u8; 12]) {
    let mut tmp = [0u8; 4];
    encode_byte(c.1, &mut tmp); out[0..4].copy_from_slice(&tmp);  // G
    encode_byte(c.0, &mut tmp); out[4..8].copy_from_slice(&tmp);  // R
    encode_byte(c.2, &mut tmp); out[8..12].copy_from_slice(&tmp); // B
}

fn led_count() -> usize {
    fs::read(LED_COUNT_PROP)
        .ok()
        .and_then(|b| <[u8; 4]>::try_from(b.as_slice()).ok())
        .map_or(1, |b| u32::from_be_bytes(b).clamp(1, 16) as usize)
}

struct Spi {
    file: fs::File,
    leds: usize,
}

impl Spi {
    fn open() -> std::io::Result<Self> {
        let file = fs::OpenOptions::new().read(true).write(true).open(SPI_DEV)?;
        let fd = file.as_raw_fd();
        // Mode 0, 8 bits/word, 2.4 MHz.
        unsafe {
            let mode: u8 = 0;
            check(libc::ioctl(fd, SPI_IOC_WR_MODE as _, &mode as *const u8))?;
            let bpw: u8 = 8;
            check(libc::ioctl(fd, SPI_IOC_WR_BITS_PER_WORD as _, &bpw as *const u8))?;
            let hz: u32 = SPI_HZ;
            check(libc::ioctl(fd, SPI_IOC_WR_MAX_SPEED_HZ as _, &hz as *const u32))?;
        }
        Ok(Self { file, leds: led_count() })
    }

    fn write_pixel(&mut self, c: Rgb) -> std::io::Result<()> {
        // [25 leading zeros | 12 data per LED, all the same colour | 25 trailing zeros]
        // 25 bytes at 3.2 MHz = ~62 µs low — more than the WS2812 reset
        // threshold (≥50 µs). The LEADING gap forces the chip into a
        // clean reset-done state right before our data (killing any
        // stale mid-frame or MOSI-pullup high tail that would otherwise
        // stretch the first bit's high pulse and light G at 128); the
        // TRAILING gap latches the pixel and survives whatever the SPI
        // hardware does with MOSI while CS is deasserted.
        let mut buf = vec![0u8; 25 + 12 * self.leds + 25];
        let mut px = [0u8; 12];
        encode_pixel(c, &mut px);
        for led in buf[25..25 + 12 * self.leds].as_chunks_mut::<12>().0 {
            *led = px;
        }
        self.file.write_all(&buf)?;
        Ok(())
    }
}

fn check(rc: libc::c_int) -> std::io::Result<()> {
    if rc < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

static RELOAD: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sighup(_: libc::c_int) {
    RELOAD.store(true, Ordering::SeqCst);
}

fn livid_main() -> std::io::Result<()> {
    // Ensure state dir exists (writers may not have created it).
    let _ = fs::create_dir_all(STATE_DIR);

    // Write PID file for SIGHUP-based reload from livi-httpd.
    let _ = fs::write(PID_PATH, format!("{}\n", std::process::id()));

    unsafe {
        libc::signal(libc::SIGHUP,  on_sighup as *const () as libc::sighandler_t);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    let mut spi = match Spi::open() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[livi-ledd] open {SPI_DEV}: {e}");
            return Err(e);
        }
    };

    let mut cfg = Config::load();
    let mut cfg_mtime = mtime(CONFIG_PATH);
    let mut tick: u64 = 0;

    loop {
        if RELOAD.swap(false, Ordering::SeqCst) {
            cfg = Config::load();
        }
        let m = mtime(CONFIG_PATH);
        if m != cfg_mtime {
            cfg_mtime = m;
            cfg = Config::load();
        }

        let state = State::read();
        let want = render(&state, &cfg, tick);

        // Rewrite every tick, even if the colour is unchanged: this
        // continuously re-affirms the pixel state so a single missed
        // latch or a stray transient cannot leave the LED stuck in a
        // stale colour.
        let _ = spi.write_pixel(want);

        let start = Instant::now();
        thread::sleep(TICK.saturating_sub(start.elapsed()));
        tick = tick.wrapping_add(1);
    }
}

fn mtime(path: &str) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

