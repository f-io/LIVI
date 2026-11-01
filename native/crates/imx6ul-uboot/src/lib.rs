// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io::Read;

use aes::Aes128;
use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use flate2::read::GzDecoder;

pub const UPDATE_BLOCK: usize = 0x20000;
pub const ENV_BLOCK: usize = 0x30000;
pub const KERNEL: usize = 0x40000;
pub const ERASE: usize = 0x10000;
pub const HEAD: usize = 3 * ERASE;

const ENV_SIZE: usize = 0x10000;
const ENV_VARS_END: usize = 0x800;
const ENV_PAYLOAD: usize = 0x3000;
const ZIMAGE_MAGIC: u32 = 0x016f_2818;
const LOAD: u32 = 0x8080_0000;
const MARKER: &[u8; 4] = b"sgub";

#[derive(Clone, Copy, Debug)]
pub struct Fuses {
    pub cfg0: u32,
    pub cfg1: u32,
    pub mac0: u32,
    pub mac1: u32,
}

impl Fuses {
    pub fn from_ocotp(nvmem: &[u8]) -> Option<Self> {
        let word = |i: usize| {
            Some(u32::from_le_bytes(
                nvmem.get(i * 4..i * 4 + 4)?.try_into().ok()?,
            ))
        };
        Some(Self {
            cfg0: word(1)?,
            cfg1: word(2)?,
            mac0: word(0x22)?,
            mac1: word(0x23)?,
        })
    }
}

pub struct UpgKey {
    key: [u8; 16],
    iv: [u8; 16],
}

impl UpgKey {
    pub fn new(f: Fuses) -> Self {
        let text = format!("{:08x}{:08x}{:08x}{:08x}", f.cfg0, f.cfg1, f.mac0, f.mac1);
        let (key, iv) = text.as_bytes().split_at(16);
        Self {
            key: key.try_into().unwrap(),
            iv: iv.try_into().unwrap(),
        }
    }

    /// The fixed key firmware before the per-unit fuse scheme used for its
    /// update environment.
    pub fn vendor_static() -> Self {
        let (key, iv) = b"https://apple.com/kb/HT208050   ".split_at(16);
        Self {
            key: key.try_into().unwrap(),
            iv: iv.try_into().unwrap(),
        }
    }

    pub fn encrypt(&self, data: &mut [u8]) {
        let (chunks, rest) = data.as_chunks_mut::<16>();
        assert!(rest.is_empty());
        let aes = Aes128::new(&self.key.into());
        let mut prev = self.iv;
        for chunk in chunks {
            chunk.iter_mut().zip(prev).for_each(|(b, p)| *b ^= p);
            let mut block = (*chunk).into();
            aes.encrypt_block(&mut block);
            *chunk = block.into();
            prev = *chunk;
        }
    }

    pub fn decrypt(&self, data: &mut [u8]) {
        let (chunks, rest) = data.as_chunks_mut::<16>();
        assert!(rest.is_empty());
        let aes = Aes128::new(&self.key.into());
        let mut prev = self.iv;
        for chunk in chunks {
            let cipher = *chunk;
            let mut block = cipher.into();
            aes.decrypt_block(&mut block);
            let plain: [u8; 16] = block.into();
            *chunk = std::array::from_fn(|i| plain[i] ^ prev[i]);
            prev = cipher;
        }
    }
}

pub fn read_env(block: &[u8], key: &UpgKey) -> Result<Vec<u8>, String> {
    if block.len() != ERASE {
        return Err(format!(
            "update block is {} bytes, not {ERASE}",
            block.len()
        ));
    }
    let word = u32::from_le_bytes(block[ERASE - 4..].try_into().unwrap()) as usize;
    let newer = word != 0 && word <= ERASE && word.is_multiple_of(16);
    let len = if newer { word } else { ENV_PAYLOAD };
    let mut payload = block[ERASE - len..].to_vec();
    key.decrypt(&mut payload);

    let fail =
        |e: String| format!("update environment does not unpack with this dongle's key: {e}");
    let mut env = Vec::with_capacity(ENV_SIZE);
    if newer {
        GzDecoder::new(&payload[..])
            .take(ENV_SIZE as u64 + 1)
            .read_to_end(&mut env)
            .map_err(|e| fail(e.to_string()))?;
    } else {
        let stream = payload
            .get(4..)
            .ok_or_else(|| fail("payload too short".into()))?;
        env = lzokay_native::decompress_all(stream, Some(ENV_SIZE))
            .map_err(|e| fail(e.to_string()))?;
    }
    if env.len() != ENV_SIZE {
        return Err(format!(
            "update environment is {:#x} bytes, not {ENV_SIZE:#x}",
            env.len()
        ));
    }
    let mut crc = flate2::Crc::new();
    crc.update(&env[4..ENV_VARS_END]);
    if crc.sum() != u32::from_le_bytes(env[..4].try_into().unwrap()) {
        return Err("update environment fails its CRC".into());
    }
    Ok(env)
}

