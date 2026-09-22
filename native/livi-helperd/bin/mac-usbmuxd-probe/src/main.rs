// Wired CarPlay over the system usbmuxd: control and auth (MFi from the dongle) plus the phone's
// own enX for AV, through to CarPlayStartSession. No :7000 receiver here, so not video itself.
use std::process::ExitCode;

#[cfg(target_os = "macos")]
mod mac {
    use std::time::Duration;

    use iap2_csm::messages::wifi::SecurityType;
    use iap2_link::LinkConfig;
    use iap2_mfi::ncm::NcmCoprocessor;
    use iap2_wired::{mac_network, usbmuxd};
    use livi_runtime::bringup::{run_accessory, BringupEvent, CpConfig};
    use livi_runtime::driver::spawn_link_stream;
    use livi_runtime::ident::{Identity, Transport};
    use livi_runtime::mfi_async::SharedCoprocessor;
    use tokio::sync::mpsc;

    fn wired_config(av_iface: Option<String>) -> (CpConfig, Identity) {
        let cp = CpConfig {
            ap_mac: None,
            wifi_iface: String::new(),
            ssid: "LIVI".into(),
            passphrase: "12345678".into(),
            channel: 36,
            security_type: SecurityType::WpaWpa2,
            airplay_port: 7000,
            source_version: "950.7.1".into(),
            public_key: String::new(),
            transport: Transport::Wired,
            av_iface,
            available_current_ma: 500,
        };
        let identity = Identity { name: "LIVI".into(), ssid: "LIVI".into(), bt_mac: [0x02, 0, 0, 0, 0, 1] };
        (cp, identity)
    }

    pub async fn run() -> Result<(), String> {
        let devices = usbmuxd::devices().await.map_err(|e| e.to_string())?;
        let Some(device) = devices.into_iter().next() else {
            return Err("no USB device on the system usbmuxd; plug the iPhone in and unlock it".into());
        };
        let tag = device.udid[..8.min(device.udid.len())].to_string();

        let mfi_addr = std::env::var("LIVI_MFI_ADDR")
            .unwrap_or_else(|_| livi_dongle::link::addr(iap2_mfi::ncm::DEFAULT_PORT));
        println!("[usbmuxd] {tag}: MFi over LIVI Link at {mfi_addr}");

        // The phone's own USB network interfaces, found by serial in the IORegistry.
        let ncm = mac_network::ncm_interfaces(&device.udid).await.unwrap_or_default();
        println!("[usbmuxd] {tag}: CarPlay NCM interfaces: {ncm:?}");
        let av_iface = ncm.into_iter().find(|i| livi_runtime::net::wlan_link_local(i).is_some());
        match &av_iface {
            Some(i) => println!("[usbmuxd] {tag}: AV interface {i}, link-local {:?}", livi_runtime::net::wlan_link_local(i)),
            None => println!("[usbmuxd] {tag}: no NCM interface with a link-local yet"),
        }

        println!("[usbmuxd] {tag}: opening carkit over usbmuxd");
        let stream = match usbmuxd::open(&device).await {
            Ok(s) => s,
            Err(e) => {
                if e.stage == usbmuxd::Stage::PairRecord {
                    return Err(format!("{e}\n[usbmuxd] {tag}: trust this Mac in Finder, confirm on the phone, then retry"));
                }
                return Err(e.to_string());
            }
        };
        println!("[usbmuxd] {tag}: carkit stream up, starting iAP2");

        let link = LinkConfig { max_outgoing: 4, control_version: 2, zero_ack: true, ..LinkConfig::default() };
        let (ch, _art_rx) = spawn_link_stream(stream, link, true);
        let (cp, id) = wired_config(av_iface);
        let auth = SharedCoprocessor::new(Box::new(NcmCoprocessor::new(&mfi_addr)));
        let (tx, mut rx) = mpsc::channel(64);
        tokio::spawn(async move { run_accessory(ch, auth, id, cp, tx).await });

        let deadline = Duration::from_secs(30);
        loop {
            match tokio::time::timeout(deadline, rx.recv()).await {
                Ok(Some(BringupEvent::CarPlayStartSent)) => {
                    println!("[usbmuxd] {tag}: CarPlayStartSession sent with the AV interface's link-local, phone would connect back to :7000");
                    return Ok(());
                }
                Ok(Some(BringupEvent::Incoming { msg_id, .. })) => {
                    println!("[usbmuxd] {tag}: incoming 0x{msg_id:04x}")
                }
                Ok(Some(BringupEvent::Failed(msg))) => return Err(format!("iAP2 failed: {msg}")),
                Ok(Some(BringupEvent::Closed)) => return Err("iAP2 closed before CarPlayStartSession".into()),
                Ok(Some(other)) => println!("[usbmuxd] {tag}: {other:?}"),
                Ok(None) => return Err("iAP2 closed before CarPlayStartSession".into()),
                Err(_) => return Err("iAP2 stalled: no progress in 30s".into()),
            }
        }
    }
}

#[cfg(target_os = "macos")]
async fn run() -> Result<(), String> {
    mac::run().await
}

#[cfg(not(target_os = "macos"))]
async fn run() -> Result<(), String> {
    Err("mac-usbmuxd-probe needs macOS (system usbmuxd + ioreg)".into())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[usbmuxd] error: {e}");
            ExitCode::FAILURE
        }
    }
}
