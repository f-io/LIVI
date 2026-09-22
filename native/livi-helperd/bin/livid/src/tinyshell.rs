// Ported from bin/livi-tinyshell/src/main.rs — see livid dispatcher in main.rs.
pub fn run(_args: Vec<String>) -> i32 {
    match livid_main() {
        Ok(()) => 0,
        Err(e) => { eprintln!("[livi-tinyshell] {e}"); 1 }
    }
}

// livi-tinyshell — TCP shell for LIVI-Link (V821B) bring-up.
//
// Binds 0.0.0.0:2323, forks a subprocess per connection, connects stdin/stdout/stderr
// to the socket, execs /bin/sh. Zero authentication. Only intended for the isolated
// usb0 NCM link during bring-up; not for production.

use std::env;
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::os::fd::AsRawFd;
use std::process::{Command, exit};

fn livid_main() -> io::Result<()> {
    let port: u16 = env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(2323);
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port))?;
    eprintln!("[livi-tinyshell] listening on 0.0.0.0:{port}");

    // Reap zombies without hanging so long-lived children don't linger.
    unsafe { libc::signal(libc::SIGCHLD, libc::SIG_IGN); }

    for conn in listener.incoming() {
        let sock = match conn {
            Ok(s) => s,
            Err(e) => { eprintln!("[livi-tinyshell] accept: {e}"); continue; }
        };
        let fd = sock.as_raw_fd();
        // Fork: child dup2s the socket to 0/1/2 and execs /bin/sh -i.
        // Parent just drops the socket and loops.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            unsafe {
                libc::dup2(fd, 0);
                libc::dup2(fd, 1);
                libc::dup2(fd, 2);
            }
            drop(sock);
            // execvp resolves /bin/sh via PATH, but we want to be explicit and safe.
            let err = Command::new("/bin/sh").arg("-i").exec_replace();
            eprintln!("[livi-tinyshell] exec /bin/sh failed: {err}");
            exit(1);
        }
        drop(sock);
    }
    Ok(())
}

// Small helper: replace this process with the child (std doesn't expose exec directly on Unix).
trait ExecReplace {
    fn exec_replace(&mut self) -> io::Error;
}
impl ExecReplace for Command {
    fn exec_replace(&mut self) -> io::Error {
        use std::os::unix::process::CommandExt;
        self.exec()
    }
}
