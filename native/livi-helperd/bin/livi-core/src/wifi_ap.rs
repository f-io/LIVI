use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use livi_core_proto::config::{Config, WifiBand};
use livi_link_host::link::CHOICE;
use serde_json::{Map, Value};
use tokio::process::Command;

use crate::privileged::{Privileged, quietly, sudo, sudo_grants, systemctl};
use crate::server::Core;
use crate::wifi_options::{band_of, wifi_band};

const UNIT_PATH: &str = "/etc/systemd/system/livi-wifi-ap.service";
const SERVICE: &str = "livi-wifi-ap.service";
/// Root's own copy of the helper the unit runs, put there by --install-wifi-ap.
const UNIT_HELPER: &str = "/usr/local/sbin/livi-helperd";
const NM_UNMANAGED_CONF: &str = "/etc/NetworkManager/conf.d/99-livi-ap-unmanaged.conf";
const UNIT_TEMPLATE: &str = "livi-wifi-ap.service.template";
const SUDOERS_TEMPLATE: &str = "99-LIVI-wifi-ap.sudoers.template";
/// A stamp of the root-only sudoers rule, which core cannot read back.
const MARKER: &str = ".wifi-ap-install";
const SETTLE_TRIES: u32 = 15;
const SETTLE_EVERY: Duration = Duration::from_secs(2);

fn wanted(cfg: &Config) -> bool {
    cfg.wifi_interface != CHOICE
        && (cfg.wifi_dedicated_interface || cfg.wireless_cp_enabled || cfg.wireless_aa_enabled)
}

/// What the unit reads at its start, a change needs a restart.
fn ap_settings(cfg: &Config) -> String {
    format!(
        "{}|{}|{}|{:?}|{}|{}|{}",
        cfg.wifi_interface,
        cfg.car_name,
        cfg.wifi_password,
        cfg.wifi_type,
        cfg.wifi_channel,
        cfg.wifi_channel_width,
        cfg.country
    )
}

fn link_settings(cfg: &Config) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        ap_settings(cfg),
        cfg.wifi_dedicated_interface,
        cfg.wireless_cp_enabled,
        cfg.wireless_aa_enabled,
        cfg.bt_adapter,
        cfg.wifi_interface
    )
}

struct Unit {
    unit: String,
    rule: String,
}

fn render(p: &Privileged) -> Option<Unit> {
    match (p.render(UNIT_TEMPLATE), p.render(SUDOERS_TEMPLATE)) {
        (Ok(unit), Ok(rule)) => Some(Unit { unit, rule }),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("[wifi-ap] the unit templates do not read: {e}");
            None
        }
    }
}

/// The sudoers file is root-only, so sudo itself is asked, with the marker as fallback.
async fn needs_install(p: &Privileged, u: &Unit) -> bool {
    let unit_current = std::fs::read_to_string(UNIT_PATH).is_ok_and(|text| text == u.unit);
    let rule_current =
        sudo_grants(&format!("restart {SERVICE}")).await || p.marker_holds(MARKER, &u.rule);
    !(unit_current && rule_current && same_helper(&p.helper, Path::new(UNIT_HELPER)))
}

fn same_helper(ours: &Path, copy: &Path) -> bool {
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
    size(ours).is_some()
        && size(ours) == size(copy)
        && matches!((std::fs::read(ours), std::fs::read(copy)), (Ok(a), Ok(b)) if a == b)
}

/// systemd says "inactive" also for a unit that is not installed.
async fn unit_state() -> String {
    Command::new(systemctl())
        .args(["is-active", SERVICE])
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

async fn release_interface(p: &Privileged) {
    // A unit caught in its start loop is neither active nor enabled, yet holds on.
    let taken = Path::new(NM_UNMANAGED_CONF).exists()
        || !matches!(unit_state().await.as_str(), "inactive" | "")
        || quietly("systemctl", &["is-enabled", "--quiet", SERVICE]).await;
    if !taken {
        return;
    }
    let sc = systemctl();
    sudo(&[&sc, "stop", SERVICE]).await;
    sudo(&[&sc, "disable", SERVICE]).await;
    // Without the root copy, sudo grants the teardown only to the helper's own path.
    let helper = if Path::new(UNIT_HELPER).exists() {
        UNIT_HELPER.to_string()
    } else {
        p.helper_named.display().to_string()
    };
    sudo(&[&helper, "--wifi-ap-teardown"]).await;
}

async fn reconcile(p: &Privileged, cfg: &Config) {
    if !wanted(cfg) {
        release_interface(p).await;
        return;
    }
    let Some(u) = render(p) else { return };
    if needs_install(p, &u).await {
        if !p.helper_installs("install-wifi-ap", &[("unit", &u.unit), ("rule", &u.rule)]).await {
            eprintln!(
                "[wifi-ap] {UNIT_PATH} not installed, the helper does not run as root. \
                 Run the LIVI install script on this host."
            );
            return;
        }
        p.write_marker(MARKER, &u.rule);
    }
    let sc = systemctl();
    let enable = if cfg.wifi_dedicated_interface { "enable" } else { "disable" };
    sudo(&[&sc, enable, SERVICE]).await;
    // A running unit ignores start and keeps the binary it started with.
    let start = if runs_older_helper().await { "restart" } else { "start" };
    sudo(&[&sc, start, SERVICE]).await;
}

async fn runs_older_helper() -> bool {
    let Ok(out) = Command::new(systemctl())
        .args(["show", "-p", "ActiveEnterTimestampMonotonic", "--value", SERVICE])
        .output()
        .await
    else {
        return false;
    };
    let since_boot_us: f64 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0.0);
    let boot: f64 = std::fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("btime ")?.trim().parse().ok()))
        .unwrap_or(0.0);
    let Ok(modified) = std::fs::metadata(UNIT_HELPER).and_then(|m| m.modified()) else {
        return false;
    };
    let modified = modified.duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    since_boot_us > 0.0 && boot > 0.0 && modified > boot + since_boot_us / 1e6
}

