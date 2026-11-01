// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub fn run(_args: Vec<String>) -> i32 {
    match livid_main() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("[livi-ledd] {e}");
            1
        }
    }
}

use std::fs;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use livi_wifid::hostapd;
use livi_wifid::radio::{self, Radio};

const SPI_DEV: &str = "/dev/spidev1.0";
const HOSTAPD_RETRY: Duration = Duration::from_secs(2);
const LEDS_DIR: &str = "/sys/class/leds";
const LED_COUNT_PROP: &str = "/sys/bus/spi/devices/spi1.0/of_node/livi,led-count";
const CONFIG_PATH: &str = "/tmp/livi/led.toml";
const PID_PATH: &str = "/tmp/livi/livi-ledd.pid";
const STATE_DIR: &str = "/tmp/livi/led";

const SPI_HZ: u32 = 3_200_000;
const TICK_HZ: u64 = 50;
const TICK: Duration = Duration::from_millis(1000 / TICK_HZ);

const SPI_IOC_MAGIC: u8 = b'k';
const SPI_IOC_WR_MODE: u32 = _iow(SPI_IOC_MAGIC, 1, 1);
const SPI_IOC_WR_BITS_PER_WORD: u32 = _iow(SPI_IOC_MAGIC, 3, 1);
const SPI_IOC_WR_MAX_SPEED_HZ: u32 = _iow(SPI_IOC_MAGIC, 4, 4);

const fn _iow(magic: u8, nr: u8, size: u32) -> u32 {
    (1u32 << 30) | ((size & 0x3fff) << 16) | ((magic as u32) << 8) | (nr as u32)
}

#[derive(Clone, Copy)]
struct Config {
    status: Rgb,
    brightness_pct: u8,
    wb: Rgb,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            status: Rgb(0x4d, 0xd0, 0xe1),
            brightness_pct: 20,
            wb: Rgb(255, 190, 130),
        }
    }
}

impl Config {
    fn load() -> Self {
        let Ok(s) = fs::read_to_string(CONFIG_PATH) else {
            return Self::default();
        };
        let mut cfg = Self::default();
        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let k = k.trim();
            let v = v.trim().trim_matches('"');
            match k {
                "status_color" => {
                    if let Some(rgb) = parse_rgb(v) {
                        cfg.status = rgb;
                    }
                }
                "brightness" => {
                    if let Ok(n) = v.parse::<u8>() {
                        cfg.brightness_pct = n.min(100);
                    }
                }
                "white_balance" => {
                    if let Some(rgb) = parse_rgb(v) {
                        cfg.wb = rgb;
                    }
                }
                _ => {}
            }
        }
        cfg
    }
}

