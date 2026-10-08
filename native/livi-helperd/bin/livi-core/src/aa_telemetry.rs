use livi_aa_stack::manager::{AaCmd, AaHandle};
use livi_aa_stack::sensors::{GpsFix, Sensor};
use serde_json::{Map, Value};
use tokio::time::Instant;

/// The maps re-plan on an energy model, so it goes out at most this often.
const ENERGY_MODEL_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

pub const LOW_FUEL_PCT: f64 = 10.0;

/// The phone's gear: 0 neutral, 1 to 10 manual, 100 drive, 101 park, 102 reverse.
pub fn gear(gear: Option<&Value>, reverse: Option<bool>) -> Option<i64> {
    match gear {
        Some(Value::Number(n)) => match n.as_f64() {
            Some(-1.0) => return Some(102),
            Some(0.0) => return Some(0),
            Some(g) if (1.0..=10.0).contains(&g) && g.fract() == 0.0 => return Some(g as i64),
            _ => {}
        },
        Some(Value::String(s)) => {
            let g = s.trim().to_uppercase();
            match g.as_str() {
                "P" => return Some(101),
                "R" => return Some(102),
                "N" => return Some(0),
                "D" | "S" => return Some(100),
                _ => {}
            }
            if let Some(n) = g.strip_prefix('M').filter(|d| (1..=2).contains(&d.len()))
                && n.bytes().all(|b| b.is_ascii_digit())
                && let Ok(n) = n.parse::<i64>()
                && (1..=10).contains(&n)
            {
                return Some(n);
            }
        }
        _ => {}
    }
    (reverse == Some(true)).then_some(102)
}

/// Halves round up, matching the UI.
fn round(v: f64) -> i64 {
    (v + 0.5).floor() as i64
}

fn num(map: &Map<String, Value>, key: &str) -> Option<f64> {
    map.get(key).and_then(Value::as_f64)
}

fn flag(map: &Map<String, Value>, key: &str) -> Option<bool> {
    map.get(key).and_then(Value::as_bool)
}

#[derive(Default)]
struct Sent {
    speed_mm_s: Option<i32>,
    rpm_e3: Option<i32>,
    gear: Option<i64>,
    night_mode: Option<bool>,
    parking_brake: Option<bool>,
    driving_status: Option<i64>,
    lights: Option<(Option<i32>, Option<i32>, Option<bool>)>,
    fuel: Option<(i32, Option<i32>, Option<bool>)>,
    odometer: Option<(i32, Option<i32>)>,
    environment: Option<(Option<i32>, Option<i32>)>,
    gps: Option<GpsFix>,
    energy_model_at: Option<Instant>,
}

#[derive(Default)]
pub struct AaTelemetry {
    sent: Sent,
}

impl AaTelemetry {
    pub fn hydrate(&mut self, snap: &Map<String, Value>, aa: &AaHandle) {
        self.sent = Sent::default();
        if !snap.is_empty() {
            self.follow(&Map::new(), snap, aa);
        }
    }

    pub fn follow(&mut self, prev: &Map<String, Value>, next: &Map<String, Value>, aa: &AaHandle) {
        for sensor in self.sensors(prev, next, Instant::now()) {
            aa.send(AaCmd::Sensor(sensor));
        }
    }

