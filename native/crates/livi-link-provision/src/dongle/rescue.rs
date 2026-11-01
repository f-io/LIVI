// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::arm::ax520;
use super::arm::imx6ul::shell::Shell;
use super::probe::Family;
use super::riscv::v821b;
use super::{Remote, link, mtd_is};

pub fn family(compatible: &str) -> Option<Family> {
    if compatible.contains("allwinner,sun300i-v821b") {
        Some(Family::V821b)
    } else if compatible.contains("axera,ax520") {
        Some(Family::Ax520)
    } else {
        None
    }
}

pub fn of(sh: &Shell) -> Option<Family> {
    family(&sh.sh("cat /proc/device-tree/compatible 2>/dev/null").ok()?)
}

fn slots(family: Family) -> [(u8, &'static str, u64); 2] {
    match family {
        Family::V821b => [
            (1, "boot", v821b::MTD1_SIZE),
            (3, "rootfs", v821b::MTD3_SIZE),
        ],
        Family::Ax520 => [
            (ax520::BOOT_MTD, "boot", ax520::BOOT_SIZE),
            (ax520::ROOTFS_MTD, "rootfs", ax520::ROOTFS_SIZE),
        ],
    }
}

fn layout_fits(family: Family, proc_mtd: &str) -> bool {
    slots(family)
        .iter()
        .all(|&(index, name, size)| mtd_is(proc_mtd, index, name, size))
}

pub fn reflash(sh: &mut Shell, family: Family) -> Result<(), String> {
    let proc_mtd = Remote::run(sh, "cat /proc/mtd")?;
    if !layout_fits(family, &proc_mtd) {
        return Err(format!(
            "the partitions of this dongle are not the ones the {} bundle is for",
            family.name()
        ));
    }
    let target = family.target();
    let bundle = link::bundle(target).ok_or_else(|| {
        format!("no {target} firmware baked in, this is a local build without CI assets")
    })?;
    match family {
        Family::V821b => v821b::write_bundle(sh, bundle)?,
        Family::Ax520 => ax520::write_bundle(sh, bundle)?,
    }
    println!("== restarting");
    sh.sh("(sleep 1; sync; reboot -f) >/dev/null 2>&1 &")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const V821B_MTD: &str = "dev:    size   erasesize  name
mtd0: 00060000 00010000 \"uboot\"
mtd1: 00310000 00010000 \"boot\"
mtd2: 00310000 00010000 \"recovery\"
mtd3: 00480000 00010000 \"rootfs\"
mtd4: 003a0000 00010000 \"customer\"
";
    const AX520_MTD: &str = "dev:    size   erasesize  name
mtd0: 00030000 00010000 \"muboot\"
mtd1: 00010000 00010000 \"env\"
mtd2: 00010000 00010000 \"env-redund\"
mtd3: 00300000 00010000 \"boot\"
mtd4: 00300000 00010000 \"recovery\"
mtd5: 00440000 00010000 \"customer\"
mtd6: 00440000 00010000 \"rootfs\"
";

    #[test]
    fn the_device_tree_names_the_board() {
        assert_eq!(
            family("livi,link-v821ballwinner,sun300i-v821b"),
            Some(Family::V821b)
        );
        assert_eq!(family("axera,ax520"), Some(Family::Ax520));
    }

    #[test]
    fn an_imx6ul_or_no_answer_is_none_of_these() {
        assert_eq!(family("livi,link-imx6ullfsl,imx6ull"), None);
        assert_eq!(family("fsl,imx6ull-14x14-evkfsl,imx6ull"), None);
        assert_eq!(family(""), None);
    }

    #[test]
    fn each_board_fits_its_own_table_only() {
        assert!(layout_fits(Family::V821b, V821B_MTD));
        assert!(layout_fits(Family::Ax520, AX520_MTD));
        assert!(!layout_fits(Family::V821b, AX520_MTD));
        assert!(!layout_fits(Family::Ax520, V821B_MTD));
    }
}