fn parse_rgb(s: &str) -> Option<Rgb> {
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        return Some(Rgb(r, g, b));
    }
    let parts: Vec<_> = s.split(',').map(|p| p.trim()).collect();
    if parts.len() != 3 {
        return None;
    }
    Some(Rgb(
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
enum Wifi {
    Off,
    #[default]
    Waiting,
    Client,
}

impl Wifi {
    fn of(switched_on: bool, client: bool) -> Self {
        match (switched_on, client) {
            (false, _) => Self::Off,
            (true, true) => Self::Client,
            (true, false) => Self::Waiting,
        }
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
enum Bt {
    Off,
    #[default]
    Idle,
    Paging,
    Connected,
}

impl Bt {
    fn of(switched_on: bool, connected: bool, paging: bool) -> Self {
        match (switched_on, connected, paging) {
            (false, _, _) => Self::Off,
            (true, true, _) => Self::Connected,
            (true, false, true) => Self::Paging,
            (true, false, false) => Self::Idle,
        }
    }
}

#[derive(Default, Clone, Copy)]
struct State {
    wifi: Wifi,
    bt: Bt,
    flash_mode: bool,
    flash_done: bool,
    flash_error: bool,
    wbtest: bool,
}

impl State {
    fn read(wifi_client: bool) -> Self {
        Self {
            wifi: Wifi::of(radio::enabled(Radio::Wifi), wifi_client),
            bt: Bt::of(
                radio::enabled(Radio::Bt),
                exists("bt-connected"),
                exists("bt-paging"),
            ),
            flash_mode: exists("flash-mode"),
            flash_done: exists("flash-done"),
            flash_error: exists("flash-error"),
            wbtest: exists("wbtest"),
        }
    }
}

fn exists(name: &str) -> bool {
    Path::new(&format!("{STATE_DIR}/{name}")).exists()
}

static WIFI_CLIENT: AtomicBool = AtomicBool::new(false);

fn follow_wifi_clients() {
    let ctrl = hostapd::ctrl("wlan0");
    loop {
        let _ = hostapd::follow_stations(&ctrl, |joined| {
            WIFI_CLIENT.store(!joined.is_empty(), Ordering::Relaxed);
            true
        });
        WIFI_CLIENT.store(false, Ordering::Relaxed);
        thread::sleep(HOSTAPD_RETRY);
    }
}

#[derive(Clone, Copy)]
struct Rgb(u8, u8, u8);
const OFF: Rgb = Rgb(0, 0, 0);
const RED: Rgb = Rgb(255, 0, 0);
const GREEN: Rgb = Rgb(0, 255, 0);
const BLUE: Rgb = Rgb(0, 0, 255);

const WHITE: Rgb = Rgb(255, 255, 255);

fn render(state: &State, cfg: &Config, tick: u64) -> Rgb {
    if cfg.brightness_pct == 0 {
        return OFF;
    }

    if state.wbtest {
        let gain = ((cfg.brightness_pct as u16 * 255) / 100) as u8;
        return balance(scale(WHITE, gain), cfg.wb);
    }

    let (slow_on, blitz_on) = (slow_on(tick), blitz_on(tick));

    let wifi = if wifi_lit(state.wifi, slow_on) {
        cfg.status
    } else {
        OFF
    };

    let base = if state.flash_error {
        RED
    } else if state.flash_mode {
        if slow_on { RED } else { BLUE }
    } else if state.flash_done {
        GREEN
    } else {
        match state.bt {
            Bt::Connected => add(wifi, BLUE),
            Bt::Paging if blitz_on => BLUE,
            Bt::Paging | Bt::Idle | Bt::Off => wifi,
        }
    };

    let gain = ((cfg.brightness_pct as u16 * 255) / 100) as u8;
    balance(scale(base, gain), cfg.wb)
}

fn render_pair(state: &State, cfg: &Config, tick: u64) -> (bool, bool) {
    let slow = slow_on(tick);
    if cfg.brightness_pct == 0 {
        (false, false)
    } else if state.wbtest {
        (true, true)
    } else if state.flash_error {
        (true, false)
    } else if state.flash_mode {
        (slow, !slow)
    } else if state.flash_done {
        (true, true)
    } else {
        let bt = match state.bt {
            Bt::Connected => true,
            Bt::Paging => blitz_on(tick),
            Bt::Idle | Bt::Off => false,
        };
        (wifi_lit(state.wifi, slow), bt)
    }
}

fn wifi_lit(wifi: Wifi, blink_on: bool) -> bool {
    match wifi {
        Wifi::Client => true,
        Wifi::Waiting => blink_on,
        Wifi::Off => false,
    }
}

fn slow_on(tick: u64) -> bool {
    (tick / (TICK_HZ / 4)).is_multiple_of(2)
}

fn blitz_on(tick: u64) -> bool {
    (tick % (TICK_HZ / 5)) < (TICK_HZ / 12)
}

fn add(a: Rgb, b: Rgb) -> Rgb {
    Rgb(
        a.0.saturating_add(b.0),
        a.1.saturating_add(b.1),
        a.2.saturating_add(b.2),
    )
}

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

fn encode_byte(b: u8, out: &mut [u8; 4]) {
    let mut acc: u32 = 0;
    for i in (0..8).rev() {
        let bit = (b >> i) & 1;
        let pat = if bit == 1 { 0b1110u32 } else { 0b1000u32 };
        acc = (acc << 4) | pat;
    }
    out[0] = ((acc >> 24) & 0xff) as u8;
    out[1] = ((acc >> 16) & 0xff) as u8;
    out[2] = ((acc >> 8) & 0xff) as u8;
    out[3] = (acc & 0xff) as u8;
}

fn encode_pixel(c: Rgb, out: &mut [u8; 12]) {
    let mut tmp = [0u8; 4];
    encode_byte(c.1, &mut tmp);
    out[0..4].copy_from_slice(&tmp);
    encode_byte(c.0, &mut tmp);
    out[4..8].copy_from_slice(&tmp);
    encode_byte(c.2, &mut tmp);
    out[8..12].copy_from_slice(&tmp);
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
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(SPI_DEV)?;
        let fd = file.as_raw_fd();
        unsafe {
            let mode: u8 = 0;
            check(libc::ioctl(fd, SPI_IOC_WR_MODE as _, &mode as *const u8))?;
            let bpw: u8 = 8;
            check(libc::ioctl(
                fd,
                SPI_IOC_WR_BITS_PER_WORD as _,
                &bpw as *const u8,
            ))?;
            let hz: u32 = SPI_HZ;
            check(libc::ioctl(
                fd,
                SPI_IOC_WR_MAX_SPEED_HZ as _,
                &hz as *const u32,
            ))?;
        }
        Ok(Self {
            file,
            leds: led_count(),
        })
    }

    fn write_pixel(&mut self, c: Rgb) -> std::io::Result<()> {
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
    if rc < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

struct Led {
    path: String,
    on: Option<bool>,
}

impl Led {
    fn open(name: &str) -> Option<Self> {
        let path = format!("{LEDS_DIR}/{name}/brightness");
        Path::new(&path).exists().then_some(Self { path, on: None })
    }

    fn set(&mut self, on: bool) {
        if self.on != Some(on) && fs::write(&self.path, if on { "1" } else { "0" }).is_ok() {
            self.on = Some(on);
        }
    }
}

enum Leds {
    Pixel(Spi),
    Pair(Led, Led),
}

impl Leds {
    fn open() -> std::io::Result<Self> {
        match Spi::open() {
            Ok(spi) => Ok(Self::Pixel(spi)),
            Err(e) => match (Led::open("red"), Led::open("blue")) {
                (Some(status), Some(bt)) => Ok(Self::Pair(status, bt)),
                _ => Err(std::io::Error::new(
                    e.kind(),
                    format!("open {SPI_DEV}: {e}, and there is no red and blue LED either"),
                )),
            },
        }
    }

    fn show(&mut self, state: &State, cfg: &Config, tick: u64) {
        match self {
            Self::Pixel(spi) => {
                let _ = spi.write_pixel(render(state, cfg, tick));
            }
            Self::Pair(status, bt) => {
                let (s, b) = render_pair(state, cfg, tick);
                status.set(s);
                bt.set(b);
            }
        }
    }
}

static RELOAD: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sighup(_: libc::c_int) {
    RELOAD.store(true, Ordering::SeqCst);
}

fn livid_main() -> std::io::Result<()> {
    let _ = fs::create_dir_all(STATE_DIR);

    let _ = fs::write(PID_PATH, format!("{}\n", std::process::id()));

    unsafe {
        libc::signal(libc::SIGHUP, on_sighup as *const () as libc::sighandler_t);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    let mut leds = Leds::open()?;

    let mut cfg = Config::load();
    let mut cfg_mtime = mtime(CONFIG_PATH);
    let mut tick: u64 = 0;
    thread::spawn(follow_wifi_clients);

    loop {
        if RELOAD.swap(false, Ordering::SeqCst) {
            cfg = Config::load();
        }
        let m = mtime(CONFIG_PATH);
        if m != cfg_mtime {
            cfg_mtime = m;
            cfg = Config::load();
        }

        leds.show(
            &State::read(WIFI_CLIENT.load(Ordering::Relaxed)),
            &cfg,
            tick,
        );

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

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(state: State, tick: u64) -> (bool, bool) {
        render_pair(&state, &Config::default(), tick)
    }

    fn pixel(state: State, tick: u64) -> (u8, u8, u8) {
        let c = render(&state, &Config::default(), tick);
        (c.0, c.1, c.2)
    }

    fn over_a_second(state: State, led: fn((bool, bool)) -> bool) -> Vec<bool> {
        (0..TICK_HZ).map(|t| led(pair(state, t))).collect()
    }

    fn with(wifi: Wifi, bt: Bt) -> State {
        State {
            wifi,
            bt,
            ..Default::default()
        }
    }

    #[test]
    fn two_leds_show_wifi_on_red_and_bluetooth_on_blue() {
        let waiting = over_a_second(State::default(), |p| p.0);
        assert!(waiting.contains(&true) && waiting.contains(&false));
        assert_eq!(pair(with(Wifi::Client, Bt::Idle), 13), (true, false));
        assert_eq!(pair(with(Wifi::Client, Bt::Connected), 13), (true, true));
        let paging = over_a_second(with(Wifi::Client, Bt::Paging), |p| p.1);
        assert!(paging.contains(&true) && paging.contains(&false));
    }

    #[test]
    fn two_leds_alternate_while_flashing_and_red_stays_on_a_failed_write() {
        for t in 0..TICK_HZ {
            let (red, blue) = pair(
                State {
                    flash_mode: true,
                    ..with(Wifi::Client, Bt::Idle)
                },
                t,
            );
            assert_ne!(red, blue);
        }
        assert_eq!(
            pair(
                State {
                    flash_error: true,
                    flash_mode: true,
                    ..Default::default()
                },
                0
            ),
            (true, false)
        );
    }

    #[test]
    fn the_switch_goes_ahead_of_what_the_radio_reports() {
        assert_eq!(Wifi::of(false, true), Wifi::Off);
        assert_eq!(Wifi::of(true, false), Wifi::Waiting);
        assert_eq!(Wifi::of(true, true), Wifi::Client);
        assert_eq!(Bt::of(false, true, true), Bt::Off);
        assert_eq!(Bt::of(true, true, true), Bt::Connected);
        assert_eq!(Bt::of(true, false, true), Bt::Paging);
        assert_eq!(Bt::of(true, false, false), Bt::Idle);
    }

    #[test]
    fn a_switched_off_radio_stays_dark() {
        let wifi_off = with(Wifi::Off, Bt::Idle);
        assert!((0..TICK_HZ).all(|t| pixel(wifi_off, t) == (0, 0, 0) && !pair(wifi_off, t).0));

        let bt_off = with(Wifi::Client, Bt::Off);
        assert_eq!(pixel(bt_off, 13), pixel(with(Wifi::Client, Bt::Idle), 13));
        assert_eq!(pair(bt_off, 13), (true, false));

        let both_off = with(Wifi::Off, Bt::Off);
        assert!(
            (0..TICK_HZ)
                .all(|t| pixel(both_off, t) == (0, 0, 0) && pair(both_off, t) == (false, false))
        );
    }

    #[test]
    fn a_failed_write_shows_with_both_radios_off() {
        let failed = State {
            flash_error: true,
            ..with(Wifi::Off, Bt::Off)
        };
        assert_ne!(pixel(failed, 0).0, 0);
        assert_eq!(pair(failed, 0), (true, false));
    }

    #[test]
    fn brightness_zero_turns_both_off() {
        let cfg = Config {
            brightness_pct: 0,
            ..Config::default()
        };
        assert_eq!(
            render_pair(&with(Wifi::Client, Bt::Connected), &cfg, 0),
            (false, false)
        );
    }
}
