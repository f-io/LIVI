// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use imx6ul_uboot::{
    ENV_BLOCK, ERASE, Fuses, KERNEL, Layout, UPDATE_BLOCK, flashing_block, kernel_partition,
    layout, read_env_auto, refit_env,
};

use super::Running;
use super::mtd::{self, Partition, sha256_hex};
use super::shell::{PUSH_PORT, Shell, md5_hex};

enum EnvAt {
    Uboot,
    Own(Partition),
}

pub struct Install {
    pub uboot: Partition,
    env: EnvAt,
    pub kernel: Partition,
    pub layout: Layout,
    pub partition: Vec<u8>,
    pub unchanged: bool,
    pub backup: PathBuf,
    uboot_image: Vec<u8>,
    boot_block: Option<Vec<u8>>,
}

const TMP_IMAGE: &str = "/tmp/livi-kernel.img";
const TMP_ROOTFS: &str = "/tmp/livi-rootfs.img";

const TYPE_KERNEL: u8 = 2;
const TYPE_ROOTFS: u8 = 3;

pub struct Bundle {
    pub kernel: Option<Vec<u8>>,
    pub rootfs: Option<Vec<u8>>,
}

pub fn read_bundle(bytes: &[u8]) -> Result<Bundle, String> {
    let mut bundle = Bundle {
        kernel: None,
        rootfs: None,
    };
    for (typ, data) in crate::dongle::lfwb::unpack(bytes)? {
        match typ {
            TYPE_KERNEL => bundle.kernel = Some(data),
            TYPE_ROOTFS if data.starts_with(b"hsqs") => bundle.rootfs = Some(data),
            TYPE_ROOTFS => return Err("the rootfs image is not a squashfs".into()),
            other => return Err(format!("image type {other} is not for this dongle")),
        }
    }
    Ok(bundle)
}

pub fn write_rootfs(
    sh: &Shell,
    image: &[u8],
    backup: &Path,
    progress: &dyn Fn(&str),
) -> Result<(), String> {
    let parts = mtd::partitions(sh)?;
    let part = parts
        .iter()
        .find(|p| p.name == "rootfs")
        .ok_or("no rootfs partition in /proc/mtd")?;
    if image.len() as u64 > part.bytes {
        return Err(format!(
            "rootfs image is {} bytes, the partition {}",
            image.len(),
            part.bytes
        ));
    }
    if !image.len().is_multiple_of(4096) {
        return Err(format!(
            "rootfs image is {} bytes, not a whole number of 4 KiB blocks",
            image.len()
        ));
    }
    match super::running(sh)? {
        Running::Vendor => {
            return Err(
                "the vendor system runs from the rootfs partition, install our kernel first".into(),
            );
        }
        Running::Livi => {
            return Err(
                "LIVI Link runs from the rootfs partition, update it through its web page".into(),
            );
        }
        Running::Rescue => {}
    }
    if sh.sh("command -v flashcp")?.trim().is_empty() {
        return Err("no flashcp on the device".into());
    }

    let current = mtd::pull(sh, part, progress)?;
    std::fs::create_dir_all(backup).map_err(|e| format!("{}: {e}", crate::tilde(backup)))?;
    let path = backup.join("rootfs.img");
    std::fs::write(&path, current).map_err(|e| format!("{}: {e}", crate::tilde(&path)))?;
    progress(&format!("saved rootfs to {}", crate::tilde(&path)));

    let dev = &part.device;
    sh.push(image, TMP_ROOTFS, PUSH_PORT, &md5_hex(image))?;
    sh.run(
        &format!("flashcp {TMP_ROOTFS} {dev} && sync"),
        Duration::from_secs(600),
    )?;
    let there = sh.run(
        &format!(
            "dd if={dev} bs=4096 count={} 2>/dev/null | sha256sum",
            image.len() / 4096
        ),
        Duration::from_secs(240),
    )?;
    sh.sh(&format!("rm -f {TMP_ROOTFS}"))?;
    if there.split_whitespace().next() != Some(sha256_hex(image).as_str()) {
        return Err(format!(
            "{dev} does not read back as written, do not reboot"
        ));
    }
    progress(&format!("wrote and verified {dev}"));

    if let Some(state) = parts.iter().find(|p| p.name == "bootstate") {
        sh.sh(&format!("flash_eraseall -q {}", state.device))?;
        progress("cleared the bootstate mark");
    }
    Ok(())
}

