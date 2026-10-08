use prost::Message;

use crate::wire::{WIRE_LEN, decode_fields, has_field};

/// prost fills a missing required field with its default.
pub fn decode<M: Message + Default>(buf: &[u8], required: &[u32]) -> Result<M, String> {
    if let Some(missing) = required.iter().find(|f| !has_field(buf, **f)) {
        return Err(format!("missing required field {missing}"));
    }
    M::decode(buf).map_err(|e| e.to_string())
}

pub fn check_nested(buf: &[u8], field: u32, required: &[u32]) -> Result<(), String> {
    for f in decode_fields(buf).filter(|f| f.field == field && f.wire == WIRE_LEN) {
        if let Some(missing) = required.iter().find(|r| !has_field(f.bytes, **r)) {
            return Err(format!("missing required field {field}.{missing}"));
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
        assert_eq!(check_nested(&[0x0a, 0x02, 0x08, 0x01], 1, &[1]), Ok(()));
        assert!(check_nested(&[0x0a, 0x02, 0x10, 0x01], 1, &[1]).is_err());
    }
}
