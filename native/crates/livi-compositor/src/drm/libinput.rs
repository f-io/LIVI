// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;
use smithay::reexports::input::event::touch::TouchEventSlot;
use smithay::reexports::input::{Device, DeviceCapability};

pub(super) fn touch_output<'a>(drm: &'a DrmState, device: &Device) -> Option<&'a Output> {
    let linked = drm
        .touch_cal
        .get(device.name())
        .filter(|c| !c.display.is_empty())
        .and_then(|c| {
            drm.connectors
                .iter()
                .find(|k| k.connected && k.id == c.display)
        })
        .map(|k| &k.name);
    match linked.or_else(|| drm.touch_outputs.get(device.sysname())) {
        Some(name) => drm.outputs.iter().find(|o| &o.connector == name),
        None => drm.outputs.iter().find(|o| o.screen_idx == 0),
    }
}

pub(super) fn touch_target(
    state: &LiviState,
    device: &Device,
) -> (usize, Size<i32, smithay::utils::Logical>, Transform) {
    let output = state.drm.as_ref().and_then(|d| touch_output(d, device));
    match output {
        Some(o) => (o.screen_idx, Size::from(o.physical()), o.transform),
        None => (0, screen_size(state, 0), Transform::Normal),
    }
}

pub(super) fn touch_point(
    state: &LiviState,
    device: &Device,
    at: impl Fn(Size<i32, smithay::utils::Logical>) -> Point<f64, smithay::utils::Logical>,
) -> (usize, f64, f64) {
    let (idx, physical, transform) = touch_target(state, device);
    let native = at(physical);
    let area =
        Size::<f64, smithay::utils::Logical>::from((f64::from(physical.w), f64::from(physical.h)));
    let local = transform.transform_point_in(native, &area);
    let size = transform.transform_size(area);
    let matrix = state
        .drm
        .as_ref()
        .and_then(|d| d.touch_cal.get(device.name()))
        .filter(|c| c.enabled)
        .map(|c| c.matrix);
    let (x, y) = match matrix {
        Some(m) => calibrated(&m, local.x, local.y, size.w, size.h),
        None => (local.x, local.y),
    };
    (idx, state.screens[idx].x as f64 + x, y)
}

pub(super) fn screen_size(state: &LiviState, idx: usize) -> Size<i32, smithay::utils::Logical> {
    let s = &state.screens[idx];
    (s.width.max(1), s.height.max(1)).into()
}

pub(super) fn pointer_moved(state: &mut LiviState, x: f64, y: f64, time: u32) {
    let s = &state.screens[0];
    let x = x.clamp(0.0, (s.width - 1).max(0) as f64);
    let y = y.clamp(0.0, (s.height - 1).max(0) as f64);
    state.host.pointer_pos = (s.x as f64 + x, y);
    if let Some(drm) = state.drm.as_mut() {
        drm.mouse_active = true;
    }
    damage(state);
    crate::input::pointer_motion(state, time);
}

pub(super) fn touched(state: &mut LiviState) {
    if let Some(drm) = state.drm.as_mut()
        && drm.mouse_active
    {
        drm.mouse_active = false;
        damage(state);
    }
}

pub(super) fn wl_output_of(sysname: &str) -> Option<String> {
    let dev = std::fs::read_to_string(format!("/sys/class/input/{sysname}/dev")).ok()?;
    let data = std::fs::read_to_string(format!("/run/udev/data/c{}", dev.trim())).ok()?;
    wl_output_in(&data)
}

