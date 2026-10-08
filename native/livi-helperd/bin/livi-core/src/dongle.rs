use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use livi_core_proto::config::Config;
use livi_core_proto::message::Radio;
use livi_core_proto::state::{DongleRadios, LinkSpeed};
use livi_link_host::ap;
use livi_link_host::link::CHOICE;
use tokio::sync::watch;

use crate::hub::Hub;
use crate::hwaddr;
use crate::projection::DongleAsks;

const POLL: Duration = Duration::from_millis(1500);
/// The rates cost the dongle's Wi-Fi driver a firmware round trip.
const RATES_EVERY: Duration = Duration::from_secs(5);
/// Applying waits for the radio, and a 5 GHz start spends the first seconds scanning.
const APPLY: Duration = Duration::from_secs(30);
const DRIFT_RETRY: Duration = Duration::from_secs(30);

type Status = HashMap<String, String>;

/// Held by every UI connection that shows the link speed.
pub struct LinkSpeedViewer(watch::Sender<usize>);

impl LinkSpeedViewer {
    pub fn new(viewers: &watch::Sender<usize>) -> Self {
        viewers.send_modify(|n| *n += 1);
        Self(viewers.clone())
    }
}

impl Drop for LinkSpeedViewer {
    fn drop(&mut self) {
        self.0.send_modify(|n| *n = n.saturating_sub(1));
    }
}

fn ssid_of(cfg: &Config) -> String {
    if cfg.car_name.is_empty() { "LIVI".into() } else { cfg.car_name.clone() }
}

fn country_of(cfg: &Config) -> String {
    if cfg.country.is_empty() { "DE".into() } else { cfg.country.to_uppercase() }
}

fn settings_for(cfg: &Config) -> Vec<String> {
    let or = |v: u32, d: u32| if v == 0 { d } else { v };
    let pass = if cfg.wifi_password.is_empty() { "12345678" } else { &cfg.wifi_password };
    vec![
        format!("set ssid {}", ssid_of(cfg)),
        format!("set country {}", country_of(cfg)),
        format!("set channel {}", or(cfg.wifi_channel, 36)),
        format!("set width {}", or(cfg.wifi_channel_width, 40)),
        format!("set passphrase {pass}"),
        "apply".into(),
        // Keeps the whole state across a reboot.
        "save".into(),
    ]
}

/// Whether the access point is on is kept on the dongle itself.
fn commands_for(cfg: &Config, status: &Status) -> Vec<String> {
    if cfg.wifi_interface != CHOICE || status.get("wifi-enabled").map(String::as_str) == Some("off")
    {
        return Vec::new();
    }
    settings_for(cfg)
}

/// On Linux the host drives the dongle's controller itself, over the tunnel.
fn accessory_on_dongle() -> bool {
    !cfg!(target_os = "linux") || std::env::var("LIVI_BT_VIA_DONGLE").as_deref() == Ok("1")
}

fn bt_commands_for(cfg: &Config) -> Vec<String> {
    let on = cfg.bt_adapter == CHOICE && cfg.wireless_cp_enabled && accessory_on_dongle();
    vec![if on { "iap on" } else { "iap off" }.into()]
}

/// Channel and width are left out, the dongle narrows those itself when its
/// radio refuses them.
fn drifted(status: &Status, cfg: &Config) -> bool {
    let get = |k: &str| status.get(k).map(String::as_str);
    if cfg.wifi_interface != CHOICE || get("wifi-enabled") == Some("off") {
        return false;
    }
    if get("state") != Some("on") {
        return true;
    }
    get("ssid") != Some(ssid_of(cfg).trim()) || get("country_code") != Some(&country_of(cfg))
}

fn radios(status: &Status) -> DongleRadios {
    let on = |k: &str| status.get(k).map(String::as_str) != Some("off");
    DongleRadios { wifi: on("wifi-enabled"), bt: on("bt-enabled") }
}

fn mbps(bytes: f64, over: Duration) -> f64 {
    let secs = over.as_secs_f64();
    if secs <= 0.0 || bytes < 0.0 {
        return 0.0;
    }
    (bytes * 8.0 / secs / 1e6 * 10.0).round() / 10.0
}

/// A host access point counts the way the dongle does: bytes on the interface, the phone's
/// rates from the kernel.
fn host_counters(iface: &str) -> Option<Status> {
    let bytes = |dir: &str| {
        std::fs::read_to_string(format!("/sys/class/net/{iface}/statistics/{dir}_bytes"))
            .ok()
            .map(|s| s.trim().to_string())
    };
    let mut status = Status::new();
    status.insert("downbytes".into(), bytes("rx")?);
    status.insert("upbytes".into(), bytes("tx")?);
    Some(status)
}

