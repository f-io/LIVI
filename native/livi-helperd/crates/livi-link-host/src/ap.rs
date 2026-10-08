use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::time::Duration;

use livi_wifi::{Channel, OnAir, Security};

use crate::link;

const TIMEOUT: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(500);
const WATCH_SILENCE: Duration = Duration::from_secs(15);

fn ask(command: &str) -> Option<HashMap<String, String>> {
    let mut stream = livi_net::connect((link::LINK_NAME, livi_net::port::CONTROL), TIMEOUT).ok()?;
    stream.set_read_timeout(Some(TIMEOUT)).ok()?;
    stream.write_all(format!("{command}\n").as_bytes()).ok()?;
    Some(fields(BufReader::new(stream)))
}

fn fields(answer: impl BufRead) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    for line in answer.lines().map_while(Result::ok) {
        if line == "ok" || line.starts_with("error") {
            break;
        }
        if let Some((key, value)) = line.split_once(' ') {
            fields.insert(key.to_string(), value.trim().to_string());
        }
    }
    fields
}

pub(crate) fn order(command: &str) -> Result<(), String> {
    let mut stream = livi_net::connect((link::LINK_NAME, livi_net::port::CONTROL), TIMEOUT)
        .map_err(|e| format!("dongle: {e}"))?;
    stream.set_read_timeout(Some(TIMEOUT)).map_err(|e| format!("dongle: {e}"))?;
    writeln!(stream, "{command}").map_err(|e| format!("dongle: {e}"))?;
    let mut answer = String::new();
    BufReader::new(&stream).read_line(&mut answer).map_err(|e| format!("dongle: {e}"))?;
    answered(&answer)
}

fn answered(answer: &str) -> Result<(), String> {
    match answer.trim() {
        "ok" => Ok(()),
        other => Err(other.trim_start_matches("error ").to_string()),
    }
}

pub fn status() -> Option<HashMap<String, String>> {
    ask("status")
}

/// Costs the dongle's Wi-Fi driver a firmware round trip, older dongles answer with nothing.
pub fn rates() -> Option<HashMap<String, String>> {
    ask("rates")
}

/// `within` is per answer, since applying waits for the radio.
pub fn talk(commands: &[String], within: Duration) -> Result<HashMap<String, String>, String> {
    let stream = livi_net::connect((link::LINK_NAME, livi_net::port::CONTROL), TIMEOUT)
        .map_err(|e| format!("dongle: {e}"))?;
    stream.set_read_timeout(Some(within)).map_err(|e| format!("dongle: {e}"))?;
    exchange(stream, commands)
}

fn exchange(
    stream: impl Read + Write,
    commands: &[String],
) -> Result<HashMap<String, String>, String> {
    let mut reader = BufReader::new(stream);
    let mut fields = HashMap::new();
    for command in commands {
        writeln!(reader.get_mut(), "{command}").map_err(|e| format!("dongle: {e}"))?;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).map_err(|e| format!("dongle: {e}"))? == 0 {
                return Err("the dongle closed the link".into());
            }
            let line = line.trim_end();
            if line == "ok" {
                break;
            }
            if let Some(reason) = line.strip_prefix("error ") {
                return Err(format!("{command}: {reason}"));
            }
            if let Some((key, value)) = line.split_once(' ') {
                fields.insert(key.to_string(), value.trim().to_string());
            }
        }
    }
    Ok(fields)
}

pub fn deauth() -> Option<usize> {
    ask("deauth")?.get("deauth")?.parse().ok()
}

fn station(line: &str) -> Option<(bool, &str)> {
    match line.trim().split_once(' ')? {
        ("joined", mac) => Some((true, mac)),
        ("left", mac) => Some((false, mac)),
        _ => None,
    }
}

pub fn watch_stations(mut on: impl FnMut(bool, &str)) -> std::io::Result<()> {
    let mut stream = livi_net::connect((link::LINK_NAME, livi_net::port::CONTROL), TIMEOUT)?;
    stream.set_read_timeout(Some(WATCH_SILENCE))?;
    stream.write_all(b"watch\n")?;
    for line in BufReader::new(stream).lines() {
        if let Some((joined, mac)) = station(&line?) {
            on(joined, mac);
        }
    }
    Ok(())
}

pub fn status_field(key: &str) -> Option<String> {
    status()?.remove(key)
}

