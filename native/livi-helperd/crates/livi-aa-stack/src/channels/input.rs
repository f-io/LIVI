//! The input channel (8), head unit to phone. Reports are stamped in microseconds.

use livi_aa_proto::{
    InputReportNotification, KeyEvent, KeyEventBatch, Keycode, RelativeEvent, RelativeEventBatch,
    TouchEvent,
};

use crate::channels::Frame;
use crate::codec::encode;
use crate::consts::{ch, frame_flags, input_msg};

/// In the advertised touchscreen's pixels, the id stable from down to up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchPointer {
    pub x: i64,
    pub y: i64,
    pub id: i64,
}

fn report(report: InputReportNotification) -> Frame {
    Frame::new(ch::INPUT, frame_flags::ENC_SIGNAL, input_msg::INPUT_REPORT, encode(&report))
}

/// `action_index` names the pointer behind a pointer-down or pointer-up.
pub fn touch(
    ts_micros: u64,
    action: u32,
    pointers: &[TouchPointer],
    action_index: u32,
) -> Option<Frame> {
    if pointers.is_empty() {
        return None;
    }
    let pointers = pointers
        .iter()
        .map(|p| livi_aa_proto::TouchPointer {
            x: p.x as u32,
            y: p.y as u32,
            pointer_id: p.id as u32,
        })
        .collect();
    Some(report(InputReportNotification {
        timestamp_us: ts_micros,
        touchscreen_event: Some(TouchEvent {
            pointers,
            action_index: Some(action_index),
            action: Some(action as i32),
        }),
        ..Default::default()
    }))
}

/// `direction` is -1 back and 1 on.
pub fn rotary(ts_micros: u64, direction: i64) -> Frame {
    let turn = RelativeEvent { keycode: Keycode::RotaryController as u32, delta: direction as i32 };
    report(InputReportNotification {
        timestamp_us: ts_micros,
        relative_event_batch: Some(RelativeEventBatch { relative_events: vec![turn] }),
        ..Default::default()
    })
}

/// Several codes ride one event, the phone picks whichever it understands where
/// the focus is. Every key field is written, some phones want longpress even when false.
pub fn button(ts_micros: u64, codes: &[u32], down: bool, longpress: bool) -> Option<Frame> {
    if codes.is_empty() {
        return None;
    }
    let key_events = codes
        .iter()
        .map(|&keycode| KeyEvent { keycode, down, meta_state: 0, long_press: Some(longpress) })
        .collect();
    Some(report(InputReportNotification {
        timestamp_us: ts_micros,
        key_event_batch: Some(KeyEventBatch { key_events }),
        ..Default::default()
    }))
}

#[cfg(test)]
mod tests {
    use livi_aa_proto::TouchAction;

    use super::*;

    #[test]
    fn reports_carry_the_timestamp_and_the_event() {
        let f = touch(1000, TouchAction::Down as u32, &[TouchPointer { x: 10, y: 20, id: 0 }], 0)
            .unwrap();
        assert_eq!(f.ch, ch::INPUT);
        assert_eq!(f.msg_id, input_msg::INPUT_REPORT);
        assert_eq!(
            f.payload,
            [
                0x08, 0xe8, 0x07, 0x1a, 0x0c, 0x0a, 0x06, 0x08, 0x0a, 0x10, 0x14, 0x18, 0x00, 0x10,
                0x00, 0x18, 0x00
            ]
        );
        assert!(touch(1, 0, &[], 0).is_none());
        assert!(button(1, &[], true, false).is_none());
        let b = button(0, &[Keycode::Home as u32], true, false).unwrap();
        assert_eq!(
            b.payload,
            [0x08, 0x00, 0x22, 0x0a, 0x0a, 0x08, 0x08, 0x03, 0x10, 0x01, 0x18, 0x00, 0x20, 0x00]
        );
        let mut rel = vec![0x08, 0x80, 0x80, 0x04, 0x10];
        rel.extend([0xff; 9]);
        rel.push(0x01);
        let mut expected = vec![0x08, 0x00, 0x32, 0x11, 0x0a, 0x0f];
        expected.extend(rel);
        assert_eq!(rotary(0, -1).payload, expected);
    }
}
