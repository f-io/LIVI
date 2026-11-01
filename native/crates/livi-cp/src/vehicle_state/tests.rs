// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

fn hex(s: &str) -> Vec<u8> {
    let s: std::string::String = s.split_whitespace().collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn opened(answer: &[u8]) -> Value {
    let outer = opack::decode(answer).unwrap();
    assert_eq!(
        outer.get("pluginID").and_then(Value::as_int),
        Some(PLUGIN_ID)
    );
    opack::decode(outer.get("pluginData").and_then(Value::as_data).unwrap()).unwrap()
}

fn by_iid(entries: &[(i64, Value)]) -> Value {
    Value::Dict(
        entries
            .iter()
            .map(|(k, v)| (Value::Int(*k), v.clone()))
            .collect(),
    )
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
    let (key, info) = VehicleState::new(false, "9.5.0", "2026-10-15 14:30").info_entry();
    assert_eq!(key, "vehicleStateProtocolInfo");
    assert_eq!(
        info.get("protocolVersion").and_then(bplist::Value::as_str),
        Some("1.0")
    );
    let mapping = info.get("pluginMapping").and_then(|m| m.get(PLUGIN));
    assert_eq!(mapping.and_then(bplist::Value::as_int), Some(0));
    let plugin = &info
        .get("pluginConfigs")
        .and_then(bplist::Value::as_array)
        .unwrap()[0];
    let accessory = &plugin
        .get("accessories")
        .and_then(bplist::Value::as_array)
        .unwrap()[0];
    assert_eq!(
        accessory.get("type").and_then(bplist::Value::as_int),
        Some(0x0E00_0002)
    );
    let services = accessory
        .get("services")
        .and_then(bplist::Value::as_array)
        .unwrap();
    let characteristics: Vec<&bplist::Value> = services
        .iter()
        .flat_map(|s| {
            s.get("characteristics")
                .and_then(bplist::Value::as_array)
                .unwrap()
        })
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
    assert_eq!(
        brand.get("initialValue").and_then(bplist::Value::as_str),
        Some("LIVI")
    );
}

#[test]
fn the_phone_reads_follows_and_writes() {
    let mut state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30");
    let ask = hex(
        "e248706c7567696e4944084a706c7567696e44617461912ee247636f6d6d616e644d636f6e666967
         526571756573744d7472616e73616374696f6e494433b960c362b52ed4f2",
    );
    let answers = state.handle(Lane::High, &ask);
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].0, Lane::High);
    let description = opened(&answers[0].1);
    assert_eq!(
        description.get("command").and_then(Value::as_str),
        Some("configResponse")
    );
    assert_eq!(
        description.get("transactionID").and_then(Value::as_int),
        Some(-949_082_264_887_402_311)
    );
    let accessories = description.get("values").and_then(|v| v.get("accessories"));
    assert_eq!(accessories, Some(&state.accessory_list()));

    let iids = [262, 264, 273, 999].map(Value::Int).to_vec();
    let answers = state.handle(
        Lane::Normal,
        &command("registerRequest", Value::Array(iids)),
    );
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].0, Lane::Normal);
    let followed = opened(&answers[0].1);
    assert_eq!(
        followed.get("command").and_then(Value::as_str),
        Some("registerResponse")
    );
    let held = by_iid(&[(264, Value::Bool(true)), (273, Value::Int(0))]);
    assert_eq!(followed.get("values"), Some(&held));
    let missing = by_iid(&[(262, Value::Int(NO_VALUE)), (999, Value::Int(NO_VALUE))]);
    assert_eq!(followed.get("errors"), Some(&missing));

    let written = by_iid(&[(262, Value::Bool(true)), (264, Value::Bool(false))]);
    let answers = state.handle(Lane::Normal, &command("writeRequest", written));
    assert_eq!(answers.len(), 2);
    assert_eq!(answers[0].0, Lane::Normal);
    let response = opened(&answers[0].1);
    assert_eq!(
        response.get("command").and_then(Value::as_str),
        Some("writeResponse")
    );
    let results = by_iid(&[(262, Value::Int(WRITTEN)), (264, Value::Int(NO_VALUE))]);
    assert_eq!(response.get("errors"), Some(&results));
    assert_eq!(
        answers[1].0,
        Lane::High,
        "updates ride the high priority stream"
    );
    let update = opened(&answers[1].1);
    assert_eq!(
        update.get("command").and_then(Value::as_str),
        Some("updateNotify")
    );
    assert_eq!(
        update.get("values"),
        Some(&by_iid(&[(262, Value::Bool(true))]))
    );
    assert!(update.get("transactionID").is_none());
    assert_eq!(
        state.values.get(&264),
        Some(&Value::Bool(true)),
        "the car keeps its own value"
    );

    let same = by_iid(&[(262, Value::Bool(true))]);
    assert_eq!(
        state
            .handle(Lane::High, &command("writeRequest", same))
            .len(),
        1
    );
    let unfollowed = by_iid(&[(266, Value::Dict(Vec::new()))]);
    assert_eq!(
        state
            .handle(Lane::High, &command("writeRequest", unfollowed))
            .len(),
        1
    );
    assert_eq!(state.values.get(&266), Some(&Value::Dict(Vec::new())));
}

#[test]
fn climate_and_seats_come_only_with_the_climate_controls() {
    let iids =
        |state: &VehicleState| -> Vec<i64> { state.accessories.iter().map(|a| a.iid).collect() };
    let mut without = VehicleState::new(false, "9.5.0", "").with_climate(false);
    assert!(without.set_climate(&default_climate()).is_empty());
    assert!(!iids(&without).contains(&512));
    assert!(!iids(&without).contains(&1024));
    assert!(without.climate().is_none());
    let with = VehicleState::new(false, "9.5.0", "").with_climate(true);
    let order = iids(&with);
    let at = |iid| order.iter().position(|i| *i == iid).unwrap();
    assert_eq!(
        (at(512) + 1, at(1024) + 1),
        (at(1024), at(VEHICLE_RESOURCES))
    );
    assert_eq!(with.climate(), Some(default_climate()));
}

