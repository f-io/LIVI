// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use base64::Engine;
use tokio::sync::watch;

use iap2_csm::messages::identification::{ChargingConnectorType, EngineType};
use iap2_csm::messages::location::StartLocationInformation;
use iap2_csm::messages::vehicle_status::VehicleStatusUpdate;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Fuels {
    pub gasoline: bool,
    pub diesel: bool,
    pub electric: bool,
    pub cng: bool,
}

impl Fuels {
    pub fn parse(list: &str) -> Self {
        let mut fuels = Self::default();
        for name in list.split(',').map(str::trim) {
            match name {
                "Gasoline" => fuels.gasoline = true,
                "Diesel" => fuels.diesel = true,
                "Electric" => fuels.electric = true,
                "CNG" => fuels.cng = true,
                _ => {}
            }
        }
        fuels
    }

    fn hybrid(&self) -> bool {
        self.electric && (self.gasoline || self.diesel || self.cng)
    }

    pub fn engine_types(&self) -> Vec<EngineType> {
        [
            (self.gasoline, EngineType::Gas),
            (self.diesel, EngineType::Diesel),
            (self.electric, EngineType::Electric),
            (self.cng, EngineType::Cng),
        ]
        .into_iter()
        .filter_map(|(runs, engine)| runs.then_some(engine))
        .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plugs(Vec<(ChargingConnectorType, Option<u32>)>);

impl Plugs {
    pub fn parse(list: &str) -> Self {
        let plugs = list.split(',').filter_map(|entry| {
            let (name, watts) = entry.trim().split_once(':').unwrap_or((entry.trim(), ""));
            let plug = match name {
                "Combo1" => ChargingConnectorType::Ccs1,
                "Combo2" => ChargingConnectorType::Ccs2,
                "J1772" => ChargingConnectorType::J1772,
                "Chademo" => ChargingConnectorType::Chademo,
                "Mennekes" => ChargingConnectorType::Mennekes,
                "Gbt" => ChargingConnectorType::GbtDc,
                "TeslaSupercharger" => ChargingConnectorType::NacsDc,
                _ => return None,
            };
            Some((plug, watts.parse().ok()))
        });
        Self(plugs.collect())
    }

    pub fn types(&self) -> Vec<ChargingConnectorType> {
        self.0.iter().map(|(plug, _)| *plug).collect()
    }

    pub fn watts(&self, plug: ChargingConnectorType) -> Option<u32> {
        self.0
            .iter()
            .find(|(p, _)| *p == plug)
            .and_then(|(_, w)| *w)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VehicleStatus {
    pub range: Option<u16>,
    pub outside_temperature: Option<i16>,
    pub range_warning: Option<bool>,
    pub range_fuel: Option<u16>,
    pub range_electric: Option<u16>,
    pub battery_min_wh: Option<u32>,
    pub battery_max_wh: Option<u32>,
    pub battery_wh: Option<u32>,
    pub battery_percent_e3: Option<u32>,
    pub charging: Option<bool>,
    pub active_connector: Option<ChargingConnectorType>,
    pub range_max_electric: Option<u16>,
}

impl VehicleStatus {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn merge(&mut self, json: &str) -> Result<(), String> {
        let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let km = |key: &str, slot: &mut Option<u16>| {
            if let Some(n) = value.get(key).and_then(|v| v.as_u64()) {
                *slot = Some(n.min(u16::MAX as u64) as u16);
            }
        };
        km("range", &mut self.range);
        km("rangeFuel", &mut self.range_fuel);
        km("rangeElectric", &mut self.range_electric);
        km("rangeMaxElectric", &mut self.range_max_electric);
        let count = |key: &str, slot: &mut Option<u32>| {
            if let Some(n) = value.get(key).and_then(|v| v.as_u64()) {
                *slot = Some(n.min(u32::MAX as u64) as u32);
            }
        };
        count("batteryMinWh", &mut self.battery_min_wh);
        count("batteryMaxWh", &mut self.battery_max_wh);
        count("batteryWh", &mut self.battery_wh);
        count("batteryPercentE3", &mut self.battery_percent_e3);
        if let Some(b) = value.get("charging").and_then(|v| v.as_bool()) {
            self.charging = Some(b);
        }
        if let Some(n) = value.get("activeConnector").and_then(|v| v.as_u64()) {
            self.active_connector = u8::try_from(n)
                .ok()
                .and_then(ChargingConnectorType::from_u8);
        }
        if let Some(b) = value.get("rangeWarning").and_then(|v| v.as_bool()) {
            self.range_warning = Some(b);
        }
        if let Some(n) = value.get("outsideTemperature").and_then(|v| v.as_i64()) {
            self.outside_temperature = Some(n.clamp(i16::MIN as i64, i16::MAX as i64) as i16);
        }
        Ok(())
    }

    pub fn update(&self, fuels: Fuels) -> VehicleStatusUpdate {
        let (fuel, electric) = if fuels.hybrid() {
            (self.range_fuel, self.range_electric)
        } else {
            (self.range, self.range)
        };
        let on = |runs: bool, range: Option<u16>| if runs { range } else { None };
        let on32 = |v: Option<u32>| v.filter(|_| fuels.electric);
        VehicleStatusUpdate {
            range: self.range,
            outside_temperature: self.outside_temperature,
            range_warning: self.range_warning,
            range_gasoline: on(fuels.gasoline, fuel),
            range_diesel: on(fuels.diesel, fuel),
            range_electric: on(fuels.electric, electric),
            range_cng: on(fuels.cng, fuel),
            range_warning_gasoline: None,
            range_warning_diesel: None,
            range_warning_electric: None,
            range_warning_cng: None,
            min_battery_charge_wh: on32(self.battery_min_wh),
            current_battery_charge_wh: on32(self.battery_wh),
            max_battery_charge_wh: on32(self.battery_max_wh),
            displayed_battery_percentage_e3: on32(self.battery_percent_e3),
            is_charging: self.charging.filter(|_| fuels.electric),
            active_connector: self.active_connector.filter(|_| fuels.electric),
            max_range_electric: on(fuels.electric, self.range_max_electric),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocationTypes(Vec<&'static str>);

impl LocationTypes {
    pub fn from_request(req: &StartLocationInformation) -> Self {
        let mut types = Vec::new();
        if req.gps_fix_data {
            types.push("GGA");
        }
        if req.recommended_minimum {
            types.push("RMC");
        }
        if req.satellites_in_view {
            types.push("GSV");
        }
        Self(types)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn names(&self) -> &[&'static str] {
        &self.0
    }

    pub fn wanted<'a>(&'a self, nmea: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        nmea.lines().map(str::trim).filter(move |line| {
            line.starts_with('$') && line.len() >= 6 && self.0.contains(&&line[3..6])
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seek {
    pub ms: u32,
    pub bt_mac: Option<String>,
}

impl Seek {
    pub fn meant_for(&self, phone_bt_mac: Option<&str>) -> bool {
        match (self.bt_mac.as_deref(), phone_bt_mac) {
            (Some(target), Some(phone)) => target.eq_ignore_ascii_case(phone),
            _ => true,
        }
    }
}

pub struct Vehicle {
    location: watch::Sender<(u64, String)>,
    status: watch::Sender<VehicleStatus>,
    seek: watch::Sender<(u64, Seek)>,
}

#[derive(Clone)]
pub struct VehicleFeed {
    pub location: watch::Receiver<(u64, String)>,
    pub status: watch::Receiver<VehicleStatus>,
    pub seek: watch::Receiver<(u64, Seek)>,
}

impl Default for Vehicle {
    fn default() -> Self {
        Self {
            location: watch::Sender::new((0, String::new())),
            status: watch::Sender::new(VehicleStatus::default()),
            seek: watch::Sender::new((0, Seek::default())),
        }
    }
}

impl Vehicle {
    pub fn feed(&self) -> VehicleFeed {
        VehicleFeed {
            location: self.location.subscribe(),
            status: self.status.subscribe(),
            seek: self.seek.subscribe(),
        }
    }

    pub fn push_seek(&self, arg: &str) -> Result<(), String> {
        let mut words = arg.split_whitespace();
        let ms: u32 = words
            .next()
            .and_then(|w| w.parse().ok())
            .ok_or_else(|| format!("not a position in ms: {arg:?}"))?;
        let bt_mac = words.next().map(str::to_string);
        self.seek.send_modify(|slot| {
            slot.0 = slot.0.wrapping_add(1);
            slot.1 = Seek { ms, bt_mac };
        });
        Ok(())
    }

    pub fn push_location(&self, arg: &str) -> Result<(), String> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(arg.trim())
            .map_err(|e| e.to_string())?;
        let nmea = String::from_utf8_lossy(&bytes).into_owned();
        if nmea.trim().is_empty() {
            return Ok(());
        }
        self.location.send_modify(|slot| {
            slot.0 = slot.0.wrapping_add(1);
            slot.1 = nmea;
        });
        Ok(())
    }

    pub fn push_status(&self, json: &str) -> Result<(), String> {
        let mut next = self.status.borrow().clone();
        next.merge(json)?;
        self.status.send_if_modified(|current| {
            if *current == next {
                return false;
            }
            *current = next;
            true
        });
        Ok(())
    }
}

impl VehicleFeed {
    pub fn quiet() -> Self {
        Vehicle::default().feed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_merges_key_by_key() {
        let mut status = VehicleStatus::default();
        status
            .merge(r#"{"range":70000,"outsideTemperature":-3}"#)
            .unwrap();
        status.merge(r#"{"rangeWarning":true}"#).unwrap();
        status
            .merge(r#"{"rangeFuel":512,"rangeElectric":40}"#)
            .unwrap();
        assert_eq!(
            status,
            VehicleStatus {
                range: Some(u16::MAX),
                outside_temperature: Some(-3),
                range_warning: Some(true),
                range_fuel: Some(512),
                range_electric: Some(40),
                ..Default::default()
            }
        );
        assert!(status.merge("nope").is_err());
    }

    #[test]
    fn plugs_carry_their_power() {
        let plugs = Plugs::parse("Combo2:150000, Mennekes:11000,Gbt,Steam:5");
        use ChargingConnectorType::*;
        assert_eq!(plugs.types(), vec![Ccs2, Mennekes, GbtDc]);
        assert_eq!(
            (plugs.watts(Ccs2), plugs.watts(Mennekes)),
            (Some(150_000), Some(11_000))
        );
        assert_eq!((plugs.watts(GbtDc), plugs.watts(J1772)), (None, None));
        assert_eq!(Plugs::parse(""), Plugs::default());
    }

    #[test]
    fn only_a_car_that_charges_tells_its_battery() {
        let mut status = VehicleStatus::default();
        status
            .merge(
                r#"{"range":300,"batteryMinWh":0,"batteryMaxWh":77000,"batteryWh":38500,
                "batteryPercentE3":50000,"charging":true,"activeConnector":1,
                "rangeMaxElectric":520}"#,
            )
            .unwrap();
        let ev = status.update(Fuels::parse("Electric"));
        assert_eq!(
            (
                ev.min_battery_charge_wh,
                ev.current_battery_charge_wh,
                ev.max_battery_charge_wh
            ),
            (Some(0), Some(38_500), Some(77_000))
        );
        assert_eq!(ev.displayed_battery_percentage_e3, Some(50_000));
        assert_eq!(
            (ev.is_charging, ev.active_connector),
            (Some(true), Some(ChargingConnectorType::Ccs2))
        );
        assert_eq!(ev.max_range_electric, Some(520));

        let diesel = status.update(Fuels::parse("Diesel"));
        assert_eq!(diesel.range, Some(300));
        assert_eq!(
            (
                diesel.current_battery_charge_wh,
                diesel.is_charging,
                diesel.max_range_electric
            ),
            (None, None, None)
        );
        status.merge(r#"{"activeConnector":42}"#).unwrap();
        assert_eq!(status.active_connector, None, "an unknown plug names none");
    }

    #[test]
    fn fuels_come_from_a_list_of_names() {
        assert_eq!(
            Fuels::parse("Diesel, Electric,CNG,Hydrogen"),
            Fuels {
                gasoline: false,
                diesel: true,
                electric: true,
                cng: true
            }
        );
        assert_eq!(Fuels::parse(""), Fuels::default());
    }

    #[test]
    fn the_phone_hears_every_engine_the_car_tops_up() {
        let hybrid = Fuels::parse("Gasoline,Electric").engine_types();
        assert_eq!(hybrid, [EngineType::Gas, EngineType::Electric]);
        assert_eq!(Fuels::parse("CNG").engine_types(), [EngineType::Cng]);
        assert_eq!(Fuels::parse("Diesel").engine_types(), [EngineType::Diesel]);
        assert!(
            Fuels::default().engine_types().is_empty(),
            "an unknown car names no engine"
        );
    }

    #[test]
    fn one_fuel_gets_the_whole_range_a_hybrid_splits_it_and_the_warning_is_shared() {
        let status = VehicleStatus {
            range: Some(600),
            outside_temperature: Some(12),
            range_warning: Some(false),
            range_fuel: Some(550),
            range_electric: Some(50),
            ..Default::default()
        };
        let ranges = |u: &VehicleStatusUpdate| {
            [
                u.range_gasoline,
                u.range_diesel,
                u.range_electric,
                u.range_cng,
            ]
        };

        let diesel = status.update(Fuels::parse("Diesel"));
        assert_eq!(
            (
                diesel.range,
                diesel.outside_temperature,
                diesel.range_warning
            ),
            (Some(600), Some(12), Some(false))
        );
        assert_eq!(ranges(&diesel), [None, Some(600), None, None]);

        let hybrid = status.update(Fuels::parse("Gasoline,Electric"));
        assert_eq!(ranges(&hybrid), [Some(550), None, Some(50), None]);
        assert_eq!(
            ranges(&status.update(Fuels::default())),
            [None; 4],
            "an unknown car"
        );

        let update = hybrid;
        assert_eq!(
            [
                update.range_warning_gasoline,
                update.range_warning_diesel,
                update.range_warning_electric,
                update.range_warning_cng
            ],
            [None; 4]
        );
    }

    #[test]
    fn only_the_sentences_asked_for_go_out() {
        let types = LocationTypes::from_request(&StartLocationInformation {
            gps_fix_data: true,
            recommended_minimum: true,
            satellites_in_view: false,
            vehicle_speed: false,
        });
        let block = "$GPGGA,1*00\r\n$GPRMC,2*00\r\n$GPGSV,3*00\r\n\r\nrubbish\n";
        let sent: Vec<&str> = types.wanted(block).collect();
        assert_eq!(sent, ["$GPGGA,1*00", "$GPRMC,2*00"]);
    }

    #[test]
    fn a_location_push_wakes_the_feed_and_an_unchanged_status_does_not() {
        let vehicle = Vehicle::default();
        let mut feed = vehicle.feed();
        vehicle
            .push_location(&base64::engine::general_purpose::STANDARD.encode("$GPGGA,1*00"))
            .unwrap();
        assert!(feed.location.has_changed().unwrap());
        assert_eq!(feed.location.borrow_and_update().1, "$GPGGA,1*00");

        vehicle.push_status(r#"{"range":100}"#).unwrap();
        assert!(feed.status.has_changed().unwrap());
        feed.status.borrow_and_update();
        vehicle.push_status(r#"{"range":100}"#).unwrap();
        assert!(!feed.status.has_changed().unwrap());
        assert!(vehicle.push_status("{").is_err());
    }

    #[test]
    fn every_seek_wakes_the_feed_even_to_the_same_position() {
        let vehicle = Vehicle::default();
        let mut feed = vehicle.feed();
        vehicle.push_seek("60000").unwrap();
        assert!(feed.seek.has_changed().unwrap());
        assert_eq!(
            feed.seek.borrow_and_update().1,
            Seek {
                ms: 60000,
                bt_mac: None
            }
        );
        vehicle.push_seek(" 60000  AA:BB:CC:DD:EE:FF ").unwrap();
        assert!(feed.seek.has_changed().unwrap());
        assert_eq!(
            feed.seek.borrow_and_update().1,
            Seek {
                ms: 60000,
                bt_mac: Some("AA:BB:CC:DD:EE:FF".into())
            }
        );
        assert!(vehicle.push_seek("1:00").is_err());
        assert!(vehicle.push_seek("").is_err());
    }

    #[test]
    fn a_seek_for_one_phone_skips_the_others() {
        let to_one = Seek {
            ms: 1,
            bt_mac: Some("aa:bb:cc:dd:ee:ff".into()),
        };
        assert!(to_one.meant_for(Some("AA:BB:CC:DD:EE:FF")));
        assert!(!to_one.meant_for(Some("11:22:33:44:55:66")));
        assert!(to_one.meant_for(None));
        assert!(
            Seek {
                ms: 1,
                bt_mac: None
            }
            .meant_for(Some("11:22:33:44:55:66"))
        );
    }
}
