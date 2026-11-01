// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};
use std::time::Duration;

use imx6ul_uboot::{ENV_BLOCK, ERASE, Fuses, UPDATE_BLOCK, read_env_auto};

use super::Running;
use super::boot::read_fuses;
use super::mtd::{self, Partition, sha256_hex};
use super::shell::{PUSH_PORT, Shell, md5_hex};
use crate::tilde;

const UBOOT: usize = 0x40000;
const KERNEL: usize = 0x340000;
const ROOTFS: usize = 0xc80000;
const JFFS2_MAGIC: [u8; 2] = [0x85, 0x19];
const TMP: &str = "/tmp/livi-restore.img";

pub struct Stock {
    pub dir: PathBuf,
    uboot: Vec<u8>,
    kernel: Vec<u8>,
    rootfs: Vec<u8>,
}

impl Stock {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("manifest.json");
        let manifest =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", tilde(&path)))?;
        if !manifest.contains("\"state\": \"stock\"") {
            return Err(format!(
                "{} is no backup of the vendor firmware",
                tilde(dir)
            ));
        }
        let read = |name: &str, size: usize| -> Result<Vec<u8>, String> {
            let file = format!("{name}.img");
            let path = dir.join(&file);
            let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", tilde(&path)))?;
            if data.len() != size {
                return Err(format!(
                    "{file} is {} bytes, the vendor's {name} {size}",
                    data.len()
                ));
            }
            match manifest_sha256(&manifest, &file) {
                Some(sha) if sha == sha256_hex(&data) => Ok(data),
                Some(_) => Err(format!("{} does not match its manifest", tilde(&path))),
                None => Err(format!("the manifest in {} names no {file}", tilde(dir))),
            }
        };
        let stock = Self {
            dir: dir.to_path_buf(),
            uboot: read("uboot", UBOOT)?,
            kernel: read("kernel", KERNEL)?,
            rootfs: read("rootfs", ROOTFS)?,
        };
        if !stock
            .rootfs
            .chunks(ERASE)
            .any(|block| block.starts_with(&JFFS2_MAGIC))
        {
            return Err(format!(
                "the rootfs in {} is not the vendor's jffs2",
                tilde(dir)
            ));
        }
        Ok(stock)
    }

    fn belongs_to(&self, uboot: &[u8], fuses: Fuses) -> bool {
        uboot.len() >= ENV_BLOCK
            && self.uboot[..ENV_BLOCK] == uboot[..ENV_BLOCK]
            && read_env_auto(fuses, &self.uboot[UPDATE_BLOCK..UPDATE_BLOCK + ERASE]).is_ok()
    }
}

fn manifest_sha256<'a>(manifest: &'a str, file: &str) -> Option<&'a str> {
    let entry = manifest
        .lines()
        .find(|l| l.contains(&format!("\"file\": \"{file}\"")))?;
    entry.split("\"sha256\": \"").nth(1)?.split('"').next()
}

fn stock_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<(String, PathBuf)> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            Some((name.split_once("-stock-")?.1.to_string(), e.path()))
        })
        .collect();
    dirs.sort_by(|a, b| b.0.cmp(&a.0));
    dirs.into_iter().map(|(_, path)| path).collect()
}

pub fn find(
    sh: &Shell,
    root: &Path,
    dir: Option<&Path>,
    progress: &dyn Fn(&str),
) -> Result<Stock, String> {
    let part = mtd::partitions(sh)?
        .into_iter()
        .find(|p| p.name == "uboot")
        .ok_or("no uboot partition in /proc/mtd")?;
    let uboot = mtd::pull(sh, &part, progress)?;
    let fuses = read_fuses(sh)?;

    if let Some(dir) = dir {
        let stock = Stock::load(dir)?;
        return if stock.belongs_to(&uboot, fuses) {
            Ok(stock)
        } else {
            Err(format!("{} is the backup of another dongle", tilde(dir)))
        };
    }
    stock_dirs(root)
        .into_iter()
        .filter_map(|dir| Stock::load(&dir).ok())
        .find(|stock| stock.belongs_to(&uboot, fuses))
        .ok_or_else(|| {
            format!(
                "no backup of this dongle's vendor firmware under {}",
                tilde(root)
            )
        })
}

pub fn write(sh: &Shell, stock: &Stock, progress: &dyn Fn(&str)) -> Result<(), String> {
    if super::running(sh)? != Running::Rescue {
        return Err("only the rescue system can write the vendor firmware back".into());
    }
    let parts = mtd::partitions(sh)?;
    let plan = plan(&parts, stock)?;
    if sh.sh("command -v flashcp")?.trim().is_empty() {
        return Err("no flashcp on the device".into());
    }
    for (part, image) in plan {
        sh.push(image, TMP, PUSH_PORT, &md5_hex(image))?;
        sh.run(
            &format!("flashcp {TMP} {} && sync", part.device),
            Duration::from_secs(600),
        )?;
        let there = sh.run(
            &format!("sha256sum {}", part.device),
            Duration::from_secs(240),
        )?;
        sh.sh(&format!("rm -f {TMP}"))?;
        if there.split_whitespace().next() != Some(sha256_hex(image).as_str()) {
            return Err(format!(
                "{} does not read back as written, do not reboot",
                part.device
            ));
        }
        progress(&format!(
            "wrote and verified {} ({})",
            part.device, part.name
        ));
    }
    Ok(())
}

