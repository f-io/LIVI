use std::path::Path;
use std::sync::Arc;

const HOSTAPD_BASE: &str = "/tmp/livi/hostapd.conf.saved";
const HOSTAPD_LIVE: [&str; 2] = ["/tmp/livi/hostapd.conf", "/tmp/livi/hostapd.alt"];
const KEYS: &str = "/tmp/livi/bt-keys";

pub fn run(_args: Vec<String>) -> i32 {
    let cfg = livi_accessoryd::Config {
        keys_path: KEYS.into(),
        ap_name: Arc::new(|| {
            livi_wifid::server::ap_name_from(
                Path::new(HOSTAPD_BASE),
                &[Path::new(HOSTAPD_LIVE[0]), Path::new(HOSTAPD_LIVE[1])],
            )
        }),
    };
    crate::exit_rc(livi_accessoryd::run(cfg))
}