#[test]
fn the_vehicle_app_shows_the_livi_version_and_build_date() {
    let lines = |version: &str, built: &str| {
        let (_, info) = VehicleState::new(false, version, built).info_entry();
        let plugin = &info
            .get("pluginConfigs")
            .and_then(bplist::Value::as_array)
            .unwrap()[0];
        let settings = plugin
            .get("accessories")
            .and_then(bplist::Value::as_array)
            .unwrap()
            .iter()
            .find(|a| a.get("type").and_then(bplist::Value::as_int) == Some(0x0500_0001))
            .unwrap()
            .clone();
        settings
            .get("services")
            .and_then(bplist::Value::as_array)
            .unwrap()
            .to_vec()
    };
    let initial = |line: &bplist::Value, kind| {
        line.get("characteristics")
            .and_then(bplist::Value::as_array)
            .unwrap()
            .iter()
            .find(|c| c.get("type").and_then(bplist::Value::as_int) == Some(kind))
            .and_then(|c| c.get("initialValue"))
            .cloned()
    };
    let text = |s: &str| Some(bplist::Value::String(s.into()));

    let kind = |l: &bplist::Value| l.get("type").and_then(bplist::Value::as_int);
    let known = lines("9.5.0", "2026-10-15 14:30");
    assert_eq!(
        known.iter().map(kind).collect::<Vec<_>>(),
        [0x1600_0015, 0x1600_0015, 0x1600_0020].map(Some)
    );
    let section = &known[2];
    assert_eq!(initial(section, 0x3000_0019), text("LIVI"));
    assert_eq!(initial(section, 0x3600_0004), text("LIVI"));
    for line in &known[..2] {
        assert_eq!(
            initial(line, 0x3600_0028),
            text("LIVI"),
            "both lines sit in the section"
        );
        assert_eq!(initial(line, 0x3600_0006), Some(bplist::Value::Int(0)));
    }
    assert_eq!(initial(&known[0], 0x3000_0001), text("Version"));
    assert_eq!(initial(&known[0], 0x3000_0017), text("9.5.0"));
    assert_eq!(initial(&known[1], 0x3000_0001), text("Version Build"));
    let label = |line| match initial(line, 0x3000_0001) {
        Some(bplist::Value::String(s)) => s,
        other => panic!("no label: {other:?}"),
    };
    assert!(
        label(&known[0]) < label(&known[1]),
        "the phone puts the version line first"
    );
    assert_eq!(initial(&known[1], 0x3000_0017), text("2026-10-15 14:30"));
    let sort_order = |line: &bplist::Value| {
        line.get("characteristics")
            .and_then(bplist::Value::as_array)
            .unwrap()
            .iter()
            .find(|c| c.get("type").and_then(bplist::Value::as_int) == Some(0x3000_0003))
            .cloned()
            .unwrap()
    };
    let (version, build) = (sort_order(&known[0]), sort_order(&known[1]));
    let number = |c: &bplist::Value, k: &str| c.get(k).and_then(bplist::Value::as_int);
    assert_eq!(
        number(&version, "initialValue"),
        Some(0),
        "the version line comes first"
    );
    assert_eq!(number(&build, "initialValue"), Some(1));
    assert_eq!(
        number(&version, "maximumValue"),
        Some(100),
        "the phone drops a number without bounds"
    );

    let unknown = lines("", "");
    assert_eq!(
        initial(&unknown[0], 0x3000_0017),
        text("?"),
        "an unknown version shows as ?"
    );
    assert_eq!(initial(&unknown[1], 0x3000_0017), text("?"));
}

#[test]
fn both_front_seats_heat_in_two_steps() {
    let (_, info) = VehicleState::new(false, "9.5.0", "2026-10-15 14:30")
        .with_climate(true)
        .info_entry();
    let plugin = &info
        .get("pluginConfigs")
        .and_then(bplist::Value::as_array)
        .unwrap()[0];
    let seats = plugin
        .get("accessories")
        .and_then(bplist::Value::as_array)
        .unwrap()
        .iter()
        .find(|a| a.get("type").and_then(bplist::Value::as_int) == Some(0x0150_0001))
        .unwrap();
    let services = seats
        .get("services")
        .and_then(bplist::Value::as_array)
        .unwrap();
    assert_eq!(services.len(), 2);
    for service in services {
        assert_eq!(
            service.get("type").and_then(bplist::Value::as_int),
            Some(0x2200_0001)
        );
        let level = service
            .get("characteristics")
            .and_then(bplist::Value::as_array)
            .unwrap()
            .iter()
            .find(|c| c.get("type").and_then(bplist::Value::as_int) == Some(0x3100_0021))
            .unwrap();
        assert_eq!(level.get("format").and_then(bplist::Value::as_int), Some(7));
        assert_eq!(
            level.get("minimumValue").and_then(bplist::Value::as_int),
            Some(0)
        );
        assert_eq!(
            level.get("maximumValue").and_then(bplist::Value::as_int),
            Some(2)
        );
        assert_eq!(
            level.get("writable").and_then(bplist::Value::as_bool),
            Some(true)
        );
    }
}

#[test]
fn a_seat_heats_while_its_level_is_above_zero() {
    let mut state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30").with_climate(true);
    let [(level, on), _] = SEAT_HEATING;
    state.handle(
        Lane::High,
        &command(
            "registerRequest",
            Value::Array(vec![Value::Int(level), Value::Int(on)]),
        ),
    );
    let update = |state: &mut VehicleState, to: i64| {
        let answers = state.handle(
            Lane::High,
            &command("writeRequest", by_iid(&[(level, Value::Int(to))])),
        );
        opened(&answers[1].1).get("values").cloned()
    };
    assert_eq!(
        update(&mut state, 2),
        Some(by_iid(&[(level, Value::Int(2)), (on, Value::Bool(true))]))
    );
    assert_eq!(
        update(&mut state, 1),
        Some(by_iid(&[(level, Value::Int(1))]))
    );
    assert_eq!(
        update(&mut state, 0),
        Some(by_iid(&[(level, Value::Int(0)), (on, Value::Bool(false))]))
    );
}

#[test]
fn front_and_rear_windows_heat_through_their_wires() {
    let (_, info) = VehicleState::new(false, "9.5.0", "2026-10-15 14:30")
        .with_climate(true)
        .info_entry();
    let plugin = &info
        .get("pluginConfigs")
        .and_then(bplist::Value::as_array)
        .unwrap()[0];
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
            let chars = s
                .get("characteristics")
                .and_then(bplist::Value::as_array)
                .unwrap();
            let of = |kind| {
                chars
                    .iter()
                    .find(|c| c.get("type").and_then(bplist::Value::as_int) == Some(kind))
                    .and_then(|c| c.get("initialValue"))
            };
            let key = of(0x3600_0065)
                .and_then(bplist::Value::as_str)
                .unwrap()
                .to_string();
            (key, of(0x3100_0014).and_then(bplist::Value::as_int))
        })
        .collect();
    assert_eq!(
        windows,
        [
            ("windshield_front".to_string(), Some(2)),
            ("windshield_rear".to_string(), Some(2))
        ]
    );
}

