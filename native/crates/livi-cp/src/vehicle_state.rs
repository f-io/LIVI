// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::{BTreeMap, BTreeSet};

use crate::bplist;
use crate::image_archive;
use crate::opack::{self, Value, dict};

mod body;
mod climate;
mod cluster;
mod engines;
mod media;
mod navigation;
mod resources;
mod rules;
mod settings;
mod volume;
mod wire;

use body::*;
use climate::*;
pub use cluster::*;
pub use engines::*;
pub use media::*;
use navigation::*;
use resources::*;
use settings::*;
pub use volume::*;
use wire::*;
pub const FEATURE: &str = "vehicleStateProtocol";
pub const HIGH_PRIORITY_DATASTREAM_UUID: &str = "FF4A6720-F2BE-4F56-A3E1-DB3B4E37D634";
pub const DATASTREAM_UUID: &str = "3E2F3C61-AAD0-42CB-A8AA-BF22186DA62E";

const PROTOCOL_VERSION: &str = "1.0";
const PLUGIN: &str = "LIVI";
const PLUGIN_ID: i64 = 0;
const ACCESSORY_VERSION: &str = "1.0";
const HIGH_PRIORITY: i64 = 1;
const MAX_LENGTH: i64 = 65535;
const CELSIUS: i64 = 1082;
const KILOMETERS: i64 = 151;
const MILES: i64 = 164;
const KILOPASCALS: i64 = 932;
const PERCENT: i64 = 1205;
const MILLILITERS_PER_100_KM: i64 = 674;
const REVOLUTIONS_PER_MINUTE: i64 = 981;
const KILOWATTS: i64 = 884;
const SECONDS: i64 = 251;
const NO_VALUE: i64 = -6727;
const WRITTEN: i64 = 0;
const NO_SUCH_COMMAND: i64 = -6735;
const SENT_BY_PHONE: i64 = 0;
const LARGE_LENGTH: i64 = 2_147_483_647;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    High,
    Normal,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Bool = 0,
    Uint8 = 1,
    Uint32 = 3,
    Uint64 = 4,
    Int32 = 7,
    Float = 9,
    String = 10,
    Data = 11,
    Dict = 12,
    Array = 13,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    Phone,
    Car,
    Shared,
}

struct Characteristic {
    iid: i64,
    kind: i64,
    name: &'static str,
    format: Format,
    owner: Owner,
    mutable: bool,
    range: Option<(f64, f64, f64)>,
    units: Option<i64>,
    enabled: bool,
    large: bool,
    notifier: Option<i64>,
    invalidable: bool,
    initial: Option<Value>,
}

fn characteristic(
    iid: i64,
    kind: i64,
    name: &'static str,
    format: Format,
    owner: Owner,
    initial: Option<Value>,
) -> Characteristic {
    let counts = matches!(format, Format::Uint32 | Format::Uint64);
    let range = counts.then_some((0.0, f64::from(u32::MAX), 1.0));
    Characteristic {
        iid,
        kind,
        name,
        format,
        owner,
        mutable: true,
        range,
        units: None,
        enabled: false,
        large: false,
        notifier: None,
        invalidable: owner == Owner::Phone && format != Format::Uint8,
        initial,
    }
}

fn phone(iid: i64, kind: i64, name: &'static str, format: Format) -> Characteristic {
    characteristic(iid, kind, name, format, Owner::Phone, None)
}

fn car(iid: i64, kind: i64, name: &'static str, format: Format, initial: Value) -> Characteristic {
    characteristic(iid, kind, name, format, Owner::Car, Some(initial))
}

fn shared(
    iid: i64,
    kind: i64,
    name: &'static str,
    format: Format,
    initial: Value,
) -> Characteristic {
    characteristic(iid, kind, name, format, Owner::Shared, Some(initial))
}

impl Characteristic {
    fn up_to(self, max: u32) -> Self {
        self.between(0.0, f64::from(max), 1.0)
    }

    fn between(self, min: f64, max: f64, step: f64) -> Self {
        Self {
            range: Some((min, max, step)),
            ..self
        }
    }

    fn enabled(self) -> Self {
        Self {
            enabled: true,
            ..self
        }
    }

    fn in_units(self, units: i64) -> Self {
        Self {
            units: Some(units),
            ..self
        }
    }

    fn fixed(self) -> Self {
        Self {
            mutable: false,
            ..self
        }
    }

    fn large(self, notifier: i64) -> Self {
        Self {
            large: true,
            notifier: Some(notifier),
            ..self
        }
    }

    fn invalidable(self) -> Self {
        Self {
            invalidable: true,
            ..self
        }
    }

    fn take(&self, v: &serde_json::Value) -> Option<Value> {
        if self.format == Format::Bool {
            return v.as_bool().map(Value::Bool);
        }
        let mut n = v.as_f64().filter(|n| n.is_finite())?;
        if let Some((min, max, step)) = self.range {
            n = (min + ((n - min) / step).round() * step).clamp(min, max);
        }
        match self.format {
            Format::Float => Some(Value::Real(n)),
            Format::Uint8 | Format::Uint32 | Format::Uint64 | Format::Int32 => {
                Some(Value::Int(n.round() as i64))
            }
            _ => None,
        }
    }

