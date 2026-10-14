//! LIVI_WRITE_CONTRACT=1 rewrites the UI's copy of these types.

use std::fs;
use std::path::PathBuf;

use ts_rs::TS;

use crate::PROTOCOL;
use crate::config::{WIFI_PASSWORD_MAX, WIFI_PASSWORD_MIN, defaults};
use crate::frame::MAX_FRAME;
use crate::gnss::GnssInfo;
use crate::message::{FromCore, ToCore};
use crate::telemetry::TelemetrySnapshot;

const FILE: &str = "contract.ts";

// ts-rs sorts its type aliases but leaves TS enums in export order, which is not
// the same on every machine.
fn sorted(contents: &str) -> String {
    let (header, body) = contents.split_once("\n\n").unwrap();
    let mut decls: Vec<&str> =
        body.split("\n\n").map(|d| d.trim_matches('\n')).filter(|d| !d.is_empty()).collect();
    decls.sort_by_key(|d| declared_name(d));
    format!("{header}\n\n{}\n", decls.join("\n\n"))
}

fn declared_name(decl: &str) -> &str {
    ["export type ", "export enum ", "export const "]
        .iter()
        .find_map(|kw| decl.split_once(kw))
        .map(|(_, rest)| rest.split(|c: char| c == '<' || c.is_whitespace()).next().unwrap_or(rest))
        .unwrap_or(decl)
}

fn checked_in() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../src/main/shared/core")
}

#[test]
fn typescript_contract_is_current() {
    let out = std::env::temp_dir().join(format!("livi-core-proto-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    let cfg = ts_rs::Config::new().with_large_int("number").with_out_dir(&out);
    ToCore::export_all(&cfg).unwrap();
    FromCore::export_all(&cfg).unwrap();
    TelemetrySnapshot::export_all(&cfg).unwrap();

    let files: Vec<_> = fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name()).collect();
    let generated = fs::read_to_string(out.join(FILE));
    let _ = fs::remove_dir_all(&out);
    assert_eq!(files, [FILE], "every contract type has to export to {FILE}");
    // Values ts-rs has no export for: the numbers a client checks against and the settings
    // before core answers.
    let defaults = serde_json::to_string_pretty(&defaults()).unwrap();
    let no_receiver = serde_json::to_string_pretty(&GnssInfo::default()).unwrap();
    let constants = format!(
        "export const MAX_FRAME = {MAX_FRAME};\n\nexport const PROTOCOL = {PROTOCOL};\n\n\
         export const WIFI_PASSWORD_MIN = {WIFI_PASSWORD_MIN};\n\n\
         export const WIFI_PASSWORD_MAX = {WIFI_PASSWORD_MAX};\n\n\
         export const DEFAULT_CONFIG: Config = {defaults};\n\n\
         export const EMPTY_GNSS_INFO: GnssInfo = {no_receiver};\n"
    );
    let generated = sorted(&format!("{}\n\n{constants}", generated.unwrap()));

    let path = checked_in().join(FILE);
    if std::env::var_os("LIVI_WRITE_CONTRACT").is_some() {
        fs::create_dir_all(checked_in()).unwrap();
        fs::write(&path, &generated).unwrap();
        return;
    }
    let current = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        current == generated,
        "{} is stale, run the tests once with LIVI_WRITE_CONTRACT=1",
        path.display()
    );
}