#[test]
fn each_vent_has_its_button_and_auto_takes_them_all() {
    let mut state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30").with_climate(true);
    let directions: Vec<_> = VENTS
        .iter()
        .map(|vent| state.values.get(&(vent + 3)).cloned())
        .collect();
    let one = |d| Some(Value::Array(vec![Value::Int(d)]));
    assert_eq!(directions, [one(UPPER), one(MIDDLE), one(LOWER)]);
    let [_, middle, lower] = VENTS;
    let automatic = VENTS
        .iter()
        .all(|v| state.values.get(&(v + 2)) == Some(&Value::Bool(true)));
    assert!(automatic, "the vents start in automatic, as AUTO does");
    let iids = [middle + 1, middle + 2, lower + 1, lower + 2]
        .map(Value::Int)
        .to_vec();
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
        Some(by_iid(&[
            (middle + 2, Value::Bool(true)),
            (lower + 2, Value::Bool(true))
        ]))
    );
    assert_eq!(state.values.get(&(middle + 1)), Some(&Value::Bool(true)));
}

#[test]
fn max_ac_cools_with_everything_and_gives_the_settings_back() {
    let mut state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30").with_climate(true);
    let (level, _) = FAN;
    let iids = [
        MAX_AC,
        DRIVER_TARGET_TEMPERATURE,
        PASSENGER_TARGET_TEMPERATURE,
        RECIRCULATION,
        level,
    ];
    state.handle(
        Lane::High,
        &command(
            "registerRequest",
            Value::Array(iids.map(Value::Int).to_vec()),
        ),
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
        Some(by_iid(&[
            (level, Value::Int(4)),
            (MAX_AC, Value::Bool(false))
        ])),
        "set by hand, Max AC ends and keeps the rest"
    );
    assert_eq!(state.values.get(&DRIVER_TARGET_TEMPERATURE), Some(&lo));
}

#[test]
fn auto_runs_the_fan_until_it_is_set_by_hand() {
    let mut state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30").with_climate(true);
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
        Some(by_iid(&[
            (AUTO_CLIMATE, Value::Bool(true)),
            (FAN_AUTO, Value::Bool(true))
        ]))
    );
}

#[test]
fn the_fan_stops_at_level_zero_and_comes_back_at_its_last_level() {
    let mut state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30").with_climate(true);
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
    let mut state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30").with_climate(true);
    let iids = [
        DRIVER_TARGET_TEMPERATURE,
        PASSENGER_TARGET_TEMPERATURE,
        ZONES_SYNCED,
    ];
    state.handle(
        Lane::High,
        &command(
            "registerRequest",
            Value::Array(iids.map(Value::Int).to_vec()),
        ),
    );
    let update = |state: &mut VehicleState, written: &[(i64, Value)]| {
        let answers = state.handle(Lane::High, &command("writeRequest", by_iid(written)));
        opened(&answers[1].1).get("values").cloned()
    };

    assert_eq!(
        update(
            &mut state,
            &[(DRIVER_TARGET_TEMPERATURE, Value::Real(23.0))]
        ),
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
        update(
            &mut state,
            &[(DRIVER_TARGET_TEMPERATURE, Value::Real(24.5))]
        ),
        Some(by_iid(&[
            (DRIVER_TARGET_TEMPERATURE, Value::Real(24.5)),
            (PASSENGER_TARGET_TEMPERATURE, Value::Real(24.5))
        ]))
    );
    assert_eq!(
        update(
            &mut state,
            &[(PASSENGER_TARGET_TEMPERATURE, Value::Real(20.0))]
        ),
        Some(by_iid(&[
            (PASSENGER_TARGET_TEMPERATURE, Value::Real(20.0)),
            (ZONES_SYNCED, Value::Bool(false))
        ]))
    );
    assert_eq!(
        state.values.get(&DRIVER_TARGET_TEMPERATURE),
        Some(&Value::Real(24.5))
    );
}

#[test]
fn both_front_seats_have_their_own_temperature() {
    let (_, info) = VehicleState::new(false, "9.5.0", "2026-10-15 14:30")
        .with_climate(true)
        .info_entry();
    let plugin = &info
        .get("pluginConfigs")
        .and_then(bplist::Value::as_array)
        .unwrap()[0];
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
        .flat_map(|s| {
            s.get("characteristics")
                .and_then(bplist::Value::as_array)
                .unwrap()
        })
        .filter(|c| c.get("type").and_then(bplist::Value::as_int) == Some(0x3600_0065))
        .filter_map(|c| c.get("initialValue").and_then(bplist::Value::as_str))
        .map(str::to_string)
        .collect();
    assert_eq!(seats, ["seat_front_left", "seat_front_right"]);
}

