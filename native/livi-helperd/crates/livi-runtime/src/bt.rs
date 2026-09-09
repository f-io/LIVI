use std::collections::HashMap;
use std::error::Error;
use std::os::fd::OwnedFd;

use tokio::sync::mpsc;
use zbus::Connection;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

pub const AA_UUID: &str = "4de17a00-52cb-11e6-bdf4-0800200c9a66";
pub const AA_CHANNEL: u16 = 8;
const AA_PATH: &str = "/livi/aa/profile";

const AA_RECORD: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<record>
  <attribute id="0x0001"><sequence><uuid value="4de17a00-52cb-11e6-bdf4-0800200c9a66" /></sequence></attribute>
  <attribute id="0x0004"><sequence>
    <sequence><uuid value="0x0100" /></sequence>
    <sequence><uuid value="0x0003" /><uint8 value="0x08" /></sequence>
  </sequence></attribute>
  <attribute id="0x0005"><sequence><uuid value="0x1002" /></sequence></attribute>
  <attribute id="0x0100"><text value="Android Auto Wireless" /></attribute>
</record>
"#;

pub const IAP_SERVER_UUID: &str = "00000000-deca-fade-deca-deafdecacaff";
pub const IAP_CLIENT_UUID: &str = "00000000-deca-fade-deca-deafdecacafe";
pub const CARPLAY_SERVICE_UUID: &str = "ec884348-cd41-40a2-9727-575d50bf1fd3";
pub const IAP_CHANNEL: u16 = 3;

const ADAPTER_WAIT: std::time::Duration = std::time::Duration::from_secs(20);
const ADAPTER_POLL: std::time::Duration = std::time::Duration::from_millis(200);

const IAP_SERVER_PATH: &str = "/livi/cp/iap_server";
const IAP_CLIENT_PATH: &str = "/livi/cp/iap_client";
const CARPLAY_PATH: &str = "/livi/cp/carplay";
const AGENT_PATH: &str = "/livi/cp/agent";

const IAP_RECORD: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<record>
    <attribute id="0x0001"><sequence><uuid value="00000000-deca-fade-deca-deafdecacaff" /></sequence></attribute>
    <attribute id="0x0002"><uint32 value="0x00000000" /></attribute>
    <attribute id="0x0004"><sequence>
        <sequence><uuid value="0x0100" /></sequence>
        <sequence><uuid value="0x0003" /><uint8 value="0x03" /></sequence>
    </sequence></attribute>
    <attribute id="0x0005"><sequence><uuid value="0x1002" /></sequence></attribute>
    <attribute id="0x0008"><uint8 value="0xff" /></attribute>
    <attribute id="0x0009"><sequence><sequence><uuid value="0x1101" /><uint16 value="0x0100" /></sequence></sequence></attribute>
    <attribute id="0x0100"><text value="Wireless iAP" /></attribute>
</record>
"#;

const CARPLAY_RECORD: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<record>
    <attribute id="0x0001"><sequence><uuid value="ec884348-cd41-40a2-9727-575d50bf1fd3" /></sequence></attribute>
    <attribute id="0x0002"><uint32 value="0x00000000" /></attribute>
    <attribute id="0x0004"><sequence>
        <sequence><uuid value="0x0100" /></sequence>
        <sequence><uuid value="0x0003" /><uint8 value="0x04" /></sequence>
    </sequence></attribute>
    <attribute id="0x0005"><sequence><uuid value="0x1002" /></sequence></attribute>
    <attribute id="0x0008"><uint8 value="0xff" /></attribute>
    <attribute id="0x0009"><sequence><sequence><uuid value="0x1101" /><uint16 value="0x0100" /></sequence></sequence></attribute>
    <attribute id="0x0100"><text value="CarPlay" /></attribute>
</record>
"#;

pub struct IncomingConn {
    pub fd: OwnedFd,
    pub peer_mac: String,
}

struct Profile {
    tx: mpsc::UnboundedSender<IncomingConn>,
}

#[zbus::interface(name = "org.bluez.Profile1")]
impl Profile {
    async fn new_connection(
        &self,
        device: ObjectPath<'_>,
        fd: zbus::zvariant::OwnedFd,
        _options: HashMap<String, OwnedValue>,
    ) {
        let peer_mac = mac_from_device_path(device.as_str());
        let fd = OwnedFd::from(fd);
        let _ = self.tx.send(IncomingConn { fd, peer_mac });
    }

