// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::bplist::{Value, dict};

pub const TOUCH_HID_UID: u32 = 0x2a2a_2a2a;
pub const KNOB_HID_UID: u32 = 0x2a2a_2a2b;
pub const MEDIA_HID_UID: u32 = 0x2a2a_2a2c;
pub const TELEPHONY_HID_UID: u32 = 0x2a2a_2a2d;
pub const CLUSTER_KNOB_HID_UID: u32 = 0x2a2a_2a2e;

pub const TOUCH_CONTACTS: usize = 2;
const BYTES_PER_FINGER: usize = 6;

fn finger_collection(x_max: u16, y_max: u16) -> Vec<u8> {
    let [x_lo, x_hi] = x_max.to_le_bytes();
    let [y_lo, y_hi] = y_max.to_le_bytes();
    vec![
        0x05, 0x0d, 0x09, 0x22, 0xa1, 0x02, 0x09, 0x38, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02, 0x15,
        0x00, 0x25, 0x01, 0x09, 0x33, 0x75, 0x01, 0x95, 0x01, 0x81, 0x02, 0x95, 0x07, 0x81, 0x03,
        0x05, 0x01, 0x26, x_lo, x_hi, 0x09, 0x30, 0x75, 0x10, 0x95, 0x01, 0x81, 0x02, 0x26, y_lo,
        y_hi, 0x09, 0x31, 0x81, 0x02, 0xc0,
    ]
}

fn multitouch_descriptor(x_max: u16, y_max: u16) -> Vec<u8> {
    let mut out = vec![0x05, 0x0d, 0x09, 0x04, 0xa1, 0x01];
    for _ in 0..TOUCH_CONTACTS {
        out.extend(finger_collection(x_max, y_max));
    }
    out.push(0xc0);
    out
}

const KNOB_DESCRIPTOR: &[u8] = &[
    0x05, 0x01, 0x09, 0x08, 0xa1, 0x01, 0x05, 0x09, 0x09, 0x01, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01,
    0x95, 0x01, 0x81, 0x02, 0x05, 0x0c, 0x0a, 0x23, 0x02, 0x0a, 0x24, 0x02, 0x95, 0x02, 0x81, 0x02,
    0x95, 0x05, 0x81, 0x01, 0x05, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x09, 0x30, 0x09, 0x31, 0x15, 0x81,
    0x25, 0x7f, 0x75, 0x08, 0x95, 0x02, 0x81, 0x02, 0xc0, 0x09, 0x38, 0x15, 0x81, 0x25, 0x7f, 0x75,
    0x08, 0x95, 0x01, 0x81, 0x06, 0xc0,
];

const MEDIA_DESCRIPTOR: &[u8] = &[
    0x05, 0x0c, 0x09, 0x01, 0xa1, 0x01, 0x15, 0x00, 0x25, 0x06, 0x05, 0x0c, 0x0a, 0x00, 0x00, 0x0a,
    0xb0, 0x00, 0x0a, 0xb1, 0x00, 0x0a, 0xcd, 0x00, 0x0a, 0xb5, 0x00, 0x0a, 0xb6, 0x00, 0x0a, 0x9e,
    0x02, 0x75, 0x08, 0x95, 0x01, 0x81, 0x00, 0xc0,
];

const TELEPHONY_DESCRIPTOR: &[u8] = &[
    0x05, 0x0b, 0x09, 0x07, 0xa1, 0x01, 0x15, 0x00, 0x25, 0x11, 0x05, 0x0b, 0x09, 0x00, 0x09, 0x20,
    0x09, 0x21, 0x09, 0x26, 0x09, 0x2f, 0x09, 0xb0, 0x09, 0xb1, 0x09, 0xb2, 0x09, 0xb3, 0x09, 0xb4,
    0x09, 0xb5, 0x09, 0xb6, 0x09, 0xb7, 0x09, 0xb8, 0x09, 0xb9, 0x09, 0xba, 0x09, 0xbb, 0x05, 0x07,
    0x09, 0x2a, 0x75, 0x08, 0x95, 0x01, 0x81, 0x00, 0xc0,
];

fn device_entry(uid: u32, name: &str, descriptor: Vec<u8>, display_uuid: &str) -> Value {
    dict([
        ("hidProductID", Value::Int(1)),
        ("hidVendorID", Value::Int(2)),
        ("hidCountryCode", Value::Int(0)),
        ("uuid", Value::String(format!("{uid:x}"))),
        ("name", Value::String(name.into())),
        ("displayUUID", Value::String(display_uuid.into())),
        ("hidDescriptor", Value::Data(descriptor)),
    ])
}

pub fn touch_device(x_max: u16, y_max: u16, display_uuid: &str) -> Value {
    device_entry(
        TOUCH_HID_UID,
        "LIVI Touchscreen",
        multitouch_descriptor(x_max, y_max),
        display_uuid,
    )
}

pub fn knob_device(display_uuid: &str) -> Value {
    device_entry(
        KNOB_HID_UID,
        "LIVI Knob",
        KNOB_DESCRIPTOR.to_vec(),
        display_uuid,
    )
}

