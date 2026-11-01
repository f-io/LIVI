// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::info::{AntennaPower, AntennaStatus, Jamming, Rf, Version};
use super::{latin1, trim};

const SYNC_1: u8 = 0xb5;
const SYNC_2: u8 = 0x62;

pub const CLASS_MON: u8 = 0x0a;
pub const ID_MON_VER: u8 = 0x04;
pub const ID_MON_RF: u8 = 0x38;

const BUFFER_MAX: usize = 8192;
const BUFFER_KEEP: usize = 2048;
const PAYLOAD_MAX: usize = 4096;

#[derive(Debug, PartialEq)]
pub struct Frame {
    pub class: u8,
    pub id: u8,
    pub payload: Vec<u8>,
}

pub fn checksum(bytes: &[u8]) -> [u8; 2] {
    let (mut a, mut b) = (0u8, 0u8);
    for &byte in bytes {
        a = a.wrapping_add(byte);
        b = b.wrapping_add(a);
    }
    [a, b]
}

pub fn build(class: u8, id: u8, payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u16;
    let mut body = vec![class, id];
    body.extend_from_slice(&len.to_le_bytes());
    body.extend_from_slice(payload);
    let mut frame = vec![SYNC_1, SYNC_2];
    frame.extend_from_slice(&body);
    frame.extend_from_slice(&checksum(&body));
    frame
}

pub fn poll_version() -> Vec<u8> {
    build(CLASS_MON, ID_MON_VER, &[])
}

pub fn poll_rf() -> Vec<u8> {
    build(CLASS_MON, ID_MON_RF, &[])
}

#[derive(Default)]
pub struct Parser {
    buffer: Vec<u8>,
}

impl Parser {
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Frame> {
        self.buffer.extend_from_slice(chunk);
        if self.buffer.len() > BUFFER_MAX {
            self.buffer.drain(..self.buffer.len() - BUFFER_KEEP);
        }
        let buf = &self.buffer;
        let mut frames = Vec::new();
        let mut offset = 0;
        loop {
            let Some(start) = buf[offset..]
                .iter()
                .position(|&b| b == SYNC_1)
                .map(|i| offset + i)
            else {
                offset = buf.len();
                break;
            };
            if start + 1 >= buf.len() {
                offset = start;
                break;
            }
            if buf[start + 1] != SYNC_2 {
                offset = start + 1;
                continue;
            }
            if start + 6 > buf.len() {
                offset = start;
                break;
            }
            let len = usize::from(u16::from_le_bytes([buf[start + 4], buf[start + 5]]));
            let end = start + 6 + len + 2;
            if len > PAYLOAD_MAX {
                offset = start + 2;
                continue;
            }
            if end > buf.len() {
                offset = start;
                break;
            }
            if checksum(&buf[start + 2..start + 6 + len]) == [buf[end - 2], buf[end - 1]] {
                frames.push(Frame {
                    class: buf[start + 2],
                    id: buf[start + 3],
                    payload: buf[start + 6..start + 6 + len].to_vec(),
                });
                offset = end;
            } else {
                offset = start + 2;
            }
        }
        self.buffer.drain(..offset);
        frames
    }
}

fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    trim(&latin1(&bytes[..end])).to_string()
}

fn constellation_list(ext: &str) -> bool {
    ext.split(';')
        .all(|name| (3..=4).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_uppercase()))
}

pub fn parse_version(payload: &[u8]) -> Option<Version> {
    if payload.len() < 40 {
        return None;
    }
    let mut version = Version {
        software: text(&payload[..30]),
        hardware: text(&payload[30..40]),
        ..Default::default()
    };
    let mut supported = Vec::new();
    for ext in payload[40..]
        .as_chunks::<30>()
        .0
        .iter()
        .map(|ext| text(ext))
    {
        if let Some(v) = ext.strip_prefix("FWVER=") {
            version.firmware = Some(v.to_string());
        } else if let Some(v) = ext.strip_prefix("PROTVER=") {
            version.protocol = Some(v.to_string());
        } else if let Some(v) = ext.strip_prefix("MOD=") {
            version.model = Some(v.to_string());
        } else if !ext.is_empty() && constellation_list(&ext) {
            supported.extend(ext.split(';').map(str::to_string));
        }
    }
    if !supported.is_empty() {
        version.supported = Some(supported);
    }
    Some(version)
}

