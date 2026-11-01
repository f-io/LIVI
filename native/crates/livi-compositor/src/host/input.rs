// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl SctkSeatHandler for LiviState {
    fn seat_state(&mut self) -> &mut SctkSeatState {
        self.host.sctk_seats.as_mut().unwrap()
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: CWlSeat) {
        self.host.host_seat = Some(seat);
    }

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: CWlSeat,
        capability: Capability,
    ) {
        self.host.host_seat = Some(seat.clone());
        let seats = self.host.sctk_seats.as_mut().unwrap();
        match capability {
            Capability::Pointer => {
                let surface = self
                    .host
                    .sctk_compositor
                    .as_ref()
                    .map(|c| c.create_surface(qh));
                self.host.pointer =
                    surface
                        .zip(self.host.shm.as_ref())
                        .and_then(|(surface, shm)| {
                            seats
                                .get_pointer_with_theme::<_, ()>(
                                    qh,
                                    &seat,
                                    shm.wl_shm(),
                                    surface,
                                    ThemeSpec::System,
                                )
                                .ok()
                        });
                self.seat.add_pointer();
            }
            Capability::Keyboard => {
                self.host.keyboard = seats.get_keyboard(qh, &seat, None).ok();
                self.seat.add_keyboard(Default::default(), 600, 25).ok();
            }
            Capability::Touch => {
                self.host.touch = seats.get_touch(qh, &seat).ok();
                self.host.has_touch_cap = true;
                self.seat.add_touch();
            }
            _ => {}
        }
        log::info!(
            "seat capabilities pointer={} keyboard={} touch={}",
            self.host.pointer.is_some(),
            self.host.keyboard.is_some(),
            self.host.touch.is_some()
        );
        apply_cursor(self);
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _seat: CWlSeat,
        capability: Capability,
    ) {
        log::info!("input device gone: {capability:?}");
        apply_cursor(self);
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: CWlSeat) {
        if self.host.host_seat.as_ref() == Some(&seat) {
            self.host.host_seat = None;
        }
    }
}

impl PointerHandler for LiviState {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _pointer: &WlPointer,
        events: &[PointerEvent],
    ) {
        for ev in events {
            let screen_idx = self.host.screen_for_surface(&ev.surface);
            match ev.kind {
                PointerEventKind::Enter { serial } => {
                    self.host.last_pointer_serial = serial;
                    self.host.pointer_screen = screen_idx;
                    apply_cursor(self);
                }
                PointerEventKind::Leave { .. } => {
                    self.host.pointer_screen = None;
                    self.host.pointer_on_deco = false;
                    crate::input::pointer_clear_focus(self);
                }
                PointerEventKind::Motion { time } => {
                    if let Some(idx) = screen_idx.or(self.host.pointer_screen) {
                        let s = &self.screens[idx];
                        self.host.pointer_pos = (s.x as f64 + ev.position.0, ev.position.1);
                        crate::input::pointer_motion(self, time);
                    }
                }
                PointerEventKind::Press {
                    time,
                    button,
                    serial,
                } => {
                    self.host.last_pointer_serial = serial;
                    crate::input::pointer_button(self, time, button, true);
                }
                PointerEventKind::Release {
                    time,
                    button,
                    serial,
                } => {
                    self.host.last_pointer_serial = serial;
                    crate::input::pointer_button(self, time, button, false);
                }
                PointerEventKind::Axis {
                    time,
                    vertical,
                    horizontal,
                    ..
                } => {
                    crate::input::pointer_axis(self, time, horizontal.absolute, vertical.absolute);
                }
            }
        }
    }
}

impl TouchHandler for LiviState {
    fn down(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _touch: &WlTouch,
        _serial: u32,
        time: u32,
        surface: CWlSurface,
        id: i32,
        position: (f64, f64),
    ) {
        let screen_idx = self.host.screen_for_surface(&surface).unwrap_or(0);
        let s = &self.screens[screen_idx];
        let (lx, ly) = (s.x as f64 + position.0, position.1);
        crate::input::touch_down(self, time, id.cast_unsigned(), lx, ly, screen_idx);
    }

    fn up(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _touch: &WlTouch,
        _serial: u32,
        time: u32,
        id: i32,
    ) {
        crate::input::touch_up(self, time, id.cast_unsigned());
    }

    fn motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _touch: &WlTouch,
        time: u32,
        id: i32,
        position: (f64, f64),
    ) {
        crate::input::touch_motion(self, time, id.cast_unsigned(), position.0, position.1);
    }

    fn shape(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlTouch,
        _: i32,
        _: f64,
        _: f64,
    ) {
    }
    fn orientation(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlTouch, _: i32, _: f64) {}

    fn cancel(&mut self, _: &Connection, _: &QueueHandle<Self>, _touch: &WlTouch) {
        crate::input::touch_cancel(self);
    }
}

impl KeyboardHandler for LiviState {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _surface: &CWlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: &CWlSurface,
        _serial: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        crate::input::key(self, event.time, event.raw_code, true, event.keysym);
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _serial: u32,
        _event: KeyEvent,
    ) {
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        crate::input::key(self, event.time, event.raw_code, false, event.keysym);
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _serial: u32,
        modifiers: Modifiers,
        _raw: RawModifiers,
        _layout: u32,
    ) {
        crate::input::modifiers(self, modifiers);
    }
}

impl ProvidesRegistryState for LiviState {
    fn registry(&mut self) -> &mut RegistryState {
        self.host.registry.as_mut().unwrap()
    }
    smithay_client_toolkit::registry_handlers![SctkOutputState, SctkSeatState];
}

impl SctkShmHandler for LiviState {
    fn shm_state(&mut self) -> &mut Shm {
        self.host.shm.as_mut().unwrap()
    }
}
