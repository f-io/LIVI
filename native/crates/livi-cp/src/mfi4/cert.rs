// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::Path;

use openssl::bn::{BigNum, BigNumContext};
use openssl::ec::{EcKey, EcPoint, PointConversionForm};
use openssl::ecdsa::EcdsaSig;
use openssl::pkey::{PKey, Public};
use openssl::x509::X509;

use super::keys::{POINT, group};
use crate::crypto::{sha1, sha256};

const ROOT_SHA256: [u8; 32] = [
    0x77, 0x57, 0xb6, 0x1f, 0x5b, 0xe7, 0xfc, 0x25, 0x9c, 0xad, 0x89, 0xa5, 0x9c, 0x11, 0x74, 0xb9,
    0xa0, 0x62, 0x34, 0xcb, 0xcb, 0x68, 0xdc, 0xd2, 0x43, 0xde, 0x30, 0x4d, 0x86, 0x75, 0x3c, 0x00,
];
pub const ROOT_FILE: &str = "accessory-root-ca.pem";
const MACOS_CARRIER: &str = "/usr/libexec/mobileactivationd";

const LEAVES: std::ops::RangeInclusive<u8> = 0xA0..=0xA3;
const INTERMEDIATES: std::ops::RangeInclusive<u8> = 0xA4..=0xA5;
const WITH_PLAN_ID: u8 = 0x01;
const WITH_ACCESSORY_DATA: u8 = 0x02;
const HELD_BY_APPLE_DEVICE: u8 = 0x03;

const NAME_LEN: usize = 8;
const TIME_LEN: usize = 14;
const PROPERTY_LEN: usize = 32;
const KEY_ID_LEN: usize = 20;
const PLAN_ID_LEN: usize = 36;
const FULL_POINT: usize = 65;

const OID_COMMON_NAME: &[u8] = &[0x55, 0x04, 0x03];
const OID_UNIT: &[u8] = &[0x55, 0x04, 0x0b];
const OID_ORGANIZATION: &[u8] = &[0x55, 0x04, 0x0a];
const OID_STATE: &[u8] = &[0x55, 0x04, 0x08];
const OID_ECDSA_SHA256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];
const OID_EC_PUBLIC_KEY: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
const OID_P256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
const OID_BASIC_CONSTRAINTS: &[u8] = &[0x55, 0x1d, 0x13];
const OID_KEY_USAGE: &[u8] = &[0x55, 0x1d, 0x0f];
const OID_SUBJECT_KEY_ID: &[u8] = &[0x55, 0x1d, 0x0e];
const OID_AUTHORITY_KEY_ID: &[u8] = &[0x55, 0x1d, 0x23];
const OID_APPLE_ACCESSORY: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x63, 0x64, 0x06, 0x47];
const ACCESSORY_PROPERTY: u8 = 1;
const PLAN_ID: u8 = 2;
const ACCESSORY_DATA: u8 = 3;

const ORGANIZATION: &str = "Apple Inc.";
const STATE: &str = "California";
const ROOT_PREFIX: &str = "Apple Accessory Root CA - ";
const AUTHORITY_PREFIX: &str = "Apple Accessory Host Attestation Authority - ";
const LEAF_UNIT: &str = "Apple Accessory Host Attestations";

#[derive(Debug, Clone)]
pub struct Unpacked {
    pub der: Vec<u8>,
    pub leaf: bool,
    pub point: [u8; FULL_POINT],
    pub authority: [u8; KEY_ID_LEN],
    pub key_id: [u8; KEY_ID_LEN],
    pub property: Option<[u8; PROPERTY_LEN]>,
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let (head, rest) = self
            .0
            .split_at_checked(n)
            .ok_or("the certificate is cut short")?;
        self.0 = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        Ok(self.take(N)?.try_into().expect("taken to length"))
    }

    fn text(&mut self, n: usize) -> Result<&'a str, String> {
        std::str::from_utf8(self.take(n)?).map_err(|_| "a name that is no text".into())
    }

    fn sequence(&mut self) -> Result<&'a [u8], String> {
        match self.0 {
            [0x30, len, ..] if *len < 0x80 => self.take(2 + usize::from(*len)),
            _ => Err("expected a short DER sequence".into()),
        }
    }
}