    fn describe(&self) -> Value {
        let mut d = vec![
            ("iid", Value::Int(self.iid)),
            ("type", Value::Int(self.kind)),
            ("format", Value::Int(self.format as i64)),
            ("mutable", Value::Bool(self.mutable)),
            ("writable", Value::Bool(self.owner != Owner::Car)),
            ("oemWritable", Value::Bool(self.owner != Owner::Phone)),
            ("priority", Value::Int(HIGH_PRIORITY)),
        ];
        let number = |n: f64| match self.format {
            Format::Float => Value::Real(n),
            _ => Value::Int(n as i64),
        };
        if let Some((min, max, step)) = self.range {
            d.extend([
                ("minimumValue", number(min)),
                ("maximumValue", number(max)),
                ("stepValue", number(step)),
            ]);
        }
        if matches!(
            self.format,
            Format::String | Format::Data | Format::Dict | Format::Array
        ) {
            let length = if self.large { LARGE_LENGTH } else { MAX_LENGTH };
            d.push(("maximumLength", Value::Int(length)));
        }
        if self.large {
            d.push(("largePayload", Value::Bool(true)));
        }
        if let Some(notifier) = self.notifier {
            d.push(("iidNotifier", Value::Int(notifier)));
        }
        if self.enabled {
            d.push(("disabled", Value::Bool(false)));
        }
        if let Some(units) = self.units {
            d.push(("units", Value::Int(units)));
        }
        if self.invalidable {
            d.push(("supportsInvalid", Value::Bool(true)));
        }
        if let Some(v) = &self.initial {
            d.push(("initialValue", v.clone()));
        }
        Value::Dict(d.into_iter().map(|(k, v)| (Value::str(k), v)).collect())
    }
}

struct Service {
    iid: i64,
    kind: i64,
    characteristics: Vec<Characteristic>,
}

struct Accessory {
    name: &'static str,
    iid: i64,
    kind: i64,
    services: Vec<Service>,
}

