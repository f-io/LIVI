use std::process::ExitCode;
use std::sync::Arc;

pub fn run(args: &[String]) -> ExitCode {
    let cfg = livi_iapd::Config {
        keys_path: "/etc/livi-bt-keys".into(),
        name_override: args.first().cloned(),
        ap_name: Arc::new(crate::wifid::ap_name),
    };
    livi_iapd::run(cfg)
}
