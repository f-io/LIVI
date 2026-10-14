//! Vehicle state over CarPlay. The car describes accessories made of services and their
//! characteristics, the phone reads that description, follows values and writes the ones it owns.
//! Every message is OPACK on one of two data streams, one of them for high priority values.

use std::collections::{BTreeMap, BTreeSet};

use crate::bplist;
use crate::opack::{self, Value, dict};

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
/// The answer for a value the car does not hold, or one the phone may not write.
const NO_VALUE: i64 = -6727;
const WRITTEN: i64 = 0;

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
    Int32 = 7,
    Float = 9,
    String = 10,
    Dict = 12,
    Array = 13,
}

/// The phone writes route data, the car states its own facts, and climate settings are asked for
/// by the phone and held by the car.
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
    let range = (format == Format::Uint32).then_some((0.0, f64::from(u32::MAX), 1.0));
    Characteristic { iid, kind, name, format, owner, mutable: true, range, units: None, initial }
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
        Self { range: Some((min, max, step)), ..self }
    }

    fn in_units(self, units: i64) -> Self {
        Self { units: Some(units), ..self }
    }

    fn fixed(self) -> Self {
        Self { mutable: false, ..self }
    }

    /// A value from outside the phone, held to the range and steps the phone was told.
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
            Format::Uint8 | Format::Uint32 | Format::Int32 => Some(Value::Int(n.round() as i64)),
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
        if matches!(self.format, Format::String | Format::Dict | Format::Array) {
            d.push(("maximumLength", Value::Int(MAX_LENGTH)));
        }
        if let Some(units) = self.units {
            d.push(("units", Value::Int(units)));
        }
        // The phone can clear what it wrote, except for its enumerations.
        if self.owner == Owner::Phone && self.format != Format::Uint8 {
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
    iid: i64,
    kind: i64,
    services: Vec<Service>,
}

/// Route sharing: the phone hands the car its route, the car says where navigation runs.
fn navigation() -> Accessory {
    use Format::*;
    Accessory {
        iid: 256,
        kind: 0x0E00_0002,
        services: vec![
            Service {
                iid: 257,
                kind: 0x1E00_0104,
                characteristics: vec![
                    phone(258, 0x4500_0103, "applicationEnabled", Bool),
                    phone(259, 0x4500_0112, "currentLegIndex", Uint32),
                    phone(260, 0x4500_0109, "legs", Array),
                    phone(261, 0x4500_010D, "sharingState", Uint8).up_to(4),
                    phone(262, 0x4500_0104, "userEnabled", Bool),
                    car(263, 0x4500_010E, "userVisibleReason", String, Value::str("LIVI")).fixed(),
                    car(264, 0x4500_0105, "vehicleEnabled", Bool, Value::Bool(true)),
                ],
            },
            Service {
                iid: 265,
                kind: 0x1E00_0102,
                characteristics: vec![
                    phone(266, 0x4500_0108, "destination", Dict),
                    phone(267, 0x4500_0106, "geodeticSystem", Uint8).up_to(2),
                    phone(268, 0x4500_0107, "origin", Dict),
                    phone(269, 0x4500_0110, "rerouteReason", Uint8).up_to(6),
                    phone(270, 0x4500_0101, "routeState", Uint8).up_to(255),
                    phone(271, 0x4500_0102, "userVisibleApplicationName", String),
                ],
            },
            Service {
                iid: 272,
                kind: 0x1E00_0103,
                characteristics: vec![
                    car(273, 0x4500_010C, "routeSource", Uint8, Value::Int(0)).up_to(5),
                    car(
                        274,
                        0x4500_010A,
                        "userVisibleNavigationSystemName",
                        String,
                        Value::str("LIVI"),
                    )
                    .fixed(),
                    car(275, 0x4500_0111, "userVisibleVehicleBrand", String, Value::str("LIVI"))
                        .fixed(),
                ],
            },
        ],
    }
}

/// Names the interior picture in the car maker's theme, without one the climate page shows a placeholder.
fn vehicle_resources() -> Accessory {
    use Format::*;
    Accessory {
        iid: 768,
        kind: 0x0120_0002,
        services: vec![Service {
            iid: 769,
            kind: 0x2000_0001,
            characteristics: vec![
                car(770, 0x4800_0004, "interiorStyle", String, Value::str("5Seater")).fixed(),
            ],
        }],
    }
}

/// Climate: the phone and Siri ask for settings, the car holds them and reports back.
fn climate() -> Accessory {
    use Format::*;
    let on = |iid, name, initial| shared(iid, 0x3000_0002, name, Bool, Value::Bool(initial));
    let seat =
        |iid, key| car(iid, 0x3600_0065, "vehicleLayoutKey", String, Value::str(key)).fixed();
    let zone = |iid, names: [&'static str; 3], key| Service {
        iid,
        kind: 0x1100_0002,
        characteristics: vec![
            on(iid + 1, names[0], true),
            shared(iid + 2, 0x3100_0017, names[1], Float, Value::Real(21.0))
                .between(TEMPERATURE_LO, TEMPERATURE_HI, 0.5)
                .in_units(CELSIUS),
            car(iid + 3, 0x3100_0019, names[2], Float, Value::Real(21.0))
                .between(-40.0, 85.0, 0.5)
                .in_units(CELSIUS),
            seat(iid + 4, key),
        ],
    };
    Accessory {
        iid: 512,
        kind: 0x0100_0001,
        services: vec![
            Service {
                iid: 513,
                kind: 0x1100_0001,
                characteristics: vec![
                    shared(HVAC_ON, 0x3100_0002, "hvacOn", Bool, Value::Bool(true)),
                    shared(COMPRESSOR, 0x3100_0003, "compressorOn", Bool, Value::Bool(true)),
                    shared(MAX_AC, 0x3100_0022, "maxACOn", Bool, Value::Bool(false)),
                    shared(MAX_DEFROST, 0x3100_0029, "maxDefrostOn", Bool, Value::Bool(false)),
                ],
            },
            zone(
                518,
                ["driverTemperatureOn", "driverTargetTemperature", "driverCurrentTemperature"],
                "seat_front_left",
            ),
            zone(
                532,
                [
                    "passengerTemperatureOn",
                    "passengerTargetTemperature",
                    "passengerCurrentTemperature",
                ],
                "seat_front_right",
            ),
            Service {
                iid: 523,
                kind: 0x1100_0007,
                characteristics: vec![
                    on(524, "fanOn", true),
                    shared(525, 0x3100_0012, "fanLevel", Uint8, Value::Int(3)).up_to(7),
                    shared(FAN_AUTO, 0x3000_005F, "fanAutoMode", Bool, Value::Bool(true)),
                    seat(531, "seat_front"),
                ],
            },
            Service {
                iid: 527,
                kind: 0x1100_0011,
                characteristics: vec![on(RECIRCULATION, "recirculationOn", false)],
            },
            Service {
                iid: 529,
                kind: 0x1100_0008,
                characteristics: vec![on(AUTO_CLIMATE, "autoClimateOn", true)],
            },
            Service {
                iid: 537,
                kind: 0x1100_0012,
                characteristics: vec![
                    on(ZONES_SYNCED, "zonesSynced", false),
                    seat(539, "seat_front"),
                ],
            },
            window_heating(WINDOWS[0], "frontWindowHeatingLevel", "windshield_front"),
            window_heating(WINDOWS[1], "rearWindowHeatingLevel", "windshield_rear"),
            vent(VENTS[0], ["upperVentOn", "upperVentAutoMode"], UPPER, false),
            vent(VENTS[1], ["middleVentOn", "middleVentAutoMode"], MIDDLE, true),
            vent(VENTS[2], ["lowerVentOn", "lowerVentAutoMode"], LOWER, false),
        ],
    }
}

