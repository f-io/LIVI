//! Input routing: host pointer/touch/keyboard into the inner seat. The
//! compositor decoration (titlebar buttons, move/resize borders) is hit-tested
//! first and handled there.

use smithay::backend::input::{ButtonState, KeyState};
use smithay::input::keyboard::{FilterResult, ModifiersState};
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent};
use smithay::input::touch::{DownEvent, MotionEvent as TouchMotionEvent, UpEvent};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, SERIAL_COUNTER};

use crate::state::{Kind, LiviState, BTN_GAP, BTN_W, RESIZE_BORDER, TITLEBAR_H};

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum DecoHit {
    None,
    Min,
    Fs,
    Close,
    Move,
    Resize(u32),
}

/// Hit-test the compositor decoration in layout coords.
pub fn deco_hit_test(state: &LiviState, lx: f64, ly: f64) -> (Option<usize>, DecoHit) {
    for (i, s) in state.screens.iter().enumerate() {
        if s.fullscreen || s.width <= 0 || s.height <= 0 {
            continue;
        }
        let (sx, sw, sh) = (s.x as f64, s.width as f64, s.height as f64);
        if lx < sx || lx >= sx + sw || ly < 0.0 || ly >= sh {
            continue;
        }
        let lxw = lx - sx;
        let mut edges = 0u32;
        if ly >= sh - RESIZE_BORDER as f64 {
            edges |= crate::host::EDGE_BOTTOM;
        }
        if lxw < RESIZE_BORDER as f64 {
            edges |= crate::host::EDGE_LEFT;
        }
        if lxw >= sw - RESIZE_BORDER as f64 {
            edges |= crate::host::EDGE_RIGHT;
        }
        if ly < TITLEBAR_H as f64 {
            let slot = (BTN_W + BTN_GAP) as f64;
            let close_x = sx + sw - slot;
            let fs_x = sx + sw - 2.0 * slot;
            let min_x = sx + sw - 3.0 * slot;
            if lx >= close_x && lx < close_x + BTN_W as f64 {
                return (Some(i), DecoHit::Close);
            }
            if lx >= fs_x && lx < fs_x + BTN_W as f64 {
                return (Some(i), DecoHit::Fs);
            }
            if lx >= min_x && lx < min_x + BTN_W as f64 {
                return (Some(i), DecoHit::Min);
            }
            if edges != 0 {
                return (Some(i), DecoHit::Resize(edges));
            }
            return (Some(i), DecoHit::Move);
        }
        if edges != 0 {
            return (Some(i), DecoHit::Resize(edges));
        }
        return (None, DecoHit::None);
    }
    (None, DecoHit::None)
}

/// Topmost surface at a layout point, with the surface's global origin
/// (smithay's focus convention). Z-order: dialogs > UI > videos > nothing.
pub fn surface_at(state: &LiviState, lx: f64, ly: f64) -> Option<(WlSurface, Point<f64, Logical>)> {
    let pt = Point::<f64, Logical>::from((lx, ly));
    let mut ordered: Vec<usize> = Vec::new();
    for (i, t) in state.toplevels.iter().enumerate() {
        if t.kind == Kind::Dialog {
            ordered.push(i);
        }
    }
    for (i, t) in state.toplevels.iter().enumerate() {
        if t.kind == Kind::Ui {
            ordered.push(i);
        }
    }
    for &i in state.video_order.iter().rev() {
        ordered.push(i);
    }
    for idx in ordered {
        let t = state.toplevels.get(idx)?;
        if t.kind == Kind::Video && !t.visible {
            continue;
        }
        let local = pt - Point::from((t.position.x as f64, t.position.y as f64));
        if let Some((surface, surf_local)) =
            crate::render::surface_under(t.toplevel.wl_surface(), local)
        {
            // origin of the hit surface in layout coords
            return Some((surface, pt - surf_local));
        }
    }
    None
}

pub fn pointer_clear_focus(state: &mut LiviState) {
    let pointer = state.seat.get_pointer();
    if let Some(pointer) = pointer {
        pointer.motion(
            state,
            None,
            &MotionEvent {
                location: Point::from((-1.0, -1.0)),
                serial: SERIAL_COUNTER.next_serial(),
                time: 0,
            },
        );
        pointer.frame(state);
    }
}

