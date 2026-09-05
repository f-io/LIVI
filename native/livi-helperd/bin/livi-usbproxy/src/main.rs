// Serves the iPhone's USB side over TCP. Four operations, all from iap2-usbmux:
//   LIST             enumerate attached iPhones
//   CONFIG <serial>  vendor request 0x52 + select the CarPlay configuration
//   RESTORE <serial> hand the phone back to the default configuration
//   ATTACH <serial>  claim the usbmux interface; the socket then carries the bulk pipes

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

const PORT: u16 = 5003;

/// Cancel flag of the attach holding the usbmux interface; a new ATTACH sets it and takes over.
fn active_attach() -> &'static Mutex<Option<Arc<AtomicBool>>> {
    static ACTIVE: OnceLock<Mutex<Option<Arc<AtomicBool>>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(None))
}

fn main() {
    let listener = match TcpListener::bind(("0.0.0.0", PORT)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[usbproxy] bind :{PORT}: {e}");
            std::process::exit(1);
        }
    };
    println!("[usbproxy] listening on :{PORT}");

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                thread::spawn(move || {
                    if let Err(e) = handle(s) {
                        eprintln!("[usbproxy] {e}");
                    }
                });
            }
            Err(e) => eprintln!("[usbproxy] accept: {e}"),
        }
    }
}

fn read_line(s: &mut TcpStream) -> Result<String, String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match s.read(&mut byte) {
            Ok(0) => return Err("client closed".into()),
            Ok(_) => {
                if byte[0] == b'\n' {
                    return Ok(String::from_utf8_lossy(&line).trim_end().to_string());
                }
                line.push(byte[0]);
                if line.len() > 512 {
                    return Err("command too long".into());
                }
            }
            Err(e) => return Err(format!("read: {e}")),
        }
    }
}

fn handle(mut s: TcpStream) -> Result<(), String> {
    s.set_nodelay(true).ok();
    let line = read_line(&mut s)?;
    let mut parts = line.split_whitespace();
    let cmd = parts.next().unwrap_or("");
    let serial = parts.next().unwrap_or("").to_string();

    match cmd {
        "LIST" => {
            let mut out = String::new();
            for p in iap2_usbmux::find_iphones() {
                let cfg = p.config_value.map(|c| c.to_string()).unwrap_or_else(|| "-".into());
                out.push_str(&format!("{} {} {}\n", p.serial, p.num_configs, cfg));
            }
            out.push_str("END\n");
            s.write_all(out.as_bytes()).map_err(|e| e.to_string())
        }
        "CONFIG" => {
            let reply = match iap2_usbmux::ensure_carplay_config(&serial) {
                Ok(p) => format!(
                    "OK {} {}\n",
                    p.num_configs,
                    p.config_value.map(|c| c.to_string()).unwrap_or_else(|| "-".into())
                ),
                Err(e) => format!("ERR {e}\n"),
            };
            s.write_all(reply.as_bytes()).map_err(|e| e.to_string())
        }
        "RESTORE" => {
            iap2_usbmux::restore_default_config(&serial);
            s.write_all(b"OK\n").map_err(|e| e.to_string())
        }
        "ATTACH" => attach(s, &serial),
        other => {
            let _ = s.write_all(format!("ERR unknown command {other}\n").as_bytes());
            Ok(())
        }
    }
}

/// Claims the usbmux interface and pumps bytes to and from the socket until either side ends.
fn attach(mut s: TcpStream, serial: &str) -> Result<(), String> {
    // Retires a previous attach and, with the lock released, waits for it to let go of the interface.
    let cancel = Arc::new(AtomicBool::new(false));
    let previous = active_attach().lock().unwrap().replace(cancel.clone());
    if let Some(old) = previous {
        old.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Arc::strong_count(&old) > 1 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
    }

    let (writer, reader) = match iap2_usbmux::open_pipes(serial) {
        Ok(p) => p,
        Err(e) => {
            let _ = s.write_all(format!("ERR {e}\n").as_bytes());
            return Ok(());
        }
    };
    s.write_all(b"OK\n").map_err(|e| e.to_string())?;
    println!("[usbproxy] {}: attached", short(serial));

    let writer = Arc::new(Mutex::new(writer));
    let mut to_phone = s.try_clone().map_err(|e| e.to_string())?;

    // USB -> socket.
    let mut sock_out = s;
    let up_cancel = cancel.clone();
    let up = thread::spawn(move || {
        let mut reader = reader;
        while !up_cancel.load(Ordering::SeqCst) {
            match reader.read(Duration::from_millis(500)) {
                Ok(d) if d.is_empty() => continue,
                Ok(d) => {
                    if sock_out.write_all(&d).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = sock_out.shutdown(std::net::Shutdown::Both);
    });

    // socket -> USB, one whole mux packet per bulk write (length in header bytes 4..8).
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 65536];
    to_phone.set_read_timeout(Some(Duration::from_millis(500))).ok();
    while !cancel.load(Ordering::SeqCst) {
        let n = match to_phone.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(_) => break,
        };
        buf.extend_from_slice(&chunk[..n]);
        while buf.len() >= 8 {
            let length = u32::from_be_bytes(buf[4..8].try_into().unwrap()) as usize;
            if length < 8 || buf.len() < length {
                break;
            }
            let pkt: Vec<u8> = buf.drain(..length).collect();
            if writer.lock().unwrap().write(&pkt).is_err() {
                buf.clear();
                break;
            }
        }
    }
    cancel.store(true, Ordering::SeqCst);
    let _ = to_phone.shutdown(std::net::Shutdown::Both);
    let _ = up.join();
    drop(writer); // release the interface before clearing the slot
    let mut slot = active_attach().lock().unwrap();
    if slot.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
        *slot = None;
    }
    println!("[usbproxy] {}: detached", short(serial));
    Ok(())
}

fn short(serial: &str) -> &str {
    &serial[..8.min(serial.len())]
}