/// One vent with its own button, the climate app shows each as one.
fn vent(iid: i64, names: [&'static str; 2], direction: i64, on: bool) -> Service {
    use Format::*;
    Service {
        iid,
        kind: 0x1100_0006,
        characteristics: vec![
            shared(iid + 1, 0x3000_0002, names[0], Bool, Value::Bool(on)),
            shared(iid + 2, 0x3000_005F, names[1], Bool, Value::Bool(true)),
            car(
                iid + 3,
                0x3100_0025,
                "ventCombinations",
                Array,
                Value::Array(vec![Value::Int(direction)]),
            )
            .fixed(),
            car(iid + 4, 0x3600_0065, "vehicleLayoutKey", String, Value::str("seat_front")).fixed(),
        ],
    }
}

const UPPER: i64 = 2;
const LOWER: i64 = 4;
const MIDDLE: i64 = 16;
/// Top, middle and bottom vent, on and automatic of each at the next two iids.
const VENTS: [i64; 3] = [548, 553, 558];
const AUTO_CLIMATE: i64 = 530;
const FAN_AUTO: i64 = 526;
const HVAC_ON: i64 = 514;
const COMPRESSOR: i64 = 515;
const MAX_AC: i64 = 516;
const MAX_DEFROST: i64 = 517;
const RECIRCULATION: i64 = 528;
/// Front and rear window, the heating level of each at the next iid.
const WINDOWS: [i64; 2] = [540, 544];
/// Shown as LO and HI at the ends of the scale.
const TEMPERATURE_LO: f64 = 16.0;
const TEMPERATURE_HI: f64 = 28.0;

/// A heated window, wires in the glass, on or off.
fn window_heating(iid: i64, name: &'static str, key: &'static str) -> Service {
    use Format::*;
    const FILAMENT: i64 = 2;
    Service {
        iid,
        kind: 0x1100_0005,
        characteristics: vec![
            shared(iid + 1, 0x3000_0004, name, Uint8, Value::Int(0)).up_to(1),
            car(iid + 2, 0x3100_0014, "defrostTypes", Uint8, Value::Int(FILAMENT)).fixed(),
            car(iid + 3, 0x3600_0065, "vehicleLayoutKey", String, Value::str(key)).fixed(),
        ],
    }
}

/// The vehicle app's menu, so far a single line with LIVI's version.
fn automaker_settings(app_version: &str) -> Accessory {
    use Format::*;
    let version = if app_version.is_empty() { "?" } else { app_version };
    Accessory {
        iid: 1280,
        kind: 0x0500_0001,
        services: vec![Service {
            iid: 1281,
            kind: 0x1600_0015,
            characteristics: vec![
                car(1282, 0x3000_0019, "settingIdentifier", String, Value::str("LiviVersion"))
                    .fixed(),
                car(1283, 0x3000_0001, "settingLabel", String, Value::str("LIVI")).fixed(),
                car(1284, 0x3000_0017, "settingValue", String, Value::str(version)).fixed(),
                car(1285, 0x3600_0006, "settingsCategory", Uint8, Value::Int(0)).fixed(),
                car(1286, 0x3000_005E, "settingSymbol", String, Value::str("info.circle")).fixed(),
                car(1287, 0x3000_0003, "settingSortOrder", Uint8, Value::Int(0)).fixed(),
            ],
        }],
    }
}

/// Heating only, the description cannot carry the negative levels of cooling.
fn seats() -> Accessory {
    use Format::*;
    let heating = |iid, names: [&'static str; 2], key| Service {
        iid,
        kind: 0x2200_0001,
        characteristics: vec![
            shared(iid + 1, 0x3100_0021, names[0], Int32, Value::Int(0)).up_to(2),
            shared(iid + 2, 0x3000_0002, names[1], Bool, Value::Bool(false)),
            car(iid + 3, 0x3600_0065, "vehicleLayoutKey", String, Value::str(key)).fixed(),
        ],
    };
    Accessory {
        iid: 1024,
        kind: 0x0150_0001,
        services: vec![
            heating(1025, ["driverSeatHeatingLevel", "driverSeatHeatingOn"], "seat_front_left"),
            heating(
                1029,
                ["passengerSeatHeatingLevel", "passengerSeatHeatingOn"],
                "seat_front_right",
            ),
        ],
    }
}

const DRIVER_TARGET_TEMPERATURE: i64 = 520;
const PASSENGER_TARGET_TEMPERATURE: i64 = 534;
const ZONES_SYNCED: i64 = 538;
/// Level and on of each front seat's heating.
const SEAT_HEATING: [(i64, i64); 2] = [(1026, 1027), (1030, 1031)];
const FAN: (i64, i64) = (525, 524);
/// Level and on of what runs at its level and stops at 0.
const LEVELLED: [(i64, i64); 3] = [SEAT_HEATING[0], SEAT_HEATING[1], FAN];

/// The climate block in LIVI's telemetry, metric, each key with the value it stands for.
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
/// One switch for the automatic of all three vents.
const VENTS_AUTO: &str = "ventsAuto";

/// The climate LIVI starts with, AUTO at 21 °C.
pub fn default_climate() -> Climate {
    VehicleState::new(false, true, "").climate().unwrap_or_default()
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
    /// The level each switched off one comes back on with.
    resume: BTreeMap<i64, Value>,
    /// What Max AC took over, given back when it ends.
    before_max_ac: Option<Vec<(i64, Value)>>,
    debug: bool,
}

impl VehicleState {
    pub fn new(debug: bool, climate_controls: bool, app_version: &str) -> Self {
        let mut accessories = vec![navigation(), automaker_settings(app_version)];
        if climate_controls {
            accessories.extend([climate(), seats(), vehicle_resources()]);
        }
        let values = accessories
            .iter()
            .flat_map(|a| &a.services)
            .flat_map(|s| &s.characteristics)
            .filter_map(|c| Some((c.iid, c.initial.clone()?)))
            .collect();
        Self {
            accessories,
            values,
            followed: BTreeSet::new(),
            resume: BTreeMap::new(),
            before_max_ac: None,
            debug,
        }
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
                        dict([
                            ("iid", Value::Int(s.iid)),
                            ("type", Value::Int(s.kind)),
                            ("characteristics", Value::Array(characteristics.collect())),
                        ])
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

    /// The /info entry that tells the phone the car speaks the protocol and what it describes.
    pub fn info_entry(&self) -> (String, bplist::Value) {
        let plugin = dict([
            ("name", Value::str(PLUGIN)),
            ("pluginID", Value::Int(PLUGIN_ID)),
            ("accessories", self.accessory_list()),
        ]);
        let info = dict([
            ("protocolVersion", Value::str(PROTOCOL_VERSION)),
            ("pluginCount", Value::Int(1)),
            ("pluginMapping", Value::Dict(vec![(Value::str(PLUGIN), Value::Int(PLUGIN_ID))])),
            ("pluginConfigs", Value::Array(vec![plugin])),
        ]);
        ("vehicleStateProtocolInfo".into(), to_plist(&info))
    }

    /// One message from the phone, and what goes back on which stream.
    pub fn handle(&mut self, lane: Lane, message: &[u8]) -> Vec<(Lane, Vec<u8>)> {
        let Some(outer) = opack::decode(message) else {
            println!("[cpVehicle] a {} B message is not OPACK", message.len());
            return Vec::new();
        };
        let plugin = outer.get("pluginID").and_then(Value::as_int);
        let request = outer.get("pluginData").and_then(Value::as_data).and_then(opack::decode);
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
                vec![(lane, wrap(&self.follow(iids, transaction)))]
            }
            "writeRequest" => {
                let written = values.and_then(Value::as_dict).unwrap_or(&[]);
                self.write(lane, written, transaction)
            }
            other => {
                println!("[cpVehicle] phone sent {other:?}, not handled");
                Vec::new()
            }
        }
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
        println!("[cpVehicle] phone follows {} values, {} of them set", iids.len(), held.len());
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
            // Route data stays out of the normal log, the settings it asks the car for do not.
            if self.debug || owner == Owner::Shared {
                println!("[cpVehicle] phone set {name} to {}", shown(v));
            } else {
                println!("[cpVehicle] phone wrote {name}");
            }
            results.push((Value::Int(iid), Value::Int(WRITTEN)));
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

    /// The climate as LIVI's telemetry holds it, None without the climate controls.
    pub fn climate(&self) -> Option<Climate> {
        self.characteristic(AUTO_CLIMATE)?;
        let mut block: Climate = CLIMATE_KEYS
            .iter()
            .filter_map(|(key, iid)| Some(((*key).to_string(), to_json(self.values.get(iid)?)?)))
            .collect();
        let auto =
            VENTS.iter().all(|vent| self.values.get(&(vent + 2)) == Some(&Value::Bool(true)));
        block.insert(VENTS_AUTO.into(), auto.into());
        Some(block)
    }

    /// Climate set from outside the phone, by the car or in LIVI. What was named stands, the car
    /// settles the rest, and the phone hears whatever it follows that changed.
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
        let new: Vec<_> = given.iter().filter(|(iid, v)| self.values.get(iid) != Some(v)).collect();
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
        let written: Vec<_> = new.iter().map(|(iid, v)| (Value::Int(*iid), v.clone())).collect();
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

    /// What a car does on its own after a setting changed.
    fn settle(&mut self, written: &[(Value, Value)], changed: &mut Vec<(Value, Value)>) {
        self.keep_zones_synced(written, changed);
        self.switch_by_level(written, changed);
        self.follow_auto(written, changed);
        self.run_max_ac(written, changed);
    }

    /// Synced, the passenger side takes the driver's temperature, and setting the passenger side
    /// on its own ends the sync, as a car does.
    fn keep_zones_synced(&mut self, written: &[(Value, Value)], changed: &mut Vec<(Value, Value)>) {
        if self.values.get(&ZONES_SYNCED) != Some(&Value::Bool(true)) {
            return;
        }
        let wrote = |iid| written.iter().any(|(k, _)| k.as_int() == Some(iid));
        if wrote(PASSENGER_TARGET_TEMPERATURE) && !wrote(ZONES_SYNCED) {
            self.car_sets(ZONES_SYNCED, Value::Bool(false), changed);
        } else if let Some(driver) = self.values.get(&DRIVER_TARGET_TEMPERATURE).cloned() {
            self.car_sets(PASSENGER_TARGET_TEMPERATURE, driver, changed);
        }
    }

    /// Level 0 is off: a level above 0 switches on, switching off drops to 0 and switching on
    /// brings back the last level.
    fn switch_by_level(&mut self, written: &[(Value, Value)], changed: &mut Vec<(Value, Value)>) {
        let find = |iid| written.iter().find(|(k, _)| k.as_int() == Some(iid)).map(|(_, v)| v);
        for (level_iid, on_iid) in LEVELLED {
            if let Some(Value::Int(level)) = find(level_iid) {
                self.car_sets(on_iid, Value::Bool(*level > 0), changed);
            } else if let Some(Value::Bool(on)) = find(on_iid) {
                let level = self.values.get(&level_iid).cloned();
                if !on && level != Some(Value::Int(0)) {
                    if let Some(level) = level {
                        self.resume.insert(level_iid, level);
                    }
                    self.car_sets(level_iid, Value::Int(0), changed);
                } else if *on && level == Some(Value::Int(0)) {
                    let back = self.resume.get(&level_iid).cloned().unwrap_or(Value::Int(1));
                    self.car_sets(level_iid, back, changed);
                }
            }
        }
    }

    /// AUTO runs the fan and picks the vents. Setting either by hand takes it out of
    /// automatic and switches AUTO off.
    fn follow_auto(&mut self, written: &[(Value, Value)], changed: &mut Vec<(Value, Value)>) {
        let find = |iid| written.iter().find(|(k, _)| k.as_int() == Some(iid)).map(|(_, v)| v);
        let vents_auto = VENTS.map(|vent| vent + 2);
        if let Some(Value::Bool(auto)) = find(AUTO_CLIMATE) {
            for iid in std::iter::once(FAN_AUTO).chain(vents_auto) {
                self.car_sets(iid, Value::Bool(*auto), changed);
            }
            return;
        }
        let (fan_level, fan_on) = FAN;
        let fan_by_hand = find(fan_level).or(find(fan_on)).is_some();
        let vents_by_hand = VENTS.iter().any(|vent| find(vent + 1).is_some());
        if fan_by_hand {
            self.car_sets(FAN_AUTO, Value::Bool(false), changed);
        }
        if vents_by_hand {
            for iid in vents_auto {
                self.car_sets(iid, Value::Bool(false), changed);
            }
        }
        if fan_by_hand || vents_by_hand {
            self.car_sets(AUTO_CLIMATE, Value::Bool(false), changed);
        }
    }

    /// Max AC cools with everything: both zones at LO, compressor and recirculation on, the fan
    /// at its top level, AUTO off. Ending it gives the settings back, setting one of them by hand
    /// ends it and keeps what was set.
    fn run_max_ac(&mut self, written: &[(Value, Value)], changed: &mut Vec<(Value, Value)>) {
        let (fan_level, fan_on) = FAN;
        let lo = Value::Real(TEMPERATURE_LO);
        let takes = [
            (DRIVER_TARGET_TEMPERATURE, lo.clone()),
            (PASSENGER_TARGET_TEMPERATURE, lo),
            (COMPRESSOR, Value::Bool(true)),
            (RECIRCULATION, Value::Bool(true)),
            (fan_level, Value::Int(7)),
            (fan_on, Value::Bool(true)),
            (FAN_AUTO, Value::Bool(false)),
            (AUTO_CLIMATE, Value::Bool(false)),
        ];
        let find = |iid| written.iter().find(|(k, _)| k.as_int() == Some(iid)).map(|(_, v)| v);
        match find(MAX_AC) {
            Some(Value::Bool(true)) if self.before_max_ac.is_none() => {
                let before =
                    takes.iter().filter_map(|(iid, _)| Some((*iid, self.values.get(iid)?.clone())));
                self.before_max_ac = Some(before.collect());
                for (iid, v) in takes {
                    self.car_sets(iid, v, changed);
                }
            }
            Some(Value::Bool(false)) => {
                for (iid, v) in self.before_max_ac.take().unwrap_or_default() {
                    self.car_sets(iid, v, changed);
                }
            }
            _ if self.before_max_ac.is_some()
                && takes.iter().any(|(iid, _)| find(*iid).is_some()) =>
            {
                self.before_max_ac = None;
                self.car_sets(MAX_AC, Value::Bool(false), changed);
            }
            _ => {}
        }
    }

    fn car_sets(&mut self, iid: i64, v: Value, changed: &mut Vec<(Value, Value)>) {
        if self.values.get(&iid) == Some(&v) {
            return;
        }
        if let Some(c) = self.characteristic(iid) {
            println!("[cpVehicle] car set {} to {}", c.name, shown(&v));
        }
        self.values.insert(iid, v.clone());
        if self.followed.contains(&iid) {
            changed.push((Value::Int(iid), v));
        }
    }
}

fn shown(v: &Value) -> std::string::String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Real(f) => f.to_string(),
        Value::String(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

fn update_notify(changed: Vec<(Value, Value)>) -> Option<(Lane, Vec<u8>)> {
    (!changed.is_empty()).then(|| {
        let update =
            dict([("command", Value::str("updateNotify")), ("values", Value::Dict(changed))]);
        (Lane::High, wrap(&update))
    })
}

fn wrap(inner: &Value) -> Vec<u8> {
    opack::encode(&dict([
        ("pluginID", Value::Int(PLUGIN_ID)),
        ("pluginData", Value::Data(opack::encode(inner))),
    ]))
}

/// The description holds no negative numbers and no empty values, so it converts one to one.
fn to_plist(v: &Value) -> bplist::Value {
    match v {
        Value::Null => bplist::Value::Data(Vec::new()),
        Value::Bool(b) => bplist::Value::Bool(*b),
        Value::Int(n) => bplist::Value::Int(u64::try_from(*n).unwrap_or(0)),
        Value::Real(f) => bplist::Value::Real(*f),
        Value::String(s) => bplist::Value::String(s.clone()),
        Value::Data(d) => bplist::Value::Data(d.clone()),
        Value::Uuid(u) => bplist::Value::Data(u.to_vec()),
        Value::Array(items) => bplist::Value::Array(items.iter().map(to_plist).collect()),
        Value::Dict(entries) => bplist::Value::Dict(
            entries
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        Value::String(s) => s.clone(),
                        Value::Int(n) => n.to_string(),
                        other => format!("{other:?}"),
                    };
                    (key, to_plist(v))
                })
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        let s: std::string::String = s.split_whitespace().collect();
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    fn opened(answer: &[u8]) -> Value {
        let outer = opack::decode(answer).unwrap();
        assert_eq!(outer.get("pluginID").and_then(Value::as_int), Some(PLUGIN_ID));
        opack::decode(outer.get("pluginData").and_then(Value::as_data).unwrap()).unwrap()
    }

    fn by_iid(entries: &[(i64, Value)]) -> Value {
        Value::Dict(entries.iter().map(|(k, v)| (Value::Int(*k), v.clone())).collect())
    }

    fn command(name: &str, values: Value) -> Vec<u8> {
        wrap(&dict([
            ("command", Value::str(name)),
            ("transactionID", Value::Int(8)),
            ("values", values),
        ]))
    }

    #[test]
    fn info_describes_route_sharing_for_one_plugin() {
        let (key, info) = VehicleState::new(false, true, "9.5.0").info_entry();
        assert_eq!(key, "vehicleStateProtocolInfo");
        assert_eq!(info.get("protocolVersion").and_then(bplist::Value::as_str), Some("1.0"));
        let mapping = info.get("pluginMapping").and_then(|m| m.get(PLUGIN));
        assert_eq!(mapping.and_then(bplist::Value::as_int), Some(0));
        let plugin = &info.get("pluginConfigs").and_then(bplist::Value::as_array).unwrap()[0];
        let accessory = &plugin.get("accessories").and_then(bplist::Value::as_array).unwrap()[0];
        assert_eq!(accessory.get("type").and_then(bplist::Value::as_int), Some(0x0E00_0002));
        let services = accessory.get("services").and_then(bplist::Value::as_array).unwrap();
        let characteristics: Vec<&bplist::Value> = services
            .iter()
            .flat_map(|s| s.get("characteristics").and_then(bplist::Value::as_array).unwrap())
            .collect();
        assert_eq!(characteristics.len(), 16);
        let find = |iid: u64| {
            *characteristics
                .iter()
                .find(|c| c.get("iid").and_then(bplist::Value::as_int) == Some(iid))
                .unwrap()
        };
        let flag = |c: &bplist::Value, k: &str| c.get(k).and_then(bplist::Value::as_bool);
        let number = |c: &bplist::Value, k: &str| c.get(k).and_then(bplist::Value::as_int);

        let destination = find(266);
        assert_eq!(flag(destination, "writable"), Some(true));
        assert_eq!(flag(destination, "oemWritable"), Some(false));
        assert_eq!(flag(destination, "supportsInvalid"), Some(true));
        assert_eq!(number(destination, "maximumLength"), Some(65535));
        assert!(destination.get("initialValue").is_none());

        let sharing = find(261);
        assert_eq!(number(sharing, "maximumValue"), Some(4));
        assert_eq!(number(sharing, "stepValue"), Some(1));
        assert!(sharing.get("supportsInvalid").is_none());
        assert_eq!(number(find(259), "maximumValue"), Some(u64::from(u32::MAX)));

        let brand = find(275);
        assert_eq!(flag(brand, "writable"), Some(false));
        assert_eq!(flag(brand, "oemWritable"), Some(true));
        assert_eq!(flag(brand, "mutable"), Some(false));
        assert_eq!(brand.get("initialValue").and_then(bplist::Value::as_str), Some("LIVI"));
    }

    #[test]
    fn the_phone_reads_follows_and_writes() {
        let mut state = VehicleState::new(false, true, "9.5.0");
        // A configRequest as an iPhone sends it.
        let ask =
            hex("e248706c7567696e4944084a706c7567696e44617461912ee247636f6d6d616e644d636f6e666967
             526571756573744d7472616e73616374696f6e494433b960c362b52ed4f2");
        let answers = state.handle(Lane::High, &ask);
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].0, Lane::High);
        let description = opened(&answers[0].1);
        assert_eq!(description.get("command").and_then(Value::as_str), Some("configResponse"));
        assert_eq!(
            description.get("transactionID").and_then(Value::as_int),
            Some(-949_082_264_887_402_311)
        );
        let accessories = description.get("values").and_then(|v| v.get("accessories"));
        assert_eq!(accessories, Some(&state.accessory_list()));

        let iids = [262, 264, 273, 999].map(Value::Int).to_vec();
        let answers = state.handle(Lane::Normal, &command("registerRequest", Value::Array(iids)));
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].0, Lane::Normal);
        let followed = opened(&answers[0].1);
        assert_eq!(followed.get("command").and_then(Value::as_str), Some("registerResponse"));
        let held = by_iid(&[(264, Value::Bool(true)), (273, Value::Int(0))]);
        assert_eq!(followed.get("values"), Some(&held));
        let missing = by_iid(&[(262, Value::Int(NO_VALUE)), (999, Value::Int(NO_VALUE))]);
        assert_eq!(followed.get("errors"), Some(&missing));

        let written = by_iid(&[(262, Value::Bool(true)), (264, Value::Bool(false))]);
        let answers = state.handle(Lane::Normal, &command("writeRequest", written));
        assert_eq!(answers.len(), 2);
        assert_eq!(answers[0].0, Lane::Normal);
        let response = opened(&answers[0].1);
        assert_eq!(response.get("command").and_then(Value::as_str), Some("writeResponse"));
        let results = by_iid(&[(262, Value::Int(WRITTEN)), (264, Value::Int(NO_VALUE))]);
        assert_eq!(response.get("errors"), Some(&results));
        assert_eq!(answers[1].0, Lane::High, "updates ride the high priority stream");
        let update = opened(&answers[1].1);
        assert_eq!(update.get("command").and_then(Value::as_str), Some("updateNotify"));
        assert_eq!(update.get("values"), Some(&by_iid(&[(262, Value::Bool(true))])));
        assert!(update.get("transactionID").is_none());
        assert_eq!(state.values.get(&264), Some(&Value::Bool(true)), "the car keeps its own value");

        let same = by_iid(&[(262, Value::Bool(true))]);
        assert_eq!(state.handle(Lane::High, &command("writeRequest", same)).len(), 1);
        let unfollowed = by_iid(&[(266, Value::Dict(Vec::new()))]);
        assert_eq!(state.handle(Lane::High, &command("writeRequest", unfollowed)).len(), 1);
        assert_eq!(state.values.get(&266), Some(&Value::Dict(Vec::new())));
    }

    #[test]
    fn without_climate_controls_the_car_names_neither_climate_nor_its_interior() {
        let (_, info) = VehicleState::new(false, false, "9.5.0").info_entry();
        let plugin = &info.get("pluginConfigs").and_then(bplist::Value::as_array).unwrap()[0];
        let kinds: Vec<u64> = plugin
            .get("accessories")
            .and_then(bplist::Value::as_array)
            .unwrap()
            .iter()
            .filter_map(|a| a.get("type").and_then(bplist::Value::as_int))
            .collect();
        assert_eq!(kinds, [0x0E00_0002, 0x0500_0001]);
    }

    #[test]
    fn the_vehicle_app_shows_the_livi_version() {
        let (_, info) = VehicleState::new(false, false, "9.5.0").info_entry();
        let plugin = &info.get("pluginConfigs").and_then(bplist::Value::as_array).unwrap()[0];
        let settings = plugin
            .get("accessories")
            .and_then(bplist::Value::as_array)
            .unwrap()
            .iter()
            .find(|a| a.get("type").and_then(bplist::Value::as_int) == Some(0x0500_0001))
            .unwrap();
        let line = &settings.get("services").and_then(bplist::Value::as_array).unwrap()[0];
        assert_eq!(line.get("type").and_then(bplist::Value::as_int), Some(0x1600_0015));
        let initial = |kind| {
            line.get("characteristics")
                .and_then(bplist::Value::as_array)
                .unwrap()
                .iter()
                .find(|c| c.get("type").and_then(bplist::Value::as_int) == Some(kind))
                .and_then(|c| c.get("initialValue"))
                .cloned()
        };
        let text = |s: &str| Some(bplist::Value::String(s.into()));
        assert_eq!(initial(0x3000_0001), text("LIVI"));
        assert_eq!(initial(0x3000_0017), text("9.5.0"));
        assert_eq!(initial(0x3600_0006), Some(bplist::Value::Int(0)));

        let (_, info) = VehicleState::new(false, false, "").info_entry();
        assert!(format!("{info:?}").contains("String(\"?\")"), "an unknown version shows as ?");
    }

    #[test]
    fn both_front_seats_heat_in_two_steps() {
        let (_, info) = VehicleState::new(false, true, "9.5.0").info_entry();
        let plugin = &info.get("pluginConfigs").and_then(bplist::Value::as_array).unwrap()[0];
        let seats = plugin
            .get("accessories")
            .and_then(bplist::Value::as_array)
            .unwrap()
            .iter()
            .find(|a| a.get("type").and_then(bplist::Value::as_int) == Some(0x0150_0001))
            .unwrap();
        let services = seats.get("services").and_then(bplist::Value::as_array).unwrap();
        assert_eq!(services.len(), 2);
        for service in services {
            assert_eq!(service.get("type").and_then(bplist::Value::as_int), Some(0x2200_0001));
            let level = service
                .get("characteristics")
                .and_then(bplist::Value::as_array)
                .unwrap()
                .iter()
                .find(|c| c.get("type").and_then(bplist::Value::as_int) == Some(0x3100_0021))
                .unwrap();
            assert_eq!(level.get("format").and_then(bplist::Value::as_int), Some(7));
            assert_eq!(level.get("minimumValue").and_then(bplist::Value::as_int), Some(0));
            assert_eq!(level.get("maximumValue").and_then(bplist::Value::as_int), Some(2));
            assert_eq!(level.get("writable").and_then(bplist::Value::as_bool), Some(true));
        }
    }

    #[test]
    fn a_seat_heats_while_its_level_is_above_zero() {
        let mut state = VehicleState::new(false, true, "9.5.0");
        let [(level, on), _] = SEAT_HEATING;
        state.handle(
            Lane::High,
            &command("registerRequest", Value::Array(vec![Value::Int(level), Value::Int(on)])),
        );
        let update = |state: &mut VehicleState, to: i64| {
            let answers = state
                .handle(Lane::High, &command("writeRequest", by_iid(&[(level, Value::Int(to))])));
            opened(&answers[1].1).get("values").cloned()
        };
        assert_eq!(
            update(&mut state, 2),
            Some(by_iid(&[(level, Value::Int(2)), (on, Value::Bool(true))]))
        );
        assert_eq!(update(&mut state, 1), Some(by_iid(&[(level, Value::Int(1))])));
        assert_eq!(
            update(&mut state, 0),
            Some(by_iid(&[(level, Value::Int(0)), (on, Value::Bool(false))]))
        );
    }

    #[test]
    fn front_and_rear_windows_heat_through_their_wires() {
        let (_, info) = VehicleState::new(false, true, "9.5.0").info_entry();
        let plugin = &info.get("pluginConfigs").and_then(bplist::Value::as_array).unwrap()[0];
        let climate = plugin
            .get("accessories")
            .and_then(bplist::Value::as_array)
            .unwrap()
            .iter()
            .find(|a| a.get("type").and_then(bplist::Value::as_int) == Some(0x0100_0001))
            .unwrap();
        let windows: Vec<(String, Option<u64>)> = climate
            .get("services")
            .and_then(bplist::Value::as_array)
            .unwrap()
            .iter()
            .filter(|s| s.get("type").and_then(bplist::Value::as_int) == Some(0x1100_0005))
            .map(|s| {
                let chars = s.get("characteristics").and_then(bplist::Value::as_array).unwrap();
                let of = |kind| {
                    chars
                        .iter()
                        .find(|c| c.get("type").and_then(bplist::Value::as_int) == Some(kind))
                        .and_then(|c| c.get("initialValue"))
                };
                let key = of(0x3600_0065).and_then(bplist::Value::as_str).unwrap().to_string();
                (key, of(0x3100_0014).and_then(bplist::Value::as_int))
            })
            .collect();
        assert_eq!(
            windows,
            [("windshield_front".to_string(), Some(2)), ("windshield_rear".to_string(), Some(2))]
        );
    }

    #[test]
    fn each_vent_has_its_button_and_auto_takes_them_all() {
        let mut state = VehicleState::new(false, true, "9.5.0");
        let directions: Vec<_> =
            VENTS.iter().map(|vent| state.values.get(&(vent + 3)).cloned()).collect();
        let one = |d| Some(Value::Array(vec![Value::Int(d)]));
        assert_eq!(directions, [one(UPPER), one(MIDDLE), one(LOWER)]);
        let [_, middle, lower] = VENTS;
        let automatic =
            VENTS.iter().all(|v| state.values.get(&(v + 2)) == Some(&Value::Bool(true)));
        assert!(automatic, "the vents start in automatic, as AUTO does");
        let iids = [middle + 1, middle + 2, lower + 1, lower + 2].map(Value::Int).to_vec();
        state.handle(Lane::High, &command("registerRequest", Value::Array(iids)));
        let update = |state: &mut VehicleState, written: (i64, Value)| {
            let answers = state.handle(Lane::High, &command("writeRequest", by_iid(&[written])));
            opened(&answers[1].1).get("values").cloned()
        };
        assert_eq!(
            update(&mut state, (lower + 1, Value::Bool(true))),
            Some(by_iid(&[
                (lower + 1, Value::Bool(true)),
                (middle + 2, Value::Bool(false)),
                (lower + 2, Value::Bool(false))
            ])),
            "a vent set by hand takes all of them out of automatic, the middle one stays on"
        );
        assert_eq!(
            update(&mut state, (AUTO_CLIMATE, Value::Bool(true))),
            Some(by_iid(&[(middle + 2, Value::Bool(true)), (lower + 2, Value::Bool(true))]))
        );
        assert_eq!(state.values.get(&(middle + 1)), Some(&Value::Bool(true)));
    }

    #[test]
    fn max_ac_cools_with_everything_and_gives_the_settings_back() {
        let mut state = VehicleState::new(false, true, "9.5.0");
        let (level, _) = FAN;
        let iids =
            [MAX_AC, DRIVER_TARGET_TEMPERATURE, PASSENGER_TARGET_TEMPERATURE, RECIRCULATION, level];
        state.handle(
            Lane::High,
            &command("registerRequest", Value::Array(iids.map(Value::Int).to_vec())),
        );
        let update = |state: &mut VehicleState, written: (i64, Value)| {
            let answers = state.handle(Lane::High, &command("writeRequest", by_iid(&[written])));
            opened(&answers[1].1).get("values").cloned()
        };
        let lo = Value::Real(TEMPERATURE_LO);
        update(&mut state, (DRIVER_TARGET_TEMPERATURE, Value::Real(23.0)));

        assert_eq!(
            update(&mut state, (MAX_AC, Value::Bool(true))),
            Some(by_iid(&[
                (MAX_AC, Value::Bool(true)),
                (DRIVER_TARGET_TEMPERATURE, lo.clone()),
                (PASSENGER_TARGET_TEMPERATURE, lo.clone()),
                (RECIRCULATION, Value::Bool(true)),
                (level, Value::Int(7)),
            ]))
        );
        assert_eq!(
            update(&mut state, (MAX_AC, Value::Bool(false))),
            Some(by_iid(&[
                (MAX_AC, Value::Bool(false)),
                (DRIVER_TARGET_TEMPERATURE, Value::Real(23.0)),
                (PASSENGER_TARGET_TEMPERATURE, Value::Real(21.0)),
                (RECIRCULATION, Value::Bool(false)),
                (level, Value::Int(3)),
            ]))
        );

        update(&mut state, (MAX_AC, Value::Bool(true)));
        assert_eq!(
            update(&mut state, (level, Value::Int(4))),
            Some(by_iid(&[(level, Value::Int(4)), (MAX_AC, Value::Bool(false))])),
            "set by hand, Max AC ends and keeps the rest"
        );
        assert_eq!(state.values.get(&DRIVER_TARGET_TEMPERATURE), Some(&lo));
    }

    #[test]
    fn auto_runs_the_fan_until_it_is_set_by_hand() {
        let mut state = VehicleState::new(false, true, "9.5.0");
        let (level, _) = FAN;
        let iids = [AUTO_CLIMATE, FAN_AUTO, level].map(Value::Int).to_vec();
        state.handle(Lane::High, &command("registerRequest", Value::Array(iids)));
        let update = |state: &mut VehicleState, written: (i64, Value)| {
            let answers = state.handle(Lane::High, &command("writeRequest", by_iid(&[written])));
            opened(&answers[1].1).get("values").cloned()
        };
        assert_eq!(
            state.values.get(&AUTO_CLIMATE),
            Some(&Value::Bool(true)),
            "climate starts in AUTO"
        );
        assert_eq!(
            update(&mut state, (level, Value::Int(5))),
            Some(by_iid(&[
                (level, Value::Int(5)),
                (FAN_AUTO, Value::Bool(false)),
                (AUTO_CLIMATE, Value::Bool(false))
            ]))
        );
        assert_eq!(
            update(&mut state, (AUTO_CLIMATE, Value::Bool(true))),
            Some(by_iid(&[(AUTO_CLIMATE, Value::Bool(true)), (FAN_AUTO, Value::Bool(true))]))
        );
    }

    #[test]
    fn the_fan_stops_at_level_zero_and_comes_back_at_its_last_level() {
        let mut state = VehicleState::new(false, true, "9.5.0");
        let (level, on) = FAN;
        let iids = vec![Value::Int(level), Value::Int(on)];
        state.handle(Lane::High, &command("registerRequest", Value::Array(iids)));
        let update = |state: &mut VehicleState, written: (i64, Value)| {
            let answers = state.handle(Lane::High, &command("writeRequest", by_iid(&[written])));
            opened(&answers[1].1).get("values").cloned()
        };
        assert_eq!(
            update(&mut state, (on, Value::Bool(false))),
            Some(by_iid(&[(on, Value::Bool(false)), (level, Value::Int(0))]))
        );
        assert_eq!(
            update(&mut state, (on, Value::Bool(true))),
            Some(by_iid(&[(on, Value::Bool(true)), (level, Value::Int(3))]))
        );
        assert_eq!(
            update(&mut state, (level, Value::Int(0))),
            Some(by_iid(&[(level, Value::Int(0)), (on, Value::Bool(false))]))
        );
        assert_eq!(
            update(&mut state, (level, Value::Int(7))),
            Some(by_iid(&[(level, Value::Int(7)), (on, Value::Bool(true))]))
        );
    }

    #[test]
    fn synced_zones_follow_the_driver_until_the_passenger_is_set() {
        let mut state = VehicleState::new(false, true, "9.5.0");
        let iids = [DRIVER_TARGET_TEMPERATURE, PASSENGER_TARGET_TEMPERATURE, ZONES_SYNCED];
        state.handle(
            Lane::High,
            &command("registerRequest", Value::Array(iids.map(Value::Int).to_vec())),
        );
        let update = |state: &mut VehicleState, written: &[(i64, Value)]| {
            let answers = state.handle(Lane::High, &command("writeRequest", by_iid(written)));
            opened(&answers[1].1).get("values").cloned()
        };

        assert_eq!(
            update(&mut state, &[(DRIVER_TARGET_TEMPERATURE, Value::Real(23.0))]),
            Some(by_iid(&[(DRIVER_TARGET_TEMPERATURE, Value::Real(23.0))])),
            "not synced, the passenger side stays"
        );
        assert_eq!(
            update(&mut state, &[(ZONES_SYNCED, Value::Bool(true))]),
            Some(by_iid(&[
                (ZONES_SYNCED, Value::Bool(true)),
                (PASSENGER_TARGET_TEMPERATURE, Value::Real(23.0))
            ]))
        );
        assert_eq!(
            update(&mut state, &[(DRIVER_TARGET_TEMPERATURE, Value::Real(24.5))]),
            Some(by_iid(&[
                (DRIVER_TARGET_TEMPERATURE, Value::Real(24.5)),
                (PASSENGER_TARGET_TEMPERATURE, Value::Real(24.5))
            ]))
        );
        assert_eq!(
            update(&mut state, &[(PASSENGER_TARGET_TEMPERATURE, Value::Real(20.0))]),
            Some(by_iid(&[
                (PASSENGER_TARGET_TEMPERATURE, Value::Real(20.0)),
                (ZONES_SYNCED, Value::Bool(false))
            ]))
        );
        assert_eq!(state.values.get(&DRIVER_TARGET_TEMPERATURE), Some(&Value::Real(24.5)));
    }

    #[test]
    fn both_front_seats_have_their_own_temperature() {
        let (_, info) = VehicleState::new(false, true, "9.5.0").info_entry();
        let plugin = &info.get("pluginConfigs").and_then(bplist::Value::as_array).unwrap()[0];
        let climate = plugin
            .get("accessories")
            .and_then(bplist::Value::as_array)
            .unwrap()
            .iter()
            .find(|a| a.get("type").and_then(bplist::Value::as_int) == Some(0x0100_0001))
            .unwrap();
        let seats: Vec<String> = climate
            .get("services")
            .and_then(bplist::Value::as_array)
            .unwrap()
            .iter()
            .filter(|s| s.get("type").and_then(bplist::Value::as_int) == Some(0x1100_0002))
            .flat_map(|s| s.get("characteristics").and_then(bplist::Value::as_array).unwrap())
            .filter(|c| c.get("type").and_then(bplist::Value::as_int) == Some(0x3600_0065))
            .filter_map(|c| c.get("initialValue").and_then(bplist::Value::as_str))
            .map(str::to_string)
            .collect();
        assert_eq!(seats, ["seat_front_left", "seat_front_right"]);
    }

    #[test]
    fn climate_is_described_and_set_from_the_phone() {
        let mut state = VehicleState::new(false, true, "9.5.0");
        let (_, info) = state.info_entry();
        let plugin = &info.get("pluginConfigs").and_then(bplist::Value::as_array).unwrap()[0];
        let accessories = plugin.get("accessories").and_then(bplist::Value::as_array).unwrap();
        fn of_type(list: &[bplist::Value], kind: u64) -> &bplist::Value {
            list.iter()
                .find(|x| x.get("type").and_then(bplist::Value::as_int) == Some(kind))
                .unwrap()
        }
        let climate = of_type(accessories, 0x0100_0001);
        let services = climate.get("services").and_then(bplist::Value::as_array).unwrap();
        let characteristics = |kind| {
            of_type(services, kind)
                .get("characteristics")
                .and_then(bplist::Value::as_array)
                .unwrap()
        };
        let target = of_type(characteristics(0x1100_0002), 0x3100_0017);
        assert_eq!(target.get("format").and_then(bplist::Value::as_int), Some(9));
        assert_eq!(target.get("units").and_then(bplist::Value::as_int), Some(1082));
        assert_eq!(target.get("writable").and_then(bplist::Value::as_bool), Some(true));
        assert_eq!(target.get("oemWritable").and_then(bplist::Value::as_bool), Some(true));
        assert_eq!(target.get("minimumValue"), Some(&bplist::Value::Real(16.0)));
        assert_eq!(target.get("initialValue"), Some(&bplist::Value::Real(21.0)));
        let level = of_type(characteristics(0x1100_0007), 0x3100_0012);
        assert_eq!(level.get("maximumValue").and_then(bplist::Value::as_int), Some(7));
        let resources = of_type(accessories, 0x0120_0002);
        let variant = of_type(
            resources.get("services").and_then(bplist::Value::as_array).unwrap(),
            0x2000_0001,
        );
        let style = of_type(
            variant.get("characteristics").and_then(bplist::Value::as_array).unwrap(),
            0x4800_0004,
        );
        assert_eq!(style.get("initialValue"), Some(&bplist::Value::String("5Seater".into())));

        let iids = [520, 525].map(Value::Int).to_vec();
        state.handle(Lane::High, &command("registerRequest", Value::Array(iids)));
        let written = by_iid(&[(520, Value::Real(22.5)), (521, Value::Real(30.0))]);
        let answers = state.handle(Lane::High, &command("writeRequest", written));
        let results = by_iid(&[(520, Value::Int(WRITTEN)), (521, Value::Int(NO_VALUE))]);
        assert_eq!(opened(&answers[0].1).get("errors"), Some(&results));
        let update = opened(&answers[1].1);
        assert_eq!(update.get("values"), Some(&by_iid(&[(520, Value::Real(22.5))])));
        assert_eq!(
            state.values.get(&521),
            Some(&Value::Real(21.0)),
            "the car measures, not the phone"
        );
    }

    #[test]
    fn climate_reads_as_the_telemetry_block() {
        let climate = serde_json::Value::Object(default_climate());
        assert_eq!(
            climate,
            serde_json::json!({
                "on": true, "auto": true, "ac": true, "maxAc": false, "maxDefrost": false,
                "recirculation": false, "zonesSynced": false,
                "driverTargetC": 21.0, "passengerTargetC": 21.0,
                "driverCabinC": 21.0, "passengerCabinC": 21.0,
                "fanLevel": 3, "fanAuto": true,
                "frontWindowHeating": 0, "rearWindowHeating": 0,
                "ventUpper": false, "ventMiddle": true, "ventLower": false, "ventsAuto": true,
                "driverSeatHeating": 0, "passengerSeatHeating": 0
            })
        );
        let mut without = VehicleState::new(false, false, "9.5.0");
        assert!(without.climate().is_none());
        assert!(without.set_climate(&default_climate()).is_empty());
        assert!(!without.values.contains_key(&AUTO_CLIMATE));
    }

    #[test]
    fn livi_sets_climate_and_the_phone_hears_what_changed() {
        let mut state = VehicleState::new(false, true, "9.5.0");
        let (level, on) = FAN;
        let iids = [AUTO_CLIMATE, FAN_AUTO, level, on, DRIVER_TARGET_TEMPERATURE];
        state.handle(
            Lane::High,
            &command("registerRequest", Value::Array(iids.map(Value::Int).to_vec())),
        );
        let set = |state: &mut VehicleState, climate: serde_json::Value| {
            let answers = state.set_climate(climate.as_object().unwrap());
            assert!(answers.len() <= 1);
            answers.first().map(|(lane, message)| {
                assert_eq!(*lane, Lane::High);
                opened(message).get("values").cloned().unwrap()
            })
        };

        assert_eq!(
            set(&mut state, serde_json::json!({ "fanLevel": 5 })),
            Some(by_iid(&[
                (level, Value::Int(5)),
                (FAN_AUTO, Value::Bool(false)),
                (AUTO_CLIMATE, Value::Bool(false))
            ])),
            "the fan set by hand ends AUTO, as on the phone"
        );
        assert_eq!(set(&mut state, serde_json::json!({ "fanLevel": 5 })), None);
        assert_eq!(
            set(&mut state, serde_json::json!({ "auto": true, "fanAuto": false })),
            Some(by_iid(&[(AUTO_CLIMATE, Value::Bool(true))])),
            "what was named stands against the rules"
        );
        assert_eq!(
            set(&mut state, serde_json::json!({ "driverTargetC": 21.3, "fanLevel": 0 })),
            Some(by_iid(&[
                (DRIVER_TARGET_TEMPERATURE, Value::Real(21.5)),
                (on, Value::Bool(false)),
                (level, Value::Int(0)),
                (AUTO_CLIMATE, Value::Bool(false))
            ]))
        );
        set(
            &mut state,
            serde_json::json!({
                "driverTargetC": 40, "driverSeatHeating": 5, "ventsAuto": false,
                "fanLevel": "high", "rear": 1
            }),
        );
        let climate = state.climate().unwrap();
        assert_eq!(climate["driverTargetC"], 28.0);
        assert_eq!(climate["driverSeatHeating"], 2);
        assert_eq!(climate["fanLevel"], 0);
        assert_eq!(climate["ventsAuto"], false);
        assert!(VENTS.iter().all(|v| state.values.get(&(v + 2)) == Some(&Value::Bool(false))));
    }

    #[test]
    fn what_cannot_be_read_gets_no_answer() {
        let mut state = VehicleState::new(true, true, "9.5.0");
        assert!(state.handle(Lane::High, b"\xA0").is_empty());
        let inner = opack::encode(&dict([("command", Value::str("configRequest"))]));
        let elsewhere =
            opack::encode(&dict([("pluginID", Value::Int(3)), ("pluginData", Value::Data(inner))]));
        assert!(state.handle(Lane::High, &elsewhere).is_empty());
        assert!(state.handle(Lane::High, &command("unregisterRequest", Value::Null)).is_empty());
        let written = by_iid(&[(270, Value::Int(2))]);
        assert_eq!(state.handle(Lane::High, &command("writeRequest", written)).len(), 1);
    }
}