pub fn pointer_motion(state: &mut LiviState, time: u32) {
    let (lx, ly) = state.host.pointer_pos;
    let (deco_screen, _hit) = deco_hit_test(state, lx, ly);
    if deco_screen.is_some() {
        pointer_clear_focus(state);
        return;
    }
    let under = surface_at(state, lx, ly);
    let pointer = state.seat.get_pointer();
    if let Some(pointer) = pointer {
        pointer.motion(
            state,
            under.clone(),
            &MotionEvent {
                location: Point::from((lx, ly)),
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        pointer.frame(state);
    }
}

pub fn pointer_button(state: &mut LiviState, time: u32, button: u32, pressed: bool) {
    let (lx, ly) = state.host.pointer_pos;
    let (deco_screen, hit) = deco_hit_test(state, lx, ly);
    if let Some(screen_idx) = deco_screen {
        if pressed {
            match hit {
                DecoHit::Close => {
                    if let Some(ui) = state
                        .toplevels
                        .iter()
                        .find(|t| t.kind == Kind::Ui && t.screen_idx == screen_idx)
                    {
                        ui.toplevel.send_close();
                    }
                }
                DecoHit::Min => crate::host::minimize(state, screen_idx),
                DecoHit::Fs => crate::layout::toggle_fullscreen(state, screen_idx),
                DecoHit::Move => crate::host::begin_move(state, screen_idx),
                DecoHit::Resize(edges) => crate::host::begin_resize(state, screen_idx, edges),
                DecoHit::None => {}
            }
        }
        return;
    }

    let pointer = state.seat.get_pointer();
    if let Some(pointer) = pointer {
        pointer.button(
            state,
            &ButtonEvent {
                button,
                state: if pressed {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                },
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        pointer.frame(state);
    }
    if pressed
        && let Some((surface, _)) = surface_at(state, lx, ly) {
            focus_surface(state, &surface);
        }
}

pub fn focus_surface(state: &mut LiviState, surface: &WlSurface) {
    // Focus skips video planes.
    if state
        .toplevels
        .iter()
        .any(|t| t.kind == Kind::Video && t.toplevel.wl_surface() == surface)
    {
        return;
    }
    if let Some(keyboard) = state.seat.get_keyboard() {
        keyboard.set_focus(state, Some(surface.clone()), SERIAL_COUNTER.next_serial());
    }
    for t in &state.toplevels {
        let active = t.toplevel.wl_surface() == surface;
        t.toplevel.with_pending_state(|st| {
            if active {
                st.states.set(
                    smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Activated,
                );
            } else {
                st.states.unset(
                    smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Activated,
                );
            }
        });
        if t.toplevel.is_initial_configure_sent() {
            t.toplevel.send_pending_configure();
        }
    }
}

pub fn pointer_axis(state: &mut LiviState, time: u32, horizontal: f64, vertical: f64) {
    let pointer = state.seat.get_pointer();
    if let Some(pointer) = pointer {
        let mut frame = AxisFrame::new(time);
        if horizontal != 0.0 {
            frame = frame.value(smithay::backend::input::Axis::Horizontal, horizontal);
        }
        if vertical != 0.0 {
            frame = frame.value(smithay::backend::input::Axis::Vertical, vertical);
        }
        pointer.axis(state, frame);
        pointer.frame(state);
    }
}

pub fn touch_down(state: &mut LiviState, time: u32, id: i32, lx: f64, ly: f64, _screen_idx: usize) {
    let under = surface_at(state, lx, ly);
    log::debug!(
        "touch down id={id} at {lx:.0},{ly:.0} surface={}",
        if under.is_some() { "yes" } else { "NONE (dropped)" }
    );
    let Some((surface, origin)) = under else { return };
    if let Some(touch) = state.seat.get_touch() {
        touch.down(
            state,
            Some((surface, origin)),
            &DownEvent {
                slot: Some(id as u32).into(),
                location: Point::from((lx, ly)),
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        touch.frame(state);
    }
    state.host.touch_positions.push((id, lx, ly));
}

pub fn touch_motion(state: &mut LiviState, time: u32, id: i32, wx: f64, wy: f64) {
    // Window-local coords: keep the finger on the screen it went down on.
    let screen_x = state
        .host
        .touch_positions
        .iter()
        .find(|(tid, _, _)| *tid == id)
        .map(|(_, lx, _)| {
            let sx = *lx;
            state
                .screens
                .iter()
                .map(|s| s.x)
                .filter(|&x| (x as f64) <= sx)
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let (lx, ly) = (screen_x as f64 + wx, wy);
    if let Some(p) = state
        .host
        .touch_positions
        .iter_mut()
        .find(|(tid, _, _)| *tid == id)
    {
        p.1 = lx;
        p.2 = ly;
    }
    if let Some(touch) = state.seat.get_touch() {
        let under = surface_at(state, lx, ly);
        touch.motion(
            state,
            under,
            &TouchMotionEvent {
                slot: Some(id as u32).into(),
                location: Point::from((lx, ly)),
                time,
            },
        );
        touch.frame(state);
    }
}

pub fn touch_up(state: &mut LiviState, time: u32, id: i32) {
    log::debug!("touch up   id={id}");
    state.host.touch_positions.retain(|(tid, _, _)| *tid != id);
    if let Some(touch) = state.seat.get_touch() {
        touch.up(
            state,
            &UpEvent {
                slot: Some(id as u32).into(),
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        touch.frame(state);
    }
}

/// Ends every touch point the seat still holds when the host takes the input
/// away.
pub fn touch_cancel(state: &mut LiviState) {
    state.host.touch_positions.clear();
    if let Some(touch) = state.seat.get_touch() {
        touch.cancel(state);
    }
}

pub fn modifiers(state: &mut LiviState, m: smithay_client_toolkit::seat::keyboard::Modifiers) {
    state.host.alt_held = m.alt;
}

pub fn key(state: &mut LiviState, time: u32, raw_code: u32, pressed: bool, keysym: smithay_client_toolkit::seat::keyboard::Keysym) {
    // Alt keybindings: Esc quits, F11 toggles main fullscreen.
    if pressed && state.host.alt_held {
        match keysym {
            k if k == smithay_client_toolkit::seat::keyboard::Keysym::Escape => {
                state.running = false;
                return;
            }
            k if k == smithay_client_toolkit::seat::keyboard::Keysym::F11 => {
                if !state.screens.is_empty() {
                    crate::layout::toggle_fullscreen(state, 0);
                }
                return;
            }
            _ => {}
        }
    }
    if let Some(keyboard) = state.seat.get_keyboard() {
        keyboard.input::<(), _>(
            state,
            (raw_code + 8).into(),
            if pressed {
                KeyState::Pressed
            } else {
                KeyState::Released
            },
            SERIAL_COUNTER.next_serial(),
            time,
            |_, _, _| FilterResult::Forward,
        );
    }
    let _: Option<ModifiersState> = None;
}
