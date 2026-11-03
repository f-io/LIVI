// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::input::Input;
use crate::patch::PatchOp;
use crate::state::{Front, Screen, State};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ToCore {
    Hello { protocol: u32, client: String },
    Resync,
    Input { input: Input },
    Action { id: u32, action: Action },
    Path { path: String },
    Spectrum { on: bool },
    LinkSpeed { on: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FromCore {
    Welcome {
        protocol: u32,
        version: String,
        rev: u64,
        state: Box<State>,
    },
    Refused {
        reason: String,
    },
    Patch {
        rev: u64,
        ops: Vec<PatchOp>,
    },
    Reply {
        id: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Spectrum {
        bands: Vec<f32>,
    },
    OpenPage {
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    SetConfig {
        patch: Value,
    },
    Show {
        screen: Screen,
        front: Front,
    },
    Media {
        control: MediaControl,
    },
    #[serde(rename_all = "camelCase")]
    Seek {
        position_ms: u32,
    },
    VideoPlay {
        playing: bool,
    },
    #[serde(rename_all = "camelCase")]
    VideoSeek {
        position_ms: u32,
    },
    VideoClose,
    SelectDevice {
        id: String,
    },
    ForgetDevice {
        id: String,
    },
    ConnectDevice {
        id: String,
    },
    NextDevice,
    SelectClusterLayout {
        layout: String,
    },
    StepClusterLayout {
        by: i32,
    },
    MoveClusterLayout {
        layout: String,
        by: i32,
    },
    ShowClusterLayout {
        layout: String,
        on: bool,
    },
    ImportMfi {
        file: MfiFile,
        path: String,
    },
    RemoveMfi {
        file: MfiFile,
    },
    ApplySettings,
    CheckUpdate,
    DownloadUpdate,
    InstallUpdate,
    AbortUpdate,
    DabScan,
    DabStopScan,
    #[serde(rename_all = "camelCase")]
    DabPlay {
        service_id: u32,
    },
    DabResume,
    DabPause,
    DabStop,
    #[serde(rename_all = "camelCase")]
    DabFavorite {
        service_id: u32,
        favorite: bool,
    },
    #[serde(rename_all = "camelCase")]
    DabHide {
        service_id: u32,
        hidden: bool,
    },
    #[serde(rename_all = "camelCase")]
    DabMove {
        service_id: u32,
        by: i32,
    },
    SetDongleRadio {
        radio: Radio,
        on: bool,
    },
    Quit,
    Restart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MfiFile {
    Certificate,
    Identity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaControl {
    PlayPause,
    Play,
    Pause,
    Next,
    Prev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Radio {
    Wifi,
    Bt,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn action_on_the_wire() {
        let msg = ToCore::Action {
            id: 7,
            action: Action::SetConfig {
                patch: json!({ "huVolume": 1.0 }),
            },
        };
        let wire = json!({ "type": "action", "id": 7, "action": { "kind": "setConfig", "patch": { "huVolume": 1.0 } } });
        assert_eq!(serde_json::to_value(&msg).unwrap(), wire);
        assert_eq!(serde_json::from_value::<ToCore>(wire).unwrap(), msg);
    }

    #[test]
    fn unit_variants_carry_only_their_tag() {
        assert_eq!(
            serde_json::to_value(ToCore::Resync).unwrap(),
            json!({ "type": "resync" })
        );
        assert_eq!(
            serde_json::to_value(Action::Quit).unwrap(),
            json!({ "kind": "quit" })
        );
    }

    #[test]
    fn reply_leaves_out_a_missing_error() {
        let ok = FromCore::Reply { id: 3, error: None };
        assert_eq!(
            serde_json::to_value(&ok).unwrap(),
            json!({ "type": "reply", "id": 3 })
        );
        assert_eq!(
            serde_json::from_value::<FromCore>(json!({ "type": "reply", "id": 3 })).unwrap(),
            ok
        );
    }

    #[test]
    fn the_spectrum_goes_both_ways_in_its_own_messages() {
        let ask = ToCore::Spectrum { on: true };
        assert_eq!(
            serde_json::to_value(&ask).unwrap(),
            json!({ "type": "spectrum", "on": true })
        );
        let frame = FromCore::Spectrum {
            bands: vec![0.5, 1.0],
        };
        let wire = json!({ "type": "spectrum", "bands": [0.5, 1.0] });
        assert_eq!(serde_json::to_value(&frame).unwrap(), wire);
        assert_eq!(serde_json::from_value::<FromCore>(wire).unwrap(), frame);
    }

    #[test]
    fn mfi_files_travel_by_name() {
        let import = Action::ImportMfi {
            file: MfiFile::Identity,
            path: "/media/usb/identity.pk8".into(),
        };
        let wire =
            json!({ "kind": "importMfi", "file": "identity", "path": "/media/usb/identity.pk8" });
        assert_eq!(serde_json::to_value(&import).unwrap(), wire);
        assert_eq!(serde_json::from_value::<Action>(wire).unwrap(), import);
        assert_eq!(
            serde_json::to_value(Action::RemoveMfi {
                file: MfiFile::Certificate
            })
            .unwrap(),
            json!({ "kind": "removeMfi", "file": "certificate" })
        );
    }

    #[test]
    fn unknown_message_is_rejected() {
        assert!(serde_json::from_value::<ToCore>(json!({ "type": "launchMissiles" })).is_err());
    }
}
