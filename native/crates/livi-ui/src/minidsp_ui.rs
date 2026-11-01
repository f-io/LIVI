// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::cell::RefCell;
use std::rc::Rc;

use livi_core_client::Client;
use livi_core_proto::message::Action;
use livi_ui_model::minidsp::{Control, Phase, View, db, outputs, status_line};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::{AppWindow, DspControlData, DspPhase, DspPresetData, MiniDspState};

thread_local! {
    static SHOWN: RefCell<Option<View>> = const { RefCell::new(None) };
}

fn control(c: &Control) -> DspControlData {
    DspControlData {
        value: db(c.value).into(),
        level: c.level(),
        at_min: c.value <= c.min,
        at_max: c.value >= c.max,
    }
}

fn phase(p: Phase) -> DspPhase {
    match p {
        Phase::Connecting => DspPhase::Connecting,
        Phase::Error => DspPhase::Error,
        Phase::Waiting => DspPhase::Waiting,
        Phase::Ready => DspPhase::Ready,
    }
}

fn show(ui: &AppWindow, v: &View) {
    let s = ui.global::<MiniDspState>();
    let tr = crate::settings_ui::tr;
    s.set_phase(phase(v.phase));
    s.set_busy(v.busy);
    s.set_status_line(status_line(v, tr).into());
    s.set_error(v.error.clone().into());
    s.set_volume(control(&v.volume));
    s.set_bass(control(&v.bass));
    let list = outputs(&v.bass_outputs);
    s.set_bass_outputs(
        crate::settings_ui::tr("minidsp.outputs", "Outputs {{outputs}}")
            .replace("{{outputs}}", &list)
            .into(),
    );
    let presets: Vec<DspPresetData> = v
        .presets
        .iter()
        .map(|p| DspPresetData {
            index: p.index as i32,
            label: p.label.clone().into(),
            active: p.active,
        })
        .collect();
    s.set_presets(ModelRc::from(Rc::new(VecModel::from(presets))));
}

pub fn apply(ui: &AppWindow, v: View) {
    show(ui, &v);
    SHOWN.with_borrow_mut(|s| *s = Some(v));
}

pub fn set_texts(ui: &AppWindow, tr: impl Fn(&str, &str) -> String) {
    let s = ui.global::<MiniDspState>();
    s.set_title(tr("minidsp.title", "MiniDSP").into());
    s.set_refresh_label(tr("minidsp.refresh", "Refresh").into());
    s.set_unavailable_title(tr("minidsp.unavailableTitle", "MiniDSP unavailable").into());
    s.set_no_device(tr("minidsp.noDevice", "No MiniDSP connected").into());
    s.set_connect_hint(
        tr(
            "minidsp.connectHint",
            "Connect your MiniDSP, then refresh to try again.",
        )
        .into(),
    );
    s.set_volume_title(tr("minidsp.masterVolume", "Master volume").into());
    s.set_volume_subtitle(tr("minidsp.fullSystemOutput", "Full system output").into());
    s.set_bass_title(tr("minidsp.bass", "Bass").into());
    s.set_presets_title(tr("minidsp.presets", "Presets").into());
    SHOWN.with_borrow(|v| {
        if let Some(v) = v {
            show(ui, v);
        }
    });
}

/// Shows `value` on the control at once, ahead of the device's answer.
fn preview(ui: &AppWindow, bass: bool, value: f64) {
    SHOWN.with_borrow_mut(|v| {
        if let Some(v) = v {
            let c = if bass { &mut v.bass } else { &mut v.volume };
            c.value = value;
            show(ui, v);
        }
    });
}

fn control_of(bass: bool) -> Option<Control> {
    SHOWN.with_borrow(|v| v.as_ref().map(|v| if bass { v.bass } else { v.volume }))
}

fn send(client: &Client, bass: bool, value: f64) {
    client.act(if bass {
        Action::MiniDspSetBass { gain_db: value }
    } else {
        Action::MiniDspSetVolume { volume_db: value }
    });
}

pub fn busy() -> bool {
    SHOWN.with_borrow(|v| v.as_ref().is_some_and(|v| v.busy))
}

/// Steps a control by one, from a button or a key.
pub fn nudge(ui: &AppWindow, client: &Client, bass: bool, direction: i32) {
    if busy() {
        return;
    }
    let Some(c) = control_of(bass) else { return };
    let next = c.nudge(direction);
    if next == c.value {
        return;
    }
    preview(ui, bass, next);
    send(client, bass, next);
}

pub fn ready() -> bool {
    SHOWN.with_borrow(|v| v.as_ref().is_some_and(|v| v.phase == Phase::Ready))
}

pub fn has_empty_state() -> bool {
    SHOWN.with_borrow(|v| {
        v.as_ref()
            .is_some_and(|v| matches!(v.phase, Phase::Error | Phase::Waiting))
    })
}

pub fn wire(ui: &AppWindow, client: &Client) {
    let s = ui.global::<MiniDspState>();
    let weak = ui.as_weak();
    s.on_refresh({
        let c = client.clone();
        move || c.act(Action::MiniDspRefresh)
    });
    s.on_nudge({
        let c = client.clone();
        let weak = weak.clone();
        move |bass, direction| {
            if let Some(ui) = weak.upgrade() {
                nudge(&ui, &c, bass, direction);
            }
        }
    });
    s.on_slide({
        let weak = weak.clone();
        move |bass, level| {
            if let (Some(ui), Some(c)) = (weak.upgrade(), control_of(bass)) {
                preview(&ui, bass, c.at_level(level));
            }
        }
    });
    s.on_slide_done({
        let c = client.clone();
        let weak = weak.clone();
        move |bass, level| {
            if let (Some(ui), Some(now)) = (weak.upgrade(), control_of(bass)) {
                let value = now.at_level(level);
                preview(&ui, bass, value);
                send(&c, bass, value);
            }
        }
    });
    s.on_pick({
        let c = client.clone();
        move |index| {
            if !busy() && index >= 0 {
                c.act(Action::MiniDspSelectPreset {
                    preset: index as u32,
                });
            }
        }
    });
}
