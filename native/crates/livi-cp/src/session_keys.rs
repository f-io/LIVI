// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_crypto::hkdf_sha512;

pub const DATA_OUT: &[u8] = b"DataStream-Output-Encryption-Key";
pub const DATA_IN: &[u8] = b"DataStream-Input-Encryption-Key";

pub fn control(shared: &[u8]) -> ([u8; 32], [u8; 32]) {
    (
        hkdf_sha512(shared, b"Control-Salt", b"Control-Write-Encryption-Key"),
        hkdf_sha512(shared, b"Control-Salt", b"Control-Read-Encryption-Key"),
    )
}

pub fn events(shared: &[u8]) -> ([u8; 32], [u8; 32]) {
    (
        hkdf_sha512(shared, b"Events-Salt", b"Events-Read-Encryption-Key"),
        hkdf_sha512(shared, b"Events-Salt", b"Events-Write-Encryption-Key"),
    )
}

pub fn data_stream(shared: &[u8], id: &str, info: &[u8]) -> [u8; 32] {
    hkdf_sha512(shared, format!("DataStream-Salt{id}").as_bytes(), info)
}
