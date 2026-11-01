// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use openssl::asn1::Asn1Time;
use openssl::bn::BigNum;
use openssl::ec::{EcGroup, EcKey};
use openssl::hash::MessageDigest;
use openssl::nid::Nid;
use openssl::pkey::{PKey, Private};
use openssl::ssl::{SslAcceptor, SslConnector, SslMethod};
use openssl::x509::extension::{BasicConstraints, SubjectAlternativeName};
use openssl::x509::{X509, X509NameBuilder};

fn key() -> PKey<Private> {
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
    PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap()
}

fn cert(name: &str, key: &PKey<Private>, issuer: Option<(&X509, &PKey<Private>)>) -> X509 {
    let mut subject = X509NameBuilder::new().unwrap();
    subject.append_entry_by_nid(Nid::COMMONNAME, name).unwrap();
    let subject = subject.build();
    let mut b = X509::builder().unwrap();
    b.set_version(2).unwrap();
    let serial = if issuer.is_some() { 2 } else { 1 };
    b.set_serial_number(&BigNum::from_u32(serial).unwrap().to_asn1_integer().unwrap())
        .unwrap();
    b.set_subject_name(&subject).unwrap();
    b.set_pubkey(key).unwrap();
    b.set_not_before(&Asn1Time::days_from_now(0).unwrap())
        .unwrap();
    b.set_not_after(&Asn1Time::days_from_now(1).unwrap())
        .unwrap();
    match issuer {
        Some((ca, ca_key)) => {
            b.set_issuer_name(ca.subject_name()).unwrap();
            let san = SubjectAlternativeName::new()
                .dns("localhost")
                .ip("127.0.0.1")
                .build(&b.x509v3_context(Some(ca), None))
                .unwrap();
            b.append_extension(san).unwrap();
            b.sign(ca_key, MessageDigest::sha256()).unwrap();
        }
        None => {
            b.set_issuer_name(&subject).unwrap();
            b.append_extension(BasicConstraints::new().critical().ca().build().unwrap())
                .unwrap();
            b.sign(key, MessageDigest::sha256()).unwrap();
        }
    }
    b.build()
}

pub fn ca() -> (SslAcceptor, SslConnector) {
    let ca_key = key();
    let ca = cert("LIVI test CA", &ca_key, None);
    let leaf_key = key();
    let leaf = cert("localhost", &leaf_key, Some((&ca, &ca_key)));
    let mut server = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server()).unwrap();
    server.set_private_key(&leaf_key).unwrap();
    server.set_certificate(&leaf).unwrap();
    let mut client = SslConnector::builder(SslMethod::tls_client()).unwrap();
    client.cert_store_mut().add_cert(ca).unwrap();
    (server.build(), client.build())
}
