// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

//! What the car and programs outside LIVI push into its telemetry, merged into one snapshot.
//!
//! Push over Socket.IO to `ws://<livi-host>:4000`, event `telemetry:push`, for example
//! `io('ws://livi.local:4000').emit('telemetry:push', { speedKph: 73 })`, or as plain HTTP,
//! `curl -d '{"speedKph":73}' http://livi.local:4000/telemetry`. From a shell LIVI's own core
//! does it, `livi-core telemetry set speedKph=73 gps.lat=53.59`, `livi-core telemetry help`
//! lists the rest.
//!
//! A push may carry any subset of the fields. `gps`, `can`, `climate`, `tirePressure` and
//! `notification` merge
//! field by field, so `{ gps: { lat: 50 } }` keeps the last `lng`, every other field is replaced
//! whole. LIVI sends the merged snapshot to every client as `telemetry:update`.
//!
//! Where each field goes, ✓ used, · not used, TODO the receiver takes it and LIVI does not send
//! it yet. LIVI holds every field in its store and hands it to every Socket.IO client.
//!
//! | Field                               | LIVI | Android Auto | CarPlay Classic | CarPlay Ultra |
//! |-------------------------------------|------|--------------|-----------------|---------------|
//! | speedKph                            | ✓    | ✓            | TODO            | TODO          |
//! | rpm                                 | ✓    | ✓            | ·               | ✓             |
//! | gear                                | ✓    | ✓            | ✓               | ✓             |
//! | reverse                             | ✓    | ✓ (gear)     | ✓ (gear)        | ✓ (gear)      |
//! | steeringDeg                         | ✓    | ·            | ·               | ·             |
//! | turnSignal                          | ✓    | ✓            | ·               | TODO          |
//! | lights, highBeam, hazards           | ✓    | ✓            | ·               | ·             |
//! | parkingBrake                        | ✓    | ✓            | ·               | ·             |
//! | nightMode                           | ·    | ✓            | ✓               | ✓             |
//! | clusterNightMode                    | ·    | ·            | ✓               | ✓             |
//! | darkMode                            | ✓    | ·            | ·               | ·             |
//! | path                                | ✓    | ·            | ·               | ·             |
//! | volume                              | ✓    | ·            | ·               | ✓             |
//! | fuelPct                             | ✓    | ✓            | ✓ (warning, %)  | ✓             |
//! | rangeKm                             | ✓    | ✓            | ✓               | ✓             |
//! | rangeFuelKm, rangeElectricKm        | ✓    | ·            | ✓ (hybrid)      | ✓ (hybrid)    |
//! | fuelRateLph                         | ✓    | ·            | ·               | ·             |
//! | consumption*                        | ✓    | ·            | ·               | ✓             |
//! | batteryCapacityKwh, batteryLevelKwh | ✓    | ✓            | ✓ (electric)    | ✓ (electric)  |
//! | charging, activeConnector           | ✓    | ·            | ✓ (electric)    | ✓ (electric)  |
//! | rangeMaxElectricKm                  | ✓    | ·            | ✓ (electric)    | ✓ (electric)  |
//! | carType                             | ✓    | ✓            | ✓               | ✓             |
//! | evConnectorTypes                    | ✓    | ✓            | ✓               | ✓             |
//! | evMaxAcPowerKw, evMaxDcPowerKw      | ✓    | ·            | ✓               | ✓             |
//! | coolantC                            | ✓    | ·            | ·               | TODO          |
//! | iatC, transmissionC                 | ✓    | ·            | ·               | ·             |
//! | oilC                                | ✓    | ·            | ·               | ·             |
//! | ambientC                            | ✓    | ✓            | ✓               | ✓             |
//! | baroKpa                             | ✓    | ✓            | ·               | ·             |
//! | mapKpa, boostKpa, throttlePct       | ✓    | ·            | ·               | ·             |
//! | lambda, afr                         | ✓    | ·            | ·               | ·             |
//! | batteryV                            | ✓    | ·            | ·               | ·             |
//! | ambientLux                          | ✓    | ·            | ·               | ·             |
//! | dimmerPct                           | ✓    | ·            | ·               | ·             |
//! | odometerKm                          | ✓    | ✓            | ·               | ✓             |
//! | odometerTripKm                      | ✓    | ✓            | ·               | ·             |
//! | drivingStatus                       | ✓    | ✓            | ·               | ·             |
//! | gps                                 | ✓    | ✓            | ✓               | ✓             |
//! | tirePressure                        | ✓    | ✓            | ·               | ✓             |
//! | can                                 | ✓    | ·            | ·               | ·             |
//! | gnss                                | ✓    | ·            | ·               | ·             |
//! | climate                             | ✓    | ·            | ·               | ✓             |
//! | notification                        | ·    | ·            | ·               | ✓             |
//!
//! With `autoSwitchOnReverse` on, `gear` and `reverse` also bring the camera to the front of the
//! main screen while the car reverses.
//!
//! A new field gets its unit in its name and in its doc comment here, its receiver in core
//! (`cp_telemetry.rs` for CarPlay, `aa_telemetry.rs` for Android Auto) and a line above.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::gnss::{GnssInfo, GpsFix};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetrySnapshot {
    /// Unix milliseconds, LIVI fills it in when a push has none.
    pub ts: Option<u64>,
    pub speed_kph: Option<f64>,
    pub rpm: Option<f64>,
    /// A number for a manual gearbox, -1 reverse, 0 neutral, 1 to 10. A string for an automatic,
    /// P, R, N, D, S or M1 to M10.
    pub gear: Option<Value>,
    /// Negative is left.
    pub steering_deg: Option<f64>,
    /// Wins over `gear`.
    pub reverse: Option<bool>,
    /// Low beam.
    pub lights: Option<bool>,
    /// Android Auto shows high beam whatever `lights` says.
    pub high_beam: Option<bool>,
    pub hazards: Option<bool>,
    /// Taken as `turn` too, the name it had before.
    pub turn_signal: Option<TurnSignal>,
    pub parking_brake: Option<bool>,
    pub coolant_c: Option<f64>,
    pub oil_c: Option<f64>,
    /// Automatic transmission oil.
    pub transmission_c: Option<f64>,
    /// Intake air.
    pub iat_c: Option<f64>,
    /// Outside, Android Auto and CarPlay get it too.
    pub ambient_c: Option<f64>,
    /// The 12 V battery.
    pub battery_v: Option<f64>,
    /// 0 to 100, for an electric car the state of charge.
    pub fuel_pct: Option<f64>,
    pub range_km: Option<f64>,
    /// Hybrids only, the range on the tank.
    pub range_fuel_km: Option<f64>,
    /// Hybrids only, the range on the battery.
    pub range_electric_km: Option<f64>,
    pub fuel_rate_lph: Option<f64>,
    pub consumption_l_per100_km: Option<f64>,
    pub consumption_avg_l_per100_km: Option<f64>,
    /// Gross capacity. Turns on electric routing in Android Auto, whatever `carType` says.
    pub battery_capacity_kwh: Option<f64>,
    /// Taken from `fuelPct` and the capacity when absent.
    pub battery_level_kwh: Option<f64>,
    /// True while the battery charges.
    pub charging: Option<bool>,
    /// The `EvConnectorType` name of the plug that charges, such as `Combo2`.
    pub active_connector: Option<String>,
    /// The range on a full battery.
    pub range_max_electric_km: Option<f64>,
    /// A `CarType` name such as `HybridDiesel`, written into the settings when it differs.
    pub car_type: Option<String>,
    /// `EvConnectorType` names such as `Combo2`, written into the settings when they differ.
    pub ev_connector_types: Option<Vec<String>>,
    /// The fastest AC charging the car takes, written into the settings when it differs.
    pub ev_max_ac_power_kw: Option<f64>,
    /// The fastest DC charging the car takes, written into the settings when it differs.
    pub ev_max_dc_power_kw: Option<f64>,
    /// Manifold absolute pressure.
    pub map_kpa: Option<f64>,
    /// Android Auto also gets it on its environment channel.
    pub baro_kpa: Option<f64>,
    pub boost_kpa: Option<f64>,
    /// Throttle position, 0 to 100.
    pub throttle_pct: Option<f64>,
    /// 1.0 is stoichiometric.
    pub lambda: Option<f64>,
    /// Air to fuel ratio.
    pub afr: Option<f64>,
    /// Android Auto gets it in steps of 0.1 km.
    pub odometer_km: Option<f64>,
    pub odometer_trip_km: Option<f64>,
    /// Android Auto's driving restrictions as they are, 0 is none.
    pub driving_status: Option<u32>,
    pub ambient_lux: Option<f64>,
    /// The dimmer of the panel lights, 0 to 100.
    pub dimmer_pct: Option<f64>,
    /// Day or night for Android Auto and CarPlay, LIVI's own look follows `darkMode`.
    pub night_mode: Option<bool>,
    /// Day or night for the CarPlay cluster, it follows `nightMode` while absent or null. A
    /// change of LIVI's appearance setting sets `nightMode` and clears this one.
    pub cluster_night_mode: Option<bool>,
    /// LIVI's own look, dark or light. A push sets the Dark Mode setting, a change of that
    /// setting shows up here.
    pub dark_mode: Option<bool>,
    /// A page such as `/media` or `/settings/devices`, LIVI's main screen opens it once each time
    /// it changes. A path that names no page is ignored. `/projection` brings the projection to the
    /// front on every push, and what follows asks the running CarPlay or Android Auto session to
    /// show that app: for CarPlay it is a `requestUI` URL (`/projection/maps`,
    /// `/projection/mobilephone`, `/projection/comgooglemaps://?daddr=…`), for Android Auto the
    /// known categories (maps, nowplaying, radio, mobilephone) map to its hard keys.
    pub path: Option<String>,
    /// The head unit's level, 0 to 1. It sets `huVolume`, which drives the amplifier and, while
    /// linked, the system mixer.
    pub volume: Option<f64>,
    /// Send the whole fix every time or only what the sensor delivers.
    pub gps: Option<GpsFix>,
    /// Only LIVI's own receiver sends it, a push replaces it whole.
    pub gnss: Option<GnssInfo>,
    pub tire_pressure: Option<TirePressure>,
    /// No receiver uses it, it is for tooling and diagnostics.
    pub can: Option<CanFrame>,
    pub climate: Option<Climate>,
    pub notification: Option<Notification>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TurnSignal {
    None,
    Left,
    Right,
}

