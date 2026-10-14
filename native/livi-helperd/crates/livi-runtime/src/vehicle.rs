// What LIVI knows about the car, handed to every iAP2 session that asked for it.

use base64::Engine;
use tokio::sync::watch;

use iap2_csm::messages::identification::EngineType;
use iap2_csm::messages::location::StartLocationInformation;
use iap2_csm::messages::vehicle_status::VehicleStatusUpdate;

/// What the car runs on, which decides the ranges per fuel the phone is told about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Fuels {
    pub gasoline: bool,
    pub diesel: bool,
    pub electric: bool,
    pub cng: bool,
}

impl Fuels {
    /// `Gasoline,Electric`
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

    /// What the car tops up from outside. None while LIVI does not know the car.
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
pub struct VehicleStatus {
    pub range: Option<u16>,
    pub outside_temperature: Option<i16>,
    pub range_warning: Option<bool>,
    /// Hybrids only, the range on the tank.
    pub range_fuel: Option<u16>,
    /// Hybrids only, the range on the battery.
    pub range_electric: Option<u16>,
    pub range_warning_electric: Option<bool>,
}

impl VehicleStatus {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Keys LIVI leaves out keep their last value.
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
        let flag = |key: &str, slot: &mut Option<bool>| {
            if let Some(b) = value.get(key).and_then(|v| v.as_bool()) {
                *slot = Some(b);
            }
        };
        flag("rangeWarning", &mut self.range_warning);
        flag("rangeWarningElectric", &mut self.range_warning_electric);
        if let Some(n) = value.get("outsideTemperature").and_then(|v| v.as_i64()) {
            self.outside_temperature = Some(n.clamp(i16::MIN as i64, i16::MAX as i64) as i16);
        }
        Ok(())
    }

    /// A car on one fuel has its whole range on it.
    pub fn update(&self, fuels: Fuels) -> VehicleStatusUpdate {
        let (fuel, electric) = if fuels.hybrid() {
            (
                (self.range_fuel, self.range_warning),
                (self.range_electric, self.range_warning_electric),
            )
        } else {
            ((self.range, self.range_warning), (self.range, self.range_warning))
        };
        let on = |runs: bool, (range, warning): (Option<u16>, Option<bool>)| {
            if runs { (range, warning) } else { (None, None) }
        };
        let (range_gasoline, range_warning_gasoline) = on(fuels.gasoline, fuel);
        let (range_diesel, range_warning_diesel) = on(fuels.diesel, fuel);
        let (range_cng, range_warning_cng) = on(fuels.cng, fuel);
        let (range_electric, range_warning_electric) = on(fuels.electric, electric);
        VehicleStatusUpdate {
            range: self.range,
            outside_temperature: self.outside_temperature,
            range_warning: self.range_warning,
            range_gasoline,
            range_diesel,
            range_electric,
            range_cng,
            range_warning_gasoline,
            range_warning_diesel,
            range_warning_electric,
            range_warning_cng,
        }
    }
}

/// The NMEA sentence types a phone subscribed to, as their three letter names.
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

    /// The sentences of a block the phone asked for, one per line.
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

/// The sending half, held by the helper state.
pub struct Vehicle {
    location: watch::Sender<(u64, String)>,
    status: watch::Sender<VehicleStatus>,
    seek: watch::Sender<(u64, Seek)>,
}

/// The receiving half, one per session.
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

    /// `seek <ms> [<bt-mac>]`
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

    /// `location <base64 nmea>`
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

    /// `vehicle-status {"range":…,"outsideTemperature":…,"rangeWarning":…}`, a hybrid adds
    /// `rangeFuel`, `rangeElectric` and `rangeWarningElectric`.
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
    /// A feed nothing is ever pushed into.
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
        status.merge(r#"{"range":250,"outsideTemperature":-3}"#).unwrap();
        status.merge(r#"{"rangeWarning":true}"#).unwrap();
        status
            .merge(r#"{"rangeFuel":70000,"rangeElectric":40,"rangeWarningElectric":false}"#)
            .unwrap();
        assert_eq!(
            status,
            VehicleStatus {
                range: Some(250),
                outside_temperature: Some(-3),
                range_warning: Some(true),
                range_fuel: Some(u16::MAX),
                range_electric: Some(40),
                range_warning_electric: Some(false),
            }
        );
        assert!(status.merge("nope").is_err());
    }

    #[test]
    fn fuels_come_from_a_list_of_names() {
        assert_eq!(
            Fuels::parse("Diesel, Electric,CNG,Hydrogen"),
            Fuels { gasoline: false, diesel: true, electric: true, cng: true }
        );
        assert_eq!(Fuels::parse(""), Fuels::default());
    }

    #[test]
    fn the_phone_hears_every_engine_the_car_tops_up() {
        let hybrid = Fuels::parse("Gasoline,Electric").engine_types();
        assert_eq!(hybrid, [EngineType::Gas, EngineType::Electric]);
        assert_eq!(Fuels::parse("CNG").engine_types(), [EngineType::Cng]);
        assert_eq!(Fuels::parse("Diesel").engine_types(), [EngineType::Diesel]);
        assert!(Fuels::default().engine_types().is_empty(), "an unknown car names no engine");
    }

    #[test]
    fn one_fuel_gets_the_whole_range_and_a_hybrid_splits_it() {
        let status = VehicleStatus {
            range: Some(600),
            outside_temperature: Some(12),
            range_warning: Some(false),
            range_fuel: Some(550),
            range_electric: Some(50),
            range_warning_electric: Some(true),
        };

        let electric = status.update(Fuels::parse("Electric"));
        assert_eq!((electric.range, electric.outside_temperature), (Some(600), Some(12)));
        assert_eq!(
            (electric.range_electric, electric.range_warning_electric),
            (Some(600), Some(false))
        );
        assert_eq!((electric.range_gasoline, electric.range_warning_gasoline), (None, None));

        let hybrid = status.update(Fuels::parse("Diesel,Electric"));
        assert_eq!((hybrid.range_diesel, hybrid.range_warning_diesel), (Some(550), Some(false)));
        assert_eq!((hybrid.range_electric, hybrid.range_warning_electric), (Some(50), Some(true)));
        assert_eq!((hybrid.range_cng, hybrid.range_gasoline), (None, None));

        let unknown = status.update(Fuels::default());
        assert_eq!(unknown.range, Some(600));
        assert_eq!(
            [
                unknown.range_gasoline,
                unknown.range_diesel,
                unknown.range_electric,
                unknown.range_cng
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
        assert_eq!(feed.seek.borrow_and_update().1, Seek { ms: 60000, bt_mac: None });
        vehicle.push_seek(" 60000  AA:BB:CC:DD:EE:FF ").unwrap();
        assert!(feed.seek.has_changed().unwrap());
        assert_eq!(
            feed.seek.borrow_and_update().1,
            Seek { ms: 60000, bt_mac: Some("AA:BB:CC:DD:EE:FF".into()) }
        );
        assert!(vehicle.push_seek("1:00").is_err());
        assert!(vehicle.push_seek("").is_err());
    }

    #[test]
    fn a_seek_for_one_phone_skips_the_others() {
        let to_one = Seek { ms: 1, bt_mac: Some("aa:bb:cc:dd:ee:ff".into()) };
        assert!(to_one.meant_for(Some("AA:BB:CC:DD:EE:FF")));
        assert!(!to_one.meant_for(Some("11:22:33:44:55:66")));
        assert!(to_one.meant_for(None));
        assert!(Seek { ms: 1, bt_mac: None }.meant_for(Some("11:22:33:44:55:66")));
    }
}
