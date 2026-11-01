// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::Path;

use super::ota;
use super::shell;
use super::web;

pub fn ly_project(appver: &str) -> Option<String> {
    appver.split('.').nth(1).map(|v| format!("ly{v}"))
}

pub fn install_bindshell(cache: &Path) -> Result<(), String> {
    if shell::is_up() {
        println!("bind-shell already listening on 2323 — skipping OTA upload");
        return Ok(());
    }
    let info = web::host()?;
    if info.update != 0 {
        return Err(format!(
            "dongle is not idle (update={}); reboot and retry",
            info.update
        ));
    }
    let project = ly_project(&info.sys.appver).ok_or_else(|| {
        format!(
            "can't read a project id out of appver {:?}",
            info.sys.appver
        )
    })?;
    println!(
        "dongle: {} appver={} project={project}",
        info.name, info.sys.appver
    );
    let image = ota::shell_image(&info.sys.appver, &project, cache)?;

    println!("uploading update.shell.img ({} B)…", image.len());
    web::upload(&image)?;
    println!("waiting for update to complete + dongle to reboot…");
    web::wait_for_update_complete(300)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ly_project_reads_the_second_dotted_field() {
        assert_eq!(ly_project("26063016.7129.2").as_deref(), Some("ly7129"));
        assert_eq!(ly_project("25091912.6238.1").as_deref(), Some("ly6238"));
    }

    #[test]
    fn ly_project_gives_up_on_something_that_is_not_dotted() {
        assert_eq!(ly_project("nope"), None);
    }
}
