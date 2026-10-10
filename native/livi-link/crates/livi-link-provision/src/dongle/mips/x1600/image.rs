//! The vendor's OTA container: a plaintext manifest, zero-padded to `bs_size`, then the kernel and
//! the rootfs. Nothing is signed, the device checks md5 and a block-wise XOR of POSIX `cksum`.

use md5::{Digest, Md5};

pub const BS_SIZE: usize = 131_072;

fn md5_hex(data: &[u8]) -> String {
    Md5::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

fn cksum_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = (i as u32) << 24;
        for _ in 0..8 {
            c = if c & 0x8000_0000 != 0 { (c << 1) ^ 0x04C1_1DB7 } else { c << 1 };
        }
        *slot = c;
    }
    table
}

/// POSIX `cksum`: CRC-32 (0x04C11DB7, MSB first) over the data, then its length, complemented.
pub fn cksum(data: &[u8]) -> u32 {
    let table = cksum_table();
    let step = |c: u32, b: u8| (c << 8) ^ table[(((c >> 24) as u8) ^ b) as usize];
    let mut c = data.iter().fold(0u32, |c, &b| step(c, b));
    let mut n = data.len();
    while n != 0 {
        c = step(c, n as u8);
        n >>= 8;
    }
    !c
}

/// `xor_crc_img` of the device's ota_utils.sh: `cksum` of every `BS_SIZE` block, XORed together.
pub fn xor_crc_img(data: &[u8]) -> u32 {
    data.chunks(BS_SIZE).fold(0, |crc, block| crc ^ cksum(block))
}

fn block(kind: &str, name: &str, data: &[u8]) -> String {
    format!(
        "img_type={kind}\nimg_name={name}\nimg_size={}\nimg_md5={}\nimg_crc={}\n\n",
        data.len(),
        md5_hex(data),
        xor_crc_img(data)
    )
}

/// An OTA image the device accepts, the way the vendor lays it out.
pub fn build(kernel: &[u8], rootfs: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(kernel.len() + rootfs.len());
    payload.extend_from_slice(kernel);
    payload.extend_from_slice(rootfs);
    let manifest = format!(
        "ota_version=0\n\n{}{}check_img=1\n\nbs_size={BS_SIZE}\nota_img_packet_md5={}\n\n",
        block("kernel", "xImage", kernel),
        block("rootfs", "rootfs.squashfs", rootfs),
        md5_hex(&payload)
    );
    let mut out = manifest.into_bytes();
    out.resize(BS_SIZE, 0);
    out.extend_from_slice(&payload);
    out
}

pub struct Parts<'a> {
    pub kernel: &'a [u8],
    pub rootfs: &'a [u8],
}

fn field<'a>(manifest: &'a str, after: &str, key: &str) -> Option<&'a str> {
    let rest = manifest.split_once(after)?.1;
    rest.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
}

/// Splits an image and checks every number the device checks, so a bad file is refused on the host.
pub fn parse(image: &[u8]) -> Result<Parts<'_>, String> {
    let head = image.get(..BS_SIZE).ok_or("image is smaller than its manifest block")?;
    let end = head.iter().position(|&b| b == 0).unwrap_or(head.len());
    let manifest = std::str::from_utf8(&head[..end]).map_err(|_| "manifest is not text")?;
    let num = |after: &str, key: &str| -> Result<usize, String> {
        field(manifest, after, key)
            .and_then(|v| v.trim().parse().ok())
            .ok_or_else(|| format!("manifest has no {key} after {after}"))
    };
    let text = |after: &str, key: &str| -> Result<String, String> {
        field(manifest, after, key)
            .map(|v| v.trim().to_string())
            .ok_or_else(|| format!("manifest has no {key} after {after}"))
    };
    let (ksize, rsize) = (num("img_type=kernel", "img_size")?, num("img_type=rootfs", "img_size")?);
    let payload = &image[BS_SIZE..];
    if payload.len() != ksize + rsize {
        return Err(format!("payload is {} B, the manifest says {}", payload.len(), ksize + rsize));
    }
    let (kernel, rootfs) = payload.split_at(ksize);
    for (kind, data) in [("kernel", kernel), ("rootfs", rootfs)] {
        let marker = format!("img_type={kind}");
        if text(&marker, "img_md5")? != md5_hex(data) {
            return Err(format!("{kind} md5 does not match the manifest"));
        }
        if num(&marker, "img_crc")? != xor_crc_img(data) as usize {
            return Err(format!("{kind} crc does not match the manifest"));
        }
    }
    if text("check_img", "ota_img_packet_md5")? != md5_hex(payload) {
        return Err("packet md5 does not match the manifest".into());
    }
    Ok(Parts { kernel, rootfs })
}

