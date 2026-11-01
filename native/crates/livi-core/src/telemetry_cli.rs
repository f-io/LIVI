// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::ExitCode;
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::telemetry::{HTTP_PUSH, PORT};

const HELP: &str = "\
livi-core telemetry pushes values into a running LIVI's telemetry.

  livi-core telemetry set <field>=<value> ... [--every-ms <ms>] [--to <host:port>]
  livi-core telemetry demo [--to <host:port>]

  set   one push, numbers, true, false and null are typed, a dot nests: gps.lat=53.59
  demo  one push that fills every field with plausible values

  --every-ms  pushes again in this rhythm until Ctrl+C
  --to        another LIVI, 127.0.0.1:4000 when absent

Examples:
  livi-core telemetry set speedKph=50 rpm=1500 gear=D lights=true
  livi-core telemetry set gear=R
  livi-core telemetry set nightMode=false clusterNightMode=true
  livi-core telemetry set climate.passengerTargetC=17
  livi-core telemetry set path=/media
  livi-core telemetry set notification.title=Frost notification.symbol=snowflake notification.open=true
  livi-core telemetry set gps.lat=53.5912 gps.lng=10.015 --every-ms 1000
";

#[derive(Debug, PartialEq)]
struct Order {
    push: Value,
    every: Option<Duration>,
    to: String,
}

pub fn run(args: &[String]) -> ExitCode {
    let order = match order(args) {
        Ok(Some(order)) => order,
        Ok(None) => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("livi-core telemetry: {e}\n\n{HELP}");
            return ExitCode::FAILURE;
        }
    };
    loop {
        if let Err(e) = post(&order.to, &order.push) {
            eprintln!("livi-core telemetry: {e}");
            return ExitCode::FAILURE;
        }
        println!("pushed {} to {}", order.push, order.to);
        let Some(every) = order.every else {
            return ExitCode::SUCCESS;
        };
        std::thread::sleep(every);
    }
}

fn order(args: &[String]) -> Result<Option<Order>, String> {
    let mut words = args.iter().map(String::as_str);
    let command = words.next();
    let mut to = format!("127.0.0.1:{PORT}");
    let mut every = None;
    let mut fields = Map::new();
    while let Some(word) = words.next() {
        match word {
            "--to" => {
                let host = words.next().ok_or("--to needs host:port")?;
                to = host
                    .trim_start_matches("http://")
                    .trim_end_matches('/')
                    .to_string();
            }
            "--every-ms" => {
                let ms = words
                    .next()
                    .and_then(|ms| ms.parse::<u64>().ok())
                    .filter(|ms| *ms > 0);
                every = Some(Duration::from_millis(
                    ms.ok_or("--every-ms needs milliseconds")?,
                ));
            }
            field => {
                let (key, raw) = field
                    .split_once('=')
                    .filter(|(key, _)| !key.is_empty())
                    .ok_or_else(|| format!("{field} is no <field>=<value>"))?;
                put(&mut fields, key, value_of(raw));
            }
        }
    }
    let push = match command {
        Some("set") if fields.is_empty() => return Err("set needs at least one field".into()),
        Some("set") => Value::Object(fields),
        Some("demo") => demo(),
        None | Some("help" | "--help" | "-h") => return Ok(None),
        Some(other) => return Err(format!("{other} is no command")),
    };
    Ok(Some(Order { push, every, to }))
}

fn value_of(raw: &str) -> Value {
    match raw {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "null" => Value::Null,
        _ => raw
            .parse::<i64>()
            .map(Value::from)
            .ok()
            .or_else(|| {
                raw.parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .map(Value::from)
            })
            .unwrap_or_else(|| Value::String(raw.to_string())),
    }
}

fn put(fields: &mut Map<String, Value>, key: &str, value: Value) {
    match key.split_once('.') {
        Some((head, rest)) => {
            let inner = fields
                .entry(head)
                .or_insert_with(|| Value::Object(Map::new()));
            if !inner.is_object() {
                *inner = Value::Object(Map::new());
            }
            if let Value::Object(inner) = inner {
                put(inner, rest, value);
            }
        }
        None => {
            fields.insert(key.to_string(), value);
        }
    }
}