#[test]
fn climate_is_described_and_set_from_the_phone() {
    let mut state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30").with_climate(true);
    let (_, info) = state.info_entry();
    let plugin = &info
        .get("pluginConfigs")
        .and_then(bplist::Value::as_array)
        .unwrap()[0];
    let accessories = plugin
        .get("accessories")
        .and_then(bplist::Value::as_array)
        .unwrap();
    fn of_type(list: &[bplist::Value], kind: u64) -> &bplist::Value {
        list.iter()
            .find(|x| x.get("type").and_then(bplist::Value::as_int) == Some(kind))
            .unwrap()
    }
    let climate = of_type(accessories, 0x0100_0001);
    let services = climate
        .get("services")
        .and_then(bplist::Value::as_array)
        .unwrap();
    let characteristics = |kind| {
        of_type(services, kind)
            .get("characteristics")
            .and_then(bplist::Value::as_array)
            .unwrap()
    };
    let target = of_type(characteristics(0x1100_0002), 0x3100_0017);
    assert_eq!(
        target.get("format").and_then(bplist::Value::as_int),
        Some(9)
    );
    assert_eq!(
        target.get("units").and_then(bplist::Value::as_int),
        Some(1082)
    );
    assert_eq!(
        target.get("writable").and_then(bplist::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        target.get("oemWritable").and_then(bplist::Value::as_bool),
        Some(true)
    );
    assert_eq!(target.get("minimumValue"), Some(&bplist::Value::Real(16.0)));
    assert_eq!(target.get("initialValue"), Some(&bplist::Value::Real(21.0)));
    let level = of_type(characteristics(0x1100_0007), 0x3100_0012);
    assert_eq!(
        level.get("maximumValue").and_then(bplist::Value::as_int),
        Some(7)
    );
    let resources = of_type(accessories, 0x0120_0002);
    let variant = of_type(
        resources
            .get("services")
            .and_then(bplist::Value::as_array)
            .unwrap(),
        0x2000_0001,
    );
    let style = of_type(
        variant
            .get("characteristics")
            .and_then(bplist::Value::as_array)
            .unwrap(),
        0x4800_0004,
    );
    assert_eq!(
        style.get("initialValue"),
        Some(&bplist::Value::String("5Seater".into()))
    );
    let themed = VehicleState::new(false, "", "").with_variant("DB12Coupe");
    for iid in [EXTERIOR_STYLE, INTERIOR_STYLE, VEHICLE_LAYOUT] {
        let named = themed.characteristic(iid).and_then(|c| c.initial.clone());
        assert_eq!(named, Some(Value::str("DB12Coupe")), "iid {iid}");
        assert_eq!(themed.values.get(&iid), Some(&Value::str("DB12Coupe")));
    }
    let plain = VehicleState::new(false, "", "").with_variant("");
    assert_eq!(
        plain.values.get(&INTERIOR_STYLE),
        Some(&Value::str("5Seater"))
    );
    assert!(
        plain.characteristic(EXTERIOR_STYLE).is_none(),
        "no theme, no model"
    );

    assert!(
        plain.characteristic(ENGINE_RPM).is_none(),
        "no car type, no engine"
    );
    let rpm = Rpm {
        max: 6000,
        redline: 5500,
    };
    let mut hybrid = VehicleState::new(false, "", "").with_engines(Engines {
        combustion: Some(rpm),
        electric: true,
    });
    assert_eq!(hybrid.values.get(&2564), Some(&Value::Int(5500)));
    assert_eq!(hybrid.values.get(&ENGINE_POWER), Some(&Value::Real(0.0)));
    hybrid.set_gauges(&serde_json::from_str(r#"{"rpm":2512.4}"#).unwrap());
    assert_eq!(hybrid.values.get(&ENGINE_RPM), Some(&Value::Int(2512)));
    hybrid.set_gauges(&serde_json::from_str(r#"{"rpm":9000}"#).unwrap());
    assert_eq!(
        hybrid.values.get(&ENGINE_RPM),
        Some(&Value::Int(6000)),
        "held at the max"
    );

    let content = ClusterContent {
        panel: "Cluster_Display".into(),
        zones: ["widget://?identifier=com.apple.tires".into(), String::new()],
    };
    let shown = VehicleState::new(false, "", "").with_cluster_content(Some(&content));
    assert_eq!(
        shown.values.get(&3074),
        Some(&Value::str("widget://?identifier=com.apple.tires"))
    );
    assert_eq!(shown.values.get(&3075), Some(&Value::Bool(true)));
    assert_eq!(
        shown.values.get(&3081),
        Some(&Value::Bool(false)),
        "an empty zone is off"
    );
    assert_eq!(shown.values.get(&3084), Some(&Value::str("zone2")));
    assert_eq!(
        shown.characteristic(3333).and_then(|c| c.initial.clone()),
        Some(Value::str("zone1"))
    );
    let mut shown = shown;
    shown.show_in_zone(0, "");
    assert_eq!(shown.values.get(&3075), Some(&Value::Bool(false)));
    shown.show_in_zone(1, "picker://");
    assert_eq!(shown.values.get(&3080), Some(&Value::str("picker://")));
    assert_eq!(shown.values.get(&3081), Some(&Value::Bool(true)));

    let iids = [520, 525].map(Value::Int).to_vec();
    state.handle(Lane::High, &command("registerRequest", Value::Array(iids)));
    let written = by_iid(&[(520, Value::Real(22.5)), (521, Value::Real(30.0))]);
    let answers = state.handle(Lane::High, &command("writeRequest", written));
    let results = by_iid(&[(520, Value::Int(WRITTEN)), (521, Value::Int(NO_VALUE))]);
    assert_eq!(opened(&answers[0].1).get("errors"), Some(&results));
    let update = opened(&answers[1].1);
    assert_eq!(
        update.get("values"),
        Some(&by_iid(&[(520, Value::Real(22.5))]))
    );
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
}

#[test]
fn livi_sets_climate_and_the_phone_hears_what_changed() {
    let mut state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30").with_climate(true);
    let (level, on) = FAN;
    let iids = [AUTO_CLIMATE, FAN_AUTO, level, on, DRIVER_TARGET_TEMPERATURE];
    state.handle(
        Lane::High,
        &command(
            "registerRequest",
            Value::Array(iids.map(Value::Int).to_vec()),
        ),
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
        set(
            &mut state,
            serde_json::json!({ "auto": true, "fanAuto": false })
        ),
        Some(by_iid(&[(AUTO_CLIMATE, Value::Bool(true))])),
        "what was named stands against the rules"
    );
    assert_eq!(
        set(
            &mut state,
            serde_json::json!({ "driverTargetC": 21.3, "fanLevel": 0 })
        ),
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
    assert!(
        VENTS
            .iter()
            .all(|v| state.values.get(&(v + 2)) == Some(&Value::Bool(false)))
    );
}

#[test]
fn fuel_and_trip_computer_are_read_only_with_bounded_numbers() {
    let state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30");
    let accessories = state.accessory_list();
    let kinds: Vec<i64> = accessories
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a.get("type").and_then(Value::as_int))
        .collect();
    assert!(kinds.contains(&0x0900_0006) && kinds.contains(&0x0400_0001));
    let flag = |iid, key| {
        state
            .characteristic(iid)
            .unwrap()
            .describe()
            .get(key)
            .cloned()
    };
    for iid in [
        FUEL_LEVEL,
        FUEL_RANGE.km,
        FUEL_RANGE.km + 1,
        ODOMETER.km,
        FUEL_EFFICIENCY,
    ] {
        assert_eq!(
            flag(iid, "writable"),
            Some(Value::Bool(false)),
            "{iid} is the car's"
        );
        assert!(
            flag(iid, "minimumValue").is_some(),
            "{iid} carries its bounds"
        );
        assert!(flag(iid, "maximumValue").is_some() && flag(iid, "stepValue").is_some());
    }
    assert_eq!(flag(FUEL_LEVEL, "maximumValue"), Some(Value::Int(100)));
    assert_eq!(
        flag(ODOMETER.km, "format"),
        Some(Value::Int(Format::Uint64 as i64))
    );
    let units = |iid| flag(iid, "units");
    assert_eq!(units(FUEL_LEVEL), Some(Value::Int(PERCENT)));
    assert_eq!(units(FUEL_RANGE.km), Some(Value::Int(KILOMETERS)));
    assert_eq!(units(FUEL_RANGE.km + 1), Some(Value::Int(MILES)));
    assert_eq!(
        units(FUEL_EFFICIENCY),
        Some(Value::Int(MILLILITERS_PER_100_KM))
    );
    assert_eq!(
        state.values.get(&FUEL_RANGE.hidden.unwrap()),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        state.values.get(&REMAINING_RANGE.hidden.unwrap()),
        Some(&Value::Bool(true))
    );
}

#[test]
fn gauges_reach_the_phone_in_both_units() {
    let mut state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30");
    let gauges = |v: serde_json::Value| v.as_object().cloned().unwrap();
    let quiet = state.set_gauges(&gauges(serde_json::json!({ "fuelPct": 42 })));
    assert!(
        quiet.is_empty(),
        "nothing goes out before the phone follows"
    );
    assert_eq!(state.values.get(&FUEL_LEVEL), Some(&Value::Int(42)));

    let iids = [
        FUEL_LEVEL,
        FUEL_RANGE.km,
        FUEL_RANGE.km + 1,
        1543,
        ODOMETER.km,
        1546,
    ];
    state.handle(
        Lane::High,
        &command(
            "registerRequest",
            Value::Array(iids.map(Value::Int).to_vec()),
        ),
    );
    let answers = state.set_gauges(&gauges(serde_json::json!({
        "fuelPct": 120,
        "rangeKm": 380.4,
        "odometerKm": 123456,
        "consumptionAvgLPer100Km": 8.1,
        "speedKph": 50,
        "rangeFuelKm": "far"
    })));
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].0, Lane::High);
    assert_eq!(
        opened(&answers[0].1).get("values"),
        Some(&by_iid(&[
            (FUEL_LEVEL, Value::Int(100)),
            (FUEL_RANGE.km, Value::Int(380)),
            (FUEL_RANGE.km + 1, Value::Int(236)),
            (1543, Value::Bool(false)),
            (ODOMETER.km, Value::Int(123_456)),
            (1546, Value::Int(8100)),
        ]))
    );
    assert_eq!(
        state.values.get(&REMAINING_RANGE.km),
        Some(&Value::Int(380))
    );
    assert_eq!(
        state.values.get(&(REMAINING_RANGE.km + 1)),
        Some(&Value::Int(236))
    );

    let again = state.set_gauges(&gauges(serde_json::json!({ "rangeKm": 380 })));
    assert!(again.is_empty(), "an unchanged reading stays home");
    state.set_gauges(&gauges(
        serde_json::json!({ "rangeKm": -5, "consumptionLPer100Km": -1 }),
    ));
    assert_eq!(state.values.get(&FUEL_RANGE.km), Some(&Value::Int(0)));
    assert_eq!(state.values.get(&FUEL_EFFICIENCY), Some(&Value::Int(0)));
}

#[test]
fn each_wheel_has_its_pressure_and_place() {
    let mut state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30");
    let tires = state
        .accessories
        .iter()
        .find(|a| a.kind == 0x0A00_0001)
        .unwrap();
    assert_eq!(tires.services.len(), 4);
    for (n, service) in tires.services.iter().enumerate() {
        assert_eq!(service.kind, 0x1B00_0001);
        let pressure = &service.characteristics[0];
        assert_eq!(
            (pressure.iid, pressure.kind),
            (tire_pressure_iid(n), 0x3000_001B)
        );
        assert_eq!(pressure.range, Some((0.0, 1000.0, 1.0)));
        assert_eq!(pressure.units, Some(KILOPASCALS));
        let place = service.characteristics[1].initial.clone();
        assert_eq!(place, Some(Value::str(WHEELS[n].1)));
        let (pressure_state, sensor_state) =
            (&service.characteristics[2], &service.characteristics[3]);
        assert_eq!(
            (pressure_state.kind, sensor_state.kind),
            (0x3000_001C, 0x3000_001A)
        );
        assert_eq!(
            pressure_state.range,
            Some((0.0, 5.0, 1.0)),
            "the phone drops a number without bounds"
        );
        assert_eq!(pressure_state.initial, Some(Value::Int(PRESSURE_UNDEFINED)));
        assert_eq!(sensor_state.initial, Some(Value::Int(SENSOR_WAITING)));
    }

    let iids = (0..4).map(|n| Value::Int(tire_pressure_iid(n))).collect();
    state.handle(Lane::High, &command("registerRequest", Value::Array(iids)));
    let gauges = serde_json::json!({ "tirePressure": {
        "frontLeftKpa": 240.4, "rearRightKpa": 1500, "rearLeftKpa": "flat"
    } });
    let answers = state.set_gauges(gauges.as_object().unwrap());
    assert_eq!(
        opened(&answers[0].1).get("values"),
        Some(&by_iid(&[
            (tire_pressure_iid(0), Value::Int(240)),
            (tire_pressure_iid(3), Value::Int(1000))
        ])),
        "a wheel pushed alone goes out alone, held to the bounds"
    );
    let states = |n| {
        let iid = tire_pressure_iid(n);
        (
            state.values.get(&(iid + 2)).cloned(),
            state.values.get(&(iid + 3)).cloned(),
        )
    };
    let measured = (
        Some(Value::Int(PRESSURE_UNDEFINED)),
        Some(Value::Int(SENSOR_ACTIVE)),
    );
    assert_eq!(states(0), measured, "a measured wheel has an active sensor");
    assert_eq!(
        states(1),
        (
            Some(Value::Int(PRESSURE_UNDEFINED)),
            Some(Value::Int(SENSOR_WAITING))
        )
    );
}

#[test]
fn the_volume_goes_both_ways_in_percent() {
    let mut state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30").with_climate(true);
    let volume = state.characteristic(VOLUME).unwrap().describe();
    assert_eq!(
        volume.get("writable"),
        Some(&Value::Bool(true)),
        "Siri may set it"
    );
    assert_eq!(volume.get("maximumValue"), Some(&Value::Int(100)));
    assert_eq!(volume.get("disabled"), Some(&Value::Bool(false)));
    let fan = state.characteristic(FAN.0).unwrap().describe();
    assert!(fan.get("disabled").is_none(), "only where it is said");
    assert_eq!(state.volume(), Some(0.5));

    state.handle(
        Lane::High,
        &command("registerRequest", Value::Array(vec![Value::Int(VOLUME)])),
    );
    let answers = state.set_gauges(serde_json::json!({ "volume": 0.304 }).as_object().unwrap());
    assert_eq!(
        opened(&answers[0].1).get("values"),
        Some(&by_iid(&[(VOLUME, Value::Int(30))]))
    );
    state.set_gauges(serde_json::json!({ "volume": 2.0 }).as_object().unwrap());
    assert_eq!(
        state.volume(),
        Some(1.0),
        "the head unit's level is held to 0 to 1"
    );

    let written = by_iid(&[(VOLUME, Value::Int(42))]);
    state.handle(Lane::Normal, &command("writeRequest", written));
    assert_eq!(state.volume(), Some(0.42));

    let mute = |on| by_iid(&[(MUTE, Value::Bool(on))]);
    state.handle(Lane::Normal, &command("writeRequest", mute(true)));
    assert_eq!(state.volume(), Some(0.0), "mute silences");
    state.handle(Lane::Normal, &command("writeRequest", mute(false)));
    assert_eq!(
        state.volume(),
        Some(0.42),
        "unmute brings the last level back"
    );

    state.handle(Lane::Normal, &command("writeRequest", mute(true)));
    state.handle(
        Lane::Normal,
        &command("writeRequest", by_iid(&[(VOLUME, Value::Int(10))])),
    );
    assert_eq!(
        state.values.get(&MUTE),
        Some(&Value::Bool(false)),
        "turning up ends the mute"
    );
    state.handle(Lane::Normal, &command("writeRequest", mute(true)));
    state.set_gauges(serde_json::json!({ "volume": 0.2 }).as_object().unwrap());
    assert_eq!(
        state.values.get(&MUTE),
        Some(&Value::Bool(false)),
        "so does the head unit"
    );

    let audio = state
        .accessories
        .iter()
        .find(|a| a.kind == 0x0200_0001)
        .unwrap();
    let types: Vec<_> = audio
        .services
        .iter()
        .map(|s| s.characteristics[1].initial.clone())
        .collect();
    assert_eq!(
        types,
        [0, 1, 2, 3, 6].map(|t| Some(Value::Int(t))),
        "media, navigation, call, other"
    );
    let kind = audio.services[1].characteristics[1].describe();
    assert_eq!(
        kind.get("maximumValue"),
        Some(&Value::Int(7)),
        "media to vehicle notification"
    );
    let label = audio.services[4].characteristics[2].initial.clone();
    assert_eq!(
        label,
        Some(Value::str("Musik")),
        "the other volume names itself"
    );
    let levels = serde_json::json!({ "navVolume": 0.3, "callVolume": 0.9 });
    state.set_gauges(levels.as_object().unwrap());
    let volumes = state.volumes();
    assert_eq!(
        volumes[..3],
        [("volume", 0.2), ("navVolume", 0.3), ("callVolume", 0.9)]
    );

    let music = state.characteristic(MUSIC_VOLUME).unwrap().describe();
    assert_eq!(music.get("writable"), Some(&Value::Bool(true)));
    assert_eq!(music.get("maximumValue"), Some(&Value::Int(100)));
    state.set_gauges(
        serde_json::json!({ "audioVolume": 0.65 })
            .as_object()
            .unwrap(),
    );
    assert_eq!(state.volumes()[3], ("audioVolume", 0.65));
    state.handle(
        Lane::Normal,
        &command("writeRequest", by_iid(&[(MUSIC_VOLUME, Value::Int(30))])),
    );
    assert_eq!(
        state.volumes()[3],
        ("audioVolume", 0.3),
        "the phone sets the music too"
    );
}

#[test]
fn what_cannot_be_read_gets_no_answer() {
    let mut state = VehicleState::new(true, "9.5.0", "2026-10-15 14:30");
    assert!(state.handle(Lane::High, b"\xA0").is_empty());
    let inner = opack::encode(&dict([("command", Value::str("configRequest"))]));
    let elsewhere = opack::encode(&dict([
        ("pluginID", Value::Int(3)),
        ("pluginData", Value::Data(inner)),
    ]));
    assert!(state.handle(Lane::High, &elsewhere).is_empty());
    assert!(
        state
            .handle(Lane::High, &command("unregisterRequest", Value::Null))
            .is_empty()
    );
    let written = by_iid(&[(270, Value::Int(2))]);
    assert_eq!(
        state
            .handle(Lane::High, &command("writeRequest", written))
            .len(),
        1
    );
}

fn service_of(state: &VehicleState, iid: i64) -> Value {
    let list = state.accessory_list();
    let service = list
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|a| a.get("services").and_then(Value::as_array).unwrap())
        .find(|s| s.get("iid").and_then(Value::as_int) == Some(iid))
        .cloned();
    service.unwrap()
}

#[test]
fn the_radio_app_finds_one_dab_source_and_its_commands() {
    let state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30");
    let source = service_of(&state, RADIO_SOURCE_SERVICE);
    assert_eq!(
        source.get("type").and_then(Value::as_int),
        Some(0x1400_0006)
    );
    assert!(source.get("controls").is_none());
    assert_eq!(state.values.get(&4098), Some(&Value::str("DAB")));
    assert_eq!(state.values.get(&4099), Some(&Value::Int(DAB_SOURCE)));
    assert_eq!(state.values.get(&RADIO_DISABLED), Some(&Value::Bool(true)));
    let items = state.characteristic(RADIO_ITEMS).unwrap().describe();
    assert_eq!(items.get("largePayload"), Some(&Value::Bool(true)));
    assert_eq!(
        items.get("maximumLength").and_then(Value::as_int),
        Some(LARGE_LENGTH)
    );
    let artwork = state
        .characteristic(NOW_PLAYING_ARTWORK)
        .unwrap()
        .describe();
    assert_eq!(
        artwork.get("format").and_then(Value::as_int),
        Some(Format::Data as i64)
    );

    let playing = service_of(&state, NOW_PLAYING_SERVICE);
    let controls = playing.get("controls").and_then(Value::as_array).unwrap();
    assert_eq!(controls.len(), 7);
    let tune = controls
        .iter()
        .find(|c| c.get("type").and_then(Value::as_int) == Some(0x0F00_004B))
        .unwrap();
    assert_eq!(
        tune.get("sender").and_then(Value::as_int),
        Some(SENT_BY_PHONE)
    );
    assert_eq!(tune.get("hasResponse"), Some(&Value::Bool(true)));
    let names: Vec<&str> = tune
        .get("requestParameters")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .filter_map(|p| p.get("name").and_then(Value::as_str))
        .collect();
    assert_eq!(names, ["identifier", "sourceIdentifier"]);
    let play = controls
        .iter()
        .find(|c| c.get("iid").and_then(Value::as_int) == Some(4360));
    assert!(play.unwrap().get("requestParameters").is_none());
}

#[test]
fn a_notice_goes_out_as_one_update_and_the_zone_opens_it() {
    let content = ClusterContent {
        panel: "Cluster_Display".into(),
        zones: [String::new(), String::new()],
    };
    let mut state = VehicleState::new(false, "", "")
        .with_cluster_content(Some(&content))
        .with_images(&[("livi".into(), b"png".to_vec())]);
    let field = state.characteristic(NOTICE + 1).unwrap();
    assert!(field.initial.is_none(), "the fields come only by update");
    assert!(!state.take_zone_ready());
    let iids = [TEMPORARY_ZONE + 1, TEMPORARY_ZONE + 2]
        .map(Value::Int)
        .to_vec();
    state.handle(
        Lane::Normal,
        &command("registerRequest", Value::Array(iids)),
    );
    assert!(state.take_zone_ready());
    assert!(!state.take_zone_ready(), "once per subscription");
    assert_eq!(
        state.symbol_name("livi"),
        "livi",
        "no picture before the phone fetched"
    );
    let asked = by_iid(&[(GET_IMAGE_ARCHIVE, Value::Null)]);
    state.handle(Lane::Normal, &command("controlRequest", asked));

    let notice = Notice {
        title: "Frost".into(),
        text: "Glätte möglich".into(),
        symbol: "livi".into(),
    };
    let out = state.show_notice(&notice);
    assert_eq!(out.len(), 2);
    let fields = opened(&out[0].1);
    let values = fields
        .get("values")
        .and_then(Value::as_dict)
        .unwrap()
        .to_vec();
    assert_eq!(values.len(), 6, "every field in one update");
    let get = |values: &[(Value, Value)], iid| {
        values
            .iter()
            .find(|(k, _)| k.as_int() == Some(iid))
            .map(|(_, v)| v.clone())
    };
    assert_eq!(get(&values, NOTICE + 1), Some(Value::str("Frost")));
    assert_eq!(get(&values, NOTICE + 2), Some(Value::str("Glätte möglich")));
    let symbol = get(&values, NOTICE + 4).unwrap();
    assert_eq!(
        symbol.get("name"),
        Some(&Value::str("imageArchive://?identifier=livi"))
    );
    let zone = opened(&out[1].1);
    let opens = by_iid(&[
        (TEMPORARY_ZONE + 1, Value::str(NOTICE_URL)),
        (TEMPORARY_ZONE + 2, Value::Bool(true)),
    ]);
    assert_eq!(zone.get("values"), Some(&opens));
    assert!(state.notice_open());

    std::thread::sleep(std::time::Duration::from_millis(2));
    let frost = Notice {
        symbol: "snowflake".into(),
        ..notice.clone()
    };
    let again = opened(&state.show_notice(&frost)[0].1);
    let again = again
        .get("values")
        .and_then(Value::as_dict)
        .unwrap()
        .to_vec();
    let name = get(&again, NOTICE + 4).and_then(|s| s.get("name").cloned());
    assert_eq!(
        name,
        Some(Value::str("snowflake")),
        "an SF Symbol passes as it is"
    );
    assert_ne!(
        get(&again, NOTICE + 3),
        get(&values, NOTICE + 3),
        "each notice is a new one"
    );

    assert_eq!(state.hide_notice().len(), 1);
    assert!(!state.notice_open());
    assert!(state.hide_notice().is_empty(), "closed already");
    state.show_notice(&notice);
    let closed = by_iid(&[(TEMPORARY_ZONE + 2, Value::Bool(false))]);
    state.handle(Lane::Normal, &command("writeRequest", closed));
    assert!(!state.notice_open(), "the phone closes it too");

    let mut plain = VehicleState::new(false, "", "");
    assert!(
        plain.show_notice(&notice).is_empty(),
        "no cluster content, no notice"
    );
}

#[test]
fn the_image_archive_is_described_and_handed_out() {
    let plain = VehicleState::new(false, "9.5.0", "");
    assert!(plain.characteristic(IMAGE_ARCHIVE_SERVICE + 1).is_none());
    let symbol = plain
        .characteristic(VERSION_LINE + 5)
        .unwrap()
        .initial
        .clone();
    assert_eq!(symbol, Some(Value::str("info.circle")));

    let images = [
        ("livi".into(), b"png".to_vec()),
        ("frost".into(), b"ice".to_vec()),
    ];
    let mut state = VehicleState::new(false, "9.5.0", "").with_images(&images);
    let archive = image_archive::build(&images);
    let version = format!(
        "{:08x}{:08x}",
        image_archive::crc32(&archive),
        archive_started()
    );
    assert_eq!(
        state.values.get(&(IMAGE_ARCHIVE_SERVICE + 1)),
        Some(&Value::String(version))
    );
    assert_eq!(
        state.values.get(&(VERSION_LINE + 5)),
        Some(&Value::str("info.circle"))
    );
    assert!(state.characteristic(VERSION_LINE + 5).unwrap().mutable);
    let line = Value::Array(vec![Value::Int(VERSION_LINE + 5)]);
    let out = state.handle(Lane::Normal, &command("registerRequest", line.clone()));
    assert_eq!(
        out.len(),
        1,
        "the (i) stays until the phone fetched the archive"
    );
    let service = service_of(&state, IMAGE_ARCHIVE_SERVICE);
    let control = &service.get("controls").and_then(Value::as_array).unwrap()[0];
    assert_eq!(
        control.get("iid").and_then(Value::as_int),
        Some(GET_IMAGE_ARCHIVE)
    );
    let answer = &control
        .get("responseParameters")
        .and_then(Value::as_array)
        .unwrap()[0];
    assert_eq!(
        answer.get("name").and_then(Value::as_str),
        Some("imageArchive")
    );
    assert_eq!(
        answer.get("format").and_then(Value::as_int),
        Some(Format::Data as i64)
    );
    assert!(control.get("requestParameters").is_none());

    let asked = by_iid(&[(GET_IMAGE_ARCHIVE, Value::Null)]);
    let answers = state.handle(Lane::Normal, &command("controlRequest", asked));
    let answer = opened(&answers[0].1);
    let fetched = by_iid(&[(
        GET_IMAGE_ARCHIVE,
        dict([("imageArchive", Value::Data(archive))]),
    )]);
    assert_eq!(answer.get("values"), Some(&fetched));
    let logo = opened(&answers[1].1);
    let set = by_iid(&[(
        VERSION_LINE + 5,
        Value::str("imageArchive://?identifier=livi"),
    )]);
    assert_eq!(logo.get("values"), Some(&set));
    let out = state.handle(Lane::Normal, &command("registerRequest", line));
    assert_eq!(out.len(), 2, "a later subscription hears the logo");
    let mut next = VehicleState::new(false, "9.5.0", "").with_images(&images);
    let line = Value::Array(vec![Value::Int(VERSION_LINE + 5)]);
    let out = next.handle(Lane::Normal, &command("registerRequest", line));
    assert_eq!(
        out.len(),
        2,
        "the next session knows the phone has the archive"
    );
    assert!(state.take_radio_commands().is_empty());
    assert!(
        VehicleState::new(false, "", "")
            .with_images(&[])
            .image_archive
            .is_none()
    );
    let mut own = VehicleState::new(false, "9.5.0", "").with_images(&images[1..]);
    let asked = by_iid(&[(GET_IMAGE_ARCHIVE, Value::Null)]);
    own.handle(Lane::Normal, &command("controlRequest", asked));
    assert!(own.image_archive.is_some());
    assert_eq!(
        own.values.get(&(VERSION_LINE + 5)),
        Some(&Value::str("info.circle"))
    );
    assert!(own.logo_symbol_update().is_empty());
}

#[test]
fn the_history_shows_notices_until_the_phone_hides_them() {
    let mut state =
        VehicleState::new(false, "", "").with_images(&[("livi".into(), b"history".to_vec())]);
    let entry = HistoryEntry {
        id: 7,
        notice: Notice {
            title: "Frost".into(),
            text: "Glätte".into(),
            symbol: "livi".into(),
        },
        at_secs: 1_760_000_000,
        hidden: false,
    };
    assert!(
        state.set_history(vec![entry.clone()]).is_empty(),
        "nobody listens yet"
    );
    let first = Value::Array(vec![Value::Int(history_slot(0) + 1)]);
    let out = state.handle(Lane::Normal, &command("registerRequest", first));
    assert_eq!(out.len(), 1 + HISTORY_SLOTS, "one update per slot");
    let slot = |out: &[(Lane, Vec<u8>)], n: usize| {
        opened(&out[n].1)
            .get("values")
            .and_then(Value::as_dict)
            .unwrap()
            .to_vec()
    };
    let get = |values: &[(Value, Value)], iid| {
        values
            .iter()
            .find(|(k, _)| k.as_int() == Some(iid))
            .map(|(_, v)| v.clone())
    };
    let filled = slot(&out, 1);
    assert_eq!(get(&filled, history_slot(0) + 1), Some(Value::str("Frost")));
    assert_eq!(
        get(&filled, history_slot(0) + 3),
        Some(Value::Int(1_760_000_000))
    );
    assert_eq!(get(&filled, history_slot(0) + 4), Some(Value::str("livi")));
    assert_eq!(get(&filled, history_slot(0) + 7), Some(Value::Bool(false)));
    assert_eq!(
        get(&slot(&out, 2), history_slot(1) + 7),
        Some(Value::Bool(true))
    );
    let unit = state.characteristic(history_slot(0) + 3).unwrap();
    assert_eq!(
        unit.units,
        Some(SECONDS),
        "the app reads the time as a measurement"
    );

    let asked = by_iid(&[(GET_IMAGE_ARCHIVE, Value::Null)]);
    let out = state.handle(Lane::Normal, &command("controlRequest", asked));
    assert_eq!(
        out.len(),
        2 + HISTORY_SLOTS,
        "answer, logo, then every slot"
    );
    let refreshed = slot(&out, 2);
    let picture = Value::str("imageArchive://?identifier=livi");
    assert_eq!(
        get(&refreshed, history_slot(0) + 4),
        Some(picture),
        "fetched, so the picture"
    );

    let hide = by_iid(&[(history_slot(0) + 7, Value::Bool(true))]);
    state.handle(Lane::Normal, &command("writeRequest", hide));
    assert_eq!(state.take_hidden_by_phone(), [7]);
    assert!(state.take_hidden_by_phone().is_empty());
    let out = state.set_history(vec![HistoryEntry {
        hidden: true,
        ..entry
    }]);
    assert_eq!(
        get(&slot(&out, 0), history_slot(0) + 7),
        Some(Value::Bool(true))
    );
}

#[test]
fn radio_app_commands_are_answered_and_handed_on() {
    let mut state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30");
    let tune = dict([
        ("identifier", Value::str("d220")),
        ("sourceIdentifier", Value::str("DAB")),
    ]);
    let elsewhere = dict([
        ("identifier", Value::str("x")),
        ("sourceIdentifier", Value::str("FM")),
    ]);
    let pressed = by_iid(&[
        (4364, tune),
        (4360, Value::Null),
        (4361, Value::Null),
        (4362, Value::Null),
        (4363, Value::Null),
        (4366, dict([("identifier", Value::str("DAB"))])),
        (4365, dict([("frequency", Value::Int(94_800))])),
        (4364, elsewhere),
        (9999, Value::Null),
    ]);
    let answers = state.handle(Lane::Normal, &command("controlRequest", pressed));
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].0, Lane::Normal);
    let answer = opened(&answers[0].1);
    assert_eq!(
        answer.get("command").and_then(Value::as_str),
        Some("controlResponse")
    );
    assert_eq!(answer.get("transactionID").and_then(Value::as_int), Some(8));
    assert_eq!(
        answer
            .get("values")
            .and_then(Value::as_dict)
            .map(<[_]>::len),
        Some(8)
    );
    assert_eq!(
        answer.get("errors"),
        Some(&by_iid(&[(9999, Value::Int(NO_SUCH_COMMAND))]))
    );
    assert_eq!(
        state.take_radio_commands(),
        vec![
            RadioCommand::Tune("d220".into()),
            RadioCommand::Play,
            RadioCommand::Pause,
            RadioCommand::Next,
            RadioCommand::Previous,
            RadioCommand::Play,
        ]
    );
    assert!(state.take_radio_commands().is_empty());
}