/// A uImage header (64 B) and its data: the bytes of a kernel partition that belong to the kernel.
pub fn uimage_len(raw: &[u8]) -> Result<usize, String> {
    if raw.get(..4) != Some(&[0x27, 0x05, 0x19, 0x56]) {
        return Err("kernel partition does not start with a uImage header".into());
    }
    let size = u32::from_be_bytes(raw[12..16].try_into().unwrap()) as usize;
    (64 + size <= raw.len()).then_some(64 + size).ok_or_else(|| "uImage is longer than its partition".into())
}

/// The squashfs superblock's `bytes_used`, rounded up to 4 KiB as mksquashfs pads it.
pub fn squashfs_len(raw: &[u8]) -> Result<usize, String> {
    if raw.get(..4) != Some(b"hsqs") {
        return Err("rootfs partition is not a little-endian squashfs".into());
    }
    let used = u64::from_le_bytes(raw[40..48].try_into().unwrap()) as usize;
    let padded = used.div_ceil(4096) * 4096;
    (padded <= raw.len()).then_some(padded).ok_or_else(|| "squashfs is longer than its partition".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cksum_matches_posix() {
        // `printf 123456789 | cksum` is 930766865.
        assert_eq!(cksum(b"123456789"), 930_766_865);
        assert_eq!(cksum(b""), 4_294_967_295);
    }

    #[test]
    fn crc_is_taken_per_block() {
        let data = vec![7u8; BS_SIZE + 5];
        assert_eq!(xor_crc_img(&data), cksum(&data[..BS_SIZE]) ^ cksum(&data[BS_SIZE..]));
    }

    #[test]
    fn a_built_image_parses_back() {
        let image = build(b"kernel-bytes", &vec![9u8; 300_000]);
        let parts = parse(&image).unwrap();
        assert_eq!(parts.kernel, b"kernel-bytes");
        assert_eq!(parts.rootfs.len(), 300_000);
    }

    #[test]
    fn a_flipped_byte_is_refused() {
        let mut image = build(b"kernel-bytes", b"rootfs-bytes");
        *image.last_mut().unwrap() ^= 1;
        assert!(parse(&image).is_err());
    }

    /// Set LIVI_STOCK_OTA to the stock image: rebuilding it must give the same bytes.
    #[test]
    fn the_stock_image_is_rebuilt_byte_for_byte() {
        let Ok(path) = std::env::var("LIVI_STOCK_OTA") else { return };
        let stock = std::fs::read(path).unwrap();
        let parts = parse(&stock).unwrap();
        assert_eq!(build(parts.kernel, parts.rootfs), stock);
    }

    #[test]
    fn partitions_are_trimmed_to_their_content() {
        let mut k = vec![0u8; 1000];
        k[..4].copy_from_slice(&[0x27, 0x05, 0x19, 0x56]);
        k[12..16].copy_from_slice(&100u32.to_be_bytes());
        assert_eq!(uimage_len(&k).unwrap(), 164);
        let mut r = vec![0u8; 20_000];
        r[..4].copy_from_slice(b"hsqs");
        r[40..48].copy_from_slice(&5000u64.to_le_bytes());
        assert_eq!(squashfs_len(&r).unwrap(), 8192);
        assert!(squashfs_len(&k).is_err());
    }
}