pub fn prepare(
    sh: &Shell,
    zimage: &[u8],
    backup_root: &Path,
    progress: &dyn Fn(&str),
) -> Result<Install, String> {
    let soc = sh.sh("cat /sys/devices/soc0/soc_id 2>/dev/null")?;
    if !matches!(soc.trim(), "i.MX6UL" | "i.MX6ULL" | "i.MX6ULZ") {
        return Err(format!("not an i.MX6UL/ULL/ULZ: {:?}", soc.trim()));
    }
    let (uboot, env, kernel) = locate(&mtd::partitions(sh)?)?;
    let erase = sh.sh(&format!(
        "cat /sys/class/mtd/{}/erasesize",
        sysfs_name(&kernel)
    ))?;
    if erase.trim() != ERASE.to_string() {
        return Err(format!("erase size is {}, not {ERASE}", erase.trim()));
    }
    let fuses = read_fuses(sh)?;

    let uboot_image = mtd::pull(sh, &uboot, progress)?;
    let kernel_image = mtd::pull(sh, &kernel, progress)?;
    let backup = backup_root.join(format!("imx6ul-kernel-{}", mtd::stamp()));
    std::fs::create_dir_all(&backup).map_err(|e| format!("{}: {e}", crate::tilde(&backup)))?;
    for (name, image) in [("uboot.img", &uboot_image), ("kernel.img", &kernel_image)] {
        let path = backup.join(name);
        std::fs::write(&path, image).map_err(|e| format!("{}: {e}", crate::tilde(&path)))?;
    }
    progress(&format!(
        "saved uboot and kernel to {}",
        crate::tilde(&backup)
    ));

    let update_block = &uboot_image[UPDATE_BLOCK..UPDATE_BLOCK + ERASE];
    let (key, update) = read_env_auto(fuses, update_block)?;
    let mut layout = layout(&update, kernel_image.len())?;
    let mut boot_block = None;
    if zimage.len() > layout.kernel_len {
        let window = layout.kernel_len;
        let word = u32::from_le_bytes(update_block[ERASE - 4..].try_into().unwrap()) as usize;
        let legacy = !(word != 0 && word <= ERASE && word.is_multiple_of(16));
        if !legacy || !matches!(env, EnvAt::Uboot) {
            return Err(format!(
                "zImage is {} bytes, this dongle's U-Boot reads {}",
                zimage.len(),
                layout.kernel_len
            ));
        }
        let refit = refit_env(&update, zimage.len(), kernel_image.len())?;
        layout = imx6ul_uboot::layout(&refit, kernel_image.len())?;
        boot_block = Some(flashing_block(&refit)?);
        progress(&format!(
            "kernel needs {:#x}, enlarging this dongle's {window:#x} boot window",
            zimage.len()
        ));
    }
    let partition = kernel_partition(&kernel_image, zimage, &layout, &key)?;
    let unchanged = partition == kernel_image;
    Ok(Install {
        uboot,
        env,
        kernel,
        layout,
        partition,
        unchanged,
        backup,
        uboot_image,
        boot_block,
    })
}