fn host_rates(iface: &str) -> Status {
    let mut status = Status::new();
    if let Some((down, up)) = livi_wifi::stations(iface).rates {
        status.insert("downrate".into(), down.to_string());
        status.insert("uprate".into(), up.to_string());
    }
    status
}

fn attached() -> bool {
    livi_link_host::link::attached()
}

async fn talk(commands: Vec<String>) -> Result<Status, String> {
    tokio::task::spawn_blocking(move || ap::talk(&commands, APPLY))
        .await
        .unwrap_or_else(|e| Err(e.to_string()))
}

fn report(what: &str, err: &str) {
    println!("[dongle] {what}: {err}");
}

#[derive(Default)]
struct Link {
    told: bool,
    last_try: Option<Instant>,
    last_drift: Option<Instant>,
    bytes: Option<(f64, f64, Instant)>,
    rates: Status,
    rates_at: Option<Instant>,
    misses: u8,
}

impl Link {
    async fn reconcile(&mut self, cfg: &Config) {
        if !attached() {
            return;
        }
        self.last_try = Some(Instant::now());
        let mut heard = true;
        let mut status = Status::new();
        match talk(vec!["status".into()]).await {
            Ok(s) => {
                status = s;
                let mut commands = commands_for(cfg, &status);
                if !commands.is_empty() {
                    commands.push("status".into());
                    match talk(commands).await {
                        Ok(s) => status = s,
                        Err(e) => {
                            heard = false;
                            report("access point", &e);
                        }
                    }
                }
            }
            Err(e) => {
                heard = false;
                report("access point", &e);
            }
        }
        if status.get("bt-enabled").map(String::as_str) != Some("off")
            && let Err(e) = talk(bt_commands_for(cfg)).await
        {
            heard = false;
            report("bluetooth", &e);
        }
        self.told = heard;
    }

    fn due(&mut self, status: Option<&Status>, cfg: &Config) -> bool {
        let Some(status) = status else {
            self.told = false;
            self.last_try = None;
            return false;
        };
        let waited = |t: Option<Instant>| t.is_none_or(|t| t.elapsed() >= DRIFT_RETRY);
        if !self.told {
            return waited(self.last_try);
        }
        if !(drifted(status, cfg) && waited(self.last_drift)) {
            return false;
        }
        self.last_drift = Some(Instant::now());
        true
    }

    async fn switch(&mut self, radio: Radio, on: bool, cfg: &Config) {
        if !attached() {
            return;
        }
        let commands = match (radio, on) {
            (Radio::Wifi, true) => [vec!["on".to_string()], settings_for(cfg)].concat(),
            (Radio::Wifi, false) => vec!["off".into()],
            (Radio::Bt, true) => vec!["bt on".into()],
            (Radio::Bt, false) => vec!["bt off".into()],
        };
        if let Err(e) = talk(commands).await {
            report(if radio == Radio::Wifi { "access point" } else { "bluetooth" }, &e);
            return;
        }
        // The accessory learns its part once the controller is up.
        if radio == Radio::Bt && on {
            self.reconcile(cfg).await;
        }
    }

    fn speed(&mut self, status: &Status) -> LinkSpeed {
        let num = |k: &str| status.get(k).and_then(|v| v.parse::<f64>().ok());
        let now = Instant::now();
        let (mut down_mbps, mut up_mbps) = (0.0, 0.0);
        let counted = num("downbytes").zip(num("upbytes"));
        if let (Some((down, up)), Some((d0, u0, at))) = (counted, self.bytes) {
            down_mbps = mbps(down - d0, now - at);
            up_mbps = mbps(up - u0, now - at);
        }
        self.bytes = counted.map(|(d, u)| (d, u, now));
        LinkSpeed {
            down_mbps,
            up_mbps,
            down_rate: num("downrate").unwrap_or(0.0),
            up_rate: num("uprate").unwrap_or(0.0),
        }
    }

    fn rates_due(&self) -> bool {
        self.rates_at.is_none_or(|at| at.elapsed() >= RATES_EVERY)
    }

