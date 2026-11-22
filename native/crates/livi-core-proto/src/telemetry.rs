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
//! A push may carry any subset of the fields. `gps`, `can`, `climate`, `tirePressure`, `doors`,
//! `seatBelts`, `telltales` and `notification` merge field by field, so `{ gps: { lat: 50 } }`
//! keeps the last `lng`, every other field is replaced whole. LIVI sends the merged snapshot to
//! every client as `telemetry:update`.
//!
//! Where each field goes, ✓ used, · not used, TODO the receiver takes it and LIVI does not send
//! it yet. LIVI holds every field in its store and hands it to every Socket.IO client.
//!
//! | Field                               | LIVI | Android Auto | CarPlay Classic | CarPlay Ultra |
//! |-------------------------------------|------|--------------|-----------------|---------------|
//! | speedKph                            | ✓    | ✓            | TODO            | ✓             |
//! | rpm                                 | ✓    | ✓            | ·               | ✓             |
//! | gear                                | ✓    | ✓            | ✓               | ✓             |
//! | reverse                             | ✓    | ✓ (gear)     | ✓ (gear)        | ✓ (gear)      |
//! | steeringDeg                         | ✓    | ·            | ·               | ·             |
//! | turnSignal                          | ✓    | ✓            | ·               | ✓             |
//! | lights, highBeam, hazards           | ✓    | ✓            | ·               | ✓ (hazards)   |
//! | parkingBrake                        | ✓    | ✓            | ·               | ·             |
//! | nightMode                           | ·    | ✓            | ✓               | ✓             |
//! | mainUiNightMode, mainMapNightMode   | ·    | ·            | ✓               | ✓             |
//! | clusterUiNightMode                  | ·    | ·            | ✓               | ✓             |
//! | clusterMapNightMode                 | ·    | ·            | ✓               | ✓             |
//! | appearanceSetting                   | ·    | ·            | ✓               | ✓             |
//! | darkMode                            | ✓    | ·            | ·               | ·             |
//! | path                                | ✓    | ·            | ·               | ·             |
//! | key                                 | ✓    | ✓            | ✓               | ✓             |
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
//! | coolantC                            | ✓    | ·            | ·               | ✓             |
//! | iatC, transmissionC                 | ✓    | ·            | ·               | ·             |
//! | oilC                                | ✓    | ·            | ·               | ·             |
//! | ambientC                            | ✓    | ✓            | ✓               | ✓             |
//! | baroKpa                             | ✓    | ✓            | ·               | ·             |
//! | mapKpa, boostKpa, throttlePct       | ✓    | ·            | ·               | ·             |
//! | lambda, afr                         | ✓    | ·            | ·               | ·             |
//! | batteryV                            | ✓    | ·            | ·               | ·             |
//! | ambientLux                          | ✓    | ·            | ·               | ·             |
//! | dimmerPct                           | ✓    | ·            | ·               | ·             |
//! | displayBrightness, displayGamma     | ✓    | ·            | ·               | ·             |
//! | displayContrast, displayColorR/G/B  | ✓    | ·            | ·               | ·             |
//! | odometerKm                          | ✓    | ✓            | ·               | ✓             |
//! | odometerTripKm                      | ✓    | ✓            | ·               | ·             |
//! | drivingStatus                       | ✓    | ✓            | ·               | ·             |
//! | gps                                 | ✓    | ✓            | ✓               | ✓             |
//! | tirePressure                        | ✓    | ✓            | ·               | ✓             |
//! | doors                               | ·    | ✓            | ·               | ✓             |
//! | seatBelts                           | ·    | ·            | ·               | ✓             |
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
    /// Values: e.g. 1795392000000
    pub ts: Option<u64>,
    /// Values: e.g. 73
    pub speed_kph: Option<f64>,
    /// Values: e.g. 2400
    pub rpm: Option<f64>,
    /// A number for a manual gearbox, -1 reverse, 0 neutral. A string for an automatic.
    /// Values: -1 to 10, P, R, N, D, S, M1 to M10
    pub gear: Option<Value>,
    /// Negative is left.
    /// Values: e.g. -90
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
    /// Values: e.g. 90
    pub coolant_c: Option<f64>,
    /// Values: e.g. 100
    pub oil_c: Option<f64>,
    /// Automatic transmission oil.
    /// Values: e.g. 80
    pub transmission_c: Option<f64>,
    /// Intake air.
    /// Values: e.g. 35
    pub iat_c: Option<f64>,
    /// Outside, Android Auto and CarPlay get it too.
    /// Values: e.g. 18
    pub ambient_c: Option<f64>,
    /// The 12 V battery.
    /// Values: e.g. 12.6, 14.2 with the engine running
    pub battery_v: Option<f64>,
    /// For an electric car the state of charge.
    /// Values: 0 to 100
    pub fuel_pct: Option<f64>,
    /// Values: e.g. 420
    pub range_km: Option<f64>,
    /// Hybrids only, the range on the tank.
    /// Values: e.g. 380
    pub range_fuel_km: Option<f64>,
    /// Hybrids only, the range on the battery.
    /// Values: e.g. 45
    pub range_electric_km: Option<f64>,
    /// Values: e.g. 6.5
    pub fuel_rate_lph: Option<f64>,
    /// Values: e.g. 7.2
    pub consumption_l_per100_km: Option<f64>,
    /// Values: e.g. 6.8
    pub consumption_avg_l_per100_km: Option<f64>,
    /// Gross capacity. Turns on electric routing in Android Auto, whatever `carType` says.
    /// Values: e.g. 77
    pub battery_capacity_kwh: Option<f64>,
    /// Taken from `fuelPct` and the capacity when absent.
    /// Values: e.g. 52
    pub battery_level_kwh: Option<f64>,
    /// True while the battery charges.
    pub charging: Option<bool>,
    /// The plug that charges.
    /// Values: EvConnectorType
    pub active_connector: Option<String>,
    /// The range on a full battery.
    /// Values: e.g. 450
    pub range_max_electric_km: Option<f64>,
    /// Written into the settings when it differs.
    /// Values: CarType
    pub car_type: Option<String>,
    /// Written into the settings when they differ.
    /// Values: EvConnectorType
    pub ev_connector_types: Option<Vec<String>>,
    /// The fastest AC charging the car takes, written into the settings when it differs.
    /// Values: e.g. 11
    pub ev_max_ac_power_kw: Option<f64>,
    /// The fastest DC charging the car takes, written into the settings when it differs.
    /// Values: e.g. 150
    pub ev_max_dc_power_kw: Option<f64>,
    /// Manifold absolute pressure.
    /// Values: e.g. 35 idling, 200 boosted
    pub map_kpa: Option<f64>,
    /// Android Auto also gets it on its environment channel.
    /// Values: e.g. 101.3
    pub baro_kpa: Option<f64>,
    /// Values: e.g. 80, 0 is ambient
    pub boost_kpa: Option<f64>,
    /// Throttle position.
    /// Values: 0 to 100
    pub throttle_pct: Option<f64>,
    /// 1.0 is stoichiometric.
    /// Values: e.g. 0.98
    pub lambda: Option<f64>,
    /// Air to fuel ratio.
    /// Values: e.g. 14.7
    pub afr: Option<f64>,
    /// Android Auto gets it in steps of 0.1 km.
    /// Values: e.g. 123456.7
    pub odometer_km: Option<f64>,
    /// Values: e.g. 312.4
    pub odometer_trip_km: Option<f64>,
    /// Android Auto's driving restrictions as they are.
    /// Values: bits 1 no video, 2 no keyboard, 4 no voice, 8 no setup, 16 short text
    pub driving_status: Option<u32>,
    /// Values: e.g. 20000 by day, 5 at night
    pub ambient_lux: Option<f64>,
    /// The dimmer of the panel lights.
    /// Values: 0 to 100
    pub dimmer_pct: Option<f64>,
    /// Values: 0 to 1
    pub display_brightness: Option<f64>,
    /// Values: 0.5 to 1.5, 1 is unchanged
    pub display_gamma: Option<f64>,
    /// Values: 0.5 to 1.5, 1 is unchanged
    pub display_contrast: Option<f64>,
    /// Values: 0.5 to 1.5, 1 is unchanged
    pub display_color_r: Option<f64>,
    /// Values: 0.5 to 1.5, 1 is unchanged
    pub display_color_g: Option<f64>,
    /// Values: 0.5 to 1.5, 1 is unchanged
    pub display_color_b: Option<f64>,
    /// Day or night for Android Auto and CarPlay, LIVI's own look follows `darkMode`. It sets the
    /// four below at once, those of them in the same push apply after it. LIVI's appearance
    /// setting at Day or Night sets it too. Any of the five turns that setting to Auto.
    pub night_mode: Option<bool>,
    /// Day or night for the UI on the main screen. Each of the four is saved as the setting of the
    /// same name and the phone starts with it. A map is never darker than its UI: a UI takes its
    /// map along, a dark map darkens its UI, and only a dark UI keeps a light map.
    pub main_ui_night_mode: Option<bool>,
    /// The map on the main screen, sent to the phone as its night mode.
    pub main_map_night_mode: Option<bool>,
    /// The UI on the cluster.
    pub cluster_ui_night_mode: Option<bool>,
    /// The map on the cluster.
    pub cluster_map_night_mode: Option<bool>,
    /// How carplay treats the four above, saved as the setting of the same name, `user` when never
    /// set.
    /// Values: AppearanceSetting
    pub appearance_setting: Option<String>,
    /// LIVI's own look, dark or light. A push sets the Dark Mode setting, a change of that
    /// setting shows up here.
    pub dark_mode: Option<bool>,
    /// A page such as `/media` or `/settings/devices`, LIVI's main screen opens it once each time
    /// it changes. A path that names no page is ignored. `/projection` brings the projection to the
    /// front on every push, and what follows asks the running CarPlay or Android Auto session to
    /// show that app: for CarPlay it is a `requestUI` URL (`/projection/maps`,
    /// `/projection/mobilephone`, `/projection/comgooglemaps://?daddr=…`), for Android Auto the
    /// known categories (maps, nowplaying, radio, mobilephone) map to its hard keys.
    /// Values: e.g. `/media`, `/settings/devices`, `/projection/maps`
    pub path: Option<String>,
    /// The name of a key binding such as `zoomIn`, `zoomOut`, `next` or `home`, pressed once on
    /// every push as if its key were pressed, bound to a key or not.
    /// Values: e.g. `down`, all names under Keys
    pub key: Option<String>,
    /// The head unit's level. It sets `huVolume`, which drives the amplifier and, while
    /// linked, the system mixer.
    /// Values: 0 to 1
    pub volume: Option<f64>,
    /// Send the whole fix every time or only what the sensor delivers.
    pub gps: Option<GpsFix>,
    /// Only LIVI's own receiver sends it, a push replaces it whole.
    /// Values: e.g. `{"connected": true, "fixQuality": "gps", "fixMode": "3d", "satellitesUsed": 9}`
    pub gnss: Option<GnssInfo>,
    pub tire_pressure: Option<TirePressure>,
    pub doors: Option<Doors>,
    pub seat_belts: Option<SeatBelts>,
    pub telltales: Option<Telltales>,
    pub dash_test: Option<bool>,
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
    /// Values: e.g. 240
    pub front_left_kpa: Option<f64>,
    /// Values: e.g. 240
    pub front_right_kpa: Option<f64>,
    /// Values: e.g. 240
    pub rear_left_kpa: Option<f64>,
    /// Values: e.g. 240
    pub rear_right_kpa: Option<f64>,
}

