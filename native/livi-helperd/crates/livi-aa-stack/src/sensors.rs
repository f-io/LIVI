//! Each sensor goes out as a SensorBatchNotification holding just that sensor.

use livi_aa_proto::{
    CarLocation, CurrentGear, DrivingStatus, EngineRpm, EnvironmentConditions, FuelLevel,
    LightStates, NightMode, Odometer, ParkingBrake, SensorBatchNotification, VehicleSpeed,
};
use prost::Message;

use crate::wire::{field_float, field_len_delim, field_varint, round_half_up};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpsFix {
    pub lat_deg: f64,
    pub lng_deg: f64,
    pub accuracy_m: Option<f64>,
    pub altitude_m: Option<f64>,
    pub speed_ms: Option<f64>,
    pub bearing_deg: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sensor {
    Fuel {
        level_percent: i32,
        range_km: Option<i32>,
        low_fuel_warning: Option<bool>,
    },
    /// Millimetres per second.
    Speed(i32),
    /// Revolutions per minute times 1000.
    Rpm(i32),
    /// 0 neutral, 1 to 10 manual, 100 drive, 101 park, 102 reverse.
    Gear(i32),
    NightMode(bool),
    ParkingBrake(bool),
    /// Head light 1 off, 2 on, 3 high. Turn indicator 1 none, 2 left, 3 right.
    Light {
        head_light: Option<i32>,
        hazard_lights: Option<bool>,
        turn_indicator: Option<i32>,
    },
    /// Temperature in milli-degrees, pressure in pascal.
    Environment {
        temperature_e3: Option<i32>,
        pressure_e3: Option<i32>,
    },
    Odometer {
        total_km_e1: i32,
        trip_km_e1: Option<i32>,
    },
    /// The restriction bits, 0 unrestricted.
    DrivingStatus(i32),
    Gps(GpsFix),
    /// The phone's maps read the minimum usable capacity as the current level.
    VehicleEnergyModel {
        capacity_wh: i64,
        current_wh: i64,
        range_m: i64,
        max_charge_power_w: Option<i64>,
        max_discharge_power_w: Option<i64>,
        auxiliary_wh_per_km: Option<f64>,
    },
}

fn location(fix: GpsFix) -> Option<CarLocation> {
    let scaled = |v: f64| v.is_finite().then(|| round_half_up(v) as i32);
    let optional = |v: Option<f64>, scale: f64| match v {
        Some(v) => scaled(v * scale).map(Some),
        None => Some(None),
    };
    Some(CarLocation {
        latitude_deg_e7: scaled(fix.lat_deg * 1e7)?,
        longitude_deg_e7: scaled(fix.lng_deg * 1e7)?,
        accuracy_m_e3: optional(fix.accuracy_m, 1000.0)?.map(|a| a as u32),
        altitude_m_e2: optional(fix.altitude_m, 100.0)?,
        speed_m_per_s_e3: optional(fix.speed_ms, 1000.0)?,
        bearing_deg_e6: optional(fix.bearing_deg, 1e6)?,
    })
}

/// Google Maps reads this with its own energy model, wider than the one Android Auto knows and
/// passes on unread, so it is written by hand.
fn vehicle_energy_model(
    capacity_wh: i64,
    current_wh: i64,
    range_m: i64,
    max_charge_power_w: Option<i64>,
    max_discharge_power_w: Option<i64>,
    auxiliary_wh_per_km: Option<f64>,
) -> Option<Vec<u8>> {
    if capacity_wh <= 0 || current_wh <= 0 || range_m <= 0 {
        return None;
    }
    let energy = |wh: i64| field_varint(1, wh);
    let reserve = round_half_up(capacity_wh as f64 * 0.05) as i64;
    let battery = [
        field_varint(1, 1),
        field_len_delim(3, &energy(current_wh)),
        field_len_delim(4, &energy(capacity_wh)),
        field_len_delim(8, &energy(reserve)),
        field_varint(9, max_charge_power_w.unwrap_or(150_000)),
        field_varint(10, max_discharge_power_w.unwrap_or(150_000)),
        field_varint(11, 1),
    ]
    .concat();
    let wh_per_km = (current_wh as f64 / range_m as f64) * 1000.0;
    let aux = auxiliary_wh_per_km.unwrap_or(2.0);
    let consumption = [
        field_len_delim(1, &field_float(1, wh_per_km)),
        field_len_delim(2, &field_float(1, aux)),
        field_len_delim(3, &field_float(1, 0.36)),
    ]
    .concat();
    let charging_prefs = field_varint(3, 1);
    let model = [
        field_len_delim(1, &battery),
        field_len_delim(2, &consumption),
        field_len_delim(12, &charging_prefs),
    ]
    .concat();
    Some(field_len_delim(23, &model))
}

pub fn batch(sensor: &Sensor) -> Option<Vec<u8>> {
    let mut b = SensorBatchNotification::default();
    match *sensor {
        Sensor::Fuel { level_percent, range_km, low_fuel_warning } => {
            b.fuel_levels.push(FuelLevel {
                level_percent: Some(level_percent),
                range_km,
                energy_is_low: low_fuel_warning,
            })
        }
        Sensor::Speed(speed) => {
            b.speeds.push(VehicleSpeed { speed_m_per_s_e3: speed, ..Default::default() });
        }
        Sensor::Rpm(rpm_e3) => b.engine_rpms.push(EngineRpm { rpm_e3 }),
        Sensor::Gear(gear) => b.gears.push(CurrentGear { gear }),
        Sensor::NightMode(night) => {
            b.night_modes.push(NightMode { night_mode_active: Some(night) })
        }
        Sensor::ParkingBrake(engaged) => b.parking_brakes.push(ParkingBrake { engaged }),
        Sensor::Light { head_light, hazard_lights, turn_indicator } => {
            if head_light.is_none() && hazard_lights.is_none() && turn_indicator.is_none() {
                return None;
            }
            b.light_states.push(LightStates {
                headlight: head_light,
                turn_indicator,
                hazard_lights_on: hazard_lights,
            });
        }
        Sensor::Environment { temperature_e3, pressure_e3 } => {
            if temperature_e3.is_none() && pressure_e3.is_none() {
                return None;
            }
            b.environment_conditions.push(EnvironmentConditions {
                temperature_e3,
                pressure_e3,
                ..Default::default()
            });
        }
        Sensor::Odometer { total_km_e1, trip_km_e1 } => {
            b.odometers.push(Odometer { odometer_km_e1: total_km_e1, trip_km_e1 });
        }
        Sensor::DrivingStatus(restrictions) => {
            b.driving_statuses.push(DrivingStatus { restrictions })
        }
        Sensor::Gps(fix) => b.locations.push(location(fix)?),
        Sensor::VehicleEnergyModel {
            capacity_wh,
            current_wh,
            range_m,
            max_charge_power_w,
            max_discharge_power_w,
            auxiliary_wh_per_km,
        } => {
            return vehicle_energy_model(
                capacity_wh,
                current_wh,
                range_m,
                max_charge_power_w,
                max_discharge_power_w,
                auxiliary_wh_per_km,
            );
        }
    }
    Some(b.encode_to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_carry_the_sensor_type_as_field() {
        assert_eq!(batch(&Sensor::NightMode(true)), Some(vec![0x52, 0x02, 0x08, 0x01]));
        assert_eq!(batch(&Sensor::DrivingStatus(0)), Some(vec![0x6a, 0x02, 0x08, 0x00]));
        assert_eq!(
            batch(&Sensor::Fuel {
                level_percent: 50,
                range_km: Some(300),
                low_fuel_warning: Some(false)
            }),
            Some(vec![0x32, 0x07, 0x08, 0x32, 0x10, 0xac, 0x02, 0x18, 0x00])
        );
        assert_eq!(
            batch(&Sensor::Light { head_light: None, hazard_lights: None, turn_indicator: None }),
            None
        );
        assert_eq!(batch(&Sensor::Environment { temperature_e3: None, pressure_e3: None }), None);
        let gps = GpsFix {
            lat_deg: 48.1,
            lng_deg: f64::NAN,
            accuracy_m: None,
            altitude_m: None,
            speed_ms: None,
            bearing_deg: None,
        };
        assert_eq!(batch(&Sensor::Gps(gps)), None);
        let energy = Sensor::VehicleEnergyModel {
            capacity_wh: 0,
            current_wh: 1,
            range_m: 1,
            max_charge_power_w: None,
            max_discharge_power_w: None,
            auxiliary_wh_per_km: None,
        };
        assert_eq!(batch(&energy), None);
    }
}
