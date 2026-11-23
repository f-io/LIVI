// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use iap2_link::*;

const DEVICE_PSN: u8 = 0x2A;

fn frame(control: u8, seq: u8, ack: u8, session_id: u8, payload: Option<&[u8]>) -> Vec<u8> {
    let length = match payload {
        Some(p) => (HEADER_LEN + p.len() + 1) as u16,
        None => HEADER_LEN as u16,
    };
    let header = FrameHeader {
        length,
        control,
        seq,
        ack,
        session_id,
    };
    let mut out = header.to_bytes().to_vec();
    if let Some(p) = payload {
        out.extend_from_slice(p);
        out.push(checksum(p));
    }
    out
}

fn data(seq: u8, ack: u8, session_id: u8, payload: &[u8]) -> Vec<u8> {
    frame(CONTROL_ACK, seq, ack, session_id, Some(payload))
}

fn device_params() -> SyncParams {
    SyncParams {
        max_outstanding: 8,
        max_packet_len: 1024,
        retransmit_timeout_ms: 1000,
        ack_timeout_ms: 100,
        max_retransmits: 3,
        max_cumulative_acks: 2,
        sessions: vec![SessionInfo {
            id: CONTROL_SESSION_ID,
            kind: 0,
            version: 1,
        }],
    }
}

fn handshaken_with(params: SyncParams) -> LinkEngine {
    let mut e = LinkEngine::new(LinkConfig::default());
    e.start(false, 0);
    e.feed(&DETECT_MARKER, 0);
    e.feed(
        &frame(CONTROL_SYN, DEVICE_PSN, 0, 0, Some(&params.to_bytes())),
        0,
    );
    e.feed(&frame(CONTROL_ACK, DEVICE_PSN, 0, 0, None), 0);
    assert_eq!(e.state(), LinkState::Normal);
    e.take_output();
    while e.poll_event().is_some() {}
    e
}

fn handshaken() -> LinkEngine {
    handshaken_with(device_params())
}

fn events(e: &mut LinkEngine) -> Vec<Event> {
    std::iter::from_fn(|| e.poll_event()).collect()
}

fn split_frames(mut bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    while !bytes.is_empty() {
        let len = if bytes.starts_with(&DETECT_MARKER) {
            DETECT_MARKER.len()
        } else {
            u16::from_be_bytes([bytes[2], bytes[3]]) as usize
        };
        out.push(bytes[..len].to_vec());
        bytes = &bytes[len..];
    }
    out
}

#[test]
fn header_pack_matches_reference() {
    let h = FrameHeader {
        length: 9,
        control: 0x40,
        seq: 0x63,
        ack: 0x05,
        session_id: 0,
    };
    assert_eq!(h.to_bytes().to_vec(), hex("ff5a000940630500f6"));
    assert_eq!(FrameHeader::parse(&h.to_bytes()), Some(h));
}

#[test]
fn default_params_round_trip() {
    let e = LinkEngine::new(LinkConfig::default());
    let packed = e.local_params().to_bytes();
    assert_eq!(packed, hex("0104ffff05dc006405020a00010b02010c0102"));
    assert_eq!(SyncParams::parse(&packed).as_ref(), Some(e.local_params()));
}

#[test]
fn zero_ack_config_clears_reliability_fields() {
    let e = LinkEngine::new(LinkConfig {
        zero_ack: true,
        ..LinkConfig::default()
    });
    let p = e.local_params();
    assert_eq!(
        (
            p.retransmit_timeout_ms,
            p.ack_timeout_ms,
            p.max_retransmits,
            p.max_cumulative_acks
        ),
        (0, 0, 0, 0)
    );
}

#[test]
fn initiator_start_emits_marker_and_syn() {
    let mut e = LinkEngine::new(LinkConfig::default());
    e.start(true, 0);
    let mut expected = DETECT_MARKER.to_vec();
    expected.extend(frame(
        CONTROL_SYN,
        0,
        0,
        0,
        Some(&e.local_params().to_bytes()),
    ));
    assert_eq!(e.take_output(), expected);
    assert_eq!(e.state(), LinkState::Negotiate);
}

#[test]
fn passive_handshake_reaches_normal() {
    let mut e = LinkEngine::new(LinkConfig::default());
    e.start(false, 0);
    assert_eq!(e.take_output(), DETECT_MARKER.to_vec());

    e.feed(&DETECT_MARKER, 0);
    assert_eq!(e.state(), LinkState::Negotiate);
    let syn = e.take_output();
    assert_eq!(syn[4], CONTROL_SYN);

    e.feed(
        &frame(
            CONTROL_SYN,
            DEVICE_PSN,
            0,
            0,
            Some(&device_params().to_bytes()),
        ),
        0,
    );
    assert_eq!(e.link_params(), &device_params());
    assert_eq!(e.take_output(), frame(CONTROL_ACK, 0, DEVICE_PSN, 0, None));

    e.feed(&frame(CONTROL_ACK, DEVICE_PSN, 0, 0, None), 0);
    assert_eq!(e.state(), LinkState::Normal);
    assert!(e.writable());
    assert_eq!(e.poll_event(), Some(Event::Writable(true)));
}

