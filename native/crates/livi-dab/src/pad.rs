// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::BTreeMap;

use crate::charset;
use crate::fec::crc16_ok;

const SUBFIELD_LEN: [usize; 8] = [4, 6, 8, 12, 16, 24, 32, 48];
const APP_DLS_START: u8 = 2;
const APP_DLS_CONT: u8 = 3;

pub fn pad_of_unit(au: &[u8]) -> Option<&[u8]> {
    if au.len() < 2 || au[0] >> 5 != 4 {
        return None;
    }
    let (count, start) = match au[1] {
        255 => (255 + *au.get(2)? as usize, 3),
        n => (n as usize, 2),
    };
    au.get(start..start + count)
}

#[derive(Default)]
pub struct PadDecoder {
    last_app: u8,
    dls: Vec<u8>,
    dls_segments: BTreeMap<u8, Vec<u8>>,
    dls_charset: u8,
    dls_toggle: Option<bool>,
    label: Option<String>,
}

impl PadDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process(&mut self, pad: &[u8]) -> Vec<String> {
        let mut events = Vec::new();
        if pad.len() < 2 {
            return events;
        }
        let (xpad, fpad) = pad.split_at(pad.len() - 2);
        let xpad: Vec<u8> = xpad.iter().rev().copied().collect();
        let indicator = (fpad[0] >> 4) & 0x03;
        let has_ci = fpad[1] & 0x02 != 0;
        match indicator {
            1 if xpad.len() >= 4 => {
                if has_ci {
                    let app = xpad[0] & 0x1f;
                    self.subfield(app, &xpad[1..4], &mut events);
                } else {
                    self.subfield(continuation(self.last_app), &xpad[..4], &mut events);
                }
            }
            2 => {
                if has_ci {
                    let mut fields = Vec::new();
                    let mut i = 0;
                    while i < xpad.len() && fields.len() < 4 {
                        let ci = xpad[i];
                        i += 1;
                        if ci & 0x1f == 0 {
                            break;
                        }
                        fields.push((ci & 0x1f, SUBFIELD_LEN[(ci >> 5) as usize]));
                    }
                    for (app, len) in fields {
                        let Some(data) = xpad.get(i..i + len) else {
                            break;
                        };
                        self.subfield(app, data, &mut events);
                        i += len;
                    }
                } else if !xpad.is_empty() {
                    self.subfield(continuation(self.last_app), &xpad, &mut events);
                }
            }
            _ => {}
        }
        events
    }

    fn subfield(&mut self, app: u8, data: &[u8], events: &mut Vec<String>) {
        self.last_app = app;
        match app {
            APP_DLS_START => {
                self.dls = data.to_vec();
                self.try_dls(events);
            }
            APP_DLS_CONT if !self.dls.is_empty() => {
                self.dls.extend_from_slice(data);
                self.try_dls(events);
            }
            _ => {}
        }
    }

    fn try_dls(&mut self, events: &mut Vec<String>) {
        let b = &self.dls;
        if b.len() < 2 {
            return;
        }
        let command = b[0] & 0x10 != 0;
        let total = if command {
            match b[0] & 0x0f {
                0x02 => 2 + (b[1] & 0x0f) as usize + 1 + 2,
                _ => 4,
            }
        } else {
            2 + (b[0] & 0x0f) as usize + 1 + 2
        };
        if b.len() < total {
            return;
        }
        let group = std::mem::take(&mut self.dls);
        let group = &group[..total];
        if !crc16_ok(group) {
            return;
        }
        if command {
            if group[0] & 0x0f == 0x01 {
                self.dls_segments.clear();
                self.label = None;
            }
            return;
        }
        let toggle = group[0] & 0x80 != 0;
        let first = group[0] & 0x40 != 0;
        let last = group[0] & 0x20 != 0;
        if self.dls_toggle != Some(toggle) {
            self.dls_toggle = Some(toggle);
            self.dls_segments.clear();
        }
        let number = if first {
            self.dls_charset = group[1] >> 4;
            0
        } else {
            (group[1] >> 4) & 0x07
        };
        self.dls_segments
            .insert(number, group[2..total - 2].to_vec());
        if last {
            self.dls_segments.insert(0x80 | number, Vec::new());
        }
        let Some(&end) = self.dls_segments.keys().find(|&&k| k & 0x80 != 0) else {
            return;
        };
        let end = end & 0x7f;
        if !(0..=end).all(|n| self.dls_segments.contains_key(&n)) {
            return;
        }
        let bytes: Vec<u8> = (0..=end)
            .flat_map(|n| self.dls_segments[&n].clone())
            .collect();
        let text = charset::decode(self.dls_charset, &bytes);
        if !text.is_empty() && self.label.as_deref() != Some(text.as_str()) {
            self.label = Some(text.clone());
            events.push(text);
        }
    }
}

fn continuation(app: u8) -> u8 {
    if app == APP_DLS_START {
        APP_DLS_CONT
    } else {
        app
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fec::crc16;

    fn with_crc(mut v: Vec<u8>) -> Vec<u8> {
        let c = crc16(&v) ^ 0xffff;
        v.extend_from_slice(&c.to_be_bytes());
        v
    }

    fn pad(fields: &[(u8, &[u8])]) -> Vec<u8> {
        let mut x = Vec::new();
        for (app, data) in fields {
            let idx = SUBFIELD_LEN.iter().position(|&l| l == data.len()).unwrap() as u8;
            x.push((idx << 5) | app);
        }
        if fields.len() < 4 {
            x.push(0);
        }
        for (_, data) in fields {
            x.extend_from_slice(data);
        }
        let mut out: Vec<u8> = x.into_iter().rev().collect();
        out.extend_from_slice(&[0x20, 0x02]);
        out
    }

    #[test]
    fn dls_in_one_group() {
        let text = b"Hallo Welt";
        let mut g = vec![0x40 | 0x20 | (text.len() as u8 - 1), 0x00];
        g.extend_from_slice(text);
        let mut g = with_crc(g);
        g.resize(16, 0);
        let mut d = PadDecoder::new();
        let ev = d.process(&pad(&[(APP_DLS_START, &g)]));
        assert_eq!(ev, vec!["Hallo Welt".to_string()]);
        assert!(d.process(&pad(&[(APP_DLS_START, &g)])).is_empty());
    }

    #[test]
    fn dls_split_over_two_pads() {
        let text = b"Dlf Nachrichten";
        let mut g = vec![0x60 | (text.len() as u8 - 1), 0x00];
        g.extend_from_slice(text);
        let g = with_crc(g);
        let mut d = PadDecoder::new();
        assert!(d.process(&pad(&[(APP_DLS_START, &g[..12])])).is_empty());
        let mut rest = g[12..].to_vec();
        rest.resize(8, 0);
        let ev = d.process(&pad(&[(APP_DLS_CONT, &rest)]));
        assert_eq!(ev, vec!["Dlf Nachrichten".to_string()]);
    }

    #[test]
    fn unit_pad_extraction() {
        let au = [0x80, 0x03, 1, 2, 3, 9, 9];
        assert_eq!(pad_of_unit(&au), Some(&[1u8, 2, 3][..]));
        assert_eq!(pad_of_unit(&[0x20, 0x03]), None);
    }
}