/// A sensor may push one closure at a time, a car without rear doors leaves them out.
/// Values: `true` open, `false` closed
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Doors {
    pub front_left: Option<bool>,
    pub front_right: Option<bool>,
    pub rear_left: Option<bool>,
    pub rear_right: Option<bool>,
    pub hood: Option<bool>,
    pub trunk: Option<bool>,
}

/// A seat the car does not know about is left out.
/// Values: `true` buckled, `false` an occupied seat without its belt
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeatBelts {
    pub front_left: Option<bool>,
    pub front_right: Option<bool>,
    pub rear_left: Option<bool>,
    pub rear_center: Option<bool>,
    pub rear_right: Option<bool>,
}

/// Values: `true` lit, `false` off
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Telltales {
    pub brake: Option<bool>,
    pub seat_belt: Option<bool>,
    pub airbag: Option<bool>,
    pub side_airbag: Option<bool>,
    pub oil_pressure: Option<bool>,
    pub coolant: Option<bool>,
    pub charging: Option<bool>,
    pub abs: Option<bool>,
    pub esc: Option<bool>,
    pub tyre_pressure: Option<bool>,
    pub engine: Option<bool>,
    pub brake_wear: Option<bool>,
    pub preheat: Option<bool>,
    pub choke: Option<bool>,
    pub rear_fog: Option<bool>,
    pub passenger_airbag_off: Option<bool>,
    pub windscreen_defrost: Option<bool>,
    pub rear_defrost: Option<bool>,
    pub position: Option<bool>,
    pub parking_lamps: Option<bool>,
    pub front_fog: Option<bool>,
    pub auto_high_beam: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CanFrame {
    /// Values: e.g. 1001
    pub id: u32,
    /// Values: e.g. [0, 255, 18, 4]
    pub data: Vec<u8>,
    /// Values: e.g. 0
    pub bus: Option<u8>,
}

/// The car's climate as CarPlay shows it. Without a running session an outside source keeps it
/// current, a new session starts from it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Climate {
    /// Who set it last: `carplay` the phone, `external` a push that names no source, `saved` the
    /// last run, `default` LIVI's start values. Anything but `carplay` reaches the phone.
    /// Values: source of last change: `carplay`, `external`, `saved`, `default`
    pub source: Option<String>,
    pub on: Option<bool>,
    pub auto: Option<bool>,
    pub ac: Option<bool>,
    pub max_ac: Option<bool>,
    pub max_defrost: Option<bool>,
    pub recirculation: Option<bool>,
    /// The passenger side follows the driver's temperature.
    pub zones_synced: Option<bool>,
    /// The ends show as LO and HI.
    /// Values: 16 to 28 in steps of 0.5
    pub driver_target_c: Option<f64>,
    /// Values: 16 to 28 in steps of 0.5
    pub passenger_target_c: Option<f64>,
    /// Measured by the car.
    /// Values: e.g. 21.5
    pub driver_cabin_c: Option<f64>,
    /// Values: e.g. 21.5
    pub passenger_cabin_c: Option<f64>,
    /// 0 is off.
    /// Values: 0 to 7
    pub fan_level: Option<u8>,
    pub fan_auto: Option<bool>,
    /// Values: 0 or 1
    pub front_window_heating: Option<u8>,
    /// Values: 0 or 1
    pub rear_window_heating: Option<u8>,
    pub vent_upper: Option<bool>,
    pub vent_middle: Option<bool>,
    pub vent_lower: Option<bool>,
    /// AUTO picks the vents.
    pub vents_auto: Option<bool>,
    /// Values: 0 to 2
    pub driver_seat_heating: Option<u8>,
    /// Values: 0 to 2
    pub passenger_seat_heating: Option<u8>,
}

