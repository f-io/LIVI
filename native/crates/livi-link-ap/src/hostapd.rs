// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashSet;
use std::io;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

pub const CTRL_DIR: &str = "/tmp/livi/hostapd";
const LEAVING: u16 = 3;
const REPLY: Duration = Duration::from_secs(2);
const QUIET: Duration = Duration::from_secs(5);

static NEXT: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, PartialEq, Eq)]
pub enum Station {
    Joined(String),
    Left(String),
}

struct Client {
    sock: UnixDatagram,
    path: PathBuf,
}

impl Client {
    fn open(ctrl: &Path) -> io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "wifid-ctrl-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&path);
        let client = Self {
            sock: UnixDatagram::bind(&path)?,
            path,
        };
        client.sock.connect(ctrl)?;
        client.sock.set_read_timeout(Some(REPLY))?;
        Ok(client)
    }

    fn request(&self, cmd: &str) -> io::Result<String> {
        self.sock.send(cmd.as_bytes())?;
        let mut buf = [0u8; 4096];
        loop {
            let n = self.sock.recv(&mut buf)?;
            let text = String::from_utf8_lossy(&buf[..n]).into_owned();
            if !text.starts_with('<') {
                return Ok(text);
            }
        }
    }

    fn stations(&self) -> io::Result<Vec<String>> {
        let mut macs = Vec::new();
        let mut reply = self.request("STA-FIRST")?;
        while let Some(mac) = first_mac(&reply) {
            reply = self.request(&format!("STA-NEXT {mac}"))?;
            macs.push(mac);
        }
        Ok(macs)
    }

    fn attached(ctrl: &Path) -> io::Result<Self> {
        let client = Self::open(ctrl)?;
        let attached = client.request("ATTACH")?;
        if attached.trim() != "OK" {
            return Err(io::Error::other(format!(
                "hostapd refused to attach: {}",
                attached.trim()
            )));
        }
        Ok(client)
    }

    fn listen(&self, mut on: impl FnMut(Option<Station>) -> bool) -> io::Result<()> {
        self.sock.set_read_timeout(Some(QUIET))?;
        let mut buf = [0u8; 4096];
        loop {
            match self.sock.recv(&mut buf) {
                Ok(n) => {
                    if let Some(station) = event(&String::from_utf8_lossy(&buf[..n]))
                        && !on(Some(station))
                    {
                        return Ok(());
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    self.sock.send(b"PING")?;
                    if !on(None) {
                        return Ok(());
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn is_mac(text: &str) -> bool {
    text.len() == 17
        && text
            .split(':')
            .all(|b| b.len() == 2 && u8::from_str_radix(b, 16).is_ok())
}

fn first_mac(reply: &str) -> Option<String> {
    let line = reply.lines().next()?.trim();
    is_mac(line).then(|| line.to_lowercase())
}

pub fn ctrl(iface: &str) -> PathBuf {
    Path::new(CTRL_DIR).join(iface)
}

pub fn deauth_all(ctrl: &Path) -> io::Result<usize> {
    let client = Client::open(ctrl)?;
    let macs = client.stations()?;
    for mac in &macs {
        let reply = client.request(&format!("DEAUTHENTICATE {mac} reason={LEAVING}"))?;
        if reply.trim() != "OK" {
            eprintln!(
                "[wifid] hostapd would not deauthenticate {mac}: {}",
                reply.trim()
            );
        }
    }
    Ok(macs.len())
}

pub fn event(text: &str) -> Option<Station> {
    let body = text.split_once('>').map_or(text, |(_, body)| body);
    let mut words = body.split_whitespace();
    let kind = words.next()?;
    let mac = words.next().filter(|m| is_mac(m))?.to_lowercase();
    match kind {
        "AP-STA-CONNECTED" => Some(Station::Joined(mac)),
        "AP-STA-DISCONNECTED" => Some(Station::Left(mac)),
        _ => None,
    }
}

pub fn watch(ctrl: &Path, on: impl FnMut(Option<Station>) -> bool) -> io::Result<()> {
    Client::attached(ctrl)?.listen(on)
}

pub fn follow_stations(
    ctrl: &Path,
    mut on: impl FnMut(&HashSet<String>) -> bool,
) -> io::Result<()> {
    let client = Client::attached(ctrl)?;
    let mut joined: HashSet<String> = client.stations()?.into_iter().collect();
    if !on(&joined) {
        return Ok(());
    }
    client.listen(|station| {
        match station {
            Some(Station::Joined(mac)) => joined.insert(mac),
            Some(Station::Left(mac)) => joined.remove(&mac),
            None => return true,
        };
        on(&joined)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn socket() -> (PathBuf, UnixDatagram) {
        let dir = std::env::temp_dir().join(format!(
            "hostapd-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("wlan0");
        let _ = std::fs::remove_file(&path);
        let server = UnixDatagram::bind(&path).unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        (path, server)
    }

    fn hostapd(
        stations: &'static [&'static str],
    ) -> (PathBuf, std::thread::JoinHandle<Vec<String>>) {
        let (path, server) = socket();
        let handle = std::thread::spawn(move || {
            let mut heard = Vec::new();
            let mut buf = [0u8; 512];
            while let Ok((n, from)) = server.recv_from(&mut buf) {
                let cmd = String::from_utf8_lossy(&buf[..n]).into_owned();
                let reply = match cmd.as_str() {
                    "STA-FIRST" => stations.first().map(|m| format!("{m}\nflags=[AUTH]\n")),
                    c if c.starts_with("STA-NEXT ") => {
                        let at = stations.iter().position(|m| c.ends_with(m)).unwrap();
                        stations.get(at + 1).map(|m| format!("{m}\nflags=[AUTH]\n"))
                    }
                    _ => Some("OK\n".to_string()),
                };
                let reply = reply.unwrap_or_default();
                server
                    .send_to(reply.as_bytes(), from.as_pathname().unwrap())
                    .unwrap();
                heard.push(cmd);
            }
            heard
        });
        (path, handle)
    }

    #[test]
    fn every_station_gets_its_own_deauthentication() {
        let (path, hostapd) = hostapd(&["9a:c4:e2:44:5e:0f", "aa:bb:cc:dd:ee:ff"]);
        assert_eq!(deauth_all(&path).unwrap(), 2);
        let heard = hostapd.join().unwrap();
        assert!(heard.contains(&"DEAUTHENTICATE 9a:c4:e2:44:5e:0f reason=3".to_string()));
        assert!(heard.contains(&"DEAUTHENTICATE aa:bb:cc:dd:ee:ff reason=3".to_string()));
    }

    #[test]
    fn an_empty_access_point_sends_nothing() {
        let (path, hostapd) = hostapd(&[]);
        assert_eq!(deauth_all(&path).unwrap(), 0);
        assert_eq!(hostapd.join().unwrap(), vec!["STA-FIRST".to_string()]);
    }

    #[test]
    fn no_hostapd_is_an_error() {
        assert!(deauth_all(Path::new("/nonexistent/wlan0")).is_err());
    }

    #[test]
    fn a_watch_hands_on_what_hostapd_reports_until_told_to_stop() {
        let (path, server) = socket();
        let hostapd = std::thread::spawn(move || {
            let mut buf = [0u8; 64];
            let (n, from) = server.recv_from(&mut buf).unwrap();
            assert_eq!(&buf[..n], b"ATTACH");
            let to = from.as_pathname().unwrap().to_path_buf();
            for line in [
                "OK\n",
                "<3>AP-STA-CONNECTED 9a:c4:e2:44:5e:0f",
                "<3>AP-STA-DISCONNECTED 9a:c4:e2:44:5e:0f",
            ] {
                server.send_to(line.as_bytes(), &to).unwrap();
            }
        });
        let mut seen = Vec::new();
        watch(&path, |station| {
            seen.extend(station);
            seen.len() < 2
        })
        .unwrap();
        hostapd.join().unwrap();
        assert_eq!(
            seen,
            [
                Station::Joined("9a:c4:e2:44:5e:0f".into()),
                Station::Left("9a:c4:e2:44:5e:0f".into())
            ]
        );
    }

    #[test]
    fn following_starts_from_the_stations_present_and_tracks_every_change() {
        let (path, server) = socket();
        let hostapd = std::thread::spawn(move || {
            let mut buf = [0u8; 64];
            let mut answer = |reply: &str| {
                let (_, from) = server.recv_from(&mut buf).unwrap();
                let to = from.as_pathname().unwrap().to_path_buf();
                server.send_to(reply.as_bytes(), &to).unwrap();
                to
            };
            answer("OK\n");
            answer("aa:bb:cc:dd:ee:ff\nflags=[AUTH]\n");
            let to = answer("");
            for line in [
                "<3>AP-STA-CONNECTED 9a:c4:e2:44:5e:0f",
                "<3>AP-STA-DISCONNECTED aa:bb:cc:dd:ee:ff",
            ] {
                server.send_to(line.as_bytes(), &to).unwrap();
            }
        });
        let mut seen = Vec::new();
        follow_stations(&path, |joined| {
            let mut macs: Vec<String> = joined.iter().cloned().collect();
            macs.sort();
            seen.push(macs);
            seen.len() < 3
        })
        .unwrap();
        hostapd.join().unwrap();
        assert_eq!(
            seen,
            [
                vec!["aa:bb:cc:dd:ee:ff"],
                vec!["9a:c4:e2:44:5e:0f", "aa:bb:cc:dd:ee:ff"],
                vec!["9a:c4:e2:44:5e:0f"]
            ]
        );
    }

    #[test]
    fn joins_and_leaves_are_read_from_the_events() {
        assert_eq!(
            event("<3>AP-STA-CONNECTED 9A:C4:E2:44:5E:0F"),
            Some(Station::Joined("9a:c4:e2:44:5e:0f".into()))
        );
        assert_eq!(
            event("<3>AP-STA-DISCONNECTED 9a:c4:e2:44:5e:0f"),
            Some(Station::Left("9a:c4:e2:44:5e:0f".into()))
        );
        assert_eq!(event("<3>CTRL-EVENT-EAP-STARTED 9a:c4:e2:44:5e:0f"), None);
        assert_eq!(event("<3>AP-STA-DISCONNECTED"), None);
        assert_eq!(event("PONG"), None);
    }

    #[test]
    fn the_socket_sits_in_the_tmpfs() {
        assert_eq!(ctrl("wlan0"), PathBuf::from("/tmp/livi/hostapd/wlan0"));
    }
}