pub fn unpack(compressed: &[u8]) -> Result<Unpacked, String> {
    let mut r = Reader(compressed);
    let kind = r.array::<1>()?[0];
    let leaf = LEAVES.contains(&kind);
    if !leaf && !INTERMEDIATES.contains(&kind) {
        return Err(format!("unknown certificate kind {kind:#04x}"));
    }
    let issuer = r.text(NAME_LEN)?;
    let not_before = r.text(TIME_LEN)?;
    let not_after = r.text(TIME_LEN)?;
    let subject = if leaf { None } else { Some(r.text(NAME_LEN)?) };
    let point = full_point(&r.array::<POINT>()?)?;
    let property = if leaf {
        Some(r.array::<PROPERTY_LEN>()?)
    } else {
        None
    };
    let authority = r.array::<KEY_ID_LEN>()?;
    let plan_id = if kind & WITH_PLAN_ID != 0 {
        Some(r.take(PLAN_ID_LEN)?)
    } else {
        None
    };
    let signature = r.sequence()?;
    let accessory_data = if leaf && kind & WITH_ACCESSORY_DATA != 0 {
        Some(r.sequence()?)
    } else {
        None
    };
    if !r.0.is_empty() {
        return Err(format!("{} bytes past the certificate", r.0.len()));
    }

    let key_id = sha1(&[&point]);
    let (issuer_name, subject_name) = match subject {
        None => (
            name(&[(OID_COMMON_NAME, &format!("{AUTHORITY_PREFIX}{issuer}"))]),
            name(&[
                (OID_COMMON_NAME, &upper_hex(&sha256(&[&point]))),
                (OID_UNIT, LEAF_UNIT),
            ]),
        ),
        Some(subject) => (
            name(&[(OID_COMMON_NAME, &format!("{ROOT_PREFIX}{issuer}"))]),
            name(&[(OID_COMMON_NAME, &format!("{AUTHORITY_PREFIX}{subject}"))]),
        ),
    };

    let mut extensions = Vec::new();
    if !leaf {
        extensions.push(extension(
            OID_BASIC_CONSTRAINTS,
            true,
            &der(0x30, &der(0x01, &[0xff])),
        ));
    }
    extensions.push(extension(
        OID_AUTHORITY_KEY_ID,
        false,
        &der(0x30, &der(0x80, &authority)),
    ));
    if !leaf {
        extensions.push(extension(OID_SUBJECT_KEY_ID, false, &der(0x04, &key_id)));
        extensions.push(extension(OID_KEY_USAGE, true, &der(0x03, &[0x01, 0x06])));
    }
    if let Some(property) = &property {
        extensions.push(extension(&apple(ACCESSORY_PROPERTY), true, property));
    }
    if let Some(plan_id) = plan_id {
        extensions.push(extension(&apple(PLAN_ID), false, plan_id));
    }
    if let Some(data) = accessory_data {
        extensions.push(extension(&apple(ACCESSORY_DATA), false, data));
    }

    let algorithm = der(0x30, &der(0x06, OID_ECDSA_SHA256));
    let key_info = der(
        0x30,
        &[
            der(
                0x30,
                &[der(0x06, OID_EC_PUBLIC_KEY), der(0x06, OID_P256)].concat(),
            ),
            bit_string(&point),
        ]
        .concat(),
    );
    let tbs = der(
        0x30,
        &[
            der(0xa0, &der(0x02, &[2])),
            der(0x02, &point[..KEY_ID_LEN]),
            algorithm.clone(),
            with_place(issuer_name),
            der(0x30, &[time(not_before)?, time(not_after)?].concat()),
            with_place(subject_name),
            key_info,
            der(0xa3, &der(0x30, &extensions.concat())),
        ]
        .concat(),
    );
    let der = der(0x30, &[tbs, algorithm, bit_string(signature)].concat());
    Ok(Unpacked {
        der,
        leaf,
        point,
        authority,
        key_id,
        property,
    })
}

fn full_point(compressed: &[u8; POINT]) -> Result<[u8; FULL_POINT], String> {
    let group = group();
    let mut ctx = BigNumContext::new().map_err(|e| e.to_string())?;
    let point = EcPoint::from_bytes(&group, compressed, &mut ctx).map_err(|e| e.to_string())?;
    point
        .to_bytes(&group, PointConversionForm::UNCOMPRESSED, &mut ctx)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "a point that is no 65 bytes".into())
}

fn der(tag: u8, content: &[u8]) -> Vec<u8> {
    let n = content.len();
    let mut out = vec![tag];
    match n {
        0..0x80 => out.push(n as u8),
        0x80..0x100 => out.extend([0x81, n as u8]),
        _ => out.extend([0x82, (n >> 8) as u8, n as u8]),
    }
    out.extend_from_slice(content);
    out
}

fn bit_string(bytes: &[u8]) -> Vec<u8> {
    der(0x03, &[&[0][..], bytes].concat())
}

fn with_place(mut entries: Vec<Vec<u8>>) -> Vec<u8> {
    entries.push(rdn(OID_ORGANIZATION, ORGANIZATION));
    entries.push(rdn(OID_STATE, STATE));
    der(0x30, &entries.concat())
}

fn name(entries: &[(&[u8], &str)]) -> Vec<Vec<u8>> {
    entries.iter().map(|(oid, value)| rdn(oid, value)).collect()
}

fn rdn(oid: &[u8], value: &str) -> Vec<u8> {
    der(
        0x31,
        &der(
            0x30,
            &[der(0x06, oid), der(0x0c, value.as_bytes())].concat(),
        ),
    )
}

fn extension(oid: &[u8], critical: bool, value: &[u8]) -> Vec<u8> {
    let critical = if critical {
        der(0x01, &[0xff])
    } else {
        Vec::new()
    };
    der(0x30, &[der(0x06, oid), critical, der(0x04, value)].concat())
}

fn apple(arc: u8) -> Vec<u8> {
    [OID_APPLE_ACCESSORY, &[arc]].concat()
}

