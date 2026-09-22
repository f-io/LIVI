// What LIVI knows about the car, handed to every iAP2 session that asked for it.

use base64::Engine;
use tokio::sync::watch;

use iap2_csm::messages::location::StartLocationInformation;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VehicleStatus {
    pub range: Option<u16>,
    pub outside_temperature: Option<i16>,
    pub range_warning: Option<bool>,
}

impl VehicleStatus {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Keys LIVI leaves out keep their last value.
    pub fn merge(&mut self, json: &str) -> Result<(), String> {
        let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if let Some(n) = value.get("range").and_then(|v| v.as_u64()) {
            self.range = Some(n.min(u16::MAX as u64) as u16);
        }
        if let Some(n) = value.get("outsideTemperature").and_then(|v| v.as_i64()) {
            self.outside_temperature = Some(n.clamp(i16::MIN as i64, i16::MAX as i64) as i16);
        }
        if let Some(b) = value.get("rangeWarning").and_then(|v| v.as_bool()) {
            self.range_warning = Some(b);
        }
        Ok(())
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

/// The sending half, held by the helper state.
pub struct Vehicle {
    location: watch::Sender<(u64, String)>,
    status: watch::Sender<VehicleStatus>,
}

/// The receiving half, one per session.
#[derive(Clone)]
pub struct VehicleFeed {
    pub location: watch::Receiver<(u64, String)>,
    pub status: watch::Receiver<VehicleStatus>,
}

impl Default for Vehicle {
    fn default() -> Self {
        Self {
            location: watch::Sender::new((0, String::new())),
            status: watch::Sender::new(VehicleStatus::default()),
        }
    }
}

impl Vehicle {
    pub fn feed(&self) -> VehicleFeed {
        VehicleFeed {
            location: self.location.subscribe(),
            status: self.status.subscribe(),
        }
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

    /// `vehicle-status {"range":…,"outsideTemperature":…,"rangeWarning":…}`
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
        assert_eq!(
            status,
            VehicleStatus {
                range: Some(250),
                outside_temperature: Some(-3),
                range_warning: Some(true)
            }
        );
        assert!(status.merge("nope").is_err());
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
        vehicle.push_location(&base64::engine::general_purpose::STANDARD.encode("$GPGGA,1*00")).unwrap();
        assert!(feed.location.has_changed().unwrap());
        assert_eq!(feed.location.borrow_and_update().1, "$GPGGA,1*00");

        vehicle.push_status(r#"{"range":100}"#).unwrap();
        assert!(feed.status.has_changed().unwrap());
        feed.status.borrow_and_update();
        vehicle.push_status(r#"{"range":100}"#).unwrap();
        assert!(!feed.status.has_changed().unwrap());
        assert!(vehicle.push_status("{").is_err());
    }
}