#[test]
fn ack_for_another_psn_does_not_finish_negotiation() {
    let mut e = LinkEngine::new(LinkConfig::default());
    e.start(true, 0);
    e.feed(&frame(CONTROL_ACK, DEVICE_PSN, 77, 0, None), 0);
    assert_eq!(e.state(), LinkState::Negotiate);
}

#[test]
fn repeated_syn_after_handshake_is_acked_again() {
    let mut e = handshaken();
    e.feed(
        &frame(
            CONTROL_SYN,
            DEVICE_PSN,
            0,
            0,
            Some(&device_params().to_bytes()),
        ),
        0,
    );
    assert_eq!(e.take_output(), frame(CONTROL_ACK, 0, DEVICE_PSN, 0, None));
    assert_eq!(e.state(), LinkState::Normal);
}

#[test]
fn syn_is_resent_then_gives_up() {
    let mut e = LinkEngine::new(LinkConfig::default());
    e.start(true, 0);
    let first = e.take_output();
    e.advance_time(500);
    assert_eq!(e.take_output(), first[DETECT_MARKER.len()..]);
    for t in 2..=21 {
        e.advance_time(t * 500);
    }
    assert_eq!(e.state(), LinkState::Dead);
    assert!(events(&mut e).contains(&Event::Dead {
        reason: Some(LinkError::NegotiationTimeout)
    }));
}

#[test]
fn wrong_marker_dies() {
    let mut e = LinkEngine::new(LinkConfig::default());
    e.start(false, 0);
    e.feed(b"\x00\x00\x00\x00\x00\x00", 0);
    assert_eq!(e.state(), LinkState::Dead);
    assert_eq!(
        e.poll_event(),
        Some(Event::Dead {
            reason: Some(LinkError::NotIap2)
        })
    );
}

#[test]
fn retransmits_max_times_then_dies() {
    let mut e = handshaken();
    e.send(CONTROL_SESSION_ID, vec![1, 2, 3], 0);
    let first = e.take_output();
    assert_eq!(first, data(1, DEVICE_PSN, CONTROL_SESSION_ID, &[1, 2, 3]));
    assert_eq!(e.next_deadline(), Some(1000));

    for t in [1000, 2000, 3000] {
        e.advance_time(t);
        assert_eq!(e.take_output(), first, "retransmit at {t}");
    }
    assert_eq!(e.state(), LinkState::Normal);
    e.advance_time(4000);
    assert_eq!(e.state(), LinkState::Dead);
    assert!(events(&mut e).contains(&Event::Dead {
        reason: Some(LinkError::Unacknowledged { psn: 1 })
    }));
}

#[test]
fn ack_clears_retransmit() {
    let mut e = handshaken();
    e.send(CONTROL_SESSION_ID, vec![1], 0);
    e.take_output();
    e.feed(&frame(CONTROL_ACK, DEVICE_PSN, 1, 0, None), 10);
    assert_eq!(e.next_deadline(), None);
    e.advance_time(5000);
    assert_eq!(e.state(), LinkState::Normal);
}

#[test]
fn window_is_exactly_max_outstanding() {
    let mut e = handshaken();
    for i in 0..10u8 {
        e.send(CONTROL_SESSION_ID, vec![i], 0);
    }
    assert_eq!(split_frames(&e.take_output()).len(), 8);
    assert!(!e.writable());

    e.feed(&frame(CONTROL_ACK, DEVICE_PSN, 8, 0, None), 10);
    assert_eq!(split_frames(&e.take_output()).len(), 2);
    assert!(e.writable());
}

#[test]
fn partial_ack_keeps_the_rest_in_flight() {
    let mut e = handshaken();
    for i in 0..8u8 {
        e.send(CONTROL_SESSION_ID, vec![i], 0);
    }
    e.take_output();
    e.feed(&frame(CONTROL_ACK, DEVICE_PSN, 2, 0, None), 10);
    e.advance_time(1000);
    let resent = split_frames(&e.take_output());
    let psns: Vec<u8> = resent.iter().map(|f| f[5]).collect();
    assert_eq!(psns, [3, 4, 5, 6, 7, 8]);
}

#[test]
fn stale_ack_is_ignored() {
    let mut e = handshaken();
    e.send(CONTROL_SESSION_ID, vec![1], 0);
    e.take_output();
    e.feed(&frame(CONTROL_ACK, DEVICE_PSN, 200, 0, None), 10);
    assert_eq!(e.next_deadline(), Some(1000));
}

