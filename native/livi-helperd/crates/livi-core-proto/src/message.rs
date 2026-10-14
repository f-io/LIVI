use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::input::Input;
use crate::patch::PatchOp;
use crate::state::{Front, Screen, State};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub enum ToCore {
    Hello {
        protocol: u32,
        client: String,
    },
    Resync,
    Input {
        input: Input,
    },
    Action {
        id: u32,
        action: Action,
    },
    /// The UI's route, only because state/status.json publishes it.
    Path {
        path: String,
    },
    Spectrum {
        on: bool,
    },
    /// The dongle's Wi-Fi driver is asked for rates only while a UI shows them.
    LinkSpeed {
        on: bool,
    },
    /// Only the browser sees the cameras, core needs one to show it while reversing.
    Camera {
        found: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub enum FromCore {
    Welcome {
        protocol: u32,
        version: String,
        rev: u64,
        state: Box<State>,
    },
    /// Sent instead of the welcome, then core closes the connection.
    Refused {
        reason: String,
    },
    /// Follows the state of `rev - 1`, anything else means resync.
    Patch {
        rev: u64,
        ops: Vec<PatchOp>,
    },
    Reply {
        id: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        error: Option<String>,
    },
    /// 0 to 1 per band, only to clients that asked. Never part of the state.
    Spectrum {
        bands: Vec<f32>,
    },
    /// The telemetry asked for a page once, never part of the state.
    OpenPage {
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub enum Action {
    SetConfig {
        #[ts(type = "Partial<Config>")]
        patch: Value,
    },
    Show {
        screen: Screen,
        front: Front,
    },
    Media {
        control: MediaControl,
    },
    /// CarPlay only
    #[serde(rename_all = "camelCase")]
    Seek {
        position_ms: u32,
    },
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
    /// For settings that only take hold once the phones negotiate anew.
    ApplySettings,
    CheckUpdate,
    DownloadUpdate,
    InstallUpdate,
    AbortUpdate,
    SetDongleRadio {
        radio: Radio,
        on: bool,
    },
    Quit,
    Restart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
pub enum MediaControl {
    PlayPause,
    Play,
    Pause,
    Next,
    Prev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "contract.ts")]
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
            action: Action::SetConfig { patch: json!({ "huVolume": 1.0 }) },
        };
        let wire = json!({ "type": "action", "id": 7, "action": { "kind": "setConfig", "patch": { "huVolume": 1.0 } } });
        assert_eq!(serde_json::to_value(&msg).unwrap(), wire);
        assert_eq!(serde_json::from_value::<ToCore>(wire).unwrap(), msg);
    }

    #[test]
    fn unit_variants_carry_only_their_tag() {
        assert_eq!(serde_json::to_value(ToCore::Resync).unwrap(), json!({ "type": "resync" }));
        assert_eq!(serde_json::to_value(Action::Quit).unwrap(), json!({ "kind": "quit" }));
    }

    #[test]
    fn reply_leaves_out_a_missing_error() {
        let ok = FromCore::Reply { id: 3, error: None };
        assert_eq!(serde_json::to_value(&ok).unwrap(), json!({ "type": "reply", "id": 3 }));
        assert_eq!(
            serde_json::from_value::<FromCore>(json!({ "type": "reply", "id": 3 })).unwrap(),
            ok
        );
    }

    #[test]
    fn the_spectrum_goes_both_ways_in_its_own_messages() {
        let ask = ToCore::Spectrum { on: true };
        assert_eq!(serde_json::to_value(&ask).unwrap(), json!({ "type": "spectrum", "on": true }));
        let frame = FromCore::Spectrum { bands: vec![0.5, 1.0] };
        let wire = json!({ "type": "spectrum", "bands": [0.5, 1.0] });
        assert_eq!(serde_json::to_value(&frame).unwrap(), wire);
        assert_eq!(serde_json::from_value::<FromCore>(wire).unwrap(), frame);
    }

    #[test]
    fn unknown_message_is_rejected() {
        assert!(serde_json::from_value::<ToCore>(json!({ "type": "launchMissiles" })).is_err());
    }
}
