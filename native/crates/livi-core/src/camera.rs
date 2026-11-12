// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::Arc;

use livi_core_proto::state::{CameraView, Front, State};
use livi_media::output::{Output, OutputEvent};
use livi_media::planes::CameraPlane;
use livi_output_proto::{CameraOrder, CameraSeen};
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, watch};

use crate::hub::Hub;
use crate::projection::UiCommand;

fn views(list: Vec<CameraSeen>) -> Vec<CameraView> {
    list.into_iter()
        .map(|c| CameraView {
            id: c.id,
            name: c.name,
        })
        .collect()
}

fn fallback(saved: &str, cameras: &[CameraView]) -> Option<String> {
    let first = cameras.first()?;
    (!cameras.iter().any(|c| c.id == saved)).then(|| first.id.clone())
}

fn wanted(s: &State) -> Option<CameraOrder> {
    if s.front.main != Front::Camera || s.cameras.is_empty() {
        return None;
    }
    let device = s
        .cameras
        .iter()
        .find(|c| c.id == s.config.camera_id)
        .unwrap_or(&s.cameras[0])
        .id
        .clone();
    Some(CameraOrder {
        device,
        rotation: s.config.camera_rotation,
        mirror: s.config.camera_mirror,
    })
}

pub async fn run(
    hub: Arc<Hub>,
    gst: Output,
    plane: Arc<CameraPlane>,
    found: mpsc::UnboundedSender<UiCommand>,
    settings: mpsc::UnboundedSender<Value>,
    mut host_ready: watch::Receiver<bool>,
) {
    if host_ready.wait_for(|ready| *ready).await.is_err() {
        return;
    }
    let mut events = gst.subscribe();
    gst.watch_cameras();
    let mut state: watch::Receiver<State> = hub.watch();
    let mut shown: Option<CameraOrder> = None;
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(OutputEvent::Cameras { list }) => {
                    let cameras = views(list);
                    let names: Vec<&str> = cameras.iter().map(|c| c.name.as_str()).collect();
                    println!("[core] cameras: {}", if names.is_empty() { "none".into() } else { names.join(", ") });
                    if let Some(id) = fallback(&hub.config().camera_id, &cameras) {
                        let _ = settings.send(json!({ "cameraId": id }));
                    }
                    let _ = found.send(UiCommand::CameraFound(!cameras.is_empty()));
                    hub.update(|s| s.cameras = cameras);
                }
                Err(RecvError::Closed) => return,
                _ => {}
            },
            changed = state.changed() => if changed.is_err() {
                return;
            },
        }
        let next = wanted(&state.borrow_and_update());
        if next == shown {
            continue;
        }
        if shown.is_some() {
            println!("[core] camera off");
            plane.close();
        }
        if let Some(order) = next.clone() {
            println!(
                "[core] camera {} on, rotation {} mirror {}",
                order.device, order.rotation, order.mirror
            );
            let (plane, turn) = (plane.clone(), plane.turn());
            tokio::spawn(async move { plane.open(turn, &order).await });
        }
        shown = next;
    }
}

#[cfg(test)]
mod tests {
    use livi_core_proto::config::defaults;

    use super::*;

    fn cams(ids: &[&str]) -> Vec<CameraView> {
        ids.iter()
            .map(|id| CameraView {
                id: (*id).into(),
                name: format!("cam {id}"),
            })
            .collect()
    }

    #[test]
    fn an_unknown_saved_camera_falls_back_to_the_first() {
        assert_eq!(fallback("", &cams(&["a", "b"])).as_deref(), Some("a"));
        assert_eq!(fallback("old-hash", &cams(&["a"])).as_deref(), Some("a"));
        assert_eq!(fallback("b", &cams(&["a", "b"])), None);
        assert_eq!(fallback("b", &[]), None);
    }

    fn state(front: Front, cameras: Vec<CameraView>) -> State {
        let mut config = defaults();
        config.camera_id = "b".into();
        config.camera_rotation = 180;
        config.camera_mirror = true;
        State {
            front: livi_core_proto::state::PerScreen {
                main: front,
                cluster: Front::Livi,
                aux: Front::Livi,
            },
            sessions: Default::default(),
            now_playing: Default::default(),
            video_playback: None,
            telemetry: Default::default(),
            navigation: Default::default(),
            system: Default::default(),
            devices: Default::default(),
            update: Default::default(),
            cluster: Default::default(),
            dab: Default::default(),
            fm: Default::default(),
            cameras,
            mfi: Default::default(),
            config,
        }
    }

    #[test]
    fn the_camera_runs_only_while_main_shows_it() {
        assert_eq!(wanted(&state(Front::Livi, cams(&["a", "b"]))), None);
        assert_eq!(wanted(&state(Front::Camera, vec![])), None);
        assert_eq!(
            wanted(&state(Front::Camera, cams(&["a", "b"]))),
            Some(CameraOrder {
                device: "b".into(),
                rotation: 180,
                mirror: true
            })
        );
        assert_eq!(
            wanted(&state(Front::Camera, cams(&["a"]))).map(|o| o.device),
            Some("a".into())
        );
    }
}