pub fn install(sh: &Shell, plan: &Install, progress: &dyn Fn(&str)) -> Result<(), String> {
    let tools = sh.sh(
        "for t in flash_erase flashcp flash_eraseall; do command -v $t >/dev/null && echo $t; done",
    )?;
    let has = |t: &str| tools.split_whitespace().any(|w| w == t);
    let erase_all = |dev: &str| {
        if has("flash_erase") {
            Ok(format!("flash_erase {dev} 0 0 >/dev/null"))
        } else if has("flash_eraseall") {
            Ok(format!("flash_eraseall -q {dev}"))
        } else {
            Err("neither flash_erase nor flash_eraseall on the device".to_string())
        }
    };

    if !plan.unchanged {
        sh.push(
            &plan.partition,
            TMP_IMAGE,
            PUSH_PORT,
            &md5_hex(&plan.partition),
        )?;
        let dev = &plan.kernel.device;
        let write = if has("flash_erase") {
            format!("{} && cat {TMP_IMAGE} > {dev}", erase_all(dev)?)
        } else if has("flashcp") {
            format!("flashcp {TMP_IMAGE} {dev}")
        } else {
            return Err("nothing on the device can write the kernel partition".into());
        };
        sh.run(&format!("{write} && sync"), Duration::from_secs(300))?;
        let there = sh.run(&format!("sha256sum {dev}"), Duration::from_secs(240))?;
        sh.sh(&format!("rm -f {TMP_IMAGE}"))?;
        if there.split_whitespace().next() != Some(sha256_hex(&plan.partition).as_str()) {
            return Err(format!(
                "{dev} does not read back as written, do not reboot"
            ));
        }
        progress(&format!("wrote and verified {dev}"));
    }

    if let Some(block) = &plan.boot_block {
        if !has("flash_erase") {
            return Err("no flash_erase to rewrite the boot block".into());
        }
        sh.push(block, TMP_IMAGE, PUSH_PORT, &md5_hex(block))?;
        let dev = &plan.uboot.device;
        let seek = ENV_BLOCK / ERASE;
        sh.run(
            &format!(
                "flash_erase {dev} {ENV_BLOCK} 1 >/dev/null && \
                 dd if={TMP_IMAGE} of={dev} bs={ERASE} seek={seek} conv=notrunc 2>/dev/null && sync"
            ),
            Duration::from_secs(120),
        )?;
        sh.sh(&format!("rm -f {TMP_IMAGE}"))?;
        let after = mtd::pull(sh, &plan.uboot, progress)?;
        if after[..ENV_BLOCK] != plan.uboot_image[..ENV_BLOCK] {
            return Err(
                "U-Boot changed although only the boot block was written, do not reboot".into(),
            );
        }
        if &after[ENV_BLOCK..] != block.as_slice() {
            return Err("the boot block did not read back as written, do not reboot".into());
        }
        progress("boot block written with the enlarged kernel window, U-Boot itself unchanged");
    } else {
        let erase = match &plan.env {
            EnvAt::Uboot if has("flash_erase") => {
                format!("flash_erase {} {ENV_BLOCK} 1 >/dev/null", plan.uboot.device)
            }
            EnvAt::Uboot => {
                return Err("no flash_erase to erase one block of the uboot partition".into());
            }
            EnvAt::Own(p) => erase_all(&p.device)?,
        };
        sh.run(&format!("{erase} && sync"), Duration::from_secs(60))?;
        let after = mtd::pull(sh, &plan.uboot, progress)?;
        if after[..UPDATE_BLOCK + ERASE] != plan.uboot_image[..UPDATE_BLOCK + ERASE] {
            return Err(
                "U-Boot changed although only the environment block was erased, do not reboot"
                    .into(),
            );
        }
        if env_block(sh, &plan.uboot, &plan.env, progress)?
            .iter()
            .any(|&b| b != 0xff)
        {
            return Err("the environment block did not erase".into());
        }
        progress("environment block erased, U-Boot itself unchanged");
    }

    progress("rebooting, U-Boot converts the staging blocks and resets once more");
    sh.sh("(sleep 1; sync; reboot; sleep 5; reboot -f) >/dev/null 2>&1 &")?;
    let gone = Instant::now();
    while sh.reachable() && gone.elapsed() < Duration::from_secs(30) {
        sleep(Duration::from_secs(1));
    }
    super::wait_for_dongle(sh, progress)?;

    let (uboot, env, _) = locate(&mtd::partitions(sh)?)?;
    if env_block(sh, &uboot, &env, progress)?
        .iter()
        .all(|&b| b == 0xff)
    {
        return Err("the dongle is back but U-Boot did not write a new environment".into());
    }
    progress("U-Boot wrote the environment block again, conversion done");
    Ok(())
}

