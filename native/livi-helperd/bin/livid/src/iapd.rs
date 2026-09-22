use std::path::Path;
use std::sync::Arc;

const HOSTAPD_BASE: &str = "/tmp/livi/hostapd.conf.saved";
const HOSTAPD_LIVE: [&str; 2] = ["/tmp/livi/hostapd.conf", "/tmp/livi/hostapd.alt"];
const KEYS: &str = "/tmp/livi/bt-keys";

pub fn run(_args: Vec<String>) -> i32 {
    let cfg = livi_iapd::Config {
        keys_path: KEYS.into(),
        name_override: None,
        ap_name: Arc::new(|| {
            livi_wifi::server::ap_name_from(
                Path::new(HOSTAPD_BASE),
                &[Path::new(HOSTAPD_LIVE[0]), Path::new(HOSTAPD_LIVE[1])],
            )
        }),
    };
    let _ = livi_iapd::run(cfg);
    0
}