#[test]
fn eak_from_peer_retransmits_listed_packets() {
    let mut e = handshaken();
    for i in 0..3u8 {
        e.send(CONTROL_SESSION_ID, vec![i], 0);
    }
    e.take_output();
    e.feed(
        &frame(CONTROL_ACK | CONTROL_EAK, DEVICE_PSN, 1, 0, Some(&[3])),
        10,
    );
    assert_eq!(
        e.take_output(),
        data(3, DEVICE_PSN, CONTROL_SESSION_ID, &[2])
    );
}

#[test]
fn large_payload_is_split_to_peer_max_packet_len() {
    let mut e = handshaken_with(SyncParams {
        max_packet_len: 20,
        ..device_params()
    });
    let msg: Vec<u8> = (0..25).collect();
    e.send(CONTROL_SESSION_ID, msg.clone(), 0);
    let frames = split_frames(&e.take_output());
    let sizes: Vec<usize> = frames.iter().map(|f| f.len() - HEADER_LEN - 1).collect();
    assert_eq!(sizes, [10, 10, 5]);
    let joined: Vec<u8> = frames
        .iter()
        .flat_map(|f| f[HEADER_LEN..f.len() - 1].to_vec())
        .collect();
    assert_eq!(joined, msg);
}

#[test]
fn delayed_ack_then_cumulative_ack() {
    let mut e = handshaken();
    e.feed(&data(0x2B, 0, CONTROL_SESSION_ID, b"abc"), 0);
    assert_eq!(e.poll_event(), Some(Event::Control(b"abc".to_vec())));
    assert!(e.take_output().is_empty());
    assert_eq!(e.next_deadline(), Some(100));
    e.advance_time(100);
    assert_eq!(e.take_output(), frame(CONTROL_ACK, 0, 0x2B, 0, None));

    e.feed(&data(0x2C, 0, CONTROL_SESSION_ID, b"d"), 200);
    e.feed(&data(0x2D, 0, CONTROL_SESSION_ID, b"e"), 200);
    assert_eq!(e.take_output(), frame(CONTROL_ACK, 0, 0x2D, 0, None));
    assert_eq!(e.next_deadline(), None);
}

#[test]
fn out_of_order_reassembly_sends_eak() {
    let mut e = handshaken();
    e.feed(&data(0x2D, 0, CONTROL_SESSION_ID, b"three"), 0);
    assert_eq!(e.poll_event(), None);
    assert_eq!(
        e.take_output(),
        frame(
            CONTROL_ACK | CONTROL_EAK,
            0,
            DEVICE_PSN,
            0,
            Some(&[0x2B, 0x2C])
        )
    );
    e.feed(&data(0x2C, 0, CONTROL_SESSION_ID, b"two"), 0);
    e.feed(&data(0x2B, 0, CONTROL_SESSION_ID, b"one"), 0);
    assert_eq!(
        events(&mut e),
        [
            Event::Control(b"one".to_vec()),
            Event::Control(b"two".to_vec()),
            Event::Control(b"three".to_vec()),
        ]
    );
}

#[test]
fn repeated_out_of_order_packet_is_delivered_once() {
    let mut e = handshaken();
    e.feed(&data(0x2C, 0, CONTROL_SESSION_ID, b"two"), 0);
    e.feed(&data(0x2C, 0, CONTROL_SESSION_ID, b"two"), 0);
    e.feed(&data(0x2B, 0, CONTROL_SESSION_ID, b"one"), 0);
    e.feed(&data(0x2C, 0, CONTROL_SESSION_ID, b"two"), 0);
    assert_eq!(
        events(&mut e),
        [
            Event::Control(b"one".to_vec()),
            Event::Control(b"two".to_vec())
        ]
    );
}

#[test]
fn duplicate_in_order_packet_is_reacked() {
    let mut e = handshaken();
    e.feed(&data(0x2B, 0, CONTROL_SESSION_ID, b"x"), 0);
    e.poll_event();
    e.take_output();
    e.feed(&data(0x2B, 0, CONTROL_SESSION_ID, b"x"), 10);
    assert_eq!(e.poll_event(), None);
    assert_eq!(e.take_output(), frame(CONTROL_ACK, 0, 0x2B, 0, None));
}

#[test]
fn ea_stream_demux_and_send() {
    let mut e = handshaken();
    e.feed(&data(0x2B, 0, EA_SESSION_ID, &[0x00, 0x07, 0xAB]), 0);
    assert_eq!(
        e.poll_event(),
        Some(Event::Ea {
            stream_id: 7,
            data: vec![0xAB]
        })
    );
    e.send_ea(7, &[0xCD], 10);
    assert_eq!(
        e.take_output(),
        data(1, 0x2B, EA_SESSION_ID, &[0x00, 0x07, 0xCD])
    );
}