    fn sensors(
        &mut self,
        prev: &Map<String, Value>,
        next: &Map<String, Value>,
        now: Instant,
    ) -> Vec<Sensor> {
        let changed = |key: &str| next.contains_key(key) && prev.get(key) != next.get(key);
        let sent = &mut self.sent;
        let mut out = Vec::new();

        if changed("speedKph")
            && let Some(kph) = num(next, "speedKph")
        {
            let v = round(kph * 1000.0 / 3.6).max(0) as i32;
            if sent.speed_mm_s != Some(v) {
                sent.speed_mm_s = Some(v);
                out.push(Sensor::Speed(v));
            }
        }
        if changed("rpm")
            && let Some(rpm) = num(next, "rpm")
        {
            let v = round(rpm * 1000.0).max(0) as i32;
            if sent.rpm_e3 != Some(v) {
                sent.rpm_e3 = Some(v);
                out.push(Sensor::Rpm(v));
            }
        }
        if (changed("gear") || changed("reverse"))
            && let Some(g) = gear(next.get("gear"), flag(next, "reverse"))
            && sent.gear != Some(g)
        {
            sent.gear = Some(g);
            out.push(Sensor::Gear(g as i32));
        }
        if changed("nightMode")
            && let Some(night) = flag(next, "nightMode")
            && sent.night_mode != Some(night)
        {
            sent.night_mode = Some(night);
            out.push(Sensor::NightMode(night));
        }
        if changed("parkingBrake")
            && let Some(on) = flag(next, "parkingBrake")
            && sent.parking_brake != Some(on)
        {
            sent.parking_brake = Some(on);
            out.push(Sensor::ParkingBrake(on));
        }
        if changed("drivingStatus")
            && let Some(status) = num(next, "drivingStatus")
            && sent.driving_status != Some(status as i64)
        {
            sent.driving_status = Some(status as i64);
            out.push(Sensor::DrivingStatus(status as i32));
        }

        if ["lights", "highBeam", "hazards", "turn"].iter().any(|k| changed(k)) {
            let head = if flag(next, "highBeam") == Some(true) {
                Some(3)
            } else {
                flag(next, "lights").map(|on| if on { 2 } else { 1 })
            };
            let turn = match next.get("turn").and_then(Value::as_str) {
                Some("left") => Some(2),
                Some("right") => Some(3),
                Some("none") => Some(1),
                _ => None,
            };
            let hazards = flag(next, "hazards");
            let lights = (head, turn, hazards);
            if sent.lights.unwrap_or_default() != lights {
                sent.lights = Some(lights);
                out.push(Sensor::Light {
                    head_light: head,
                    hazard_lights: hazards,
                    turn_indicator: turn,
                });
            }
        }

        if (changed("fuelPct") || changed("rangeKm"))
            && let Some(pct) = num(next, "fuelPct")
        {
            let level = round(pct).clamp(0, 100) as i32;
            let range = num(next, "rangeKm").map(|km| round(km).max(0) as i32);
            let low = Some(pct < LOW_FUEL_PCT);
            if sent.fuel != Some((level, range, low)) {
                sent.fuel = Some((level, range, low));
                out.push(Sensor::Fuel {
                    level_percent: level,
                    range_km: range,
                    low_fuel_warning: low,
                });
            }
        }

        if (changed("odometerKm") || changed("odometerTripKm"))
            && let Some(km) = num(next, "odometerKm")
        {
            let total = round(km * 10.0) as i32;
            let trip = num(next, "odometerTripKm").map(|t| round(t * 10.0) as i32);
            if sent.odometer != Some((total, trip)) {
                sent.odometer = Some((total, trip));
                out.push(Sensor::Odometer { total_km_e1: total, trip_km_e1: trip });
            }
        }

        if changed("ambientC") || changed("baroKpa") {
            let temp = num(next, "ambientC").map(|c| round(c * 1000.0) as i32);
            let pressure = num(next, "baroKpa").map(|k| round(k * 1000.0) as i32);
            if sent.environment.unwrap_or_default() != (temp, pressure) {
                sent.environment = Some((temp, pressure));
                out.push(Sensor::Environment { temperature_e3: temp, pressure_e3: pressure });
            }
        }

        if changed("gps")
            && let Some(gps) = next.get("gps").and_then(Value::as_object)
            && let (Some(lat), Some(lng)) = (num(gps, "lat"), num(gps, "lng"))
        {
            let fix = GpsFix {
                lat_deg: lat,
                lng_deg: lng,
                accuracy_m: num(gps, "accuracyM"),
                altitude_m: num(gps, "alt"),
                speed_ms: num(gps, "speedMs"),
                bearing_deg: num(gps, "heading"),
            };
            if sent.gps != Some(fix) {
                sent.gps = Some(fix);
                out.push(Sensor::Gps(fix));
            }
        }

        if let (Some(capacity_kwh), Some(range_km)) =
            (num(next, "batteryCapacityKwh"), num(next, "rangeKm"))
            && range_km > 0.0
            && sent.energy_model_at.is_none_or(|at| now.duration_since(at) >= ENERGY_MODEL_EVERY)
        {
            let capacity_wh = round(capacity_kwh * 1000.0);
            let current_wh = match (num(next, "batteryLevelKwh"), num(next, "fuelPct")) {
                (Some(kwh), _) => round(kwh * 1000.0),
                (None, Some(pct)) => round(pct / 100.0 * capacity_wh as f64),
                (None, None) => 0,
            };
            if capacity_wh > 0 && current_wh > 0 {
                out.push(Sensor::VehicleEnergyModel {
                    capacity_wh,
                    current_wh,
                    range_m: round(range_km * 1000.0),
                    max_charge_power_w: None,
                    max_discharge_power_w: None,
                    auxiliary_wh_per_km: None,
                });
                sent.energy_model_at = Some(now);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn map(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn gears_read_as_the_phone_counts_them() {
        assert_eq!(gear(Some(&json!(-1)), None), Some(102));
        assert_eq!(gear(Some(&json!(0)), None), Some(0));
        assert_eq!(gear(Some(&json!(4)), None), Some(4));
        assert_eq!(gear(Some(&json!(11)), None), None);
        assert_eq!(gear(Some(&json!(" p ")), None), Some(101));
        assert_eq!(gear(Some(&json!("D")), None), Some(100));
        assert_eq!(gear(Some(&json!("S")), None), Some(100));
        assert_eq!(gear(Some(&json!("M3")), None), Some(3));
        assert_eq!(gear(Some(&json!("M11")), None), None);
        assert_eq!(gear(Some(&json!("M123")), None), None);
        assert_eq!(gear(Some(&json!("x")), Some(true)), Some(102));
        assert_eq!(gear(None, None), None);
    }

    #[test]
    fn only_what_the_phone_would_see_differently_goes_out() {
        let mut t = AaTelemetry::default();
        let now = Instant::now();
        let first = map(json!({ "speedKph": 73.4, "rpm": 2.5, "gear": "D", "nightMode": true }));
        let sent = t.sensors(&Map::new(), &first, now);
        assert_eq!(
            sent,
            [Sensor::Speed(20_389), Sensor::Rpm(2500), Sensor::Gear(100), Sensor::NightMode(true),]
        );
        let tick = map(json!({ "speedKph": 73.40001, "rpm": 2.5, "gear": "D", "nightMode": true }));
        assert!(t.sensors(&first, &tick, now).is_empty());
    }

    #[test]
    fn bundles_carry_their_fields_together() {
        let mut t = AaTelemetry::default();
        let now = Instant::now();
        let snap = map(json!({
            "highBeam": true, "turn": "left", "hazards": false,
            "fuelPct": 8.4, "rangeKm": 120.5,
            "odometerKm": 12345.67, "odometerTripKm": 12.34,
            "ambientC": -2.5, "baroKpa": 101.3,
            "gps": { "lat": 52.5, "lng": 13.4, "alt": 34.0, "speedMs": 3.0, "heading": 90.0 },
            "batteryCapacityKwh": 77.0
        }));
        let sent = t.sensors(&Map::new(), &snap, now);
        assert!(sent.contains(&Sensor::Light {
            head_light: Some(3),
            hazard_lights: Some(false),
            turn_indicator: Some(2)
        }));
        assert!(sent.contains(&Sensor::Fuel {
            level_percent: 8,
            range_km: Some(121),
            low_fuel_warning: Some(true)
        }));
        assert!(sent.contains(&Sensor::Odometer { total_km_e1: 123_457, trip_km_e1: Some(123) }));
        assert!(sent.contains(&Sensor::Environment {
            temperature_e3: Some(-2500),
            pressure_e3: Some(101_300)
        }));
        assert!(sent.iter().any(|s| matches!(s, Sensor::Gps(f) if f.lat_deg == 52.5)));
        assert!(sent.contains(&Sensor::VehicleEnergyModel {
            capacity_wh: 77_000,
            current_wh: 6468,
            range_m: 120_500,
            max_charge_power_w: None,
            max_discharge_power_w: None,
            auxiliary_wh_per_km: None
        }));
        let later = map(json!({ "batteryCapacityKwh": 77.0, "rangeKm": 119.0 }));
        assert!(
            !t.sensors(&snap, &later, now)
                .iter()
                .any(|s| matches!(s, Sensor::VehicleEnergyModel { .. }))
        );
        let much_later = now + ENERGY_MODEL_EVERY;
        let with_level =
            map(json!({ "batteryCapacityKwh": 77.0, "rangeKm": 119.0, "batteryLevelKwh": 30.0 }));
        assert!(
            t.sensors(&later, &with_level, much_later)
                .iter()
                .any(|s| matches!(s, Sensor::VehicleEnergyModel { current_wh: 30_000, .. }))
        );
    }

    #[test]
    fn plain_lights_without_high_beam() {
        let mut t = AaTelemetry::default();
        let now = Instant::now();
        let off = t.sensors(&Map::new(), &map(json!({ "lights": false, "turn": "none" })), now);
        assert_eq!(
            off,
            [Sensor::Light { head_light: Some(1), hazard_lights: None, turn_indicator: Some(1) }]
        );
        let on = t.sensors(&Map::new(), &map(json!({ "lights": true, "turn": "right" })), now);
        assert_eq!(
            on,
            [Sensor::Light { head_light: Some(2), hazard_lights: None, turn_indicator: Some(3) }]
        );
    }
}