pub fn parse_rf(payload: &[u8]) -> Option<Rf> {
    let block = payload.get(4..4 + 24)?;
    let jamming = match block[1] & 0x03 {
        0 => Jamming::Unknown,
        1 => Jamming::Ok,
        2 => Jamming::Warning,
        _ => Jamming::Critical,
    };
    let antenna_status = match block[2] {
        0 => AntennaStatus::Init,
        2 => AntennaStatus::Ok,
        3 => AntennaStatus::Short,
        4 => AntennaStatus::Open,
        _ => AntennaStatus::Unknown,
    };
    let antenna_power = match block[3] {
        0 => AntennaPower::Off,
        1 => AntennaPower::On,
        _ => AntennaPower::Unknown,
    };
    Some(Rf {
        jamming,
        antenna_status,
        antenna_power,
        noise: u16::from_le_bytes([block[12], block[13]]),
        agc: u16::from_le_bytes([block[14], block[15]]),
        jamming_indicator: block[16],
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn field(text: &str, size: usize) -> Vec<u8> {
        let mut buf = vec![0u8; size];
        let bytes = text.as_bytes();
        buf[..bytes.len().min(size)].copy_from_slice(&bytes[..bytes.len().min(size)]);
        buf
    }

    fn real_version() -> Vec<u8> {
        [
            field("ROM CORE 4.04 (d964f4)", 30),
            field("00190000", 10),
            field("FWVER=SPG 4.04", 30),
            field("PROTVER=32.01", 30),
            field("GPS;GLO;GAL;BDS", 30),
            field("SBAS;QZSS", 30),
        ]
        .concat()
    }

    pub fn rf_payload(jamming: u8, status: u8, power: u8) -> Vec<u8> {
        let mut buf = vec![0u8; 28];
        buf[1] = 1;
        buf[5] = jamming;
        buf[6] = status;
        buf[7] = power;
        buf[16..18].copy_from_slice(&87u16.to_le_bytes());
        buf[18..20].copy_from_slice(&4321u16.to_le_bytes());
        buf[20] = 12;
        buf
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn the_polls_are_byte_exact() {
        assert_eq!(checksum(&[0x0a, 0x04, 0x00, 0x00]), [0x0e, 0x34]);
        assert_eq!(hex(&poll_version()), "b5620a0400000e34");
        assert_eq!(hex(&poll_rf()), "b5620a38000042d0");
        let frame = build(0x06, 0x01, &[0u8; 300]);
        assert_eq!(u16::from_le_bytes([frame[4], frame[5]]), 300);
        assert_eq!(frame.len(), 308);
        assert_eq!(checksum(&[0xff, 0xff, 0xff]), [0xfd, 0xfa]);
    }

    #[test]
    fn frames_are_found_in_a_stream_of_nmea_and_split_reads() {
        let mut p = Parser::default();
        let frames = p.push(&build(CLASS_MON, ID_MON_VER, &real_version()));
        assert_eq!(
            (frames.len(), frames[0].class, frames[0].id),
            (1, CLASS_MON, ID_MON_VER)
        );

        let stream = [
            b"$GNRMC,,V,,,,,,,,,,N,V*37\r\n".to_vec(),
            build(CLASS_MON, ID_MON_VER, &real_version()),
            b"$GNGGA,,,,,,0,00,99.99,,,,,,*56\r\n".to_vec(),
        ]
        .concat();
        assert_eq!(Parser::default().push(&stream).len(), 1);

        let frame = build(CLASS_MON, ID_MON_VER, &real_version());
        let mut p = Parser::default();
        assert!(p.push(&frame[..5]).is_empty());
        assert!(p.push(&frame[5..100]).is_empty());
        let done = p.push(&frame[100..]);
        assert_eq!(done[0].payload, real_version());

        let frame = build(CLASS_MON, ID_MON_VER, &[1, 2, 3]);
        let mut p = Parser::default();
        assert!(p.push(&frame[..1]).is_empty());
        assert_eq!(p.push(&frame[1..]).len(), 1);
    }

    #[test]
    fn broken_frames_are_skipped() {
        let mut bad = build(CLASS_MON, ID_MON_VER, &[1, 2, 3]);
        let last = bad.len() - 1;
        bad[last] ^= 0xff;
        let good = build(CLASS_MON, ID_MON_VER, &[4, 5, 6]);
        let frames = Parser::default().push(&[bad, good].concat());
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload, [4, 5, 6]);

        let lone = [vec![0xb5, 0x00, 0xb5, 0x01], build(0x0a, 0x04, &[])].concat();
        assert_eq!(Parser::default().push(&lone).len(), 1);

        let absurd = [
            vec![0xb5, 0x62, 0x0a, 0x04, 0xff, 0xff, 0x00, 0x00],
            build(0x0a, 0x04, &[9]),
        ];
        assert_eq!(Parser::default().push(&absurd.concat()).len(), 1);

        let mut p = Parser::default();
        p.push("$GNRMC,,V,,,,,,,,,,N,V*37\r\n".repeat(500).as_bytes());
        assert!(p.buffer.len() <= BUFFER_KEEP);
        assert_eq!(p.push(&build(0x0a, 0x04, &[1])).len(), 1);
    }

    #[test]
    fn the_identity_is_read_from_the_real_answer() {
        assert_eq!(
            parse_version(&real_version()),
            Some(Version {
                software: "ROM CORE 4.04 (d964f4)".into(),
                hardware: "00190000".into(),
                firmware: Some("SPG 4.04".into()),
                protocol: Some("32.01".into()),
                model: None,
                supported: Some(
                    ["GPS", "GLO", "GAL", "BDS", "SBAS", "QZSS"]
                        .map(String::from)
                        .to_vec()
                ),
            })
        );
        let named = [
            field("ROM CORE 4.04", 30),
            field("00190000", 10),
            field("MOD=NEO-M9N", 30),
        ];
        assert_eq!(
            parse_version(&named.concat()).unwrap().model.as_deref(),
            Some("NEO-M9N")
        );

        let odd = [
            field("ROM CORE 4.04", 30),
            field("00190000", 10),
            field("some vendor note", 30),
            field("", 30),
        ];
        let version = parse_version(&odd.concat()).unwrap();
        assert_eq!(
            (version.supported, version.software.as_str()),
            (None, "ROM CORE 4.04")
        );

        assert_eq!(parse_version(&[0u8; 20]), None);
        let partial = [field("ROM", 30), field("HW", 10), vec![0u8; 12]].concat();
        assert_eq!(parse_version(&partial).unwrap().software, "ROM");
        let full = ["A".repeat(30).into_bytes(), field("00190000", 10)].concat();
        assert_eq!(parse_version(&full).unwrap().software, "A".repeat(30));
        let padded = [b" ROM\xa0".to_vec(), vec![0; 25], field("HW", 10)].concat();
        assert_eq!(parse_version(&padded).unwrap().software, "ROM");
    }

    #[test]
    fn the_rf_front_end_is_read() {
        assert_eq!(
            parse_rf(&rf_payload(1, 2, 1)),
            Some(Rf {
                jamming: Jamming::Ok,
                antenna_status: AntennaStatus::Ok,
                antenna_power: AntennaPower::On,
                noise: 87,
                agc: 4321,
                jamming_indicator: 12,
            })
        );
        let status = |raw| parse_rf(&rf_payload(1, raw, 1)).unwrap().antenna_status;
        let statuses = [0, 1, 2, 3, 4, 9].map(status);
        use AntennaStatus as S;
        assert_eq!(
            statuses,
            [S::Init, S::Unknown, S::Ok, S::Short, S::Open, S::Unknown]
        );
        let power = |raw| parse_rf(&rf_payload(1, 2, raw)).unwrap().antenna_power;
        use AntennaPower as P;
        assert_eq!(
            [0, 1, 2, 9].map(power),
            [P::Off, P::On, P::Unknown, P::Unknown]
        );
        let jamming = |raw| parse_rf(&rf_payload(raw, 2, 1)).unwrap().jamming;
        use Jamming as J;
        assert_eq!(
            [0, 1, 2, 3, 0xfe].map(jamming),
            [J::Unknown, J::Ok, J::Warning, J::Critical, J::Warning]
        );
        assert_eq!(parse_rf(&[0u8; 20]), None);
    }
}