#[test]
fn file_transfer_session_is_delivered() {
    let mut e = handshaken();
    e.feed(&data(0x2B, 0, FILE_TRANSFER_SESSION_ID, &[1, 2]), 0);
    assert_eq!(e.poll_event(), Some(Event::FileTransfer(vec![1, 2])));
}

#[test]
fn reset_frame_dies() {
    let mut e = handshaken();
    e.feed(&frame(CONTROL_RST, 0, 0, 0, None), 0);
    assert_eq!(e.state(), LinkState::Dead);
}

#[test]
fn resync_skips_garbage_and_false_start() {
    let mut e = handshaken();
    let mut noisy = vec![0xDE, 0xAD, 0xFF, 0x5A, 0x00, 0x01, 0xBE, 0xEF];
    noisy.extend(data(0x2B, 0, CONTROL_SESSION_ID, b"x"));
    e.feed(&noisy[..5], 0);
    e.feed(&noisy[5..], 0);
    assert_eq!(e.poll_event(), Some(Event::Control(b"x".to_vec())));
}

#[test]
fn zero_ack_link_does_not_track_or_ack() {
    let zero = SyncParams {
        retransmit_timeout_ms: 0,
        ack_timeout_ms: 0,
        max_retransmits: 0,
        max_cumulative_acks: 0,
        ..device_params()
    };
    let mut e = handshaken_with(zero);
    for i in 0..20u8 {
        e.send(CONTROL_SESSION_ID, vec![i], 0);
    }
    assert_eq!(split_frames(&e.take_output()).len(), 20);
    assert_eq!(e.next_deadline(), None);
    e.feed(&data(0x2B, 0, CONTROL_SESSION_ID, b"x"), 0);
    assert!(e.take_output().is_empty());
}

struct Lossy(u64);

impl Lossy {
    fn roll(&mut self, percent: u64) -> bool {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        (self.0 >> 33) % 100 < percent
    }

    fn carry(&mut self, bytes: &[u8], held: &mut Vec<Vec<u8>>) -> Vec<u8> {
        let mut out = Vec::new();
        for f in split_frames(bytes) {
            if f.starts_with(&DETECT_MARKER) {
                out.extend(f);
                continue;
            }
            if self.roll(10) {
                continue;
            }
            if self.roll(5) {
                out.extend(&f);
            }
            if self.roll(10) {
                held.push(f);
                continue;
            }
            out.extend(f);
            out.extend(held.drain(..).flatten());
        }
        out
    }
}

#[test]
fn lossy_loopback_delivers_everything_once_in_order() {
    for seed in 1..=50 {
        loopback(seed);
    }
}

fn loopback(seed: u64) {
    let cfg = LinkConfig {
        max_outstanding: 6,
        max_packet_len: 64,
        ..LinkConfig::default()
    };
    let mut a = LinkEngine::new(cfg.clone());
    let mut b = LinkEngine::new(LinkConfig {
        initial_psn: 200,
        ..cfg
    });
    a.start(false, 0);
    b.start(true, 0);

    let msgs: Vec<Vec<u8>> = (0..300u32)
        .map(|i| (0..(i % 120) as u8).map(|x| x ^ i as u8).collect())
        .filter(|m: &Vec<u8>| !m.is_empty())
        .collect();
    let expected: Vec<u8> = msgs.concat();

    let mut net = Lossy(seed);
    let (mut held_ab, mut held_ba) = (Vec::new(), Vec::new());
    let (mut got_a, mut got_b) = (Vec::new(), Vec::new());
    let mut sent = false;
    let mut t = 0;
    while t < 600_000 && (got_a.len() < expected.len() || got_b.len() < expected.len()) {
        t += 10;
        if !sent && a.state() == LinkState::Normal && b.state() == LinkState::Normal {
            for m in &msgs {
                a.send(CONTROL_SESSION_ID, m.clone(), t);
                b.send(CONTROL_SESSION_ID, m.clone(), t);
            }
            sent = true;
        }
        let ab = net.carry(&a.take_output(), &mut held_ab);
        let ba = net.carry(&b.take_output(), &mut held_ba);
        b.feed(&ab, t);
        a.feed(&ba, t);
        a.advance_time(t);
        b.advance_time(t);
        for (e, got) in [(&mut a, &mut got_a), (&mut b, &mut got_b)] {
            while let Some(ev) = e.poll_event() {
                match ev {
                    Event::Control(d) => got.extend(d),
                    Event::Dead { reason } => panic!("seed {seed}: link died: {reason:?}"),
                    _ => {}
                }
            }
        }
    }
    assert_eq!(got_a, expected);
    assert_eq!(got_b, expected);
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