    /// A single missed poll keeps the dongle listed, the route needs a moment after plug in.
    async fn poll(&mut self, hub: &Hub, speed_shown: bool, wifi_interface: &str) -> Option<Status> {
        let status = if attached() {
            tokio::task::spawn_blocking(ap::status).await.ok().flatten()
        } else {
            None
        };
        self.misses = if status.is_some() { 0 } else { self.misses.saturating_add(1) };
        let answers =
            status.is_some() || (self.misses < 2 && hub.watch().borrow().system.dongle.is_some());
        let host = wifi_interface != CHOICE;
        let mut counted = match (host, speed_shown) {
            (_, false) => None,
            (false, true) => status.clone(),
            (true, true) => {
                let iface = wifi_interface.to_string();
                tokio::task::spawn_blocking(move || host_counters(&iface)).await.ok().flatten()
            }
        };
        let link_speed = match counted.as_mut() {
            Some(s) => {
                if self.rates_due() {
                    let iface = wifi_interface.to_string();
                    self.rates = tokio::task::spawn_blocking(move || {
                        if host { host_rates(&iface) } else { ap::rates().unwrap_or_default() }
                    })
                    .await
                    .unwrap_or_default();
                    self.rates_at = Some(Instant::now());
                }
                // Older dongles carry the rates in the status.
                s.extend(self.rates.clone());
                Some(self.speed(s))
            }
            None => {
                self.bytes = None;
                self.rates_at = None;
                None
            }
        };
        let mut wifi_interfaces = hwaddr::wifi_interfaces();
        let mut bt_adapters = hwaddr::bt_adapters();
        let mut dongle = None;
        if answers {
            dongle = match &status {
                Some(s) => Some(radios(s)),
                None => hub.watch().borrow().system.dongle,
            };
            wifi_interfaces.push(CHOICE.into());
            bt_adapters.push(CHOICE.into());
        }
        hub.update(|s| {
            s.system.wifi_interfaces = wifi_interfaces;
            s.system.bt_adapters = bt_adapters;
            s.system.dongle = dongle;
            s.system.link_speed = link_speed;
        });
        status
    }
}

fn link_settings(cfg: &Config) -> impl PartialEq + use<> {
    (
        cfg.car_name.clone(),
        cfg.country.clone(),
        cfg.wifi_channel,
        cfg.wifi_channel_width,
        cfg.wifi_password.clone(),
        cfg.wifi_interface.clone(),
        cfg.bt_adapter.clone(),
        cfg.wireless_cp_enabled,
    )
}

fn adapter_switches(before: &Config, next: &Config) -> Vec<(Radio, bool)> {
    let mut out = Vec::new();
    for (radio, b, n) in [
        (Radio::Wifi, &before.wifi_interface, &next.wifi_interface),
        (Radio::Bt, &before.bt_adapter, &next.bt_adapter),
    ] {
        if b == n {
            continue;
        }
        if n == CHOICE {
            out.push((radio, true));
        } else if b == CHOICE {
            out.push((radio, false));
        }
    }
    out
}