#[test]
fn the_radio_reaches_the_phone_as_updates() {
    let mut state = VehicleState::new(false, "9.5.0", "2026-10-15 14:30");
    let followed = [
        RADIO_DISABLED,
        RADIO_ITEMS,
        RADIO_ITEM_IMAGES,
        NOW_PLAYING_TITLE,
        PLAYBACK_STATE,
    ];
    let iids = followed.iter().map(|i| Value::Int(*i)).collect();
    state.handle(Lane::High, &command("registerRequest", Value::Array(iids)));
    let radio = Radio {
        available: true,
        stations: vec![
            RadioStation {
                id: "d220".into(),
                name: "Dlf Kultur".into(),
                short_name: "Kultur".into(),
                ensemble: "DR Deutschland".into(),
                image: Some(vec![0xff, 0xd8]),
            },
            RadioStation {
                id: "d210".into(),
                name: "Dlf".into(),
                short_name: "Dlf".into(),
                ensemble: "DR Deutschland".into(),
                image: None,
            },
        ],
        current: Some("d220".into()),
        title: "Lesart".into(),
        artist: "Dlf Kultur".into(),
        album: "DR Deutschland".into(),
        artwork: vec![1, 2],
        playing: true,
    };
    let answers = state.set_radio(&radio);
    assert_eq!(answers.len(), 1);
    let update = opened(&answers[0].1);
    let values = update.get("values").and_then(Value::as_dict).unwrap();
    let value = |iid| {
        values
            .iter()
            .find(|(k, _)| k.as_int() == Some(iid))
            .map(|(_, v)| v)
    };
    assert_eq!(value(RADIO_DISABLED), Some(&Value::Bool(false)));
    assert_eq!(value(NOW_PLAYING_TITLE), Some(&Value::str("Lesart")));
    assert_eq!(value(PLAYBACK_STATE), Some(&Value::Int(PLAYING)));
    let items = value(RADIO_ITEMS).and_then(Value::as_array).unwrap();
    assert_eq!(
        items[0].get("identifier").and_then(Value::as_str),
        Some("d220")
    );
    assert_eq!(
        items[0].get("mediaItemType").and_then(Value::as_str),
        Some("dab")
    );
    assert_eq!(
        items[0]
            .get("mediaItemImageIdentifier")
            .and_then(Value::as_str),
        Some("d220")
    );
    assert!(items[1].get("mediaItemImageIdentifier").is_none());
    let images = value(RADIO_ITEM_IMAGES).and_then(Value::as_array).unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(
        images[0].get("imageData").and_then(Value::as_data),
        Some(&[0xff, 0xd8][..])
    );
    assert_eq!(
        state.values.get(&NOW_PLAYING_ARTWORK),
        Some(&Value::Data(vec![1, 2]))
    );

    assert!(
        state.set_radio(&radio).is_empty(),
        "nothing changed, nothing sent"
    );
    assert_eq!(shown(&Value::Data(vec![0; 3])), "3 B");
    assert_eq!(shown(&Value::Array(vec![Value::Null; 5])), "5 items");
}
