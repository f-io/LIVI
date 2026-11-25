// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};

use super::lfwb;
use super::parse_mtd;
use super::shell::BindShell;

pub fn raw(sh: &mut BindShell, out_dir: &Path) -> Result<PathBuf, String> {
    let proc_mtd = sh.run("cat /proc/mtd")?;
    let parts = parse_mtd(&proc_mtd);
    if parts.is_empty() {
        return Err("the dongle lists no mtd partitions to back up".into());
    }

    let dir = out_dir.join(format!("raw-{}", lfwb::stamp()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {dir:?}: {e}"))?;
    std::fs::write(dir.join("proc-mtd.txt"), &proc_mtd)
        .map_err(|e| format!("write proc-mtd.txt: {e}"))?;

    for part in &parts {
        println!("pulling mtd{} {} ({} B)…", part.index, part.name, part.size);
        let data = sh.stream_out(
            &format!(
                "dd if=/dev/mtdblock{} bs=64k 2>/dev/null; sleep 1",
                part.index
            ),
            part.size,
        )?;
        let safe: String = part
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let file = dir.join(format!("mtd{}-{safe}.bin", part.index));
        std::fs::write(&file, &data).map_err(|e| format!("write {file:?}: {e}"))?;
    }
    Ok(dir)
}