fn locate(parts: &[Partition]) -> Result<(Partition, EnvAt, Partition), String> {
    let at = parts
        .iter()
        .position(|p| p.name == "uboot")
        .ok_or("no uboot partition in /proc/mtd")?;
    let next = |i: usize, name: &str| parts.get(at + i).filter(|p| p.name == name).cloned();
    let uboot = parts[at].clone();
    let found = match uboot.bytes as usize {
        KERNEL => next(1, "kernel").map(|k| (uboot, EnvAt::Uboot, k)),
        ENV_BLOCK => match (next(1, "env"), next(2, "kernel")) {
            (Some(e), Some(k)) if e.bytes == ERASE as u64 => Some((uboot, EnvAt::Own(e), k)),
            _ => None,
        },
        _ => None,
    };
    found.ok_or_else(|| {
        "partitions are neither the vendor's (uboot 256K, kernel) nor ours (uboot 192K, env 64K, kernel)"
            .to_string()
    })
}

fn env_block(
    sh: &Shell,
    uboot: &Partition,
    env: &EnvAt,
    progress: &dyn Fn(&str),
) -> Result<Vec<u8>, String> {
    match env {
        EnvAt::Uboot => Ok(mtd::pull(sh, uboot, progress)?.split_off(ENV_BLOCK)),
        EnvAt::Own(p) => mtd::pull(sh, p, progress),
    }
}

pub(super) fn read_fuses(sh: &Shell) -> Result<Fuses, String> {
    let out = sh.sh(
        "if [ -d /sys/fsl_otp ]; then cd /sys/fsl_otp && cat HW_OCOTP_CFG0 HW_OCOTP_CFG1 HW_OCOTP_MAC0 HW_OCOTP_MAC1; \
         else n=/sys/bus/nvmem/devices/imx-ocotp0/nvmem; \
         hexdump -s 4 -n 8 -e '1/4 \"%08x\\n\"' $n && hexdump -s 136 -n 8 -e '1/4 \"%08x\\n\"' $n; fi",
    )?;
    let words: Option<Vec<u32>> = out
        .split_whitespace()
        .map(|w| u32::from_str_radix(w.trim_start_matches("0x"), 16).ok())
        .collect();
    match words.as_deref() {
        Some(&[cfg0, cfg1, mac0, mac1]) => Ok(Fuses {
            cfg0,
            cfg1,
            mac0,
            mac1,
        }),
        _ => Err(format!("could not read the fuses: {out:?}")),
    }
}

fn sysfs_name(p: &Partition) -> &str {
    p.device.trim_start_matches("/dev/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_splits_into_kernel_and_rootfs() {
        let kernel = vec![0x5a; 0x40000];
        let rootfs = [b"hsqs".as_slice(), &[0u8; 60]].concat();
        let bytes = crate::dongle::lfwb::pack(&[(2, &kernel), (3, &rootfs)]);
        let bundle = read_bundle(&bytes).unwrap();
        assert_eq!(bundle.kernel.as_deref(), Some(kernel.as_slice()));
        assert_eq!(bundle.rootfs.as_deref(), Some(rootfs.as_slice()));

        assert!(read_bundle(&crate::dongle::lfwb::pack(&[(3, &kernel)])).is_err());
        assert!(read_bundle(&crate::dongle::lfwb::pack(&[(1, &rootfs)])).is_err());
    }

    fn part(name: &str, n: u32, bytes: usize) -> Partition {
        Partition {
            name: name.into(),
            device: format!("/dev/mtd{n}"),
            bytes: bytes as u64,
        }
    }

    #[test]
    fn the_vendor_layout_and_ours_are_both_found() {
        let vendor = [
            part("uboot", 0, 0x40000),
            part("kernel", 1, 0x340000),
            part("rootfs", 2, 0xc80000),
        ];
        let (_, env, kernel) = locate(&vendor).unwrap();
        assert!(matches!(env, EnvAt::Uboot));
        assert_eq!(kernel.device, "/dev/mtd1");

        let ours = [
            part("uboot", 0, 0x30000),
            part("env", 1, 0x10000),
            part("kernel", 2, 0x340000),
            part("rootfs", 3, 0xc80000),
        ];
        let (_, env, kernel) = locate(&ours).unwrap();
        assert!(matches!(env, EnvAt::Own(ref e) if e.device == "/dev/mtd1"));
        assert_eq!(kernel.device, "/dev/mtd2");

        assert!(locate(&[part("uboot", 0, 0x40000), part("rootfs", 1, 0xc80000)]).is_err());
        assert!(locate(&[part("uboot", 0, 0x30000), part("kernel", 1, 0x340000)]).is_err());
    }
}
