use iap2_link::*;

fn frame(control: u8, seq: u8, ack: u8, session_id: u8, payload: Option<&[u8]>) -> Vec<u8> {
    let length = match payload {
        Some(p) => p.len() as u16 + 10,
        None => 9,
    };
    let header = LinkPacketHeader { length, control, seq, ack, session_id };
    let mut out = header.pack().to_vec();
    if let Some(p) = payload {
        out.extend_from_slice(p);
        out.push(gen_checksum(p));
    }
    out
}

fn device_lsp() -> LinkSynchronizationPayload {
    LinkSynchronizationPayload {
        max_outgoing: 8,
        max_len: 1024,
        retransmission_timeout: 1000,
        ack_timeout: 100,
        max_retransmissions: 3,
        max_ack: 2,
        sessions: vec![LspSession { id: 10, kind: 0, version: 1 }],
    }
}

fn handshaken() -> LinkEngine {
    let mut e = LinkEngine::new(LinkConfig::default());
    e.start(false, 0);
    e.feed(&IAP2_MARKER, 0);
    e.feed(&frame(0x80, 0x2A, 0, 0, Some(&device_lsp().pack())), 0);
    e.feed(&frame(0x40, 0, 99, 0, None), 0);
    e.take_output();
    while e.poll_event().is_some() {}
    e
}

#[test]
fn header_pack_matches_reference() {
    let h = LinkPacketHeader { length: 9, control: 0x40, seq: 0x63, ack: 0x05, session_id: 0 };
    assert_eq!(h.pack().to_vec(), hex("ff5a000940630500f6"));
    assert_eq!(LinkPacketHeader::parse(&h.pack()), Some(h));
}

#[test]
fn lsp_pack_matches_reference() {
    let e = LinkEngine::new(LinkConfig::default());
    assert_eq!(e.lsp.pack(), hex("011effff0fa001f404030a00010b02010c0102"));
    assert_eq!(LinkSynchronizationPayload::parse(&e.lsp.pack()), Some(e.lsp.clone()));
}

#[test]
fn initiator_start_emits_marker_and_syn() {
    let mut e = LinkEngine::new(LinkConfig::default());
    e.start(true, 0);
    let mut expected = IAP2_MARKER.to_vec();
    expected.extend_from_slice(&hex(
        "ff5a001d80630000a7011effff0fa001f404030a00010b02010c010210",
    ));
    assert_eq!(e.take_output(), expected);
    assert_eq!(e.state(), LinkState::Negotiate);
}

#[test]
fn passive_handshake_reaches_normal() {
    let mut e = LinkEngine::new(LinkConfig::default());
    e.start(false, 0);
    assert_eq!(e.take_output(), IAP2_MARKER.to_vec());

    e.feed(&IAP2_MARKER, 0);
    assert_eq!(e.state(), LinkState::Negotiate);
    let out = e.take_output();
    assert_eq!(out[..2], [0xFF, 0x5A]);
    assert_eq!(out[4], 0x80);

    e.feed(&frame(0x80, 0x2A, 0, 0, Some(&device_lsp().pack())), 0);
    assert_eq!(e.lsp, device_lsp());
    let ack = e.take_output();
    assert_eq!(ack, frame(0x40, 99, 0x2A, 0, None));

    e.feed(&frame(0x40, 0, 99, 0, None), 0);
    assert_eq!(e.state(), LinkState::Normal);
    assert!(e.writable());
    assert_eq!(e.poll_event(), Some(Event::Writable(true)));
}

#[test]
fn wrong_marker_dies() {
    let mut e = LinkEngine::new(LinkConfig::default());
    e.start(false, 0);
    e.feed(b"\x00\x00\x00\x00\x00\x00", 0);
    assert_eq!(e.state(), LinkState::Dead);
    assert!(matches!(e.poll_event(), Some(Event::Dead { reason: Some(_) })));
}