fn time(digits: &str) -> Result<Vec<u8>, String> {
    if digits.len() != TIME_LEN || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("a validity time {digits:?}"));
    }
    let year: u32 = digits[..4].parse().expect("digits");
    Ok(if (1950..2050).contains(&year) {
        der(0x17, format!("{}Z", &digits[2..]).as_bytes())
    } else {
        der(0x18, format!("{digits}Z").as_bytes())
    })
}

fn upper_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

struct Issuer {
    key_id: [u8; KEY_ID_LEN],
    key: PKey<Public>,
}

pub struct Trust {
    issuers: Vec<Issuer>,
    rooted: bool,
}

impl Trust {
    pub fn new(dir: Option<&Path>) -> Self {
        match find_root(dir) {
            Some(root) => Self {
                issuers: vec![root],
                rooted: true,
            },
            None => Self {
                issuers: Vec::new(),
                rooted: false,
            },
        }
    }

    pub fn rooted(&self) -> bool {
        self.rooted
    }

    pub fn knows_issuer_of(&self, cert: &Unpacked) -> bool {
        self.issuers.iter().any(|i| i.key_id == cert.authority)
    }

    pub fn check(&self, cert: &Unpacked) -> Result<(), String> {
        if cert.leaf && cert.property.map(|p| p[PROPERTY_LEN - 1]) != Some(HELD_BY_APPLE_DEVICE) {
            return Err("a leaf not held by an Apple device".into());
        }
        if !self.rooted {
            return Ok(());
        }
        let issuer = self
            .issuers
            .iter()
            .find(|i| i.key_id == cert.authority)
            .ok_or_else(|| format!("no known issuer {}", upper_hex(&cert.authority)))?;
        let x509 = X509::from_der(&cert.der).map_err(|e| e.to_string())?;
        if !x509.verify(&issuer.key).map_err(|e| e.to_string())? {
            return Err("the issuer did not sign it".into());
        }
        Ok(())
    }

    pub fn admit(&mut self, cert: &Unpacked) -> Result<(), String> {
        self.check(cert)?;
        self.issuers.push(Issuer {
            key_id: cert.key_id,
            key: public_key(&cert.point)?,
        });
        Ok(())
    }
}

fn find_root(dir: Option<&Path>) -> Option<Issuer> {
    let the_root = |blocks: Vec<&[u8]>| {
        blocks.into_iter().find_map(|block| {
            let cert = X509::from_pem(block).ok()?;
            (sha256(&[&cert.to_der().ok()?]) == ROOT_SHA256).then_some(cert)
        })
    };
    let mut root = None;
    if let Some(file) = dir.map(|d| d.join(ROOT_FILE))
        && let Ok(pem) = std::fs::read(&file)
    {
        root = the_root(pem_blocks(&pem));
        if root.is_none() {
            println!(
                "[cp] mfi4: {} is not Apple's accessory root",
                file.display()
            );
        }
    }
    if root.is_none()
        && cfg!(target_os = "macos")
        && let Ok(binary) = std::fs::read(MACOS_CARRIER)
    {
        root = the_root(pem_blocks(&binary));
    }
    let root = root?;
    let key_id = root.subject_key_id()?.as_slice().try_into().ok()?;
    Some(Issuer {
        key_id,
        key: root.public_key().ok()?,
    })
}

fn pem_blocks(bytes: &[u8]) -> Vec<&[u8]> {
    const BEGIN: &[u8] = b"-----BEGIN CERTIFICATE-----";
    const END: &[u8] = b"-----END CERTIFICATE-----";
    let mut out = Vec::new();
    let mut rest = bytes;
    while let Some(start) = rest.windows(BEGIN.len()).position(|w| w == BEGIN) {
        let from = &rest[start..];
        let Some(end) = from.windows(END.len()).position(|w| w == END) else {
            break;
        };
        out.push(&from[..end + END.len()]);
        rest = &from[end + END.len()..];
    }
    out
}

fn public_key(point: &[u8; FULL_POINT]) -> Result<PKey<Public>, String> {
    PKey::from_ec_key(ec_key(point)?).map_err(|e| e.to_string())
}

fn ec_key(point: &[u8; FULL_POINT]) -> Result<EcKey<Public>, String> {
    let group = group();
    let mut ctx = BigNumContext::new().map_err(|e| e.to_string())?;
    let point = EcPoint::from_bytes(&group, point, &mut ctx).map_err(|e| e.to_string())?;
    EcKey::from_public_key(&group, &point).map_err(|e| e.to_string())
}

pub fn signed_by(cert: &Unpacked, data: &[u8], signature: &[u8; 64]) -> Result<bool, String> {
    let r = BigNum::from_slice(&signature[..32]).map_err(|e| e.to_string())?;
    let s = BigNum::from_slice(&signature[32..]).map_err(|e| e.to_string())?;
    let sig = EcdsaSig::from_private_components(r, s).map_err(|e| e.to_string())?;
    let key = ec_key(&cert.point)?;
    sig.verify(&sha256(&[data]), &key)
        .map_err(|e| e.to_string())
}
