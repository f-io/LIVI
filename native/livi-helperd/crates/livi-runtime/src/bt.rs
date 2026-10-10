use std::collections::HashMap;
use std::error::Error;
use std::os::fd::OwnedFd;

use tokio::sync::mpsc;
use zbus::Connection;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

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
const LET_GO_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

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
    adapter: String,
}

#[zbus::interface(name = "org.bluez.Profile1")]
impl Profile {
    async fn new_connection(
        &self,
        device: ObjectPath<'_>,
        fd: zbus::zvariant::OwnedFd,
        _options: HashMap<String, OwnedValue>,
        #[zbus(connection)] conn: &Connection,
    ) -> zbus::fdo::Result<()> {
        refuse_other_adapter(device.as_str(), &self.adapter)?;
        trust(conn, device.as_str()).await;
        let peer_mac = mac_from_device_path(device.as_str());
        let fd = OwnedFd::from(fd);
        let _ = self.tx.send(IncomingConn { fd, peer_mac });
        Ok(())
    }

    fn request_disconnection(&self, _device: ObjectPath<'_>) {}

    fn release(&self) {}
}

struct Agent {
    adapter: String,
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {}
    fn authorize_service(&self, device: ObjectPath<'_>, _uuid: String) -> zbus::fdo::Result<()> {
        refuse_other_adapter(device.as_str(), &self.adapter)
    }
    fn request_pin_code(&self, device: ObjectPath<'_>) -> zbus::fdo::Result<String> {
        refuse_other_adapter(device.as_str(), &self.adapter)?;
        Ok("0000".into())
    }
    fn request_passkey(&self, device: ObjectPath<'_>) -> zbus::fdo::Result<u32> {
        refuse_other_adapter(device.as_str(), &self.adapter)?;
        Ok(0)
    }
    fn display_passkey(&self, _device: ObjectPath<'_>, _passkey: u32, _entered: u16) {}
    fn display_pin_code(&self, _device: ObjectPath<'_>, _pincode: String) {}
    fn request_confirmation(&self, device: ObjectPath<'_>, _passkey: u32) -> zbus::fdo::Result<()> {
        refuse_other_adapter(device.as_str(), &self.adapter)
    }
    fn request_authorization(&self, device: ObjectPath<'_>) -> zbus::fdo::Result<()> {
        refuse_other_adapter(device.as_str(), &self.adapter)
    }
    fn cancel(&self) {}
}

/// A controller's settings as LIVI found them.
#[derive(Clone, Debug, PartialEq)]
struct Found {
    alias: String,
    discoverable: bool,
    pairable: bool,
    discoverable_timeout: u32,
}

/// Every controller LIVI changed, with what goes back when it lets go.
#[derive(Default)]
pub struct Held(Vec<(String, Found)>);

/// LIVI's name on a controller it has not touched yet is left over from a run that never let go,
/// so that one goes back to the system's name, out of sight.
fn to_put_back(found: Found, alias: &str) -> Found {
    if found.alias == alias {
        Found { alias: String::new(), discoverable: false, ..found }
    } else {
        found
    }
}

async fn controllers(conn: &Connection) -> Result<Vec<(String, Found)>, Box<dyn Error>> {
    let reply = conn
        .call_method(
            Some("org.bluez"),
            "/",
            Some("org.freedesktop.DBus.ObjectManager"),
            "GetManagedObjects",
            &(),
        )
        .await?;
    let objects: HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>> =
        reply.body().deserialize()?;
    let mut out = Vec::new();
    for (path, interfaces) in objects {
        let Some(props) = interfaces.get("org.bluez.Adapter1") else { continue };
        let Some(name) = path.as_str().strip_prefix("/org/bluez/") else { continue };
        let value = |key: &str| props.get(key).and_then(|v| v.try_clone().ok());
        out.push((
            name.to_string(),
            Found {
                alias: value("Alias").and_then(|v| String::try_from(v).ok()).unwrap_or_default(),
                discoverable: value("Discoverable").and_then(|v| bool::try_from(v).ok())
                    == Some(true),
                pairable: value("Pairable").and_then(|v| bool::try_from(v).ok()) == Some(true),
                discoverable_timeout: value("DiscoverableTimeout")
                    .and_then(|v| u32::try_from(v).ok())
                    .unwrap_or(0),
            },
        ));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Phones pair on LIVI's controller only, every other one stays out of their sight meanwhile.
async fn hold(conn: &Connection, adapter: &str, alias: &str) -> Held {
    let found = match controllers(conn).await {
        Ok(found) => found,
        Err(e) => {
            eprintln!("[bt] the controllers could not be read, nothing will be put back: {e}");
            return Held::default();
        }
    };
    let held: Vec<(String, Found)> =
        found.into_iter().map(|(name, found)| (name, to_put_back(found, alias))).collect();
    for (name, back) in &held {
        if name == adapter {
            continue;
        }
        let path = format!("/org/bluez/{name}");
        if back.alias.is_empty()
            && let Err(e) = set_prop(conn, &path, "Alias", Value::from("")).await
        {
            eprintln!("[bt] {name} keeps LIVI's name: {e}");
        }
        for prop in ["Discoverable", "Pairable"] {
            if let Err(e) = set_prop(conn, &path, prop, Value::from(false)).await {
                eprintln!("[bt] {name} could not be set {prop}=false: {e}");
            }
        }
    }
    Held(held)
}

/// Puts every controller back the way LIVI found it. Powered stays as it is.
pub async fn let_go(conn: &Connection, adapter: &str, held: &Held) {
    // While our advertisement runs the controller answers Busy to every discoverable change.
    stop_ble_ad(conn, adapter).await;
    for (name, back) in &held.0 {
        let path = format!("/org/bluez/{name}");
        let props = [
            ("Alias", Value::from(back.alias.as_str())),
            ("DiscoverableTimeout", Value::from(back.discoverable_timeout)),
            ("Discoverable", Value::from(back.discoverable)),
            ("Pairable", Value::from(back.pairable)),
        ];
        for (prop, value) in props {
            if let Err(e) = set_prop_within(conn, &path, prop, value, LET_GO_WAIT).await {
                eprintln!("[bt] {name}: {prop} could not be put back: {e}");
            }
        }
    }
}

/// BlueZ offers a profile on every controller, but only the chosen one is LIVI's.
fn refuse_other_adapter(device: &str, adapter: &str) -> zbus::fdo::Result<()> {
    if device.starts_with(&format!("/org/bluez/{adapter}/")) {
        return Ok(());
    }
    println!(
        "[bt] {} came in over another controller than {adapter}, turned away",
        mac_from_device_path(device)
    );
    Err(zbus::fdo::Error::AccessDenied(format!("LIVI listens on {adapter} only")))
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
) -> Result<(Connection, mpsc::UnboundedReceiver<IncomingConn>, Held), Box<dyn Error>> {
    let conn = Connection::system().await?;
    let (tx, rx) = mpsc::unbounded_channel();

    let ours = || adapter.to_string();
    conn.object_server().at(IAP_SERVER_PATH, Profile { tx: tx.clone(), adapter: ours() }).await?;
    conn.object_server().at(IAP_CLIENT_PATH, Profile { tx: tx.clone(), adapter: ours() }).await?;
    conn.object_server().at(CARPLAY_PATH, Profile { tx, adapter: ours() }).await?;
    conn.object_server().at(AGENT_PATH, Agent { adapter: ours() }).await?;

    let mut iap_opts: HashMap<&str, Value> = HashMap::new();
    iap_opts.insert("Role", Value::from("server"));
    iap_opts.insert("Channel", Value::from(IAP_CHANNEL));
    iap_opts.insert("ServiceRecord", Value::from(IAP_RECORD));
    iap_opts.insert("RequireAuthentication", Value::from(false));
    iap_opts.insert("RequireAuthorization", Value::from(false));
    register_profile(&conn, IAP_SERVER_PATH, IAP_SERVER_UUID, iap_opts).await?;

    // The client profile with AutoConnect lets BlueZ page a known phone back after a restart.
    // Without authentication on our own pages a phone that lost its key pairs without bonding,
    // and BlueZ forgets it again on the next disconnect.
    let mut client_opts: HashMap<&str, Value> = HashMap::new();
    client_opts.insert("Role", Value::from("client"));
    client_opts.insert("AutoConnect", Value::from(true));
    client_opts.insert("RequireAuthentication", Value::from(true));
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
    let held = hold(&conn, adapter, alias).await;
    set_prop(&conn, &adapter_path, "Alias", Value::from(alias)).await?;
    set_prop(&conn, &adapter_path, "DiscoverableTimeout", Value::from(0u32)).await?;
    // A soft-blocked adapter refuses Powered with org.bluez.Error.Blocked.
    let _ = std::process::Command::new(crate::sys::tool("rfkill"))
        .args(["unblock", "bluetooth"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    set_prop(&conn, &adapter_path, "Powered", Value::from(true)).await?;
    set_prop(&conn, &adapter_path, "Discoverable", Value::from(discoverable)).await?;
    set_prop(&conn, &adapter_path, "Pairable", Value::from(discoverable)).await?;

    Ok((conn, rx, held))
}

/// A tunnelled controller shows up in BlueZ late.
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

async fn set_prop(
    conn: &Connection,
    path: &str,
    name: &str,
    value: Value<'_>,
) -> Result<(), Box<dyn Error>> {
    set_prop_within(conn, path, name, value, ADAPTER_WAIT).await
}

/// BlueZ answers Busy on an adapter it has only just published.
async fn set_prop_within(
    conn: &Connection,
    path: &str,
    name: &str,
    value: Value<'_>,
    patience: std::time::Duration,
) -> Result<(), Box<dyn Error>> {
    let deadline = std::time::Instant::now() + patience;
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

async fn trust(conn: &Connection, device: &str) {
    let set = conn
        .call_method(
            Some("org.bluez"),
            device,
            Some("org.freedesktop.DBus.Properties"),
            "Set",
            &("org.bluez.Device1", "Trusted", Value::from(true)),
        )
        .await;
    if let Err(e) = set {
        println!("[bt] {device}: not marked trusted: {e}");
    }
}

fn busy(e: &zbus::Error) -> bool {
    match e {
        zbus::Error::MethodError(name, message, _) => {
            busy_answer(name.as_str(), message.as_deref())
        }
        _ => false,
    }
}

/// A mode change that meets another one in flight comes back as Failed with Busy in its text.
fn busy_answer(name: &str, message: Option<&str>) -> bool {
    name == "org.bluez.Error.Busy" || (name == "org.bluez.Error.Failed" && message == Some("Busy"))
}

pub async fn start_aa(
    conn: &Connection,
    adapter: &str,
) -> Result<mpsc::UnboundedReceiver<IncomingConn>, Box<dyn Error>> {
    let (tx, rx) = mpsc::unbounded_channel();
    conn.object_server().at(AA_PATH, Profile { tx, adapter: adapter.to_string() }).await?;

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
    adapter: String,
}

#[zbus::interface(name = "org.bluez.Profile1")]
impl HfpProfile {
    fn new_connection(
        &self,
        device: ObjectPath<'_>,
        fd: zbus::zvariant::OwnedFd,
        _options: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<()> {
        refuse_other_adapter(device.as_str(), &self.adapter)?;
        let mac = mac_from_device_path(device.as_str());
        println!("[hfp] connection from {mac}");
        self.hfp.accept(OwnedFd::from(fd), mac);
        Ok(())
    }

    fn request_disconnection(&self, _device: ObjectPath<'_>) {}

    fn release(&self) {}
}

/// The audio daemon usually holds HF already, and a second SLC makes the phone drop one.
pub async fn start_hfp(
    conn: &Connection,
    adapter: &str,
    hfp: crate::hfp::Hfp,
) -> Result<(), Box<dyn Error>> {
    conn.object_server().at(HFP_PATH, HfpProfile { hfp, adapter: adapter.to_string() }).await?;
    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("Name", Value::from("HFP Hands-Free"));
    opts.insert("Role", Value::from("client"));
    // We page the phone on this profile, see the iAP client.
    opts.insert("RequireAuthentication", Value::from(true));
    opts.insert("RequireAuthorization", Value::from(false));
    opts.insert("Features", Value::from(0x009cu16));
    opts.insert("Version", Value::from(0x0108u16));
    match register_profile(conn, HFP_PATH, HFP_HF_UUID, opts).await {
        Ok(()) => println!("[hfp] HF profile registered"),
        Err(e) => println!("[hfp] HF profile held by the audio daemon ({e})"),
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

/// Lets phones find the head unit without a BR/EDR scan.
pub async fn start_ble_ad(
    conn: &Connection,
    adapter: &str,
    name: &str,
) -> Result<(), Box<dyn Error>> {
    conn.object_server().at(BLE_AD_PATH, BleAd { name: name.into() }).await?;
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

/// Without wireless Android Auto there is none, so a refusal is no news.
async fn stop_ble_ad(conn: &Connection, adapter: &str) {
    let Ok(ad) = ObjectPath::try_from(BLE_AD_PATH) else { return };
    let _ = conn
        .call_method(
            Some("org.bluez"),
            format!("/org/bluez/{adapter}").as_str(),
            Some("org.bluez.LEAdvertisingManager1"),
            "UnregisterAdvertisement",
            &(ad,),
        )
        .await;
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

/// BlueZ hands AVRCP passthrough keys to this player.
pub struct MprisPlayer {
    events: crate::livi_sock::Broadcaster,
    status: std::sync::Arc<std::sync::Mutex<String>>,
}

/// The peer's play/pause toggle picks its verb from PlaybackStatus.
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
        if let Ok(iface) = self.conn.object_server().interface::<_, MprisPlayer>(PLAYER_PATH).await
        {
            let _ = iface.get().await.playback_status_changed(iface.signal_emitter()).await;
        }
    }
}

impl MprisPlayer {
    fn emit(&self, command: &str) {
        self.events.push_json(format!("{{\"event\":\"input\",\"command\":\"{command}\"}}"));
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
    conn.object_server().at(PLAYER_PATH, MprisPlayer { events, status: status.clone() }).await?;

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
    Ok(MediaPlayerHandle { conn: conn.clone(), status })
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

/// CarPlay over the cable keeps the phone's Bluetooth to the accessory disconnected.
pub fn drop_link(conn: &Connection, adapter: &str, mac: String) {
    let (conn, adapter) = (conn.clone(), adapter.to_string());
    tokio::spawn(async move {
        if let Err(e) = crate::livi_sock::device_disconnect(&conn, &adapter, &mac).await {
            eprintln!("[helperd] {mac} stays on bluetooth: {e}");
        }
    });
}

pub fn on_cable(
    state: std::sync::Arc<crate::state::HelperState>,
    conn: &Connection,
    adapter: &str,
) -> crate::bringup::OnCable {
    let (conn, adapter) = (conn.clone(), adapter.to_string());
    crate::bringup::OnCable(std::sync::Arc::new(move |mac: &str| {
        let cabled = state.carkit_claims(mac);
        if cabled {
            drop_link(&conn, &adapter, mac.to_string());
        }
        cabled
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_chosen_controller_is_answered() {
        let phone = "/org/bluez/hci2/dev_0C_6A_C4_4E_F3_2A";
        assert!(refuse_other_adapter(phone, "hci2").is_ok());
        assert!(refuse_other_adapter(phone, "hci0").is_err());
        assert!(refuse_other_adapter("/org/bluez/hci10/dev_0C_6A_C4_4E_F3_2A", "hci1").is_err());
    }

    fn found(alias: &str, discoverable: bool) -> Found {
        Found { alias: alias.into(), discoverable, pairable: true, discoverable_timeout: 180 }
    }

    #[test]
    fn a_controller_goes_back_the_way_it_was_found() {
        assert_eq!(to_put_back(found("blacky", false), "LIVI blacky"), found("blacky", false));
        assert_eq!(to_put_back(found("blacky #2", true), "LIVI blacky"), found("blacky #2", true));
    }

    #[test]
    fn livis_name_left_on_a_controller_goes_back_to_the_systems() {
        let back = to_put_back(found("LIVI blacky", true), "LIVI blacky");
        assert_eq!(back, Found { alias: String::new(), discoverable: false, ..found("", true) });
    }

    #[test]
    fn busy_comes_as_its_own_error_or_as_a_failed_mode_change() {
        assert!(busy_answer("org.bluez.Error.Busy", None));
        assert!(busy_answer("org.bluez.Error.Failed", Some("Busy")));
        assert!(!busy_answer("org.bluez.Error.Failed", Some("Not Powered")));
        assert!(!busy_answer("org.bluez.Error.Blocked", Some("Busy")));
    }
}