/// A sensor may push one wheel at a time.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TirePressure {
    pub front_left_kpa: Option<f64>,
    pub front_right_kpa: Option<f64>,
    pub rear_left_kpa: Option<f64>,
    pub rear_right_kpa: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CanFrame {
    pub id: u32,
    pub data: Vec<u8>,
    pub bus: Option<u8>,
}

/// The car's climate as CarPlay shows it. Without a running session an outside source keeps it
/// current, a new session starts from it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Climate {
    /// Who set it last: `carplay` the phone, `external` a push that names no source, `saved` the
    /// last run, `default` LIVI's start values. Anything but `carplay` reaches the phone.
    pub source: Option<String>,
    pub on: Option<bool>,
    pub auto: Option<bool>,
    pub ac: Option<bool>,
    pub max_ac: Option<bool>,
    pub max_defrost: Option<bool>,
    pub recirculation: Option<bool>,
    /// The passenger side follows the driver's temperature.
    pub zones_synced: Option<bool>,
    /// 16 to 28 in steps of 0.5, the ends show as LO and HI.
    pub driver_target_c: Option<f64>,
    pub passenger_target_c: Option<f64>,
    /// Measured by the car.
    pub driver_cabin_c: Option<f64>,
    pub passenger_cabin_c: Option<f64>,
    /// 0 to 7, 0 is off.
    pub fan_level: Option<u8>,
    pub fan_auto: Option<bool>,
    /// 0 or 1.
    pub front_window_heating: Option<u8>,
    pub rear_window_heating: Option<u8>,
    pub vent_upper: Option<bool>,
    pub vent_middle: Option<bool>,
    pub vent_lower: Option<bool>,
    /// AUTO picks the vents.
    pub vents_auto: Option<bool>,
    /// 0 to 2.
    pub driver_seat_heating: Option<u8>,
    pub passenger_seat_heating: Option<u8>,
}

