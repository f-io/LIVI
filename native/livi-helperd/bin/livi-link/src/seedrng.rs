//! Tops up the kernel entropy pool. Lab bring-up: it credits entropy without a source.

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::process::ExitCode;
use std::thread::sleep;
use std::time::Duration;

/// _IOW('R', 0x03, int[2])
const RNDADDENTROPY: libc::c_ulong = 0x4008_5203;
const ROUND: Duration = Duration::from_millis(100);

#[repr(C)]
struct RandPoolInfo {
    entropy_count: libc::c_int,
    buf_size: libc::c_int,
    buf: [u8; 256],
}

pub fn run() -> ExitCode {
    let pool = match OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/urandom")
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[seedrng] open /dev/urandom: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut source = match File::open("/dev/urandom") {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[seedrng] open /dev/urandom: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("[seedrng] crediting {} bits per {ROUND:?}", 256 * 8);

    let mut info = RandPoolInfo {
        entropy_count: 256 * 8,
        buf_size: 256,
        buf: [0u8; 256],
    };
    loop {
        if source.read_exact(&mut info.buf).is_err() {
            continue;
        }
        if unsafe { libc::ioctl(pool.as_raw_fd(), RNDADDENTROPY, &info) } < 0 {
            eprintln!(
                "[seedrng] RNDADDENTROPY: {}",
                std::io::Error::last_os_error()
            );
            return ExitCode::FAILURE;
        }
        sleep(ROUND);
    }
}
