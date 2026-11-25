// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod arm;
pub mod backup;
pub mod hook;
pub mod lfwb;
pub mod link;
pub mod ota;
pub mod probe;
pub mod rescue;
pub mod riscv;
pub mod shell;
pub mod web;

pub const DONGLE_HOST: &str = "192.168.50.100";
pub const BIND_SHELL_PORT: u16 = 2323;

pub trait Remote {
    fn run(&mut self, cmd: &str) -> Result<String, String>;
    fn write_mtd(&mut self, node: &str, data: &[u8]) -> Result<(), String>;
}

pub(crate) struct MtdPart {
    pub index: u8,
    pub size: u64,
    pub name: String,
}

pub(crate) fn parse_mtd(proc_mtd: &str) -> Vec<MtdPart> {
    proc_mtd
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("mtd")?;
            let (idx, rest) = rest.split_once(':')?;
            let index = idx.parse().ok()?;
            let mut fields = rest.split_whitespace();
            let size = u64::from_str_radix(fields.next()?, 16).ok()?;
            let name = fields.nth(1)?.trim_matches('"').to_string();
            Some(MtdPart { index, size, name })
        })
        .collect()
}

pub(crate) fn mtd_is(proc_mtd: &str, index: u8, name: &str, size: u64) -> bool {
    parse_mtd(proc_mtd)
        .iter()
        .any(|p| p.index == index && p.name == name && p.size == size)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LY10201_MTD: &str = "dev:    size   erasesize  name\n\
        mtd0: 00080000 00010000 \"uboot\"\n\
        mtd1: 00260000 00010000 \"boot\"\n\
        mtd2: 00260000 00010000 \"recovery\"\n\
        mtd3: 00440000 00010000 \"rootfs\"\n\
        mtd4: 00040000 00010000 \"env\"\n\
        mtd5: 005a0000 00010000 \"customer\"\n\
        mtd6: 00010000 00010000 \"private\"\n\
        mtd7: 00090000 00010000 \"UDISK\"\n";

    #[test]
    fn every_partition_is_parsed_off_proc_mtd() {
        let parts = parse_mtd(LY10201_MTD);
        assert_eq!(parts.len(), 8);
        assert_eq!(parts[0].index, 0);
        assert_eq!(parts[0].name, "uboot");
        assert_eq!(parts[0].size, 0x0008_0000);
        assert_eq!(parts[5].name, "customer");
        assert_eq!(parts[5].size, 0x005a_0000);
        assert_eq!(parts[7].name, "UDISK");
    }

    #[test]
    fn the_header_line_is_not_a_partition() {
        assert!(parse_mtd("dev:    size   erasesize  name\n").is_empty());
    }

    #[test]
    fn mtd_is_still_matches_a_named_slot() {
        assert!(mtd_is(LY10201_MTD, 5, "customer", 0x005a_0000));
        assert!(!mtd_is(LY10201_MTD, 5, "customer", 0x0044_0000));
        assert!(!mtd_is(LY10201_MTD, 3, "boot", 0x0044_0000));
    }
}
