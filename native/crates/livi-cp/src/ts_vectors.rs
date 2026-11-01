// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde_json::Value as Json;

use crate::bplist::{self, Value, dict};
use crate::control_cipher::ControlCipher;
use crate::crypto::{
    aes_ctr128, chacha_seal, ed25519_sign, hkdf_sha512, nonce_label, nonce64, sha1, sha256,
};
use crate::hid::{Contact, KnobState, knob_report, touch_device, touch_report};
use crate::identity::hex;
use crate::info::{DisplayConfig, Icon, InfoConfig, Insets, build};
use crate::rtsp::{Response, build_response, parse};
use crate::srp::SrpServer;
use crate::tlv8;

fn vectors() -> Json {
    serde_json::from_str(include_str!("ts_vectors.json")).expect("valid vector file")
}

fn check(name: &str, bytes: &[u8]) {
    let all = vectors();
    match &all[name] {
        Json::String(expected) => assert_eq!(&hex(bytes), expected, "{name}"),
        Json::Object(o) => {
            assert_eq!(
                bytes.len() as u64,
                o["len"].as_u64().unwrap(),
                "{name} length"
            );
            assert_eq!(
                hex(&sha256(&[bytes])),
                o["sha256"].as_str().unwrap(),
                "{name} content"
            );
        }
        _ => panic!("no vector {name}"),
    }
}

fn info_config() -> InfoConfig {
    InfoConfig {
        device_name: "Golf".into(),
        device_id: "AA:BB:CC:DD:EE:FF".into(),
        bt_mac: "11:22:33:44:55:66".into(),
        source_version: "950.7.1".into(),
        hevc: true,
        main: DisplayConfig {
            width_pixels: 1280,
            height_pixels: 720,
            width_physical_mm: Some(200),
            fps: Some(60),
            view_area: Some(Insets::default()),
            safe_area: Some(Insets {
                top: 0,
                bottom: 0,
                left: 10,
                right: 10,
            }),
            safe_area_draw_outside: Some(true),
            ..Default::default()
        },
        cluster: Some(DisplayConfig {
            width_pixels: 800,
            height_pixels: 480,
            fps: Some(30),
            width_physical_mm: Some(150),
            initial_url: Some("maps:/car/instrumentcluster".into()),
            ..Default::default()
        }),
        entertainment_sample_rate: 44100,
        disable_audio_output: false,
        oem_label: "App".into(),
        icons: vec![Icon {
            width_pixels: 120,
            height_pixels: 120,
            png: vec![0x89, 0x50, 0x4e, 0x47],
        }],
        right_hand_drive: true,
        theme_displays: None,
    }
}

#[test]
fn tlv8_matches() {
    check(
        "tlv8",
        &tlv8::encode(&[(6, &[1]), (3, &[7; 300]), (3, b"x"), (1, &[])]),
    );
}

#[test]
fn rtsp_matches() {
    let (reqs, _) = parse(b"GET /info RTSP/1.0\r\nCSeq: 5\r\n\r\n");
    let res = Response {
        headers: vec![
            (
                "Content-Type".into(),
                "application/x-apple-binary-plist".into(),
            ),
            ("CSeq".into(), "0".into()),
        ],
        body: b"ab".to_vec(),
        ..Default::default()
    };
    check("rtsp", &build_response(&reqs[0], res));
    check(
        "rtsp500",
        &build_response(
            &reqs[0],
            Response {
                status: Some(500),
                ..Default::default()
            },
        ),
    );
}

#[test]
fn bplist_matches() {
    let value = dict([
        ("type", Value::String("hidSendReport".into())),
        ("uuid", Value::String("2a2a2a2a".into())),
        ("hidReport", Value::Data(vec![0, 1, 2, 3])),
        ("n0", Value::Int(0)),
        ("n255", Value::Int(255)),
        ("n256", Value::Int(256)),
        ("n65536", Value::Int(65536)),
        ("big", Value::Int(1 << 40)),
        ("neg", Value::Real(-1.0)),
        ("frac", Value::Real(0.5)),
        ("t", Value::Bool(true)),
        ("f", Value::Bool(false)),
        ("uni", Value::String("Bülli".into())),
        (
            "list",
            Value::Array(vec![
                Value::Int(1),
                Value::String("two".into()),
                Value::Array(vec![Value::Int(3)]),
            ]),
        ),
        ("empty", Value::Dict(Vec::new())),
        ("long", Value::String("x".repeat(20))),
        ("many", Value::Array((0..20).map(Value::Int).collect())),
    ]);
    let bytes = bplist::encode(&value);
    check("bplist", &bytes);
    assert_eq!(bplist::decode(&bytes), Ok(value));
}

#[test]
fn info_matches() {
    check("info", &bplist::encode(&build(&info_config())));
    let mut cfg = info_config();
    cfg.hevc = false;
    cfg.cluster = None;
    cfg.icons.clear();
    cfg.disable_audio_output = true;
    cfg.entertainment_sample_rate = 48000;
    check("infoNoAudio", &bplist::encode(&build(&cfg)));
}

#[test]
fn hid_matches() {
    check(
        "touch",
        &touch_report(&[
            Contact {
                x: 100.5,
                y: 719.49,
                down: true,
            },
            Contact {
                x: 3.0,
                y: 4.0,
                down: false,
            },
        ]),
    );
    check(
        "knob",
        &knob_report(KnobState {
            select: true,
            x: -200.0,
            y: 3.5,
            wheel: -1.0,
            ..Default::default()
        }),
    );
    let Some(Value::Data(desc)) = touch_device(1920, 1080, "u").get("hidDescriptor").cloned()
    else {
        panic!("no descriptor");
    };
    check("touchDescriptor", &desc);
}

#[test]
fn cipher_matches() {
    let mut c = ControlCipher::new([1; 32], [2; 32]);
    check(
        "cipher",
        &c.encrypt(&[vec![0x41; 16384], b"tail".to_vec()].concat()),
    );
    check("cipher2", &c.encrypt(b"second"));
}

#[test]
fn primitives_match() {
    check(
        "hkdf",
        &hkdf_sha512(&[9; 32], b"Control-Salt", b"Control-Read-Encryption-Key"),
    );
    check(
        "aesctr",
        &aes_ctr128(&[1; 16], &[2; 16], b"a signature of some length here"),
    );
    check("sha1", &sha1(&[b"AES-KEY", &[3; 32]]));
    check("sha256", &sha256(&[b"a", b"b"]));
    check("edSig", &ed25519_sign(&[5; 32], b"sign me"));
    check(
        "chachaLabel",
        &chacha_seal(&[4; 32], &nonce_label("PV-Msg02"), b"sub tlv", &[]),
    );
    check("nonce", &nonce64(0x01_0203_0405));
}

#[test]
fn srp_matches() {
    let srp = SrpServer::with_secrets("Pair-Setup", "3939", [0x11; 16], [0x22; 32]);
    check("srpSalt", &srp.salt);
    check("srpB", &srp.public);
}
