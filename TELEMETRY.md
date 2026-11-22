# Telemetry

A push carries any subset of the fields below, a dot names a field inside a group:

- Socket.IO: `io('ws://livi.local:4000').emit('telemetry:push', { speedKph: 73 })`
- HTTP: `curl -d '{"speedKph":73}' http://livi.local:4000/telemetry`
- Shell: `livi-core telemetry set speedKph=73 gps.lat=53.59`

Which receiver takes which field is listed in [telemetry.rs](native/crates/livi-core-proto/src/telemetry.rs).

| Field                          | Type             | Unit     | Values                                                                                                                                           |
| ------------------------------ | ---------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `ts`                           | number           | Unix ms  | e.g. 1795392000000                                                                                                                               |
| `speedKph`                     | number           | km/h     | e.g. 73                                                                                                                                          |
| `rpm`                          | number           | 1/min    | e.g. 2400                                                                                                                                        |
| `gear`                         | number or string |          | -1 to 10, P, R, N, D, S, M1 to M10                                                                                                               |
| `steeringDeg`                  | number           | °        | e.g. -90                                                                                                                                         |
| `reverse`                      | bool             |          | `true`, `false`                                                                                                                                  |
| `lights`                       | bool             |          | `true`, `false`                                                                                                                                  |
| `highBeam`                     | bool             |          | `true`, `false`                                                                                                                                  |
| `hazards`                      | bool             |          | `true`, `false`                                                                                                                                  |
| `turnSignal`                   | string           |          | `none`, `left`, `right`                                                                                                                          |
| `parkingBrake`                 | bool             |          | `true`, `false`                                                                                                                                  |
| `coolantC`                     | number           | °C       | e.g. 90                                                                                                                                          |
| `oilC`                         | number           | °C       | e.g. 100                                                                                                                                         |
| `transmissionC`                | number           | °C       | e.g. 80                                                                                                                                          |
| `iatC`                         | number           | °C       | e.g. 35                                                                                                                                          |
| `ambientC`                     | number           | °C       | e.g. 18                                                                                                                                          |
| `batteryV`                     | number           | V        | e.g. 12.6, 14.2 with the engine running                                                                                                          |
| `fuelPct`                      | number           | %        | 0 to 100                                                                                                                                         |
| `rangeKm`                      | number           | km       | e.g. 420                                                                                                                                         |
| `rangeFuelKm`                  | number           | km       | e.g. 380                                                                                                                                         |
| `rangeElectricKm`              | number           | km       | e.g. 45                                                                                                                                          |
| `fuelRateLph`                  | number           | l/h      | e.g. 6.5                                                                                                                                         |
| `consumptionLPer100Km`         | number           | l/100 km | e.g. 7.2                                                                                                                                         |
| `consumptionAvgLPer100Km`      | number           | l/100 km | e.g. 6.8                                                                                                                                         |
| `batteryCapacityKwh`           | number           | kWh      | e.g. 77                                                                                                                                          |
| `batteryLevelKwh`              | number           | kWh      | e.g. 52                                                                                                                                          |
| `charging`                     | bool             |          | `true`, `false`                                                                                                                                  |
| `activeConnector`              | string           |          | `J1772`, `Mennekes`, `Chademo`, `Combo1`, `Combo2`, `TeslaSupercharger`, `Gbt`, `Other`                                                          |
| `rangeMaxElectricKm`           | number           | km       | e.g. 450                                                                                                                                         |
| `carType`                      | string           |          | `Gasoline`, `DieselWinter`, `Diesel`, `Biodiesel`, `E85`, `LPG`, `CNG`, `LNG`, `Electric`, `Hydrogen`, `Other`, `HybridGasoline`, `HybridDiesel` |
| `evConnectorTypes`             | list of strings  |          | `J1772`, `Mennekes`, `Chademo`, `Combo1`, `Combo2`, `TeslaSupercharger`, `Gbt`, `Other`                                                          |
| `evMaxAcPowerKw`               | number           | kW       | e.g. 11                                                                                                                                          |
| `evMaxDcPowerKw`               | number           | kW       | e.g. 150                                                                                                                                         |
| `mapKpa`                       | number           | kPa      | e.g. 35 idling, 200 boosted                                                                                                                      |
| `baroKpa`                      | number           | kPa      | e.g. 101.3                                                                                                                                       |
| `boostKpa`                     | number           | kPa      | e.g. 80, 0 is ambient                                                                                                                            |
| `throttlePct`                  | number           | %        | 0 to 100                                                                                                                                         |
| `lambda`                       | number           |          | e.g. 0.98                                                                                                                                        |
| `afr`                          | number           |          | e.g. 14.7                                                                                                                                        |
| `odometerKm`                   | number           | km       | e.g. 123456.7                                                                                                                                    |
| `odometerTripKm`               | number           | km       | e.g. 312.4                                                                                                                                       |
| `drivingStatus`                | number           |          | bits 1 no video, 2 no keyboard, 4 no voice, 8 no setup, 16 short text                                                                            |
| `ambientLux`                   | number           | lx       | e.g. 20000 by day, 5 at night                                                                                                                    |
| `dimmerPct`                    | number           | %        | 0 to 100                                                                                                                                         |
| `displayBrightness`            | number           |          | 0 to 1                                                                                                                                           |
| `displayGamma`                 | number           |          | 0.5 to 1.5, 1 is unchanged                                                                                                                       |
| `displayContrast`              | number           |          | 0.5 to 1.5, 1 is unchanged                                                                                                                       |
| `displayColorR`                | number           |          | 0.5 to 1.5, 1 is unchanged                                                                                                                       |
| `displayColorG`                | number           |          | 0.5 to 1.5, 1 is unchanged                                                                                                                       |
| `displayColorB`                | number           |          | 0.5 to 1.5, 1 is unchanged                                                                                                                       |
| `nightMode`                    | bool             |          | `true`, `false`                                                                                                                                  |
| `mainUiNightMode`              | bool             |          | `true`, `false`                                                                                                                                  |
| `mainMapNightMode`             | bool             |          | `true`, `false`                                                                                                                                  |
| `clusterUiNightMode`           | bool             |          | `true`, `false`                                                                                                                                  |
| `clusterMapNightMode`          | bool             |          | `true`, `false`                                                                                                                                  |
| `appearanceSetting`            | string           |          | `auto`, `user`, `always`                                                                                                                         |
| `darkMode`                     | bool             |          | `true`, `false`                                                                                                                                  |
| `path`                         | string           |          | e.g. `/media`, `/settings/devices`, `/projection/maps`                                                                                           |
| `key`                          | string           |          | e.g. `down`, all names under Keys                                                                                                                |
| `volume`                       | number           |          | 0 to 1                                                                                                                                           |
| `gps.lat`                      | number           | °        | -90 to 90                                                                                                                                        |
| `gps.lng`                      | number           | °        | -180 to 180                                                                                                                                      |
| `gps.alt`                      | number           | m        | e.g. 34                                                                                                                                          |
| `gps.heading`                  | number           | °        | 0 to 360 clockwise from north                                                                                                                    |
| `gps.speedMs`                  | number           | m/s      | e.g. 20.3                                                                                                                                        |
| `gps.accuracyM`                | number           | m        | e.g. 5                                                                                                                                           |
| `gps.satellites`               | number           |          | e.g. 9                                                                                                                                           |
| `gps.fixTs`                    | number           | Unix ms  | e.g. 1795392000000                                                                                                                               |
| `gnss`                         | object           |          | e.g. `{"connected": true, "fixQuality": "gps", "fixMode": "3d", "satellitesUsed": 9}`                                                            |
| `tirePressure.frontLeftKpa`    | number           | kPa      | e.g. 240                                                                                                                                         |
| `tirePressure.frontRightKpa`   | number           | kPa      | e.g. 240                                                                                                                                         |
| `tirePressure.rearLeftKpa`     | number           | kPa      | e.g. 240                                                                                                                                         |
| `tirePressure.rearRightKpa`    | number           | kPa      | e.g. 240                                                                                                                                         |
| `doors.frontLeft`              | bool             |          | `true` open, `false` closed                                                                                                                      |
| `doors.frontRight`             | bool             |          | `true` open, `false` closed                                                                                                                      |
| `doors.rearLeft`               | bool             |          | `true` open, `false` closed                                                                                                                      |
| `doors.rearRight`              | bool             |          | `true` open, `false` closed                                                                                                                      |
| `doors.hood`                   | bool             |          | `true` open, `false` closed                                                                                                                      |
| `doors.trunk`                  | bool             |          | `true` open, `false` closed                                                                                                                      |
| `seatBelts.frontLeft`          | bool             |          | `true` buckled, `false` an occupied seat without its belt                                                                                        |
| `seatBelts.frontRight`         | bool             |          | `true` buckled, `false` an occupied seat without its belt                                                                                        |
| `seatBelts.rearLeft`           | bool             |          | `true` buckled, `false` an occupied seat without its belt                                                                                        |
| `seatBelts.rearCenter`         | bool             |          | `true` buckled, `false` an occupied seat without its belt                                                                                        |
| `seatBelts.rearRight`          | bool             |          | `true` buckled, `false` an occupied seat without its belt                                                                                        |
| `telltales.brake`              | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.seatBelt`           | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.airbag`             | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.sideAirbag`         | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.oilPressure`        | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.coolant`            | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.charging`           | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.abs`                | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.esc`                | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.tyrePressure`       | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.engine`             | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.brakeWear`          | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.preheat`            | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.choke`              | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.rearFog`            | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.passengerAirbagOff` | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.windscreenDefrost`  | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.rearDefrost`        | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.position`           | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.parkingLamps`       | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.frontFog`           | bool             |          | `true` lit, `false` off                                                                                                                          |
| `telltales.autoHighBeam`       | bool             |          | `true` lit, `false` off                                                                                                                          |
| `dashTest`                     | bool             |          | `true`, `false`                                                                                                                                  |
| `can.id`                       | number           |          | e.g. 1001                                                                                                                                        |
| `can.data`                     | list of numbers  |          | e.g. [0, 255, 18, 4]                                                                                                                             |
| `can.bus`                      | number           |          | e.g. 0                                                                                                                                           |
| `climate.source`               | string           |          | source of last change: `carplay`, `external`, `saved`, `default`                                                                                 |
| `climate.on`                   | bool             |          | `true`, `false`                                                                                                                                  |
| `climate.auto`                 | bool             |          | `true`, `false`                                                                                                                                  |
| `climate.ac`                   | bool             |          | `true`, `false`                                                                                                                                  |
| `climate.maxAc`                | bool             |          | `true`, `false`                                                                                                                                  |
| `climate.maxDefrost`           | bool             |          | `true`, `false`                                                                                                                                  |
| `climate.recirculation`        | bool             |          | `true`, `false`                                                                                                                                  |
| `climate.zonesSynced`          | bool             |          | `true`, `false`                                                                                                                                  |
| `climate.driverTargetC`        | number           | °C       | 16 to 28 in steps of 0.5                                                                                                                         |
| `climate.passengerTargetC`     | number           | °C       | 16 to 28 in steps of 0.5                                                                                                                         |
| `climate.driverCabinC`         | number           | °C       | e.g. 21.5                                                                                                                                        |
| `climate.passengerCabinC`      | number           | °C       | e.g. 21.5                                                                                                                                        |
| `climate.fanLevel`             | number           |          | 0 to 7                                                                                                                                           |
| `climate.fanAuto`              | bool             |          | `true`, `false`                                                                                                                                  |
| `climate.frontWindowHeating`   | number           |          | 0 or 1                                                                                                                                           |
| `climate.rearWindowHeating`    | number           |          | 0 or 1                                                                                                                                           |
| `climate.ventUpper`            | bool             |          | `true`, `false`                                                                                                                                  |
| `climate.ventMiddle`           | bool             |          | `true`, `false`                                                                                                                                  |
| `climate.ventLower`            | bool             |          | `true`, `false`                                                                                                                                  |
| `climate.ventsAuto`            | bool             |          | `true`, `false`                                                                                                                                  |
| `climate.driverSeatHeating`    | number           |          | 0 to 2                                                                                                                                           |
| `climate.passengerSeatHeating` | number           |          | 0 to 2                                                                                                                                           |
| `notification.title`           | string           |          | e.g. Ice warning                                                                                                                                 |
| `notification.text`            | string           |          | e.g. Outside 2 °C                                                                                                                                |
| `notification.symbol`          | string           |          | e.g. `snowflake`                                                                                                                                 |
| `notification.open`            | bool             |          | `true`, `false`                                                                                                                                  |

## Keys

`key` takes the name of a key binding, case sensitive. While carplay or android auto is in front the key goes to the phone, while LIVI is in front the navigation keys move LIVI's UI.

- `up`
- `down`
- `left`
- `right`
- `selectUp`
- `selectDown`
- `back`
- `knobLeft`
- `knobRight`
- `knobUp`
- `knobDown`
- `home`
- `cycleSession`
- `clusterLayout`
- `clusterContent`
- `zoomIn`
- `zoomOut`
- `playPause`
- `play`
- `pause`
- `next`
- `prev`
- `acceptPhone`
- `rejectPhone`
- `phoneKey0`
- `phoneKey1`
- `phoneKey2`
- `phoneKey3`
- `phoneKey4`
- `phoneKey5`
- `phoneKey6`
- `phoneKey7`
- `phoneKey8`
- `phoneKey9`
- `phoneKeyStar`
- `phoneKeyHash`
- `phoneKeyHookSwitch`
- `voiceAssistant`
- `voiceAssistantRelease`

---

Written from `native/crates/livi-core-proto`, `LIVI_WRITE_DOCS=1 cargo test -p livi-core-proto`
