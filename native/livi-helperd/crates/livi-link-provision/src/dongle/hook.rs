use std::path::Path;

use super::ota;
use super::shell;
use super::web;

/// The vendor's OTA project id, e.g. "ly7129" — the second `.`-field of `appver`
/// ("26063016.7129.2"), same field `cpbox.js` uses for its OSS path.
pub fn ly_project(appver: &str) -> Option<String> {
    appver.split('.').nth(1).map(|v| format!("ly{v}"))
}

/// Refuses a dongle of another hardware family than the caller asked for, so `ax520 install-shell`
/// can never push the V821B image to an AX520 or the other way round.
pub fn check_project(actual: &str, expected: Option<&str>) -> Result<(), String> {
    match expected {
        Some(want) if want != actual => Err(format!("this dongle is project {actual}, not {want}")),
        _ => Ok(()),
    }
}

/// Gets a shell onto a stock dongle through its own web updater. The package it takes is the
/// vendor's own update for the dongle's project, patched here (see `ota`), so nothing of the
/// vendor's ships with the tool. `cache` is where that download is kept for the next run. A package
/// of another project is rejected by the dongle (update code >3), so `ota` checks the project first.
pub fn install_bindshell(expected: Option<&str>, cache: &Path) -> Result<(), String> {
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
    let project = ly_project(&info.sys.appver)
        .ok_or_else(|| format!("can't read a project id out of appver {:?}", info.sys.appver))?;
    check_project(&project, expected)?;
    println!("dongle: {} appver={} project={project}", info.name, info.sys.appver);
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
    use crate::dongle::arm::ax520;
    use crate::dongle::riscv::v821b;

    #[test]
    fn ly_project_reads_the_second_dotted_field() {
        assert_eq!(ly_project("26063016.7129.2").as_deref(), Some("ly7129"));
        assert_eq!(ly_project("25091912.6238.1").as_deref(), Some("ly6238"));
    }

    #[test]
    fn ly_project_gives_up_on_something_that_is_not_dotted() {
        assert_eq!(ly_project("nope"), None);
    }

    #[test]
    fn a_dongle_of_another_family_is_refused_before_anything_is_uploaded() {
        assert!(check_project(ax520::PROJECT, Some(v821b::PROJECT)).is_err());
        assert!(check_project(v821b::PROJECT, Some(ax520::PROJECT)).is_err());
        assert!(check_project(ax520::PROJECT, Some(ax520::PROJECT)).is_ok());
        assert!(check_project(ax520::PROJECT, None).is_ok());
    }
}