    fn request_disconnection(&self, _device: ObjectPath<'_>) {}

    fn release(&self) {}
}

struct Agent;

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {}
    fn authorize_service(&self, _device: ObjectPath<'_>, _uuid: String) {}
    fn request_pin_code(&self, _device: ObjectPath<'_>) -> String {
        "0000".into()
    }
    fn request_passkey(&self, _device: ObjectPath<'_>) -> u32 {
        0
    }
    fn display_passkey(&self, _device: ObjectPath<'_>, _passkey: u32, _entered: u16) {}
    fn display_pin_code(&self, _device: ObjectPath<'_>, _pincode: String) {}
    fn request_confirmation(&self, _device: ObjectPath<'_>, _passkey: u32) {}
    fn request_authorization(&self, _device: ObjectPath<'_>) {}
    fn cancel(&self) {}
}

fn mac_from_device_path(path: &str) -> String {
    let tail = path.rsplit("/dev_").next().unwrap_or("");
    let parts: Vec<&str> = tail.split('_').collect();
    if parts.len() != 6 {
        return String::new();
    }
    parts.join(":").to_uppercase()
}

async fn register_profile(
    conn: &Connection,
    path: &str,
    uuid: &str,
    options: HashMap<&str, Value<'_>>,
) -> Result<(), Box<dyn Error>> {
    let object_path = ObjectPath::try_from(path)?;
    conn.call_method(
        Some("org.bluez"),
        "/org/bluez",
        Some("org.bluez.ProfileManager1"),
        "RegisterProfile",
        &(object_path, uuid, options),
    )
    .await?;
    Ok(())
}

pub async fn start(
    adapter: &str,
    alias: &str,
    discoverable: bool,
) -> Result<(Connection, mpsc::UnboundedReceiver<IncomingConn>), Box<dyn Error>> {
    let conn = Connection::system().await?;
    let (tx, rx) = mpsc::unbounded_channel();

    conn.object_server()
        .at(IAP_SERVER_PATH, Profile { tx: tx.clone() })
        .await?;
    conn.object_server()
        .at(IAP_CLIENT_PATH, Profile { tx: tx.clone() })
        .await?;
    conn.object_server()
        .at(CARPLAY_PATH, Profile { tx })
        .await?;
    conn.object_server().at(AGENT_PATH, Agent).await?;

    let mut iap_opts: HashMap<&str, Value> = HashMap::new();
    iap_opts.insert("Role", Value::from("server"));
    iap_opts.insert("Channel", Value::from(IAP_CHANNEL));
    iap_opts.insert("ServiceRecord", Value::from(IAP_RECORD));
    iap_opts.insert("RequireAuthentication", Value::from(false));
    iap_opts.insert("RequireAuthorization", Value::from(false));
    register_profile(&conn, IAP_SERVER_PATH, IAP_SERVER_UUID, iap_opts).await?;

    // The client profile with AutoConnect lets BlueZ page a known phone back after a restart.
    let mut client_opts: HashMap<&str, Value> = HashMap::new();
    client_opts.insert("Role", Value::from("client"));
    client_opts.insert("AutoConnect", Value::from(true));
    if let Err(e) = register_profile(&conn, IAP_CLIENT_PATH, IAP_CLIENT_UUID, client_opts).await {
        eprintln!("[cp] could not register iAP client profile: {e}");
    }

    let mut cp_opts: HashMap<&str, Value> = HashMap::new();
    cp_opts.insert("Role", Value::from("server"));
    cp_opts.insert("ServiceRecord", Value::from(CARPLAY_RECORD));
    cp_opts.insert("RequireAuthentication", Value::from(false));
    cp_opts.insert("RequireAuthorization", Value::from(false));
    if let Err(e) = register_profile(&conn, CARPLAY_PATH, CARPLAY_SERVICE_UUID, cp_opts).await {
        eprintln!("[cp] could not publish CarPlay service UUID: {e}");
    }

    conn.call_method(
        Some("org.bluez"),
        "/org/bluez",
        Some("org.bluez.AgentManager1"),
        "RegisterAgent",
        &(ObjectPath::try_from(AGENT_PATH)?, "KeyboardDisplay"),
    )
    .await?;
    conn.call_method(
        Some("org.bluez"),
        "/org/bluez",
        Some("org.bluez.AgentManager1"),
        "RequestDefaultAgent",
        &(ObjectPath::try_from(AGENT_PATH)?,),
    )
    .await?;

    let adapter_path = format!("/org/bluez/{adapter}");
    wait_for_adapter(&conn, &adapter_path).await?;
    set_prop(&conn, &adapter_path, "Alias", Value::from(alias)).await?;
    set_prop(
        &conn,
        &adapter_path,
        "DiscoverableTimeout",
        Value::from(0u32),
    )
    .await?;
    set_prop(&conn, &adapter_path, "Powered", Value::from(true)).await?;
    set_prop(
        &conn,
        &adapter_path,
        "Discoverable",
        Value::from(discoverable),
    )
    .await?;
    set_prop(&conn, &adapter_path, "Pairable", Value::from(discoverable)).await?;

    Ok((conn, rx))
}

/// BlueZ publishes an adapter a moment after the kernel registers it, and a tunnelled controller
/// takes longer than a local one, so give it that moment before setting anything on it.
async fn wait_for_adapter(conn: &Connection, path: &str) -> Result<(), Box<dyn Error>> {
    let deadline = std::time::Instant::now() + ADAPTER_WAIT;
    loop {
        let asked = conn
            .call_method(
                Some("org.bluez"),
                path,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &("org.bluez.Adapter1", "Address"),
            )
            .await;
        match asked {
            Ok(_) => return Ok(()),
            Err(e) if std::time::Instant::now() >= deadline => {
                return Err(format!("BlueZ never published {path}: {e}").into());
            }
            Err(_) => tokio::time::sleep(ADAPTER_POLL).await,
        }
    }
}

/// BlueZ answers Busy while it is still settling an adapter it has only just published, so the
/// same set is offered again until it takes.
async fn set_prop(
    conn: &Connection,
    path: &str,
    name: &str,
    value: Value<'_>,
) -> Result<(), Box<dyn Error>> {
    let deadline = std::time::Instant::now() + ADAPTER_WAIT;
    loop {
        let asked = conn
            .call_method(
                Some("org.bluez"),
                path,
                Some("org.freedesktop.DBus.Properties"),
                "Set",
                &("org.bluez.Adapter1", name, &value),
            )
            .await;
        match asked {
            Ok(_) => return Ok(()),
            Err(e) if busy(&e) && std::time::Instant::now() < deadline => {
                println!("[bt] {path} is still busy, offering {name} again");
                tokio::time::sleep(ADAPTER_POLL).await;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Whether BlueZ turned the call down because it is in the middle of something else.
fn busy(e: &zbus::Error) -> bool {
    matches!(e, zbus::Error::MethodError(name, _, _) if name.as_str() == "org.bluez.Error.Busy")
}

/// Publishes the Android Auto wireless profile so a phone can open the RFCOMM channel that
/// carries the Wi-Fi bootstrap.
pub async fn start_aa(
    conn: &Connection,
) -> Result<mpsc::UnboundedReceiver<IncomingConn>, Box<dyn Error>> {
    let (tx, rx) = mpsc::unbounded_channel();
    conn.object_server().at(AA_PATH, Profile { tx }).await?;

    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("Role", Value::from("server"));
    opts.insert("Channel", Value::from(AA_CHANNEL));
    opts.insert("ServiceRecord", Value::from(AA_RECORD));
    opts.insert("RequireAuthentication", Value::from(false));
    opts.insert("RequireAuthorization", Value::from(false));
    register_profile(conn, AA_PATH, AA_UUID, opts).await?;
    println!("[aa] profile registered (RFCOMM ch {AA_CHANNEL})");
    Ok(rx)
}

pub const HFP_HF_UUID: &str = "0000111e-0000-1000-8000-00805f9b34fb";
const HFP_PATH: &str = "/livi/bt/hfp";
const BLE_AD_PATH: &str = "/livi/bt/ble";
const PLAYER_PATH: &str = "/livi/bt/player";

struct HfpProfile {
    hfp: crate::hfp::Hfp,
}

#[zbus::interface(name = "org.bluez.Profile1")]
impl HfpProfile {
    fn new_connection(
        &self,
        device: ObjectPath<'_>,
        fd: zbus::zvariant::OwnedFd,
        _options: HashMap<String, OwnedValue>,
    ) {
        let mac = mac_from_device_path(device.as_str());
        println!("[hfp] connection from {mac}");
        self.hfp.accept(OwnedFd::from(fd), mac);
    }

    fn request_disconnection(&self, _device: ObjectPath<'_>) {}

    fn release(&self) {}
}

/// The audio daemon usually holds HFP HF (incl. SCO); ours registers only as fallback,
/// and calls go through the daemon's — a second SLC just makes the phone drop one.
pub async fn start_hfp(conn: &Connection, hfp: crate::hfp::Hfp) -> Result<(), Box<dyn Error>> {
    conn.object_server()
        .at(HFP_PATH, HfpProfile { hfp: hfp.clone() })
        .await?;
    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("Name", Value::from("HFP Hands-Free"));
    opts.insert("Role", Value::from("client"));
    opts.insert("RequireAuthentication", Value::from(false));
    opts.insert("RequireAuthorization", Value::from(false));
    opts.insert("Features", Value::from(0x009cu16));
    opts.insert("Version", Value::from(0x0108u16));
    match register_profile(conn, HFP_PATH, HFP_HF_UUID, opts).await {
        Ok(()) => println!("[hfp] HF profile registered"),
        Err(e) => {
            println!("[hfp] HF profile held by the audio daemon, prober stands down ({e})");
            hfp.set_owned_elsewhere();
        }
    }
    Ok(())
}

struct BleAd {
    name: String,
}

#[zbus::interface(name = "org.bluez.LEAdvertisement1")]
impl BleAd {
    fn release(&self) {}

    #[zbus(property, name = "Type")]
    fn ad_type(&self) -> String {
        "peripheral".into()
    }

    #[zbus(property, name = "ServiceUUIDs")]
    fn service_uuids(&self) -> Vec<String> {
        vec![AA_UUID.into()]
    }

    #[zbus(property, name = "LocalName")]
    fn local_name(&self) -> String {
        self.name.clone()
    }
}

/// BLE advertisement with the AA UUID, so phones find the head unit without a BR/EDR scan.
pub async fn start_ble_ad(
    conn: &Connection,
    adapter: &str,
    name: &str,
) -> Result<(), Box<dyn Error>> {
    conn.object_server()
        .at(BLE_AD_PATH, BleAd { name: name.into() })
        .await?;
    let path = format!("/org/bluez/{adapter}");
    let opts: HashMap<&str, Value> = HashMap::new();
    conn.call_method(
        Some("org.bluez"),
        path.as_str(),
        Some("org.bluez.LEAdvertisingManager1"),
        "RegisterAdvertisement",
        &(ObjectPath::try_from(BLE_AD_PATH)?, opts),
    )
    .await?;
    println!("[aa] BLE advertisement registered");
    Ok(())
}

struct MprisRoot;

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl MprisRoot {
    fn raise(&self) {}
    fn quit(&self) {}

    #[zbus(property, name = "CanQuit")]
    fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property, name = "CanRaise")]
    fn can_raise(&self) -> bool {
        false
    }

    #[zbus(property, name = "HasTrackList")]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property, name = "Identity")]
    fn identity(&self) -> String {
        "LIVI".into()
    }

    #[zbus(property, name = "SupportedUriSchemes")]
    fn supported_uri_schemes(&self) -> Vec<String> {
        vec![]
    }

    #[zbus(property, name = "SupportedMimeTypes")]
    fn supported_mime_types(&self) -> Vec<String> {
        vec![]
    }
}

/// AVRCP passthrough lands here via BlueZ; every key becomes an input event for LIVI.
pub struct MprisPlayer {
    events: crate::livi_sock::Broadcaster,
    status: std::sync::Arc<std::sync::Mutex<String>>,
}

/// Updates the player's PlaybackStatus, so the peer's play/pause toggle sends the right verb.
#[derive(Clone)]
pub struct MediaPlayerHandle {
    conn: Connection,
    status: std::sync::Arc<std::sync::Mutex<String>>,
}

impl MediaPlayerHandle {
    pub async fn set_status(&self, status: &str) {
        {
            let mut s = self.status.lock().unwrap();
            if *s == status {
                return;
            }
            *s = status.to_string();
        }
        println!("[aa] avrcp playback status -> {status}");
        if let Ok(iface) = self
            .conn
            .object_server()
            .interface::<_, MprisPlayer>(PLAYER_PATH)
            .await
        {
            let _ = iface
                .get()
                .await
                .playback_status_changed(iface.signal_context())
                .await;
        }
    }
}

impl MprisPlayer {
    fn emit(&self, command: &str) {
        self.events
            .push_json(format!("{{\"event\":\"input\",\"command\":\"{command}\"}}"));
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl MprisPlayer {
    fn play(&self) {
        self.emit("play");
    }
    fn pause(&self) {
        self.emit("pause");
    }
    fn play_pause(&self) {
        self.emit("playPause");
    }
    fn stop(&self) {
        self.emit("stop");
    }
    fn next(&self) {
        self.emit("next");
    }
    fn previous(&self) {
        self.emit("previous");
    }
    fn seek(&self, offset: i64) {
        self.emit(if offset > 0 { "fastForward" } else { "rewind" });
    }
    fn set_position(&self, _track: ObjectPath<'_>, _position: i64) {}
    fn open_uri(&self, _uri: String) {}

    #[zbus(property, name = "PlaybackStatus")]
    fn playback_status(&self) -> String {
        self.status.lock().unwrap().clone()
    }

    #[zbus(property, name = "LoopStatus")]
    fn loop_status(&self) -> String {
        "None".into()
    }

    #[zbus(property, name = "Rate")]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property, name = "Shuffle")]
    fn shuffle(&self) -> bool {
        false
    }

    #[zbus(property, name = "Metadata")]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        HashMap::new()
    }

    #[zbus(property, name = "Volume")]
    fn volume(&self) -> f64 {
        1.0
    }

    #[zbus(property, name = "Position")]
    fn position(&self) -> i64 {
        0
    }

    #[zbus(property, name = "MinimumRate")]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property, name = "MaximumRate")]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property, name = "CanGoNext")]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property, name = "CanGoPrevious")]
    fn can_go_previous(&self) -> bool {
        true
    }

    #[zbus(property, name = "CanPlay")]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property, name = "CanPause")]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property, name = "CanSeek")]
    fn can_seek(&self) -> bool {
        false
    }

    #[zbus(property, name = "CanControl")]
    fn can_control(&self) -> bool {
        true
    }
}

pub async fn start_media_player(
    conn: &Connection,
    adapter: &str,
    events: crate::livi_sock::Broadcaster,
) -> Result<MediaPlayerHandle, Box<dyn Error>> {
    let status = std::sync::Arc::new(std::sync::Mutex::new("Playing".to_string()));
    conn.object_server().at(PLAYER_PATH, MprisRoot).await?;
    conn.object_server()
        .at(
            PLAYER_PATH,
            MprisPlayer {
                events,
                status: status.clone(),
            },
        )
        .await?;

    let mut props: HashMap<&str, Value> = HashMap::new();
    props.insert("PlaybackStatus", Value::from("Playing"));
    props.insert("LoopStatus", Value::from("None"));
    props.insert("Rate", Value::from(1.0f64));
    props.insert("Shuffle", Value::from(false));
    props.insert("Volume", Value::from(1.0f64));
    props.insert("Position", Value::from(0i64));
    props.insert("MinimumRate", Value::from(1.0f64));
    props.insert("MaximumRate", Value::from(1.0f64));
    props.insert("CanGoNext", Value::from(true));
    props.insert("CanGoPrevious", Value::from(true));
    props.insert("CanPlay", Value::from(true));
    props.insert("CanPause", Value::from(true));
    props.insert("CanSeek", Value::from(false));
    props.insert("CanControl", Value::from(true));

    let path = format!("/org/bluez/{adapter}");
    conn.call_method(
        Some("org.bluez"),
        path.as_str(),
        Some("org.bluez.Media1"),
        "RegisterPlayer",
        &(ObjectPath::try_from(PLAYER_PATH)?, props),
    )
    .await?;
    println!("[aa] media player registered at {PLAYER_PATH}");
    Ok(MediaPlayerHandle {
        conn: conn.clone(),
        status,
    })
}

/// Stops advertising, so phones no longer try to reach a head unit that is gone.
pub async fn set_discoverable(conn: &Connection, adapter: &str, on: bool) {
    let path = format!("/org/bluez/{adapter}");
    for prop in ["Discoverable", "Pairable"] {
        if let Err(e) = set_prop(conn, &path, prop, Value::from(on)).await {
            eprintln!("[cp] could not set {prop}={on}: {e}");
        }
    }
}

pub async fn adapter_address(conn: &Connection, adapter: &str) -> Result<[u8; 6], Box<dyn Error>> {
    let path = format!("/org/bluez/{adapter}");
    let reply = conn
        .call_method(
            Some("org.bluez"),
            path.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.bluez.Adapter1", "Address"),
        )
        .await?;
    let value: OwnedValue = reply.body().deserialize()?;
    let addr = String::try_from(value)?;
    let mut mac = [0u8; 6];
    for (i, part) in addr.split(':').enumerate().take(6) {
        mac[i] = u8::from_str_radix(part, 16).unwrap_or(0);
    }
    Ok(mac)
}
