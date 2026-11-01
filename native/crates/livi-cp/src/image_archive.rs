// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

const MANIFEST: &str = "Contents.json";
const LIGHT: u8 = 0;
const DARK: u8 = 1;
const TRANSPARENT: u8 = 2;

const STORED: u16 = 0;
const JANUARY_1980: u16 = 0x21;
const ZIP_VERSION: u16 = 20;

pub fn build(images: &[(String, Vec<u8>)]) -> Vec<u8> {
    let files: Vec<_> = images
        .iter()
        .map(|(name, png)| (format!("{name}.png"), png))
        .collect();
    let entries: Vec<_> = images
        .iter()
        .zip(&files)
        .map(|((name, _), (file, _))| {
            serde_json::json!({
                "symbolName": name,
                "properties": [
                    { "fileName": file, "appearance": LIGHT },
                    { "fileName": file, "appearance": DARK },
                    { "fileName": file, "appearance": TRANSPARENT },
                ],
            })
        })
        .collect();
    let contents = serde_json::json!({ "images": entries }).to_string();
    let mut parts: Vec<(&str, &[u8])> = vec![(MANIFEST, contents.as_bytes())];
    parts.extend(
        files
            .iter()
            .map(|(file, png)| (file.as_str(), png.as_slice())),
    );
    zip(&parts)
}

fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, data) in files {
        let size = (data.len() as u32).to_le_bytes();
        let mut entry = Vec::new();
        for field in [ZIP_VERSION, 0, STORED, 0, JANUARY_1980] {
            entry.extend(field.to_le_bytes());
        }
        entry.extend(crc32(data).to_le_bytes());
        entry.extend(size);
        entry.extend(size);
        entry.extend((name.len() as u16).to_le_bytes());
        entry.extend(0u16.to_le_bytes());

        directory.extend(0x0201_4b50u32.to_le_bytes());
        directory.extend(ZIP_VERSION.to_le_bytes());
        directory.extend(&entry);
        directory.extend([0u8; 10]);
        directory.extend((out.len() as u32).to_le_bytes());
        directory.extend(name.as_bytes());

        out.extend(0x0403_4b50u32.to_le_bytes());
        out.extend(&entry);
        out.extend(name.as_bytes());
        out.extend_from_slice(data);
    }
    let count = (files.len() as u16).to_le_bytes();
    let (size, offset) = (directory.len() as u32, out.len() as u32);
    out.extend(directory);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend(count);
    out.extend(count);
    out.extend(size.to_le_bytes());
    out.extend(offset.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

pub fn crc32(data: &[u8]) -> u32 {
    !data.iter().fold(!0, |crc, &byte| {
        (0..8).fold(crc ^ u32::from(byte), |c, _| {
            if c & 1 == 1 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_at(b: &[u8], at: usize) -> usize {
        u16::from_le_bytes([b[at], b[at + 1]]) as usize
    }

    fn u32_at(b: &[u8], at: usize) -> usize {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize
    }

    #[test]
    fn the_archive_names_the_picture_for_both_appearances() {
        let png = b"\x89PNG not really";
        let archive = build(&[
            ("livi".into(), png.to_vec()),
            ("frost".into(), b"ice".to_vec()),
        ]);
        let end = archive.len() - 22;
        assert_eq!(u32_at(&archive, end), 0x0605_4b50);
        assert_eq!(u16_at(&archive, end + 10), 3);
        let (mut at, mut files) = (u32_at(&archive, end + 16), Vec::new());
        while u32_at(&archive, at) == 0x0201_4b50 {
            let (size, name_len, offset) = (
                u32_at(&archive, at + 20),
                u16_at(&archive, at + 28),
                u32_at(&archive, at + 42),
            );
            let name = String::from_utf8(archive[at + 46..at + 46 + name_len].to_vec()).unwrap();
            let data_at = offset + 30 + u16_at(&archive, offset + 26);
            let data = archive[data_at..data_at + size].to_vec();
            assert_eq!(u32_at(&archive, at + 16) as u32, crc32(&data), "{name}");
            files.push((name, data));
            at += 46 + name_len;
        }
        assert_eq!(files[1], ("livi.png".to_string(), png.to_vec()));
        assert_eq!(files[2], ("frost.png".to_string(), b"ice".to_vec()));
        let contents: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
        assert_eq!(files[0].0, MANIFEST);
        assert_eq!(contents["images"][0]["symbolName"], "livi");
        let appearances: Vec<_> = contents["images"][0]["properties"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                (
                    p["fileName"].as_str().unwrap(),
                    p["appearance"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            appearances,
            [("livi.png", 0), ("livi.png", 1), ("livi.png", 2)]
        );
        assert_eq!(contents["images"][1]["symbolName"], "frost");
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
