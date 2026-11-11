// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use prost::Message;

use crate::wire::{WIRE_LEN, decode_fields, has_field};

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("missing required field {0}")]
    MissingField(u32),
    #[error("missing required field {field}.{missing}")]
    MissingNestedField { field: u32, missing: u32 },
    #[error(transparent)]
    Proto(#[from] prost::DecodeError),
}

pub fn decode<M: Message + Default>(buf: &[u8], required: &[u32]) -> Result<M, DecodeError> {
    if let Some(missing) = required.iter().find(|f| !has_field(buf, **f)) {
        return Err(DecodeError::MissingField(*missing));
    }
    Ok(M::decode(buf)?)
}

pub fn check_nested(buf: &[u8], field: u32, required: &[u32]) -> Result<(), DecodeError> {
    for f in decode_fields(buf).filter(|f| f.field == field && f.wire == WIRE_LEN) {
        if let Some(missing) = required.iter().find(|r| !has_field(f.bytes, **r)) {
            return Err(DecodeError::MissingNestedField {
                field,
                missing: *missing,
            });
        }
    }
    Ok(())
}

pub fn encode<M: Message>(msg: &M) -> Vec<u8> {
    msg.encode_to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use livi_aa_proto::PingRequest;

    #[test]
    fn a_missing_required_field_fails() {
        assert!(decode::<PingRequest>(&[0x10, 0x01], &[1]).is_err());
        let ping: PingRequest = decode(&[0x08, 0x05], &[1]).unwrap();
        assert_eq!(ping.timestamp_ns, 5);
        assert!(decode::<PingRequest>(&[0x08], &[1]).is_err());
        assert!(check_nested(&[0x0a, 0x02, 0x08, 0x01], 1, &[1]).is_ok());
        assert!(check_nested(&[0x0a, 0x02, 0x10, 0x01], 1, &[1]).is_err());
    }
}