pub fn read_env_auto(fuses: Fuses, block: &[u8]) -> Result<(UpgKey, Vec<u8>), String> {
    let newer = block.len() == ERASE && {
        let word = u32::from_le_bytes(block[ERASE - 4..].try_into().unwrap()) as usize;
        word != 0 && word <= ERASE && word.is_multiple_of(16)
    };
    let key = if newer {
        UpgKey::new(fuses)
    } else {
        UpgKey::vendor_static()
    };
    let env = read_env(block, &key)?;
    Ok((key, env))
}

pub fn env_var<'a>(env: &'a [u8], name: &str) -> Option<&'a str> {
    env[4..ENV_VARS_END.min(env.len())]
        .split(|&b| b == 0)
        .take_while(|v| !v.is_empty())
        .filter_map(|v| std::str::from_utf8(v).ok())
        .find_map(|v| v.strip_prefix(name)?.strip_prefix('='))
}

#[derive(Debug, PartialEq, Eq)]
pub struct Layout {
    pub kernel_len: usize,
    pub staging: usize,
}

pub fn layout(env: &[u8], kernel_part: usize) -> Result<Layout, String> {
    let norboot = commands(env_var(env, "norboot").ok_or("no norboot in the update environment")?);
    let kernel_len = norboot
        .iter()
        .find_map(|c| match c.as_slice() {
            ["sf", "read", load, "0x40000", len] if hex(load) == Some(LOAD) => hex(len),
            _ => None,
        })
        .ok_or("norboot does not read the kernel from 0x40000")? as usize;
    let head: Vec<u32> = norboot
        .iter()
        .filter_map(|c| match c.as_slice() {
            ["mm", "w", at, "0x10000"] => hex(at),
            _ => None,
        })
        .collect();
    if head != [LOAD, LOAD + 0x10000, LOAD + 0x20000] {
        return Err("norboot does not decrypt exactly the first three blocks".into());
    }

    let script =
        commands(env_var(env, "heweiencrypt").ok_or("no heweiencrypt in the update environment")?);
    let mut from = None;
    let mut pairs = Vec::new();
    for c in &script {
        match c.as_slice() {
            ["sf", "read", load, src, "0x10000"] if hex(load) == Some(LOAD) => from = hex(src),
            ["sf", "update", load, dst, "0x10000"] if hex(load) == Some(LOAD) => {
                if let (Some(src), Some(dst)) = (from.take(), hex(dst)) {
                    pairs.push((src as usize, dst as usize));
                }
            }
            _ => {}
        }
    }
    let staging = pairs
        .first()
        .map(|p| p.0)
        .ok_or("heweiencrypt converts no kernel blocks")?;
    let expected: Vec<(usize, usize)> = (0..3)
        .map(|i| (staging + i * ERASE, KERNEL + i * ERASE))
        .collect();
    if pairs
        .iter()
        .filter(|p| p.1 != ENV_BLOCK)
        .cloned()
        .collect::<Vec<_>>()
        != expected
    {
        return Err("heweiencrypt does not convert three consecutive staging blocks".into());
    }

    if kernel_len <= HEAD || staging < KERNEL + kernel_len || staging + HEAD > KERNEL + kernel_part
    {
        return Err(format!(
            "kernel {kernel_len:#x} and staging at {staging:#x} do not fit the kernel partition"
        ));
    }
    Ok(Layout {
        kernel_len,
        staging,
    })
}

