// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use crate::checksum::crc32;

pub const MAGIC: &[u8; 4] = b"LFWB";
pub const HOLD: &str = "/tmp/livi/flashing";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 8;
const DESC_LEN: usize = 12;
const TARGET: &[u8; 4] = b"LFWT";

pub fn pack(images: &[(u8, &[u8])]) -> Vec<u8> {
    let payload: usize = images.iter().map(|(_, data)| data.len()).sum();
    let mut out = Vec::with_capacity(HEADER_LEN + images.len() * DESC_LEN + payload);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(images.len() as u8);
    out.extend_from_slice(&[0, 0]);
    for (typ, data) in images {
        out.push(*typ);
        out.push(0);
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&crc32(data).to_le_bytes());
    }
    for (_, data) in images {
        out.extend_from_slice(data);
    }
    out
}

pub fn unpack(bytes: &[u8]) -> Result<Vec<(u8, &[u8])>, String> {
    split(bytes).map(|(images, _)| images)
}

/// The target the bundle was built for, from the trailer after the images: "LFWT" | length:u8 | name.
/// Readers from before the trailer stop at the last image and never see it.
pub fn target(bytes: &[u8]) -> Option<&str> {
    let (_, tail) = split(bytes).ok()?;
    let name = tail.strip_prefix(TARGET)?;
    let (&len, name) = name.split_first()?;
    std::str::from_utf8(name.get(..usize::from(len))?).ok()
}

type Split<'a> = (Vec<(u8, &'a [u8])>, &'a [u8]);

fn split(bytes: &[u8]) -> Result<Split<'_>, String> {
    if bytes.len() < HEADER_LEN || &bytes[..4] != MAGIC {
        return Err("not an LFWB bundle".into());
    }
    if bytes[4] != VERSION {
        return Err(format!("unsupported LFWB version {}", bytes[4]));
    }
    let count = usize::from(bytes[5]);
    let table = bytes
        .get(HEADER_LEN..HEADER_LEN + count * DESC_LEN)
        .ok_or("LFWB bundle is truncated in its descriptor table")?;
    let mut rest = &bytes[HEADER_LEN + count * DESC_LEN..];
    let mut images = Vec::with_capacity(count);
    for desc in table.as_chunks::<DESC_LEN>().0 {
        let typ = desc[0];
        let len = u32::from_le_bytes([desc[4], desc[5], desc[6], desc[7]]) as usize;
        let crc = u32::from_le_bytes([desc[8], desc[9], desc[10], desc[11]]);
        let (data, tail) = rest
            .split_at_checked(len)
            .ok_or_else(|| format!("LFWB bundle is truncated in image type {typ}"))?;
        if crc32(data) != crc {
            return Err(format!("LFWB image type {typ} does not match its crc32"));
        }
        images.push((typ, data));
        rest = tail;
    }
    Ok((images, rest))
}

pub fn image<'a>(images: &[(u8, &'a [u8])], typ: u8) -> Option<&'a [u8]> {
    images
        .iter()
        .find(|(t, _)| *t == typ)
        .map(|(_, data)| *data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_packed_bundle_unpacks_to_the_same_images() {
        let boot = vec![7u8; 1000];
        let rootfs: Vec<u8> = (0..3000u32).map(|i| i as u8).collect();
        let bundle = pack(&[(3, &boot), (6, &rootfs)]);
        let images = unpack(&bundle).unwrap();
        assert_eq!(image(&images, 3), Some(boot.as_slice()));
        assert_eq!(image(&images, 6), Some(rootfs.as_slice()));
        assert_eq!(image(&images, 1), None);
    }

    #[test]
    fn a_damaged_bundle_is_refused_not_flashed() {
        let mut bundle = pack(&[(3, &[1, 2, 3, 4])]);
        assert!(unpack(&bundle[..bundle.len() - 1]).is_err(), "truncated");
        *bundle.last_mut().unwrap() ^= 1;
        assert!(unpack(&bundle).is_err(), "flipped bit");
        assert!(unpack(b"nope").is_err(), "not a bundle");
        let mut huge = pack(&[(3, &[1, 2, 3, 4])]);
        huge[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(unpack(&huge).is_err(), "length past the end");
    }

    #[test]
    fn the_target_trailer_is_read_and_ignored_by_unpack() {
        let plain = pack(&[(3, &[1, 2, 3, 4])]);
        assert_eq!(target(&plain), None);
        let mut tagged = plain.clone();
        tagged.extend_from_slice(b"LFWT\x09f133b_xfh");
        assert_eq!(target(&tagged), Some("f133b_xfh"));
        assert_eq!(unpack(&tagged).unwrap(), unpack(&plain).unwrap());
        tagged.pop();
        assert_eq!(target(&tagged), None, "cut short");
    }
}
