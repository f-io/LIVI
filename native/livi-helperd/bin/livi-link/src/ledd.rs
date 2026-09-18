// LED
//   flash-mode   → red + blue alternating
//   bt-connected → blue solid, bt-paging → blue blinking
//   wifi client  → red solid, no client → red blinking

use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

const RED: u32 = 2;
const BLUE: u32 = 9;
const IFACE: &str = "wlan0";
const STATE_DIR: &str = "/tmp/livi/led";
const TICK: Duration = Duration::from_millis(120);

pub fn run() -> ExitCode {
    export(RED);
    export(BLUE);
    let mut tick: u64 = 0;
    let mut client = false;
    loop {
        // Associations change slowly; a netlink dump every ~1 s is plenty.
        if tick.is_multiple_of(8) {
            client = livi_wifi::station_count(IFACE) > 0;
        }
        let slow_on = (tick / 4).is_multiple_of(2); // ~1 Hz
        let (red, blue) = if exists("flash-mode") {
            (slow_on, !slow_on)
        } else {
            let blue = exists("bt-connected") || (exists("bt-paging") && slow_on);
            let red = client || slow_on;
            (red, blue)
        };
        set(RED, red);
        set(BLUE, blue);
        thread::sleep(TICK);
        tick += 1;
    }
}

fn exists(name: &str) -> bool {
    Path::new(&format!("{STATE_DIR}/{name}")).exists()
}

fn export(g: u32) {
    if !Path::new(&format!("/sys/class/gpio/gpio{g}")).exists() {
        let _ = fs::write("/sys/class/gpio/export", g.to_string());
    }
    let _ = fs::write(format!("/sys/class/gpio/gpio{g}/direction"), "out");
}

fn set(g: u32, on: bool) {
    // active-low: 0 = on, 1 = off
    let _ = fs::write(format!("/sys/class/gpio/gpio{g}/value"), if on { "0" } else { "1" });
}