pub fn kernel_partition(
    current: &[u8],
    zimage: &[u8],
    layout: &Layout,
    key: &UpgKey,
) -> Result<Vec<u8>, String> {
    if zimage.len() <= HEAD || zimage[0x24..0x28] != ZIMAGE_MAGIC.to_le_bytes() {
        return Err("not an ARM zImage".into());
    }
    if zimage.len() > layout.kernel_len {
        return Err(format!(
            "zImage is {} bytes, this dongle's U-Boot reads {}",
            zimage.len(),
            layout.kernel_len
        ));
    }
    let staging = layout.staging - KERNEL;
    if staging + HEAD > current.len() {
        return Err("staging blocks lie outside the kernel partition".into());
    }

    let mut part = vec![0xff; current.len()];
    part[..HEAD].copy_from_slice(&current[..HEAD]);
    part[HEAD..zimage.len()].copy_from_slice(&zimage[HEAD..]);
    let head = &mut part[staging..staging + HEAD];
    head.copy_from_slice(&zimage[..HEAD]);
    for block in head.as_chunks_mut::<ERASE>().0 {
        key.encrypt(block);
    }
    Ok(part)
}

pub fn stage(zimage: &[u8], kernel: &[u8], uboot: &[u8], fuses: Fuses) -> Result<Vec<u8>, String> {
    let block = uboot
        .get(UPDATE_BLOCK..UPDATE_BLOCK + ERASE)
        .ok_or("the uboot partition ends before the update block")?;
    let (key, env) = read_env_auto(fuses, block)?;
    kernel_partition(kernel, zimage, &layout(&env, kernel.len())?, &key)
}

pub fn refit_env(env: &[u8], zimage_len: usize, kernel_part: usize) -> Result<Vec<u8>, String> {
    if env.len() != ENV_SIZE {
        return Err(format!(
            "environment is {:#x} bytes, not {ENV_SIZE:#x}",
            env.len()
        ));
    }
    let old = layout(env, kernel_part)?;
    let new = Layout {
        kernel_len: zimage_len,
        staging: KERNEL + zimage_len,
    };
    if new.kernel_len <= HEAD || new.staging + HEAD > KERNEL + kernel_part {
        return Err(format!(
            "a kernel of {zimage_len:#x} does not fit the kernel partition"
        ));
    }
    let mut out = env.to_vec();
    let vars = &mut out[4..ENV_VARS_END];
    replace_hex(vars, old.kernel_len, new.kernel_len)?;
    for i in 0..3 {
        replace_hex(vars, old.staging + i * ERASE, new.staging + i * ERASE)?;
    }
    let mut crc = flate2::Crc::new();
    crc.update(&out[4..ENV_VARS_END]);
    out[..4].copy_from_slice(&crc.sum().to_le_bytes());
    Ok(out)
}

fn replace_hex(vars: &mut [u8], old: usize, new: usize) -> Result<(), String> {
    let (from, to) = (format!("{old:#x}"), format!("{new:#x}"));
    if from.len() != to.len() {
        return Err(format!("{from} and {to} are not the same width"));
    }
    let (from, to) = (from.as_bytes(), to.as_bytes());
    let mut at = 0;
    let mut hits = 0;
    while let Some(i) = vars[at..].windows(from.len()).position(|w| w == from) {
        let i = at + i;
        vars[i..i + to.len()].copy_from_slice(to);
        at = i + to.len();
        hits += 1;
    }
    if hits == 0 {
        return Err(format!(
            "{} not found in the environment",
            String::from_utf8_lossy(from)
        ));
    }
    Ok(())
}

pub fn flashing_block(env: &[u8]) -> Result<Vec<u8>, String> {
    if env.len() != ENV_SIZE {
        return Err(format!(
            "environment is {:#x} bytes, not {ENV_SIZE:#x}",
            env.len()
        ));
    }
    let mut block = env.to_vec();
    UpgKey::vendor_static().encrypt(&mut block);
    block[ENV_SIZE - 4..].copy_from_slice(MARKER);
    Ok(block)
}

fn commands(script: &str) -> Vec<Vec<&str>> {
    script
        .split(';')
        .map(|c| c.split_whitespace().collect::<Vec<_>>())
        .filter(|c| !c.is_empty())
        .collect()
}