#[test]
fn send_data_and_retransmit_until_dead() {
    let mut e = handshaken();
    e.send(CONTROL_SESSION_ID, vec![1, 2, 3], 0);
    let first = e.take_output();
    assert_eq!(first, frame(0x40, 100, 0x2A, 10, Some(&[1, 2, 3])));
    assert_eq!(e.next_deadline(), Some(1000));

    e.advance_time(1000);
    assert_eq!(e.take_output(), first, "retransmit repeats the frame");
    e.advance_time(2000);
    assert_eq!(e.take_output(), first);
    e.advance_time(3000);
    assert_eq!(e.state(), LinkState::Dead);
    let mut dead = false;
    while let Some(ev) = e.poll_event() {
        dead |= matches!(ev, Event::Dead { .. });
    }
    assert!(dead);
}

#[test]
fn ack_clears_retransmit() {
    let mut e = handshaken();
    e.send(CONTROL_SESSION_ID, vec![1], 0);
    e.take_output();
    e.feed(&frame(0x40, 0, 100, 0, None), 10);
    assert_eq!(e.next_deadline(), None);
    e.advance_time(5000);
    assert_eq!(e.state(), LinkState::Normal);
}

#[test]
fn window_full_queues_and_resumes() {
    let mut e = handshaken();
    for i in 0..10u8 {
        e.send(CONTROL_SESSION_ID, vec![i], 0);
    }
    let sent = e.take_output();
    let frames = sent.chunks(9 + 1 + 1 + 1).count();
    assert_eq!(frames, 9, "window of 8 plus the first in-flight packet");
    assert!(!e.writable());

    e.feed(&frame(0x40, 0, 108, 0, None), 10);
    let resumed = e.take_output();
    assert!(!resumed.is_empty());
    assert!(e.writable());
}

#[test]
fn in_order_delivery_and_auto_ack() {
    let mut e = handshaken();
    e.feed(&frame(0x40, 0x2B, 99, 10, Some(b"abc")), 0);
    assert_eq!(e.poll_event(), Some(Event::Control(b"abc".to_vec())));
    let cumulative_ack = frame(0x40, 99, 0x2B, 0, None);
    assert_eq!(e.take_output(), cumulative_ack);
    assert_eq!(e.next_deadline(), Some(100), "delayed ack armed with device ack timeout");

    e.advance_time(100);
    assert_eq!(e.take_output(), cumulative_ack);
}

#[test]
fn out_of_order_reassembly() {
    let mut e = handshaken();
    e.feed(&frame(0x40, 0x2C, 99, 10, Some(b"two")), 0);
    assert_eq!(e.poll_event(), None);
    e.feed(&frame(0x40, 0x2B, 99, 10, Some(b"one")), 0);
    assert_eq!(e.poll_event(), Some(Event::Control(b"one".to_vec())));
    assert_eq!(e.poll_event(), Some(Event::Control(b"two".to_vec())));
}

#[test]
fn ea_stream_demux() {
    let mut e = handshaken();
    e.feed(&frame(0x40, 0x2B, 99, 11, Some(&[0x00, 0x07, 0xAB])), 0);
    assert_eq!(e.poll_event(), Some(Event::Ea { stream_id: 7, data: vec![0xAB] }));
    e.take_output();

    e.send_ea(7, &[0xCD], 200);
    let out = e.take_output();
    assert_eq!(out, frame(0x40, 100, 0x2B, 11, Some(&[0x00, 0x07, 0xCD])));
}

#[test]
fn reset_frame_dies() {
    let mut e = handshaken();
    e.feed(&frame(0x10, 0, 0, 0, None), 0);
    assert_eq!(e.state(), LinkState::Dead);
}

#[test]
fn resync_skips_garbage() {
    let mut e = handshaken();
    let mut noisy = vec![0xDE, 0xAD, 0xBE, 0xEF];
    noisy.extend_from_slice(&frame(0x40, 0x2B, 100, 10, Some(b"x")));
    e.feed(&noisy, 0);
    assert_eq!(e.poll_event(), Some(Event::Control(b"x".to_vec())));
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