/// A CarPlay notification. It lands in the notification center, and with
/// a matching theme installed also in the dashboard.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Notification {
    /// Values: e.g. Ice warning
    pub title: Option<String>,
    /// Values: e.g. Outside 2 °C
    pub text: Option<String>,
    /// An SF Symbol such as `snowflake`, or the name of a PNG in `~/.config/LIVI/carplay-images`
    /// such as `livi`. A PNG shows only once the phone fetched the folder, from the next CarPlay
    /// start and in practice only with a theme, else just SF Symbols work.
    /// Values: e.g. `snowflake`
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
            doors: Some(Doors {
                rear_left: Some(true),
                ..Default::default()
            }),
            seat_belts: Some(SeatBelts {
                rear_center: Some(false),
                ..Default::default()
            }),
            climate: Some(Climate {
                driver_target_c: Some(21.5),
                ..Default::default()
            }),
            dash_test: Some(true),
            telltales: Some(Telltales {
                passenger_airbag_off: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let named = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(named["consumptionLPer100Km"], 6.5);
        assert_eq!(named["iatC"], 30.0);
        assert_eq!(named["turnSignal"], "left");
        assert_eq!(named["tirePressure"]["rearLeftKpa"], 230.0);
        assert_eq!(named["doors"]["rearLeft"], true);
        assert_eq!(named["seatBelts"]["rearCenter"], false);
        assert_eq!(
            (&named["can"]["id"], &named["can"]["data"]),
            (&json!(1), &json!([2]))
        );
        assert_eq!(named["climate"]["driverTargetC"], 21.5);
        assert_eq!(named["telltales"]["passengerAirbagOff"], true);
        assert_eq!(named["dashTest"], true);
        let unknown = serde_json::from_value::<Climate>(json!({ "seatCooling": 1 }));
        assert!(
            unknown.is_err(),
            "a climate key the contract does not name is refused"
        );
    }
}