struct Control {
    iid: i64,
    kind: i64,
    name: &'static str,
    parameters: Vec<(&'static str, Format)>,
    response: Vec<(&'static str, Format)>,
    urgent: bool,
}

impl Control {
    fn describe(&self) -> Value {
        let mut d = vec![
            ("iid", Value::Int(self.iid)),
            ("type", Value::Int(self.kind)),
            ("sender", Value::Int(SENT_BY_PHONE)),
            ("hasResponse", Value::Bool(true)),
        ];
        if self.urgent {
            d.push(("priority", Value::Int(HIGH_PRIORITY)));
        }
        let described = |values: &[(&'static str, Format)]| {
            let values = values.iter().map(|(name, format)| {
                dict([
                    ("name", Value::str(name)),
                    ("format", Value::Int(*format as i64)),
                ])
            });
            Value::Array(values.collect())
        };
        if !self.parameters.is_empty() {
            d.push(("requestParameters", described(&self.parameters)));
        }
        if !self.response.is_empty() {
            d.push(("responseParameters", described(&self.response)));
        }
        Value::Dict(d.into_iter().map(|(k, v)| (Value::str(k), v)).collect())
    }
}

pub type Gauges = serde_json::Map<String, serde_json::Value>;
pub const GAUGE_KEYS: [&str; 8] = [
    "rpm",
    "fuelPct",
    "rangeKm",
    "consumptionLPer100Km",
    "consumptionAvgLPer100Km",
    "odometerKm",
    "tirePressure",
    "volume",
];

const DRIVER_TARGET_TEMPERATURE: i64 = 520;
const PASSENGER_TARGET_TEMPERATURE: i64 = 534;
const ZONES_SYNCED: i64 = 538;
const SEAT_HEATING: [(i64, i64); 2] = [(1026, 1027), (1030, 1031)];
const FAN: (i64, i64) = (525, 524);
const LEVELLED: [(i64, i64); 3] = [SEAT_HEATING[0], SEAT_HEATING[1], FAN];

pub type Climate = serde_json::Map<String, serde_json::Value>;
const CLIMATE_KEYS: [(&str, i64); 20] = [
    ("on", HVAC_ON),
    ("auto", AUTO_CLIMATE),
    ("ac", COMPRESSOR),
    ("maxAc", MAX_AC),
    ("maxDefrost", MAX_DEFROST),
    ("recirculation", RECIRCULATION),
    ("zonesSynced", ZONES_SYNCED),
    ("driverTargetC", DRIVER_TARGET_TEMPERATURE),
    ("passengerTargetC", PASSENGER_TARGET_TEMPERATURE),
    ("driverCabinC", DRIVER_TARGET_TEMPERATURE + 1),
    ("passengerCabinC", PASSENGER_TARGET_TEMPERATURE + 1),
    ("fanLevel", FAN.0),
    ("fanAuto", FAN_AUTO),
    ("frontWindowHeating", WINDOWS[0] + 1),
    ("rearWindowHeating", WINDOWS[1] + 1),
    ("ventUpper", VENTS[0] + 1),
    ("ventMiddle", VENTS[1] + 1),
    ("ventLower", VENTS[2] + 1),
    ("driverSeatHeating", SEAT_HEATING[0].0),
    ("passengerSeatHeating", SEAT_HEATING[1].0),
];
const VENTS_AUTO: &str = "ventsAuto";

pub fn default_climate() -> Climate {
    VehicleState::new(false, "", "")
        .with_climate(true)
        .climate()
        .unwrap_or_default()
}

fn to_json(v: &Value) -> Option<serde_json::Value> {
    match v {
        Value::Bool(b) => Some((*b).into()),
        Value::Int(n) => Some((*n).into()),
        Value::Real(f) => serde_json::Number::from_f64(*f).map(serde_json::Value::Number),
        _ => None,
    }
}

pub struct VehicleState {
    accessories: Vec<Accessory>,
    values: BTreeMap<i64, Value>,
    followed: BTreeSet<i64>,
    resume: BTreeMap<i64, Value>,
    before_max_ac: Option<Vec<(i64, Value)>>,
    controls: Vec<(i64, Control)>,
    radio_commands: Vec<RadioCommand>,
    image_archive: Option<Vec<u8>>,
    images: BTreeSet<String>,
    history: Vec<HistoryEntry>,
    hidden_by_phone: Vec<u64>,
    zone_ready: bool,
    phone: String,
    debug: bool,
}

impl VehicleState {
    pub fn new(debug: bool, app_version: &str, build_date: &str) -> Self {
        let accessories = vec![
            navigation(),
            automaker_settings(app_version, build_date),
            vehicle_resources(),
            fuel(),
            trip_computer(),
            tires(),
            audio(),
            media(),
            now_playing(),
            notification_history(),
        ];
        Self {
            values: initial_values(&accessories).collect(),
            accessories,
            followed: BTreeSet::new(),
            resume: BTreeMap::new(),
            before_max_ac: None,
            controls: radio_controls(),
            radio_commands: Vec::new(),
            image_archive: None,
            images: BTreeSet::new(),
            history: Vec::new(),
            hidden_by_phone: Vec::new(),
            zone_ready: false,
            phone: String::new(),
            debug,
        }
    }

    pub fn with_climate(mut self, controls: bool) -> Self {
        if !controls {
            return self;
        }
        let before = self
            .accessories
            .iter()
            .position(|a| a.iid == VEHICLE_RESOURCES)
            .unwrap_or(self.accessories.len());
        let climate = vec![climate(), seats()];
        self.values.extend(initial_values(&climate));
        self.accessories.splice(before..before, climate);
        self
    }

    pub fn with_images(mut self, images: &[(String, Vec<u8>)]) -> Self {
        if images.is_empty() {
            return self;
        }
        let archive = image_archive::build(images);
        let version = Value::String(format!(
            "{:08x}{:08x}",
            image_archive::crc32(&archive),
            archive_started()
        ));
        let version_iid = IMAGE_ARCHIVE_SERVICE + 1;
        if let Some(resources) = self
            .accessories
            .iter_mut()
            .find(|a| a.iid == VEHICLE_RESOURCES)
        {
            let identifier = car(
                version_iid,
                0x3000_0019,
                "imageArchiveVersion",
                Format::String,
                version.clone(),
            );
            resources.services.push(Service {
                iid: IMAGE_ARCHIVE_SERVICE,
                kind: 0x2000_0002,
                characteristics: vec![identifier.fixed()],
            });
        }
        self.controls.push((
            IMAGE_ARCHIVE_SERVICE,
            Control {
                iid: GET_IMAGE_ARCHIVE,
                kind: 0x4800_0008,
                name: "getImageArchive",
                parameters: Vec::new(),
                response: vec![("imageArchive", Format::Data)],
                urgent: false,
            },
        ));
        self.values.insert(version_iid, version);
        self.images = images.iter().map(|(name, _)| name.clone()).collect();
        self.image_archive = Some(archive);
        if !self.images.contains(LOGO) {
            return self;
        }
        let line = self
            .accessories
            .iter_mut()
            .flat_map(|a| &mut a.services)
            .flat_map(|s| &mut s.characteristics)
            .find(|c| c.iid == VERSION_LINE + 5);
        if let Some(line) = line {
            line.mutable = true;
        }
        self
    }

    fn archive_version(&self) -> Option<&str> {
        self.image_archive.as_ref()?;
        self.values
            .get(&(IMAGE_ARCHIVE_SERVICE + 1))
            .and_then(Value::as_str)
    }

    pub fn set_phone(&mut self, controller_id: &str) {
        self.phone = controller_id.to_string();
    }

    fn archive_key(&self) -> Option<(String, String)> {
        self.archive_version()
            .map(|v| (self.phone.clone(), v.to_string()))
    }

    fn archive_fetched(&self) -> bool {
        self.archive_key()
            .is_some_and(|k| fetched_archives().contains(&k))
    }

    fn symbol_name(&self, name: &str) -> String {
        if self.archive_fetched() && self.images.contains(name) {
            format!("imageArchive://?identifier={name}")
        } else {
            name.to_string()
        }
    }

    fn logo_symbol_update(&mut self) -> Vec<(Lane, Vec<u8>)> {
        if !self.archive_fetched() || !self.images.contains(LOGO) {
            return Vec::new();
        }
        println!("[cpVehicle] car sets the version line's logo");
        let symbol = Value::String(self.symbol_name(LOGO));
        self.group(vec![(VERSION_LINE + 5, symbol)])
            .into_iter()
            .collect()
    }

    pub fn set_history(&mut self, entries: Vec<HistoryEntry>) -> Vec<(Lane, Vec<u8>)> {
        self.history = entries;
        self.history.truncate(HISTORY_SLOTS);
        if self.followed.contains(&(history_slot(0) + 1)) {
            self.history_update()
        } else {
            Vec::new()
        }
    }

    pub fn take_hidden_by_phone(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.hidden_by_phone)
    }

    fn history_update(&mut self) -> Vec<(Lane, Vec<u8>)> {
        let mut out = Vec::new();
        for n in 0..HISTORY_SLOTS {
            let iid = history_slot(n);
            let entry = self.history.get(n).cloned().unwrap_or_default();
            let filled = n < self.history.len();
            let symbol = Value::String(self.symbol_name(&entry.notice.symbol));
            out.extend(self.group(vec![
                (iid + 1, Value::String(entry.notice.title)),
                (iid + 2, Value::String(entry.notice.text)),
                (iid + 3, Value::Int(entry.at_secs as i64)),
                (iid + 4, symbol),
                (iid + 5, Value::Int(0)),
                (iid + 6, Value::Bool(true)),
                (iid + 7, Value::Bool(!filled || entry.hidden)),
            ]));
        }
        out
    }

    pub fn notice_open(&self) -> bool {
        self.values.get(&(TEMPORARY_ZONE + 2)) == Some(&Value::Bool(true))
    }

    pub fn take_zone_ready(&mut self) -> bool {
        std::mem::take(&mut self.zone_ready)
    }

    pub fn show_notice(&mut self, notice: &Notice) -> Vec<(Lane, Vec<u8>)> {
        if self.characteristic(NOTICE + 1).is_none()
            || self.characteristic(TEMPORARY_ZONE + 1).is_none()
        {
            return Vec::new();
        }
        println!("[cpVehicle] car shows the notification {:?}", notice.title);
        let name = self.symbol_name(&notice.symbol);
        let symbol = dict([("name", Value::String(name)), ("color", Value::Int(0))]);
        let button = dict([
            ("contentURLAction", Value::str("")),
            ("type", Value::Int(1)),
        ]);
        let identifier = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let fields = self.group(vec![
            (NOTICE + 1, Value::String(notice.title.clone())),
            (NOTICE + 2, Value::String(notice.text.clone())),
            (NOTICE + 3, Value::String(format!("livi-{identifier}"))),
            (NOTICE + 4, symbol),
            (NOTICE + 7, button),
            (NOTICE + 8, Value::Int(0)),
        ]);
        let zone = self.group(vec![
            (TEMPORARY_ZONE + 1, Value::str(NOTICE_URL)),
            (TEMPORARY_ZONE + 2, Value::Bool(true)),
        ]);
        [fields, zone].into_iter().flatten().collect()
    }

    pub fn hide_notice(&mut self) -> Vec<(Lane, Vec<u8>)> {
        if !self.notice_open() {
            return Vec::new();
        }
        println!("[cpVehicle] car closes the notification");
        self.group(vec![
            (TEMPORARY_ZONE + 1, Value::str(NOTICE_URL)),
            (TEMPORARY_ZONE + 2, Value::Bool(false)),
        ])
        .into_iter()
        .collect()
    }

    fn group(&mut self, values: Vec<(i64, Value)>) -> Option<(Lane, Vec<u8>)> {
        let pairs = values
            .into_iter()
            .map(|(iid, v)| {
                self.values.insert(iid, v.clone());
                (Value::Int(iid), v)
            })
            .collect();
        update_notify(pairs)
    }

    pub fn with_variant(mut self, model: &str) -> Self {
        if model.is_empty() {
            return self;
        }
        let named = Value::str(model);
        let variant = self
            .accessories
            .iter_mut()
            .flat_map(|a| &mut a.services)
            .find(|s| s.characteristics.iter().any(|c| c.iid == INTERIOR_STYLE));
        if let Some(service) = variant {
            let style =
                |iid, kind, name| car(iid, kind, name, Format::String, named.clone()).fixed();
            service.characteristics = vec![
                style(EXTERIOR_STYLE, 0x4800_0003, "exteriorStyle"),
                style(INTERIOR_STYLE, 0x4800_0004, "interiorStyle"),
                style(VEHICLE_LAYOUT, 0x4800_0005, "vehicleLayout"),
            ];
        }
        for iid in [EXTERIOR_STYLE, INTERIOR_STYLE, VEHICLE_LAYOUT] {
            self.values.insert(iid, named.clone());
        }
        self
    }

    pub fn with_cluster_content(mut self, content: Option<&ClusterContent>) -> Self {
        let Some(content) = content else { return self };
        let added = cluster_content(content);
        self.values.extend(initial_values(&added));
        self.accessories.extend(added);
        self
    }

    pub fn show_in_zone(&mut self, zone: usize, url: &str) -> Vec<(Lane, Vec<u8>)> {
        let base = 3073 + 6 * zone as i64;
        if self.characteristic(base + 1).is_none() {
            return Vec::new();
        }
        let mut changed = Vec::new();
        self.car_sets(base + 1, Value::str(url), &mut changed);
        self.car_sets(base + 2, Value::Bool(!url.is_empty()), &mut changed);
        update_notify(changed).into_iter().collect()
    }

    pub fn with_engines(mut self, engines: Engines) -> Self {
        let added: Vec<Accessory> = engines
            .combustion
            .map(combustion_engine)
            .into_iter()
            .chain(engines.electric.then(electric_engine))
            .collect();
        self.values.extend(initial_values(&added));
        self.accessories.extend(added);
        self
    }

    fn characteristic(&self, iid: i64) -> Option<&Characteristic> {
        self.accessories
            .iter()
            .flat_map(|a| &a.services)
            .flat_map(|s| &s.characteristics)
            .find(|c| c.iid == iid)
    }

    fn accessory_list(&self) -> Value {
        Value::Array(
            self.accessories
                .iter()
                .map(|a| {
                    let services = a.services.iter().map(|s| {
                        let characteristics =
                            s.characteristics.iter().map(Characteristic::describe);
                        let mut d = vec![
                            (Value::str("iid"), Value::Int(s.iid)),
                            (Value::str("type"), Value::Int(s.kind)),
                            (
                                Value::str("characteristics"),
                                Value::Array(characteristics.collect()),
                            ),
                        ];
                        let controls: Vec<Value> = self
                            .controls
                            .iter()
                            .filter(|(service, _)| *service == s.iid)
                            .map(|(_, c)| c.describe())
                            .collect();
                        if !controls.is_empty() {
                            d.push((Value::str("controls"), Value::Array(controls)));
                        }
                        Value::Dict(d)
                    });
                    dict([
                        ("iid", Value::Int(a.iid)),
                        ("type", Value::Int(a.kind)),
                        ("version", Value::str(ACCESSORY_VERSION)),
                        ("services", Value::Array(services.collect())),
                    ])
                })
                .collect(),
        )
    }

    pub fn info_entry(&self) -> (String, bplist::Value) {
        let plugin = dict([
            ("name", Value::str(PLUGIN)),
            ("pluginID", Value::Int(PLUGIN_ID)),
            ("accessories", self.accessory_list()),
        ]);
        let info = dict([
            ("protocolVersion", Value::str(PROTOCOL_VERSION)),
            ("pluginCount", Value::Int(1)),
            (
                "pluginMapping",
                Value::Dict(vec![(Value::str(PLUGIN), Value::Int(PLUGIN_ID))]),
            ),
            ("pluginConfigs", Value::Array(vec![plugin])),
        ]);
        ("vehicleStateProtocolInfo".into(), to_plist(&info))
    }

    pub fn handle(&mut self, lane: Lane, message: &[u8]) -> Vec<(Lane, Vec<u8>)> {
        let Some(outer) = opack::decode(message) else {
            println!("[cpVehicle] a {} B message is not OPACK", message.len());
            return Vec::new();
        };
        let plugin = outer.get("pluginID").and_then(Value::as_int);
        let request = outer
            .get("pluginData")
            .and_then(Value::as_data)
            .and_then(opack::decode);
        let Some(request) = request.filter(|_| plugin == Some(PLUGIN_ID)) else {
            println!("[cpVehicle] message for plugin {plugin:?} without readable data, dropped");
            return Vec::new();
        };
        let transaction = request.get("transactionID").cloned().unwrap_or(Value::Null);
        let values = request.get("values");
        match request.get("command").and_then(Value::as_str).unwrap_or("") {
            "configRequest" => {
                println!("[cpVehicle] phone reads the vehicle description");
                let answer = dict([
                    ("command", Value::str("configResponse")),
                    ("transactionID", transaction),
                    ("values", dict([("accessories", self.accessory_list())])),
                ]);
                vec![(lane, wrap(&answer))]
            }
            "registerRequest" => {
                let iids = values.and_then(Value::as_array).unwrap_or(&[]);
                let zone = iids.iter().any(|v| v.as_int() == Some(TEMPORARY_ZONE + 1));
                let symbol = iids.iter().any(|v| v.as_int() == Some(VERSION_LINE + 5));
                let history = iids.iter().any(|v| v.as_int() == Some(history_slot(0) + 1));
                let mut out = vec![(lane, wrap(&self.follow(iids, transaction)))];
                if symbol {
                    out.extend(self.logo_symbol_update());
                }
                if history {
                    out.extend(self.history_update());
                }
                if zone {
                    self.zone_ready = true;
                }
                out
            }
            "readRequest" => {
                let iids = values.and_then(Value::as_array).unwrap_or(&[]);
                vec![(lane, wrap(&self.read(iids, transaction)))]
            }
            "writeRequest" => {
                let written = values.and_then(Value::as_dict).unwrap_or(&[]);
                self.write(lane, written, transaction)
            }
            "controlRequest" => {
                let commands = values.and_then(Value::as_dict).unwrap_or(&[]);
                let fetched = commands
                    .iter()
                    .any(|(k, _)| k.as_int() == Some(GET_IMAGE_ARCHIVE));
                let mut out = vec![(lane, wrap(&self.control(commands, transaction)))];
                if fetched && self.image_archive.is_some() {
                    if let Some(key) = self.archive_key() {
                        fetched_archives().insert(key);
                    }
                    out.extend(self.logo_symbol_update());
                    if self.followed.contains(&(history_slot(0) + 1)) {
                        out.extend(self.history_update());
                    }
                }
                out
            }
            other => {
                println!("[cpVehicle] phone sent {other:?}, not handled");
                Vec::new()
            }
        }
    }

    fn control(&mut self, commands: &[(Value, Value)], transaction: Value) -> Value {
        let heard = self
            .values
            .get(&NOW_PLAYING_SOURCE)
            .and_then(Value::as_str)
            .and_then(RadioBand::named)
            .unwrap_or_default();
        let mut answered = Vec::new();
        let mut refused = Vec::new();
        for (key, parameters) in commands {
            let name = key
                .as_int()
                .and_then(|iid| self.controls.iter().find(|(_, c)| c.iid == iid))
                .map(|(_, c)| c.name);
            let Some(name) = name else {
                println!(
                    "[cpVehicle] phone sent command {}, the car has none such",
                    shown(key)
                );
                refused.push((key.clone(), Value::Int(NO_SUCH_COMMAND)));
                continue;
            };
            if name == "getImageArchive" {
                let zip = self.image_archive.clone().unwrap_or_default();
                println!(
                    "[cpVehicle] phone fetched the image archive, {} B",
                    zip.len()
                );
                answered.push((key.clone(), dict([("imageArchive", Value::Data(zip))])));
                continue;
            }
            println!("[cpVehicle] phone pressed {name} {}", shown(parameters));
            let text = |k| parameters.get(k).and_then(Value::as_str);
            let band = text("sourceIdentifier").map_or(Some(heard), RadioBand::named);
            let khz = parameters
                .get("frequency")
                .and_then(Value::as_int)
                .and_then(|n| u32::try_from(n).ok());
            let command = match name {
                "play" => Some(RadioCommand::Play),
                "pause" => Some(RadioCommand::Pause),
                "nextItem" => Some(RadioCommand::Next),
                "previousItem" => Some(RadioCommand::Previous),
                "tuneToIdentifier" => band
                    .zip(text("identifier"))
                    .map(|(b, id)| RadioCommand::Tune(b, id.to_string())),
                "tuneToFrequency" if text("sourceIdentifier").is_none_or(|s| s == FM_SOURCE) => {
                    khz.map(RadioCommand::TuneFrequency)
                }
                "changeMediaSource" => text("identifier")
                    .and_then(RadioBand::named)
                    .map(RadioCommand::Source),
                _ => None,
            };
            self.radio_commands.extend(command);
            answered.push((key.clone(), Value::Null));
        }
        let mut answer = vec![
            (Value::str("command"), Value::str("controlResponse")),
            (Value::str("transactionID"), transaction),
        ];
        if !answered.is_empty() {
            answer.push((Value::str("values"), Value::Dict(answered)));
        }
        if !refused.is_empty() {
            answer.push((Value::str("errors"), Value::Dict(refused)));
        }
        Value::Dict(answer)
    }

    pub fn take_radio_commands(&mut self) -> Vec<RadioCommand> {
        std::mem::take(&mut self.radio_commands)
    }

    pub fn set_radio(&mut self, radio: &Radio) -> Vec<(Lane, Vec<u8>)> {
        let items = |source: &RadioSource, band| {
            Value::Array(
                source
                    .stations
                    .iter()
                    .map(|s| radio_item(s, band))
                    .collect(),
            )
        };
        let images = Value::Array(
            radio
                .dab
                .stations
                .iter()
                .filter_map(|s| {
                    let image = s.image.clone()?;
                    Some(dict([
                        ("identifier", Value::str(&s.id)),
                        ("imageData", Value::Data(image)),
                    ]))
                })
                .collect(),
        );
        let current =
            |source: &RadioSource| Value::String(source.current.clone().unwrap_or_default());
        let dab_off = !radio.available || radio.dab.stations.is_empty();
        let state = if radio.playing { PLAYING } else { PAUSED };
        let mut changed = Vec::new();
        self.car_sets(RADIO_DISABLED, Value::Bool(dab_off), &mut changed);
        self.car_sets(RADIO_ITEMS, items(&radio.dab, RadioBand::Dab), &mut changed);
        self.car_sets(RADIO_ITEM_IMAGES, images, &mut changed);
        self.car_sets(RADIO_CURRENT_ITEM, current(&radio.dab), &mut changed);
        self.car_sets(FM_DISABLED, Value::Bool(!radio.available), &mut changed);
        self.car_sets(FM_ITEMS, items(&radio.fm, RadioBand::Fm), &mut changed);
        self.car_sets(FM_CURRENT_ITEM, current(&radio.fm), &mut changed);
        if let Some(khz) = radio.frequency {
            self.car_sets(FM_FREQUENCY, Value::Int(i64::from(khz)), &mut changed);
        }
        self.car_sets(
            NOW_PLAYING_SOURCE,
            Value::str(radio.band.source()),
            &mut changed,
        );
        self.car_sets(NOW_PLAYING_TITLE, Value::str(&radio.title), &mut changed);
        self.car_sets(NOW_PLAYING_ARTIST, Value::str(&radio.artist), &mut changed);
        self.car_sets(NOW_PLAYING_ALBUM, Value::str(&radio.album), &mut changed);
        self.car_sets(
            NOW_PLAYING_ARTWORK,
            Value::Data(radio.artwork.clone()),
            &mut changed,
        );
        self.car_sets(PLAYBACK_STATE, Value::Int(state), &mut changed);
        update_notify(changed).into_iter().collect()
    }

    fn follow(&mut self, iids: &[Value], transaction: Value) -> Value {
        let mut held = Vec::new();
        let mut missing = Vec::new();
        for iid in iids.iter().filter_map(Value::as_int) {
            self.followed.insert(iid);
            match self.values.get(&iid) {
                Some(v) => held.push((Value::Int(iid), v.clone())),
                None => missing.push((Value::Int(iid), Value::Int(NO_VALUE))),
            }
        }
        let by_accessory: Vec<String> = self
            .accessories
            .iter()
            .filter_map(|a| {
                let names: Vec<&str> = a
                    .services
                    .iter()
                    .flat_map(|s| &s.characteristics)
                    .filter(|c| iids.iter().any(|i| i.as_int() == Some(c.iid)))
                    .map(|c| c.name)
                    .collect();
                (!names.is_empty()).then(|| format!("{}: {}", a.name, names.join(" ")))
            })
            .collect();
        println!(
            "[cpVehicle] phone follows {} values ({}), {} of them set",
            iids.len(),
            by_accessory.join(", "),
            held.len()
        );
        let mut answer = vec![
            (Value::str("command"), Value::str("registerResponse")),
            (Value::str("transactionID"), transaction),
            (Value::str("values"), Value::Dict(held)),
        ];
        if !missing.is_empty() {
            answer.push((Value::str("errors"), Value::Dict(missing)));
        }
        Value::Dict(answer)
    }

    fn read(&self, iids: &[Value], transaction: Value) -> Value {
        let mut held = Vec::new();
        let mut missing = Vec::new();
        let mut names = Vec::new();
        for iid in iids.iter().filter_map(Value::as_int) {
            names.extend(self.characteristic(iid).map(|c| c.name));
            match self.values.get(&iid) {
                Some(v) => held.push((Value::Int(iid), v.clone())),
                None => missing.push((Value::Int(iid), Value::Int(NO_VALUE))),
            }
        }
        println!("[cpVehicle] phone reads {}", names.join(" "));
        let mut answer = vec![
            (Value::str("command"), Value::str("readResponse")),
            (Value::str("transactionID"), transaction),
        ];
        if !held.is_empty() {
            answer.push((Value::str("values"), Value::Dict(held)));
        }
        if !missing.is_empty() {
            answer.push((Value::str("errors"), Value::Dict(missing)));
        }
        Value::Dict(answer)
    }

    fn write(
        &mut self,
        lane: Lane,
        written: &[(Value, Value)],
        transaction: Value,
    ) -> Vec<(Lane, Vec<u8>)> {
        let mut results = Vec::new();
        let mut changed = Vec::new();
        for (key, v) in written {
            let Some(iid) = key.as_int() else { continue };
            let (name, owner) = match self.characteristic(iid) {
                Some(c) if c.owner != Owner::Car => (c.name, c.owner),
                _ => {
                    println!("[cpVehicle] phone may not write value {iid}");
                    results.push((Value::Int(iid), Value::Int(NO_VALUE)));
                    continue;
                }
            };
            if self.debug || owner == Owner::Shared {
                println!("[cpVehicle] phone set {name} to {}", shown(v));
            } else {
                println!("[cpVehicle] phone wrote {name}");
            }
            results.push((Value::Int(iid), Value::Int(WRITTEN)));
            if *v == Value::Bool(true)
                && let Some(n) = (0..HISTORY_SLOTS).find(|n| history_slot(*n) + 7 == iid)
                && let Some(entry) = self.history.get_mut(n)
            {
                entry.hidden = true;
                self.hidden_by_phone.push(entry.id);
            }
            let before = self.values.insert(iid, v.clone());
            if before.as_ref() != Some(v) && self.followed.contains(&iid) {
                changed.push((Value::Int(iid), v.clone()));
            }
        }
        self.settle(written, &mut changed);
        let answer = dict([
            ("command", Value::str("writeResponse")),
            ("transactionID", transaction),
            ("errors", Value::Dict(results)),
        ]);
        let mut out = vec![(lane, wrap(&answer))];
        out.extend(update_notify(changed));
        out
    }

    pub fn volume(&self) -> Option<f64> {
        self.level(VOLUME)
    }

    pub fn volumes(&self) -> Vec<(&'static str, f64)> {
        VOLUMES
            .iter()
            .filter_map(|(name, iid)| Some((*name, self.level(*iid)?)))
            .collect()
    }

    fn level(&self, iid: i64) -> Option<f64> {
        Some(self.values.get(&iid)?.as_int()? as f64 / 100.0)
    }

    pub fn climate(&self) -> Option<Climate> {
        self.characteristic(AUTO_CLIMATE)?;
        let mut block: Climate = CLIMATE_KEYS
            .iter()
            .filter_map(|(key, iid)| Some(((*key).to_string(), to_json(self.values.get(iid)?)?)))
            .collect();
        let auto = VENTS
            .iter()
            .all(|vent| self.values.get(&(vent + 2)) == Some(&Value::Bool(true)));
        block.insert(VENTS_AUTO.into(), auto.into());
        Some(block)
    }

    pub fn set_climate(&mut self, climate: &Climate) -> Vec<(Lane, Vec<u8>)> {
        if self.characteristic(AUTO_CLIMATE).is_none() {
            return Vec::new();
        }
        let mut given = Vec::new();
        for (key, v) in climate {
            if key == VENTS_AUTO {
                given.extend(
                    v.as_bool()
                        .into_iter()
                        .flat_map(|auto| VENTS.map(|vent| (vent + 2, Value::Bool(auto)))),
                );
            } else if let Some((_, iid)) = CLIMATE_KEYS.iter().find(|(k, _)| k == key)
                && let Some(v) = self.characteristic(*iid).and_then(|c| c.take(v))
            {
                given.push((*iid, v));
            }
        }
        let new: Vec<_> = given
            .iter()
            .filter(|(iid, v)| self.values.get(iid) != Some(v))
            .collect();
        if new.is_empty() {
            return Vec::new();
        }
        let before = self.values.clone();
        for (iid, v) in &new {
            if let Some(c) = self.characteristic(*iid) {
                println!("[cpVehicle] LIVI set {} to {}", c.name, shown(v));
            }
            self.values.insert(*iid, v.clone());
        }
        let written: Vec<_> = new
            .iter()
            .map(|(iid, v)| (Value::Int(*iid), v.clone()))
            .collect();
        self.settle(&written, &mut Vec::new());
        for (iid, v) in given {
            self.values.insert(iid, v);
        }
        let changed = self
            .followed
            .iter()
            .filter_map(|iid| Some((*iid, self.values.get(iid)?)))
            .filter(|(iid, v)| before.get(iid) != Some(*v))
            .map(|(iid, v)| (Value::Int(iid), v.clone()))
            .collect();
        update_notify(changed).into_iter().collect()
    }

    pub fn set_gauges(&mut self, gauges: &Gauges) -> Vec<(Lane, Vec<u8>)> {
        let num = |key| {
            gauges
                .get(key)
                .and_then(serde_json::Value::as_f64)
                .filter(|n| n.is_finite())
        };
        let mut given: Vec<(i64, serde_json::Value)> = Vec::new();
        for (name, iid) in VOLUMES {
            if let Some(level) = num(name) {
                given.push((iid, (level.clamp(0.0, 1.0) * 100.0).into()));
            }
        }
        if let Some(pct) = num("fuelPct") {
            given.push((FUEL_LEVEL, pct.into()));
        }
        if let Some(rpm) = num("rpm") {
            given.push((ENGINE_RPM, rpm.into()));
        }
        let ranges: [(&str, &[Distance]); 2] = [
            ("rangeKm", &[FUEL_RANGE, REMAINING_RANGE]),
            ("odometerKm", &[ODOMETER]),
        ];
        for (key, distances) in ranges {
            let Some(km) = num(key).map(|km| km.max(0.0)) else {
                continue;
            };
            for d in distances {
                given.push((d.km, km.into()));
                given.push((d.km + 1, (km / KM_PER_MILE).into()));
                if let Some(hidden) = d.hidden {
                    given.push((hidden, false.into()));
                }
            }
        }
        if let Some(tires) = gauges
            .get("tirePressure")
            .and_then(serde_json::Value::as_object)
        {
            for (n, (name, _)) in WHEELS.iter().enumerate() {
                if let Some(kpa) = tires.get(*name).and_then(serde_json::Value::as_f64) {
                    let iid = tire_pressure_iid(n);
                    given.push((iid, kpa.max(0.0).into()));
                    given.push((iid + 3, SENSOR_ACTIVE.into()));
                }
            }
        }
        let efficiencies = [
            ("consumptionLPer100Km", FUEL_EFFICIENCY),
            ("consumptionAvgLPer100Km", AVERAGE_FUEL_EFFICIENCY),
        ];
        for (key, iid) in efficiencies {
            if let Some(l) = num(key) {
                given.push((iid, (l.max(0.0) * 1000.0).into()));
            }
        }
        let mut changed = Vec::new();
        for (iid, v) in given {
            if let Some(v) = self.characteristic(iid).and_then(|c| c.take(&v)) {
                self.car_sets(iid, v, &mut changed);
            }
        }
        if self
            .values
            .get(&VOLUME)
            .and_then(Value::as_int)
            .is_some_and(|v| v > 0)
        {
            self.car_sets(MUTE, Value::Bool(false), &mut changed);
        }
        update_notify(changed).into_iter().collect()
    }
}

#[cfg(test)]
mod tests;
