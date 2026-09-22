//! What a dongle turns out to be once it has a shell: read from the machine itself, so a project
//! we have never seen can still be told apart from the boards we have a LIVI Link image for.

use crate::dongle::arm::ax520;
use crate::dongle::riscv::v821b;
use crate::dongle::shell::BindShell;

/// The boards a LIVI Link image exists for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    V821b,
    Ax520,
}

impl Family {
    pub fn name(self) -> &'static str {
        match self {
            Family::V821b => "V821B + AIC8800D80",
            Family::Ax520 => "AX520 + AIC8800D80",
        }
    }
}

#[derive(Debug, Default)]
pub struct Probe {
    pub uname: String,
    pub cpuinfo_head: String,
    pub proc_mtd: String,
    pub cmdline: String,
    pub model: String,
    pub aic_modules: String,
}

impl Probe {
    pub fn run(sh: &mut BindShell) -> Result<Self, String> {
        Ok(Self {
            uname: sh.run("uname -a")?,
            cpuinfo_head: sh.run("head -20 /proc/cpuinfo")?,
            proc_mtd: sh.run("cat /proc/mtd")?,
            cmdline: sh.run("cat /proc/cmdline || true")?,
            model: sh.run("cat /proc/device-tree/model 2>/dev/null || true")?,
            aic_modules: sh.run("ls /sys/module 2>/dev/null | grep -i aic8800 || true")?,
        })
    }

    /// The board, if it passes the same checks that gate flashing its image.
    pub fn family(&self) -> Option<Family> {
        let ax520 = ax520::HardwareInfo {
            cpuinfo_head: self.cpuinfo_head.clone(),
            proc_mtd: self.proc_mtd.clone(),
        };
        if ax520.looks_like_ax520_aic8800d80() {
            return Some(Family::Ax520);
        }
        let v821b = v821b::HardwareInfo {
            cpuinfo_head: self.cpuinfo_head.clone(),
            proc_mtd: self.proc_mtd.clone(),
            aic_modules: self.aic_modules.clone(),
        };
        v821b.looks_like_v821b_aic8800d80().then_some(Family::V821b)
    }

    /// Everything read, for the screen and for a file to attach to an issue.
    pub fn report(&self) -> String {
        let section = |title: &str, text: &str| format!("--- {title} ---\n{}\n\n", text.trim_end_matches(['\0', '\n', ' ']));
        let mut out = String::new();
        out += &section("uname -a", &self.uname);
        out += &section("device tree model", &self.model);
        out += &section("/proc/cpuinfo", &self.cpuinfo_head);
        out += &section("/proc/mtd", &self.proc_mtd);
        out += &section("/proc/cmdline", &self.cmdline);
        out += &section("aic8800 modules", &self.aic_modules);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AX520_MTD: &str = "dev:    size   erasesize  name\n\
        mtd0: 00030000 00010000 \"muboot\"\n\
        mtd3: 00300000 00010000 \"boot\"\n\
        mtd6: 00440000 00010000 \"rootfs\"\n";

    #[test]
    fn an_ax520_is_told_by_its_processor_and_partitions() {
        let probe = Probe {
            cpuinfo_head: "CPU part\t: 0xc07\n".into(),
            proc_mtd: AX520_MTD.into(),
            ..Probe::default()
        };
        assert_eq!(probe.family(), Some(Family::Ax520));
    }

    #[test]
    fn the_same_processor_with_other_partitions_is_not_taken_for_one() {
        let probe = Probe {
            cpuinfo_head: "CPU part\t: 0xc07\n".into(),
            proc_mtd: "mtd3: 00200000 00010000 \"boot\"\nmtd6: 00440000 00010000 \"rootfs\"\n".into(),
            ..Probe::default()
        };
        assert_eq!(probe.family(), None);
    }

    #[test]
    fn a_v821b_is_told_by_riscv_partitions_and_the_wifi_module() {
        let probe = Probe {
            cpuinfo_head: "isa\t\t: rv32imafdc\n".into(),
            proc_mtd: (0..9).map(|i| format!("mtd{i}: 00010000 00010000 \"p{i}\"\n")).collect(),
            aic_modules: "aic8800_bsp\n".into(),
            ..Probe::default()
        };
        assert_eq!(probe.family(), Some(Family::V821b));
    }

    #[test]
    fn nothing_known_gives_no_family() {
        assert_eq!(Probe::default().family(), None);
    }

    #[test]
    fn the_report_names_every_part_it_read() {
        let probe = Probe { uname: "Linux x 4.9.337\n".into(), model: "AXERA AX520\0".into(), ..Probe::default() };
        let report = probe.report();
        assert!(report.contains("--- uname -a ---\nLinux x 4.9.337\n"));
        assert!(report.contains("AXERA AX520\n"));
        assert!(!report.contains('\0'));
        assert!(report.contains("--- /proc/mtd ---"));
    }
}