pub async fn run(hub: Arc<Hub>, mut asks: DongleAsks) {
    let mut applied = hub.applied();
    let mut cfg = applied.borrow_and_update().clone();
    let mut link = Link::default();
    link.reconcile(&cfg).await;
    let mut tick = tokio::time::interval(POLL);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let speed_shown = *asks.link_speed_viewers.borrow() > 0;
                let status = link.poll(&hub, speed_shown, &cfg.wifi_interface).await;
                if link.due(status.as_ref(), &cfg) {
                    link.reconcile(&cfg).await;
                }
            }
            changed = applied.changed() => {
                if changed.is_err() {
                    return;
                }
                let next = applied.borrow_and_update().clone();
                for (radio, on) in adapter_switches(&cfg, &next) {
                    link.switch(radio, on, &next).await;
                }
                if link_settings(&cfg) != link_settings(&next) {
                    link.reconcile(&next).await;
                }
                cfg = next;
            }
            Some((radio, on)) = asks.radios.recv() => {
                // The pick the user just made counts, the applied settings
                // only follow it once nothing projects or with apply.
                let now = hub.config();
                let picked = match radio {
                    Radio::Wifi => &now.wifi_interface,
                    Radio::Bt => &now.bt_adapter,
                };
                if picked == CHOICE {
                    link.switch(radio, on, &cfg).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_file::defaults;

    fn status(pairs: &[(&str, &str)]) -> Status {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn on_dongle() -> Config {
        let mut cfg = defaults();
        cfg.wifi_interface = CHOICE.into();
        cfg.car_name = "Car".into();
        cfg.country = "at".into();
        cfg
    }

    #[test]
    fn the_access_point_gets_the_settings_while_chosen_and_on() {
        let cfg = on_dongle();
        assert_eq!(
            commands_for(&cfg, &Status::new()),
            [
                "set ssid Car",
                "set country AT",
                &format!(
                    "set channel {}",
                    if cfg.wifi_channel == 0 { 36 } else { cfg.wifi_channel }
                ),
                &format!(
                    "set width {}",
                    if cfg.wifi_channel_width == 0 { 40 } else { cfg.wifi_channel_width }
                ),
                &format!("set passphrase {}", cfg.wifi_password),
                "apply",
                "save",
            ]
        );
        assert!(commands_for(&cfg, &status(&[("wifi-enabled", "off")])).is_empty());
        assert!(commands_for(&defaults(), &Status::new()).is_empty());

        let mut blank = on_dongle();
        (blank.car_name, blank.country, blank.wifi_password) =
            (String::new(), String::new(), String::new());
        (blank.wifi_channel, blank.wifi_channel_width) = (0, 0);
        assert_eq!(
            settings_for(&blank)[..5],
            [
                "set ssid LIVI",
                "set country DE",
                "set channel 36",
                "set width 40",
                "set passphrase 12345678"
            ]
        );
    }

    #[test]
    fn the_accessory_runs_on_the_dongle_only_with_its_bluetooth_and_wireless_carplay() {
        let mut cfg = on_dongle();
        assert_eq!(bt_commands_for(&cfg), ["iap off"]);
        cfg.bt_adapter = CHOICE.into();
        cfg.wireless_cp_enabled = true;
        let expected = if accessory_on_dongle() { "iap on" } else { "iap off" };
        assert_eq!(bt_commands_for(&cfg), [expected]);
    }

    #[test]
    fn a_drift_is_a_stopped_access_point_or_another_name_or_country() {
        let cfg = on_dongle();
        let fine = status(&[("state", "on"), ("ssid", "Car"), ("country_code", "AT")]);
        assert!(!drifted(&fine, &cfg));
        assert!(drifted(&status(&[("state", "off")]), &cfg));
        assert!(drifted(
            &status(&[("state", "on"), ("ssid", "Other"), ("country_code", "AT")]),
            &cfg
        ));
        assert!(!drifted(&status(&[("wifi-enabled", "off")]), &cfg));
        assert!(!drifted(&Status::new(), &defaults()));
    }

    #[test]
    fn a_poll_asks_again_after_a_failure_or_a_drift_but_not_at_once() {
        let cfg = on_dongle();
        let mut link = Link::default();
        let fine = status(&[("state", "on"), ("ssid", "Car"), ("country_code", "AT")]);
        assert!(link.due(Some(&fine), &cfg));
        link.last_try = Some(Instant::now());
        assert!(!link.due(Some(&fine), &cfg));
        link.told = true;
        assert!(!link.due(Some(&fine), &cfg));
        let off = status(&[("state", "off")]);
        assert!(link.due(Some(&off), &cfg));
        assert!(!link.due(Some(&off), &cfg));
        assert!(!link.due(None, &cfg));
        assert!(!link.told);
    }

    #[test]
    fn radios_are_on_unless_switched_off() {
        assert_eq!(radios(&Status::new()), DongleRadios { wifi: true, bt: true });
        assert_eq!(
            radios(&status(&[("wifi-enabled", "off"), ("bt-enabled", "on")])),
            DongleRadios { wifi: false, bt: true }
        );
    }

    #[test]
    fn throughput_comes_from_the_counters_between_polls() {
        assert_eq!(mbps(1_250_000.0, Duration::from_secs(1)), 10.0);
        assert_eq!(mbps(-5.0, Duration::from_secs(1)), 0.0);
        assert_eq!(mbps(5.0, Duration::ZERO), 0.0);

        let mut link = Link::default();
        let first =
            link.speed(&status(&[("downbytes", "0"), ("upbytes", "0"), ("downrate", "866")]));
        assert_eq!((first.down_mbps, first.down_rate, first.up_rate), (0.0, 866.0, 0.0));
        let (_, _, at) = link.bytes.unwrap();
        link.bytes = Some((0.0, 0.0, at - Duration::from_secs(1)));
        let next = link.speed(&status(&[("downbytes", "1250000"), ("upbytes", "125000")]));
        assert!((next.down_mbps - 10.0).abs() < 0.2 && (next.up_mbps - 1.0).abs() < 0.1);
    }

    #[test]
    fn the_rates_are_asked_for_at_once_and_then_only_every_few_seconds() {
        let mut link = Link::default();
        assert!(link.rates_due());
        link.rates_at = Some(Instant::now());
        assert!(!link.rates_due());
        link.rates_at = Some(Instant::now() - RATES_EVERY);
        assert!(link.rates_due());
    }

    #[test]
    fn picking_the_dongle_switches_its_radio() {
        let before = defaults();
        let mut next = defaults();
        next.wifi_interface = CHOICE.into();
        assert_eq!(adapter_switches(&before, &next), [(Radio::Wifi, true)]);
        assert_eq!(adapter_switches(&next, &before), [(Radio::Wifi, false)]);
        let mut other = defaults();
        other.bt_adapter = "hci9".into();
        assert!(adapter_switches(&before, &other).is_empty());
    }
}