pub(super) fn wl_output_in(udev_data: &str) -> Option<String> {
    udev_data
        .lines()
        .find_map(|l| l.strip_prefix("E:WL_OUTPUT="))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

pub(super) fn device_added(state: &mut LiviState, device: &Device) {
    log::info!("input device: {}", device.name());
    let Some(drm) = state.drm.as_mut() else {
        return;
    };
    if let Some(output) = wl_output_of(device.sysname()) {
        log::info!("{} touches {output}", device.name());
        drm.touch_outputs
            .insert(device.sysname().to_string(), output);
    }
    if device.has_capability(DeviceCapability::Touch) {
        drm.touch_devices.push(device.clone());
        crate::ctrl::send_displays(state);
    }
}

fn device_removed(state: &mut LiviState, device: &Device) {
    log::info!("input device gone: {}", device.name());
    let Some(drm) = state.drm.as_mut() else {
        return;
    };
    drm.touch_outputs.remove(device.sysname());
    let before = drm.touch_devices.len();
    drm.touch_devices
        .retain(|d| d.sysname() != device.sysname());
    if drm.touch_devices.len() != before {
        crate::ctrl::send_displays(state);
    }
}

pub(super) fn input(state: &mut LiviState, event: InputEvent<LibinputInputBackend>) {
    match event {
        InputEvent::Keyboard { event } => key(state, event),
        InputEvent::PointerMotion { event } => {
            let (x, y) = state.host.pointer_pos;
            let x = x - state.screens[0].x as f64 + event.delta_x();
            let y = y + event.delta_y();
            pointer_moved(state, x, y, event.time_msec());
        }
        InputEvent::PointerMotionAbsolute { event } => {
            let at = event.position_transformed(screen_size(state, 0));
            pointer_moved(state, at.x, at.y, event.time_msec());
        }
        InputEvent::PointerButton { event } => {
            if let Some(drm) = state.drm.as_mut() {
                drm.mouse_active = true;
            }
            damage(state);
            let pressed = event.state() == ButtonState::Pressed;
            crate::input::pointer_button(state, event.time_msec(), event.button_code(), pressed);
        }
        InputEvent::PointerAxis { event } => {
            let amount = |axis| {
                event
                    .amount(axis)
                    .or_else(|| event.amount_v120(axis).map(|v| v * 15.0 / 120.0))
                    .unwrap_or(0.0)
            };
            let (horizontal, vertical) = (amount(Axis::Horizontal), amount(Axis::Vertical));
            crate::input::pointer_axis(state, event.time_msec(), horizontal, vertical);
        }
        InputEvent::TouchDown { event } => {
            touched(state);
            let (idx, x, y) = touch_point(state, &event.device(), |size| {
                event.position_transformed(size)
            });
            crate::input::touch_down(state, event.time_msec(), event.seat_slot(), x, y, idx);
        }
        InputEvent::TouchMotion { event } => {
            let (idx, x, y) = touch_point(state, &event.device(), |size| {
                event.position_transformed(size)
            });
            let local_x = x - state.screens[idx].x as f64;
            crate::input::touch_motion(state, event.time_msec(), event.seat_slot(), local_x, y);
        }
        InputEvent::TouchUp { event } => {
            crate::input::touch_up(state, event.time_msec(), event.seat_slot());
        }
        InputEvent::TouchCancel { .. } => crate::input::touch_cancel(state),
        InputEvent::DeviceAdded { device } => device_added(state, &device),
        InputEvent::DeviceRemoved { device } => device_removed(state, &device),
        _ => {}
    }
}

pub(super) fn key(
    state: &mut LiviState,
    event: <LibinputInputBackend as smithay::backend::input::InputBackend>::KeyboardKeyEvent,
) {
    let Some(keyboard) = state.seat.get_keyboard() else {
        return;
    };
    let pressed = event.state() == KeyState::Pressed;
    let action = keyboard.input::<KeyAction, _>(
        state,
        event.key_code(),
        event.state(),
        SERIAL_COUNTER.next_serial(),
        event.time_msec(),
        |_, modifiers, handle| {
            let sym = handle.modified_sym().raw();
            let first = Keysym::XF86_Switch_VT_1.raw();
            if pressed && (first..=Keysym::XF86_Switch_VT_12.raw()).contains(&sym) {
                return FilterResult::Intercept(KeyAction::SwitchVt((sym - first + 1) as i32));
            }
            if pressed && modifiers.alt && sym == Keysym::Escape.raw() {
                return FilterResult::Intercept(KeyAction::Quit);
            }
            FilterResult::Forward
        },
    );
    match action {
        Some(KeyAction::SwitchVt(vt)) => {
            if let Some(drm) = state.drm.as_mut()
                && let Err(e) = drm.session.change_vt(vt)
            {
                log::error!("cannot switch to vt {vt}: {e}");
            }
        }
        Some(KeyAction::Quit) => state.running = false,
        None => {}
    }
}