fn hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.strip_prefix("0x")?, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;

    const FUSES: Fuses = Fuses {
        cfg0: 0x0123abcd,
        cfg1: 0x89ef4567,
        mac0: 0xdeadbeef,
        mac1: 1,
    };

    fn env_with(vars: &[&str]) -> Vec<u8> {
        let mut env = vec![0u8; ENV_SIZE];
        let text = vars.join("\0");
        env[4..4 + text.len()].copy_from_slice(text.as_bytes());
        env[0x800..0x804].copy_from_slice(&0xd00dfeed_u32.to_be_bytes());
        let mut crc = flate2::Crc::new();
        crc.update(&env[4..ENV_VARS_END]);
        env[..4].copy_from_slice(&crc.sum().to_le_bytes());
        env
    }

    fn update_block(env: &[u8], key: &UpgKey) -> Vec<u8> {
        let mut gz = GzEncoder::new(Vec::new(), Compression::best());
        gz.write_all(env).unwrap();
        let mut payload = gz.finish().unwrap();
        payload.resize(payload.len().div_ceil(0x1000) * 0x1000, 0);
        key.encrypt(&mut payload);
        let mut block = vec![0x5a; ERASE];
        block[ERASE - payload.len()..].copy_from_slice(&payload);
        block[ERASE - 4..].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        block
    }

    fn old_block(env: &[u8]) -> Vec<u8> {
        let comp = lzokay_native::compress(env).unwrap();
        let mut payload = Vec::with_capacity(ENV_PAYLOAD);
        payload.extend_from_slice(&(comp.len() as u32).to_le_bytes());
        payload.extend_from_slice(&comp);
        payload.resize(payload.len().div_ceil(16) * 16, 0);
        UpgKey::vendor_static().encrypt(&mut payload);
        let mut block = vec![0xff; ERASE];
        let start = ERASE - ENV_PAYLOAD;
        block[start..start + payload.len()].copy_from_slice(&payload);
        block
    }

    fn vendor_env(kernel_len: usize) -> Vec<u8> {
        let norboot = [
            "run norargs".to_string(),
            "sf probe 0".into(),
            "cp 0x80800800 0x83000000 0xf800".into(),
            format!("sf read 0x80800000 0x40000 {kernel_len:#x}"),
            "mm w 0x80800000 0x10000".into(),
            "mm w 0x80810000 0x10000".into(),
            "mm w 0x80820000 0x10000".into(),
            "bootz 0x80800000 - 0x83000000".into(),
        ];
        let mut hewei = vec!["sf probe 0".to_string()];
        for i in 0..3 {
            hewei.push(format!(
                "sf read 0x80800000 {:#x} 0x10000",
                KERNEL + kernel_len + i * ERASE
            ));
            hewei.push("mm d 0x80800000 0x10000".into());
            hewei.push("mm m 0x80800000 0x10000".into());
            hewei.push(format!(
                "sf update 0x80800000 {:#x}  0x10000",
                KERNEL + i * ERASE
            ));
        }
        hewei.extend([
            " sf probe 0".to_string(),
            "sf read 0x80810000 0x2d000 0x3000".into(),
            "mm d 0x80810000 0x3000".into(),
            "mm z 0x80810000 0x2ff0 0x80800000 0x10000".into(),
            "mm m 0x80800000 0x10000".into(),
            "sf update 0x80800000 0x30000  0x10000".into(),
        ]);
        env_with(&[
            "bootdelay=3",
            &format!("norboot={};", norboot.join(";")),
            &format!("heweiencrypt={};", hewei.join(";")),
        ])
    }

    fn zimage(len: usize) -> Vec<u8> {
        let mut z: Vec<u8> = (0..len).map(|i| (i * 7 + i / 251) as u8).collect();
        z[0x24..0x28].copy_from_slice(&ZIMAGE_MAGIC.to_le_bytes());
        z
    }

    #[test]
    fn the_upgkey_is_the_fuses_in_hex() {
        let k = UpgKey::new(FUSES);
        assert_eq!(&k.key, b"0123abcd89ef4567");
        assert_eq!(&k.iv, b"deadbeef00000001");
    }

    #[test]
    fn the_fuses_are_read_from_the_ocotp_words() {
        let mut nvmem = vec![0u8; 0x100];
        for (word, value) in [
            (1, FUSES.cfg0),
            (2, FUSES.cfg1),
            (0x22, FUSES.mac0),
            (0x23, FUSES.mac1),
        ] {
            nvmem[word * 4..word * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let f = Fuses::from_ocotp(&nvmem).unwrap();
        assert_eq!(
            (f.cfg0, f.cfg1, f.mac0, f.mac1),
            (FUSES.cfg0, FUSES.cfg1, FUSES.mac0, FUSES.mac1)
        );
        assert!(Fuses::from_ocotp(&nvmem[..0x88]).is_none());
    }

    #[test]
    fn cbc_matches_the_published_test_vector() {
        let k = UpgKey {
            key: 0x2b7e151628aed2a6abf7158809cf4f3c_u128.to_be_bytes(),
            iv: 0x000102030405060708090a0b0c0d0e0f_u128.to_be_bytes(),
        };
        let plain = [
            0x6bc1bee22e409f96e93d7e117393172a_u128.to_be_bytes(),
            0xae2d8a571e03ac9c9eb76fac45af8e51_u128.to_be_bytes(),
        ]
        .concat();
        let mut data = plain.clone();
        k.encrypt(&mut data);
        assert_eq!(
            data[..16],
            0x7649abac8119b246cee98e9b12e9197d_u128.to_be_bytes()
        );
        assert_eq!(
            data[16..],
            0x5086cb9b507219ee95db113a917678b2_u128.to_be_bytes()
        );
        k.decrypt(&mut data);
        assert_eq!(data, plain);
    }

    #[test]
    fn the_update_environment_unpacks_and_checks_its_crc() {
        let key = UpgKey::new(FUSES);
        let env = vendor_env(0x302fd8);
        assert_eq!(read_env(&update_block(&env, &key), &key).unwrap(), env);

        let mut bad = env.clone();
        bad[0x10] ^= 1;
        assert!(
            read_env(&update_block(&bad, &key), &key)
                .unwrap_err()
                .contains("CRC")
        );

        let other = UpgKey::new(Fuses { mac1: 2, ..FUSES });
        assert!(read_env(&update_block(&env, &key), &other).is_err());
    }

    #[test]
    fn the_old_firmware_environment_unpacks_with_the_static_key() {
        let env = vendor_env(0x302fd8);
        let block = old_block(&env);
        assert_eq!(read_env(&block, &UpgKey::vendor_static()).unwrap(), env);
        assert_eq!(read_env_auto(FUSES, &block).unwrap().1, env);
        assert_eq!(
            read_env_auto(FUSES, &update_block(&env, &UpgKey::new(FUSES)))
                .unwrap()
                .1,
            env
        );
    }

    #[test]
    fn refit_moves_the_window_and_the_flashing_block_round_trips() {
        let env = vendor_env(0x267f38);
        let refit = refit_env(&env, 0x2e86d9, 0x340000).unwrap();
        assert_eq!(
            layout(&refit, 0x340000).unwrap(),
            Layout {
                kernel_len: 0x2e86d9,
                staging: KERNEL + 0x2e86d9,
            }
        );

        let block = flashing_block(&refit).unwrap();
        assert_eq!(&block[ERASE - 4..], MARKER);
        let mut dec = block.clone();
        UpgKey::vendor_static().decrypt(&mut dec);
        let mut crc = flate2::Crc::new();
        crc.update(&dec[4..ENV_VARS_END]);
        assert_eq!(crc.sum(), u32::from_le_bytes(dec[..4].try_into().unwrap()));
        assert!(env_var(&dec, "norboot").unwrap().contains("0x2e86d9"));
        assert!(
            env_var(&dec, "heweiencrypt")
                .unwrap()
                .contains(&format!("{:#x}", KERNEL + 0x2e86d9))
        );

        assert!(refit_env(&env, 0x340000, 0x340000).is_err());
    }

    #[test]
    fn the_layout_comes_from_norboot_and_heweiencrypt() {
        let layout = layout(&vendor_env(0x302fd8), 0x340000).unwrap();
        assert_eq!(
            layout,
            Layout {
                kernel_len: 0x302fd8,
                staging: 0x342fd8
            }
        );
        assert_eq!(env_var(&vendor_env(0x302fd8), "bootdelay"), Some("3"));
    }

    #[test]
    fn a_layout_that_leaves_the_partition_is_refused() {
        assert!(layout(&vendor_env(0x302fd8), 0x320000).is_err());
        assert!(layout(&env_with(&["norboot=bootz 0x80800000"]), 0x340000).is_err());
    }

    #[test]
    fn the_head_goes_into_the_staging_blocks_and_the_rest_stays_clear() {
        let key = UpgKey::new(FUSES);
        let layout = Layout {
            kernel_len: 0x300000,
            staging: 0x340000,
        };
        let current: Vec<u8> = (0..0x340000).map(|i| (i % 13) as u8).collect();
        let z = zimage(0x2f0001);
        let part = kernel_partition(&current, &z, &layout, &key).unwrap();

        assert_eq!(part[..HEAD], current[..HEAD]);
        assert_eq!(part[HEAD..z.len()], z[HEAD..]);
        assert!(part[z.len()..0x300000].iter().all(|&b| b == 0xff));
        let mut staged = part[0x300000..0x330000].to_vec();
        for block in staged.as_chunks_mut::<ERASE>().0 {
            key.decrypt(block);
        }
        assert_eq!(staged, z[..HEAD]);
        assert!(part[0x330000..].iter().all(|&b| b == 0xff));
    }

    #[test]
    fn staging_reads_the_layout_from_the_uboot_partition() {
        let key = UpgKey::new(FUSES);
        let mut uboot = vec![0x5a; ENV_BLOCK];
        uboot[UPDATE_BLOCK..].copy_from_slice(&update_block(&vendor_env(0x302fd8), &key));
        let current: Vec<u8> = (0..0x340000).map(|i| (i % 13) as u8).collect();
        let z = zimage(0x2f0001);

        let layout = Layout {
            kernel_len: 0x302fd8,
            staging: 0x342fd8,
        };
        assert_eq!(
            stage(&z, &current, &uboot, FUSES).unwrap(),
            kernel_partition(&current, &z, &layout, &key).unwrap()
        );
        assert!(stage(&z, &current, &uboot, Fuses { mac1: 2, ..FUSES }).is_err());
        assert!(stage(&z, &current, &uboot[..UPDATE_BLOCK], FUSES).is_err());
    }

    #[test]
    fn a_zimage_uboot_would_cut_off_is_refused() {
        let key = UpgKey::new(FUSES);
        let layout = Layout {
            kernel_len: 0x100000,
            staging: 0x140000,
        };
        let current = vec![0; 0x340000];
        assert!(kernel_partition(&current, &zimage(0x100001), &layout, &key).is_err());
        assert!(kernel_partition(&current, &vec![0; 0x100000], &layout, &key).is_err());
    }

    #[test]
    #[ignore]
    fn the_stock_kernel_packs_back_to_the_flash() {
        let var = |n: &str| std::env::var(n).unwrap_or_else(|_| panic!("{n} not set"));
        let f: Vec<u32> = var("LIVI_IMX6UL_FUSES")
            .split(',')
            .map(|v| u32::from_str_radix(v.trim().trim_start_matches("0x"), 16).unwrap())
            .collect();
        let key = UpgKey::new(Fuses {
            cfg0: f[0],
            cfg1: f[1],
            mac0: f[2],
            mac1: f[3],
        });
        let mtd0 = std::fs::read(var("LIVI_IMX6UL_MTD0")).unwrap();
        let mtd1 = std::fs::read(var("LIVI_IMX6UL_MTD1")).unwrap();
        let zimage = std::fs::read(var("LIVI_IMX6UL_ZIMAGE")).unwrap();

        let env = read_env(&mtd0[UPDATE_BLOCK..UPDATE_BLOCK + ERASE], &key).unwrap();
        let layout = layout(&env, mtd1.len()).unwrap();
        eprintln!("{layout:?}");
        let part = kernel_partition(&mtd1, &zimage, &layout, &key).unwrap();
        let diff: Vec<usize> = (0..part.len()).filter(|&i| part[i] != mtd1[i]).collect();
        assert!(
            diff.is_empty(),
            "{} bytes differ, first at {:#x}, last at {:#x}",
            diff.len(),
            diff[0] + KERNEL,
            diff[diff.len() - 1] + KERNEL
        );
    }
}