/// A CarPlay notification. It lands in the notification center, and with
/// a matching theme installed also in the dashboard.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Notification {
    pub title: Option<String>,
    pub text: Option<String>,
    /// An SF Symbol such as `snowflake`, or the name of a PNG in `~/.config/LIVI/carplay-images`
    /// such as `livi`. A PNG shows only once the phone fetched the folder, from the next CarPlay
    /// start and in practice only with a theme, else just SF Symbols work.
    pub symbol: Option<String>,
    /// True shows it, false closes it.
    pub open: Option<bool>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn fields_keep_the_names_producers_send() {
        let snapshot = TelemetrySnapshot {
            consumption_l_per100_km: Some(6.5),
            iat_c: Some(30.0),
            turn_signal: Some(TurnSignal::Left),
            can: Some(CanFrame {
                id: 1,
                data: vec![2],
                bus: None,
            }),
            tire_pressure: Some(TirePressure {
                rear_left_kpa: Some(230.0),
                ..Default::default()
            }),
            climate: Some(Climate {
                driver_target_c: Some(21.5),
                ..Default::default()
            }),
            ..Default::default()
        };
        let named = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(named["consumptionLPer100Km"], 6.5);
        assert_eq!(named["iatC"], 30.0);
        assert_eq!(named["turnSignal"], "left");
        assert_eq!(named["tirePressure"]["rearLeftKpa"], 230.0);
        assert_eq!(
            (&named["can"]["id"], &named["can"]["data"]),
            (&json!(1), &json!([2]))
        );
        assert_eq!(named["climate"]["driverTargetC"], 21.5);
        let unknown = serde_json::from_value::<Climate>(json!({ "seatCooling": 1 }));
        assert!(
            unknown.is_err(),
            "a climate key the contract does not name is refused"
        );
    }
}