fn demo() -> Value {
    json!({
        "speedKph": 50, "rpm": 1500, "gear": 4, "steeringDeg": 0,
        "reverse": false, "lights": true, "highBeam": false, "hazards": false,
        "turnSignal": "none", "parkingBrake": false,
        "coolantC": 90, "oilC": 95, "transmissionC": 80, "iatC": 28, "ambientC": 20,
        "batteryV": 14.1,
        "fuelPct": 4, "rangeKm": 38, "fuelRateLph": 6.4,
        "consumptionLPer100Km": 12.8, "consumptionAvgLPer100Km": 7.6,
        "mapKpa": 55, "baroKpa": 101.3, "boostKpa": 0, "throttlePct": 18, "lambda": 1.0, "afr": 14.7,
        "tirePressure": {
            "frontLeftKpa": 230, "frontRightKpa": 230, "rearLeftKpa": 220, "rearRightKpa": 220
        },
        "odometerKm": 87432.5, "odometerTripKm": 142.7, "drivingStatus": 0,
        "ambientLux": 80, "nightMode": true,
        "gps": {
            "lat": 53.557_732_245_303_99, "lng": 9.997_866_754_244_244, "alt": 8,
            "heading": 90, "speedMs": 13.89, "accuracyM": 4, "satellites": 11
        }
    })
}

pub(crate) fn post(to: &str, push: &Value) -> Result<(), String> {
    let body = push.to_string();
    let mut stream =
        TcpStream::connect(to).map_err(|e| format!("cannot reach LIVI at {to}: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let request = format!(
        "POST {HTTP_PUSH} HTTP/1.1\r\nHost: {to}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("cannot send to {to}: {e}"))?;
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    let status = answer.lines().next().unwrap_or("no answer");
    match status.split_whitespace().nth(1) {
        Some(code) if code.starts_with('2') => Ok(()),
        _ => Err(format!("LIVI at {to} answered {status}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn fields_are_typed_and_nested() {
        let typed = order(&words(
            "set gear=R speedKph=50 batteryV=14.1 lights=true climate.passengerTargetC=17 \
             climate.auto=false gps.lat=53.59 note= gone=null --every-ms 500 --to http://10.0.0.71:4000/",
        ));
        assert_eq!(
            typed,
            Ok(Some(Order {
                push: json!({
                    "gear": "R", "speedKph": 50, "batteryV": 14.1, "lights": true,
                    "climate": { "passengerTargetC": 17, "auto": false },
                    "gps": { "lat": 53.59 }, "note": "", "gone": null
                }),
                every: Some(Duration::from_millis(500)),
                to: "10.0.0.71:4000".into(),
            }))
        );
        let replaced = order(&words("set gps=lost gps.lat=1")).unwrap().unwrap();
        assert_eq!(replaced.push, json!({ "gps": { "lat": 1 } }));
    }

    #[test]
    fn help_demo_and_mistakes() {
        assert_eq!(order(&[]), Ok(None));
        assert_eq!(order(&words("help")), Ok(None));
        let demo = order(&words("demo")).unwrap().unwrap();
        assert_eq!(
            (demo.push["gps"]["satellites"].as_u64(), demo.every),
            (Some(11), None)
        );
        assert_eq!(demo.to, "127.0.0.1:4000");
        for (line, says) in [
            ("set", "at least one field"),
            ("set gear", "no <field>=<value>"),
            ("set =R", "no <field>=<value>"),
            ("set gear=R --every-ms", "milliseconds"),
            ("set gear=R --every-ms 0", "milliseconds"),
            ("set gear=R --to", "host:port"),
            ("fly", "no command"),
        ] {
            let error = order(&words(line)).unwrap_err();
            assert!(error.contains(says), "{line}: {error}");
        }
        assert_eq!(run(&words("help")), ExitCode::SUCCESS);
        assert_eq!(run(&words("fly")), ExitCode::FAILURE);
    }

    #[test]
    fn a_push_nobody_takes_fails() {
        let error = post("127.0.0.1:1", &json!({ "gear": "R" })).unwrap_err();
        assert!(error.contains("cannot reach LIVI"), "{error}");
        assert_eq!(
            run(&words("set gear=R --to 127.0.0.1:1")),
            ExitCode::FAILURE
        );
    }
}