fn plan<'a>(
    parts: &'a [Partition],
    stock: &'a Stock,
) -> Result<Vec<(&'a Partition, &'a [u8])>, String> {
    let get = |name: &str| {
        parts
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| format!("no {name} partition in /proc/mtd"))
    };
    let span = [get("rootfs")?, get("customer")?, get("bootstate")?];
    let (kernel, env) = (get("kernel")?, get("env")?);
    if span.iter().map(|p| p.bytes as usize).sum::<usize>() != ROOTFS
        || kernel.bytes as usize != KERNEL
        || env.bytes as usize != UBOOT - ENV_BLOCK
    {
        return Err("the partitions do not add up to the vendor's layout".into());
    }
    let mut plan = Vec::new();
    let mut at = 0;
    for part in span {
        let end = at + part.bytes as usize;
        plan.push((part, &stock.rootfs[at..end]));
        at = end;
    }
    plan.push((kernel, stock.kernel.as_slice()));
    plan.push((env, &stock.uboot[ENV_BLOCK..]));
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(name: &str, n: u32, bytes: usize) -> Partition {
        Partition {
            name: name.into(),
            device: format!("/dev/mtd{n}"),
            bytes: bytes as u64,
        }
    }

    fn ours() -> Vec<Partition> {
        vec![
            part("uboot", 0, 0x30000),
            part("env", 1, 0x10000),
            part("kernel", 2, 0x340000),
            part("rootfs", 3, 0xc60000),
            part("customer", 4, 0x10000),
            part("bootstate", 5, 0x10000),
        ]
    }

    fn stock() -> Stock {
        let mut rootfs = vec![0u8; ROOTFS];
        rootfs[..2].copy_from_slice(&JFFS2_MAGIC);
        rootfs[0xc60000] = 0xc0;
        rootfs[0xc70000] = 0xb5;
        let mut uboot = vec![0u8; UBOOT];
        uboot[ENV_BLOCK] = 0xe0;
        Stock {
            dir: PathBuf::new(),
            uboot,
            kernel: vec![0x4b; KERNEL],
            rootfs,
        }
    }

    #[test]
    fn the_vendor_rootfs_spans_three_of_our_partitions_before_kernel_and_env() {
        let parts = ours();
        let stock = stock();
        let plan = plan(&parts, &stock).unwrap();
        let names: Vec<&str> = plan.iter().map(|(p, _)| p.name.as_str()).collect();
        assert_eq!(names, ["rootfs", "customer", "bootstate", "kernel", "env"]);
        assert_eq!(plan[0].1.len(), 0xc60000);
        assert_eq!(plan[1].1[0], 0xc0);
        assert_eq!(plan[2].1[0], 0xb5);
        assert_eq!(plan[3].1.len(), KERNEL);
        assert_eq!(plan[4].1[0], 0xe0);
    }

    #[test]
    fn a_layout_that_does_not_add_up_is_refused() {
        let mut parts = ours();
        parts[3].bytes = 0xc50000;
        assert!(plan(&parts, &stock()).is_err());
        assert!(plan(&ours()[..4], &stock()).is_err());
    }

    #[test]
    fn the_manifest_hash_is_found_per_file() {
        let manifest = "{\n  \"partitions\": [\n    { \"name\": \"uboot\", \"sha256\": \"aa\", \"file\": \"uboot.img\" },\n    { \"name\": \"kernel\", \"sha256\": \"bb\", \"file\": \"kernel.img\" }\n  ]\n}\n";
        assert_eq!(manifest_sha256(manifest, "kernel.img"), Some("bb"));
        assert_eq!(manifest_sha256(manifest, "rootfs.img"), None);
    }

    #[test]
    fn stock_backups_come_newest_first() {
        let root = std::env::temp_dir().join(format!("livi-restore-test-{}", std::process::id()));
        for name in [
            "Auto_Box-2025.04.17.1054-stock-20260930T200158Z",
            "Auto_Box-2025.04.17.1054-stock-20260930T210000Z",
            "imx6ul-kernel-20260930T220000Z",
            "dongle-unknown-livi-link-20260930T230000Z",
        ] {
            std::fs::create_dir_all(root.join(name)).unwrap();
        }
        let found: Vec<String> = stock_dirs(&root)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            found,
            [
                "Auto_Box-2025.04.17.1054-stock-20260930T210000Z",
                "Auto_Box-2025.04.17.1054-stock-20260930T200158Z"
            ]
        );
    }
}