/// The dongle has no 6 GHz, so its bare channel number is enough. A dongle from before it
/// told its security runs WPA2.
pub fn on_air() -> Option<OnAir> {
    on_air_in(status()?)
}

fn on_air_in(mut status: HashMap<String, String>) -> Option<OnAir> {
    if status.get("state")? != "on" {
        return None;
    }
    Some(OnAir {
        ssid: status.remove("ssid")?,
        channel: Channel::of_number(status.get("channel")?.parse().ok()?),
        security: status.get("security").and_then(|s| Security::of_name(s)).unwrap_or_default(),
    })
}

pub fn ready(within: Duration) -> bool {
    let deadline = std::time::Instant::now() + within;
    loop {
        if status_field("state").as_deref() == Some("on") {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
    }
}

/// The SSID on air, which is what the phone must look for.
pub fn ssid() -> Option<String> {
    status_field("ssid")
}

pub fn mac() -> Option<String> {
    status_field("mac")
}

/// Most significant byte first.
pub fn bt_mac() -> Option<[u8; 6]> {
    let text = status_field("btmac")?;
    let mut out = [0u8; 6];
    let mut parts = text.trim().split(':');
    for byte in &mut out {
        *byte = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    parts.next().is_none().then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status_of(text: &str) -> HashMap<String, String> {
        fields(text.as_bytes())
    }

    #[test]
    fn the_dongle_on_air_carries_its_channel_and_security() {
        let live =
            on_air_in(status_of("state on\nssid LIVI\nchannel 149\nsecurity wpa2-wpa3\nok\n"));
        assert_eq!(
            live,
            Some(OnAir {
                ssid: "LIVI".into(),
                channel: Channel::of_number(149),
                security: Security::Wpa2Wpa3
            })
        );
    }

    #[test]
    fn a_dongle_that_names_no_security_runs_wpa2() {
        let live = on_air_in(status_of("state on\nssid LIVI\nchannel 6\nok\n")).unwrap();
        assert_eq!(live.security, Security::Wpa2);
        assert_eq!(on_air_in(status_of("state off\nssid LIVI\nchannel 6\nok\n")), None);
    }

    struct Wire {
        answer: std::io::Cursor<Vec<u8>>,
        sent: Vec<u8>,
    }

    impl Read for Wire {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.answer.read(buf)
        }
    }

    impl Write for Wire {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.sent.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn wire(answer: &str) -> Wire {
        Wire { answer: std::io::Cursor::new(answer.as_bytes().to_vec()), sent: Vec::new() }
    }

    #[test]
    fn a_talk_runs_the_commands_in_order_and_stops_at_a_refusal() {
        let cmds = ["status", "set ssid Car", "apply"].map(String::from);
        let fields = exchange(wire("state on\nmac aa:bb\nok\nok\nok\n"), &cmds).unwrap();
        assert_eq!(fields.get("mac").map(String::as_str), Some("aa:bb"));

        let refused = exchange(wire("ok\nerror bad ssid\nok\n"), &cmds);
        assert_eq!(refused, Err("set ssid Car: bad ssid".to_string()));
        assert_eq!(exchange(wire("ok\n"), &cmds), Err("the dongle closed the link".to_string()));
    }

    #[test]
    fn an_answer_reads_up_to_its_end() {
        let answer = fields("deauth 2\nok\nstate on\n".as_bytes());
        assert_eq!(answer.get("deauth").map(String::as_str), Some("2"));
        assert!(!answer.contains_key("state"));
        assert!(fields("error hostapd is gone\n".as_bytes()).is_empty());
    }

    #[test]
    fn an_order_is_done_on_ok_and_names_what_went_wrong_otherwise() {
        assert_eq!(answered("ok\n"), Ok(()));
        assert_eq!(answered("error not an address\n").unwrap_err(), "not an address");
        assert!(answered("").is_err());
    }

    #[test]
    fn a_watch_line_names_the_phone_and_which_way_it_went() {
        assert_eq!(station("joined 9a:c4:e2:44:5e:0f"), Some((true, "9a:c4:e2:44:5e:0f")));
        assert_eq!(station("left 9a:c4:e2:44:5e:0f\n"), Some((false, "9a:c4:e2:44:5e:0f")));
        assert_eq!(station(""), None);
        assert_eq!(station("ok"), None);
        assert_eq!(station("moved 9a:c4:e2:44:5e:0f"), None);
    }
}
