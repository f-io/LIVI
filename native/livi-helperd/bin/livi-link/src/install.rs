use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::ExitCode;

/// (on-disk target, contents) — the scripts and web assets livi-link-provision also installs, minus
/// the stack .gz (this binary is it) and the version file (only a full install can stamp that).
const SCRIPTS: [(&str, &str); 6] = [
    ("/script/livi/livi-link.sh", include_str!("../scripts/livi-link.sh")),
    ("/script/livi/l2fwd-watch.sh", include_str!("../scripts/l2fwd-watch.sh")),
    ("/script/livi/flash-image.sh", include_str!("../scripts/flash-image.sh")),
    ("/script/start_main_service.sh", include_str!("../scripts/livi-bringup.sh")),
    ("/etc/boa/cgi-bin/server.cgi", include_str!("../web/server.cgi")),
    ("/etc/boa/www/index.html", include_str!("../web/index.html")),
];

pub fn run() -> ExitCode {
    let mut ok = true;
    for (target, body) in SCRIPTS {
        if let Err(e) = write_exec(target, body) {
            eprintln!("sync {target}: {e}");
            ok = false;
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn write_exec(target: &str, body: &str) -> std::io::Result<()> {
    if let Some(dir) = Path::new(target).parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = format!("{target}.new");
    fs::write(&tmp, body)?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
    fs::rename(&tmp, target)
}

#[cfg(test)]
mod tests {
    use super::SCRIPTS;

    #[test]
    fn ships_the_launcher_that_links_ledd() {
        let (_, link) = SCRIPTS
            .iter()
            .find(|(t, _)| *t == "/script/livi/livi-link.sh")
            .expect("launcher not carried");
        assert!(link.contains("ledd"), "the carried launcher must link ledd");
    }

    #[test]
    fn every_target_is_an_absolute_path() {
        for (target, _) in SCRIPTS {
            assert!(target.starts_with('/'), "{target} is not absolute");
        }
    }
}
