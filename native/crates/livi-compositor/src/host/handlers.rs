// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl SctkCompositorHandler for LiviState {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &CWlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &CWlSurface,
        _: CTransform,
    ) {
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &CWlSurface,
        output: &WlOutput,
    ) {
        if let Some(idx) = self.host.screen_for_surface(surface) {
            if let Some(w) = self.host.window_for_screen(idx) {
                w.host_output = Some(output.clone());
            }
            crate::ctrl::send_panels(self);
        }
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &CWlSurface,
        _: &WlOutput,
    ) {
        if let Some(idx) = self.host.screen_for_surface(surface)
            && let Some(w) = self.host.window_for_screen(idx)
        {
            w.host_output = None;
        }
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &CWlSurface, _time: u32) {
        if let Some(idx) = self.host.screen_for_surface(surface)
            && let Some(w) = self.host.window_for_screen(idx)
        {
            w.frame_pending = false;
            if w.needs_redraw {
                crate::render::render_screen(self, idx);
            }
        }
    }
}

impl SctkOutputHandler for LiviState {
    fn output_state(&mut self) -> &mut SctkOutputState {
        self.host.sctk_outputs.as_mut().unwrap()
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _output: WlOutput) {
        crate::ctrl::send_panels(self);
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _output: WlOutput) {
        crate::ctrl::send_panels(self);
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _output: WlOutput) {}
}

impl WindowHandler for LiviState {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, window: &Window) {
        let Some(idx) = self
            .host
            .windows
            .iter()
            .position(|(_, w)| &w.window == window)
        else {
            return;
        };
        let screen_idx = self.host.windows[idx].0;
        close_screen(self, screen_idx);
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        let Some(pos) = self
            .host
            .windows
            .iter()
            .position(|(_, w)| &w.window == window)
        else {
            return;
        };
        let screen_idx = self.host.windows[pos].0;
        let cur = (
            self.host.windows[pos].1.width,
            self.host.windows[pos].1.height,
        );
        let w = configure
            .new_size
            .0
            .map(|v| v.get() as i32)
            .unwrap_or(cur.0);
        let h = configure
            .new_size
            .1
            .map(|v| v.get() as i32)
            .unwrap_or(cur.1);
        let host_fullscreen = configure.is_fullscreen();
        let _ = configure.decoration_mode == DecorationMode::Server;
        let floating = !(host_fullscreen
            || configure.is_maximized()
            || configure.state.intersects(WindowState::TILED));

        let report = {
            let hw = &mut self.host.windows[pos].1;
            let report = hw.configured && hw.fullscreen_seen != host_fullscreen;
            hw.configured = true;
            hw.floating = floating;
            hw.fullscreen_seen = host_fullscreen;
            report
        };
        if self.screens[screen_idx].fullscreen != host_fullscreen {
            self.screens[screen_idx].fullscreen = host_fullscreen;
        }
        if report {
            let line = format!(
                "fullscreen {} {}\n",
                self.screens[screen_idx].role,
                u8::from(host_fullscreen)
            );
            crate::ctrl::send(self, &line);
        }
        let size = match self.screens[screen_idx].aspect {
            Some(aspect) if floating => fit_aspect((w, h), TITLEBAR_H, aspect),
            _ => (w, h),
        };
        resize_host_window(self, screen_idx, size);
    }
}