#[derive(Debug, PartialEq)]
struct Running {
    /// None from a helper that did not name it yet.
    band: Option<WifiBand>,
    channel: u32,
    width: u32,
}

fn parse_status(out: &str) -> Option<Running> {
    let value = |key: &str| out.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(' '));
    let channel: u32 = value("channel")?.trim().parse().ok().filter(|c| *c > 0)?;
    if value("running") != Some("true") {
        return None;
    }
    Some(Running {
        band: value("band").and_then(|b| livi_wifi::Band::of_setting(b.trim())).map(wifi_band),
        channel,
        width: value("width").and_then(|w| w.trim().parse().ok()).unwrap_or(0),
    })
}

/// The band may refuse the configured channel or width.
async fn settle(p: &Privileged, core: &Core, started: &Config) -> Config {
    let mut ran = started.clone();
    for _ in 0..SETTLE_TRIES {
        let out = Command::new(&p.helper).arg("--wifi-ap-status").output().await;
        if let Some(live) = out.ok().and_then(|o| parse_status(&String::from_utf8_lossy(&o.stdout)))
        {
            ran.wifi_channel = live.channel;
            if live.width > 0 {
                ran.wifi_channel_width = live.width;
            }
            let cfg = started;
            let mut patch = Map::new();
            if let Some(band) = live.band.filter(|b| *b != cfg.wifi_type) {
                ran.wifi_type = band;
                patch.insert("wifiType".into(), band_of(band).setting().into());
            }
            if live.channel != cfg.wifi_channel {
                patch.insert("wifiChannel".into(), live.channel.into());
            }
            if live.width > 0 && live.width != cfg.wifi_channel_width {
                patch.insert("wifiChannelWidth".into(), live.width.into());
            }
            if !patch.is_empty() {
                println!(
                    "[wifi-ap] channel {} at {} MHz was refused, it runs on {} at {} MHz",
                    cfg.wifi_channel, cfg.wifi_channel_width, live.channel, live.width
                );
                if let Err(e) = core.set_config(&Value::Object(patch)).await {
                    eprintln!("[wifi-ap] channel not saved: {e}");
                }
            }
            return ran;
        }
        tokio::time::sleep(SETTLE_EVERY).await;
    }
    ran
}

pub async fn follow(core: Arc<Core>, p: Privileged) {
    if !cfg!(target_os = "linux") {
        return;
    }
    let mut applied = core.hub.applied();
    let cfg = applied.borrow_and_update().clone();
    reconcile(&p, &cfg).await;
    let ran = settle(&p, &core, &cfg).await;
    let mut linked = link_settings(&ran);
    let mut started_with = ap_settings(&ran);
    while applied.changed().await.is_ok() {
        let cfg = applied.borrow_and_update().clone();
        if link_settings(&cfg) != linked {
            linked = link_settings(&cfg);
            reconcile(&p, &cfg).await;
        }
        if ap_settings(&cfg) == started_with || !wanted(&cfg) {
            continue;
        }
        let Some(u) = render(&p) else { continue };
        if needs_install(&p, &u).await {
            continue;
        }
        println!("[wifi-ap] restarting with the new settings");
        sudo(&[&systemctl(), "restart", SERVICE]).await;
        let ran = settle(&p, &core, &cfg).await;
        linked = link_settings(&ran);
        started_with = ap_settings(&ran);
    }
}

pub async fn release_for_quit(cfg: &Config) {
    if !cfg!(target_os = "linux") || cfg.wifi_dedicated_interface {
        return;
    }
    sudo(&[&systemctl(), "stop", SERVICE]).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::defaults;
    use crate::config_file::tests::TempDir;

    #[test]
    fn the_units_copy_counts_only_when_it_is_the_same_binary() {
        let dir = TempDir::new();
        let (ours, copy) = (dir.0.join("ours"), dir.0.join("copy"));
        std::fs::write(&ours, b"helper v2").unwrap();
        assert!(!same_helper(&ours, &copy));
        std::fs::write(&copy, b"helper v1").unwrap();
        assert!(!same_helper(&ours, &copy));
        std::fs::write(&copy, b"helper v2").unwrap();
        assert!(same_helper(&ours, &copy));
    }

    #[test]
    fn the_status_names_the_channel_only_while_running() {
        let up = "running true\nssid LIVI\nband 6ghz\nchannel 37\nwidth 80\n";
        assert_eq!(
            parse_status(up),
            Some(Running { band: Some(WifiBand::Ghz6), channel: 37, width: 80 })
        );
        assert_eq!(parse_status("running false\nssid \nchannel 0\nwidth 0\n"), None);
        assert_eq!(
            parse_status("running true\nchannel 6\n"),
            Some(Running { band: None, channel: 6, width: 0 })
        );
        assert_eq!(parse_status(""), None);
    }

    #[test]
    fn the_interface_is_wanted_for_wireless_projection_off_the_link() {
        let mut cfg = defaults();
        cfg.wifi_interface = "wlan0".into();
        assert!(!wanted(&cfg));
        cfg.wireless_cp_enabled = true;
        assert!(wanted(&cfg));
        cfg.wifi_interface = CHOICE.into();
        assert!(!wanted(&cfg));
        let before = ap_settings(&cfg);
        cfg.wifi_channel += 1;
        assert_ne!(ap_settings(&cfg), before);
    }
}