pub fn cluster_knob_device(display_uuid: &str) -> Value {
    device_entry(
        CLUSTER_KNOB_HID_UID,
        "LIVI Cluster Knob",
        KNOB_DESCRIPTOR.to_vec(),
        display_uuid,
    )
}

pub fn media_device(display_uuid: &str) -> Value {
    device_entry(
        MEDIA_HID_UID,
        "LIVI Media",
        MEDIA_DESCRIPTOR.to_vec(),
        display_uuid,
    )
}

pub fn telephony_device(display_uuid: &str) -> Value {
    device_entry(
        TELEPHONY_HID_UID,
        "LIVI Telephony",
        TELEPHONY_DESCRIPTOR.to_vec(),
        display_uuid,
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    pub x: f64,
    pub y: f64,
    pub down: bool,
}

pub fn touch_report(contacts: &[Contact]) -> [u8; BYTES_PER_FINGER * TOUCH_CONTACTS] {
    let mut r = [0u8; BYTES_PER_FINGER * TOUCH_CONTACTS];
    for i in 0..TOUCH_CONTACTS {
        let off = i * BYTES_PER_FINGER;
        r[off] = i as u8;
        let Some(c) = contacts.get(i) else { continue };
        r[off + 1] = u8::from(c.down);
        r[off + 2..off + 4].copy_from_slice(&axis(c.x).to_le_bytes());
        r[off + 4..off + 6].copy_from_slice(&axis(c.y).to_le_bytes());
    }
    r
}

pub(crate) fn round_half_up(v: f64) -> f64 {
    (v + 0.5).floor()
}

fn axis(v: f64) -> u16 {
    round_half_up(v).clamp(0.0, f64::from(u16::MAX)) as u16
}

pub mod media_button {
    pub const NONE: u8 = 0;
    pub const PLAY: u8 = 1;
    pub const PAUSE: u8 = 2;
    pub const PLAY_PAUSE: u8 = 3;
    pub const NEXT: u8 = 4;
    pub const PREV: u8 = 5;
    pub const NAV_GUIDANCE: u8 = 6;
}

pub mod telephony_button {
    pub const NONE: u8 = 0;
    pub const HOOK_SWITCH: u8 = 1;
    pub const FLASH: u8 = 2;
    pub const DROP: u8 = 3;
    pub const MUTE: u8 = 4;
    pub const KEY0: u8 = 5;
    pub const STAR: u8 = 15;
    pub const POUND: u8 = 16;
    pub const DELETE: u8 = 17;
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct KnobState {
    pub select: bool,
    pub home: bool,
    pub back: bool,
    pub x: f64,
    pub y: f64,
    pub wheel: f64,
}

fn knob_axis(v: f64) -> u8 {
    (round_half_up(v).clamp(-127.0, 127.0) as i8) as u8
}

pub fn knob_report(s: KnobState) -> [u8; 4] {
    let buttons = u8::from(s.select) | (u8::from(s.home) << 1) | (u8::from(s.back) << 2);
    [buttons, knob_axis(s.x), knob_axis(s.y), knob_axis(s.wheel)]
}

pub fn media_report(index: u8) -> [u8; 1] {
    [index]
}

pub fn telephony_report(index: u8) -> [u8; 1] {
    [index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_descriptor_carries_the_display_size() {
        let Some(Value::Data(desc)) = touch_device(1280, 720, "main")
            .get("hidDescriptor")
            .cloned()
        else {
            panic!("no descriptor");
        };
        assert_eq!(desc.len(), 6 + 2 * 51 + 1);
        assert_eq!(&desc[6 + 32..6 + 35], &[0x26, 0x00, 0x05]);
        assert_eq!(&desc[6 + 43..6 + 46], &[0x26, 0xd0, 0x02]);
    }

    #[test]
    fn touch_report_fills_fixed_slots() {
        let r = touch_report(&[Contact {
            x: 100.4,
            y: 50.6,
            down: true,
        }]);
        assert_eq!(r, [0, 1, 100, 0, 51, 0, 1, 0, 0, 0, 0, 0]);
        let r = touch_report(&[
            Contact {
                x: -3.0,
                y: 70000.0,
                down: false,
            },
            Contact {
                x: 300.0,
                y: 2.0,
                down: true,
            },
        ]);
        assert_eq!(r, [0, 0, 0, 0, 0xff, 0xff, 1, 1, 0x2c, 1, 2, 0]);
    }

    #[test]
    fn knob_report_packs_buttons_and_clamps_axes() {
        let r = knob_report(KnobState {
            home: true,
            back: true,
            x: -300.0,
            y: 5.4,
            wheel: 1.0,
            ..Default::default()
        });
        assert_eq!(r, [0b110, 0x81, 5, 1]);
    }

    #[test]
    fn devices_name_their_uid_in_hex() {
        assert_eq!(
            knob_device("m").get("uuid").and_then(Value::as_str),
            Some("2a2a2a2b")
        );
        assert_eq!(media_report(media_button::NEXT), [4]);
        assert_eq!(telephony_report(telephony_button::KEY0 + 9), [14]);
    }
}
