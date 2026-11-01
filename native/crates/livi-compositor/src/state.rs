// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::VecDeque;
use std::ffi::OsString;
use std::time::Instant;

use smithay::input::{Seat, SeatState};
use smithay::output::Output;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{EventLoop, Interest, LoopHandle, Mode, PostAction};
use smithay::reexports::wayland_server::backend::ClientData;
use smithay::reexports::wayland_server::{Display, DisplayHandle};
use smithay::utils::{Logical, Point};
use smithay::wayland::compositor::CompositorState;
use smithay::wayland::cursor_shape::CursorShapeManagerState;
use smithay::wayland::dmabuf::DmabufState;
use smithay::wayland::drm_syncobj::DrmSyncobjState;
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::shell::xdg::decoration::XdgDecorationState;
use smithay::wayland::shell::xdg::{ToplevelSurface, XdgShellState};
use smithay::wayland::shm::ShmState;
use smithay::wayland::socket::ListeningSocketSource;
use smithay::wayland::viewporter::ViewporterState;

use crate::host::HostState;

pub const SCREEN_X_SLOT: i32 = 100_000;
pub const TITLEBAR_H: i32 = 32;
pub const BTN_W: i32 = 32;
pub const BTN_GAP: i32 = 2;
pub const RESIZE_BORDER: i32 = 8;

#[derive(Default, Clone)]
pub struct VideoCfg {
    pub tag: String,
    pub screen: String,
    pub has_crop: bool,
    pub crop_l: f64,
    pub crop_t: f64,
    pub vis_w: f64,
    pub vis_h: f64,
    pub tier_w: f64,
    pub tier_h: f64,
    pub has_visible: bool,
    pub visible: bool,
}

#[derive(Clone, PartialEq)]
pub enum Kind {
    Pending,
    Ui,
    Video,
    Dialog,
}

pub struct TopLevel {
    pub toplevel: ToplevelSurface,
    pub kind: Kind,
    pub screen_idx: usize,
    pub tag: String,
    pub awaiting_claim: bool,
    pub visible: bool,
    pub has_crop: bool,
    pub crop_l: f64,
    pub crop_t: f64,
    pub vis_w: f64,
    pub vis_h: f64,
    pub tier_w: f64,
    pub tier_h: f64,
    pub position: Point<i32, Logical>,
    pub clear: bool,
    pub fill: bool,
    pub mapped: bool,
}

pub struct Screen {
    pub role: String,
    pub x: i32,
    pub width: i32,
    pub height: i32,
    pub req_width: i32,
    pub req_height: i32,
    pub fullscreen: bool,
    pub output: Option<Output>,
    pub resize_pending: Option<Instant>,
    pub applied_width: i32,
    pub applied_height: i32,
    pub backdrop_color: [f32; 4],
    pub has_backdrop_color: bool,
    pub aspect: Option<(i32, i32)>,
}

impl Screen {
    pub fn top_inset(&self) -> i32 {
        if self.fullscreen { 0 } else { TITLEBAR_H }
    }
}

pub struct CalState {
    pub in_lut: bool,
    pub active: bool,
    pub gamma: f32,
    pub contrast: f32,
    pub gain: [f32; 3],
}

pub struct LiviState {
    pub running: bool,
    pub display_handle: DisplayHandle,
    pub ui_socket: String,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub _xdg_decoration_state: XdgDecorationState,
    pub seat_state: SeatState<LiviState>,
    pub seat: Seat<LiviState>,
    pub data_device_state: DataDeviceState,
    pub shm_state: ShmState,
    pub dmabuf_state: DmabufState,
    pub render_node: Option<std::path::PathBuf>,
    pub syncobj_state: Option<DrmSyncobjState>,
    pub loop_handle: LoopHandle<'static, LiviState>,
    pub _viewporter_state: ViewporterState,
    pub _cursor_shape_state: CursorShapeManagerState,

    pub output_app_id: String,
    pub fill_app_ids: Vec<String>,
    pub screens: Vec<Screen>,
    pub toplevels: Vec<TopLevel>,
    pub video_order: Vec<usize>,
    pub pending_video_tags: VecDeque<String>,
    pub video_cfgs: Vec<VideoCfg>,

    pub cal: CalState,
    pub renderer: Option<smithay::backend::renderer::gles::GlesRenderer>,
    pub cal_program: Option<smithay::backend::renderer::gles::GlesTexProgram>,
    pub host: HostState,
    pub drm: Option<crate::drm::DrmState>,
    pub fingers: crate::projection::Fingers,

    pub ctrl_client: Option<std::os::unix::net::UnixStream>,
    pub ctrl_buf: String,
    pub ctrl_out: Vec<u8>,
    pub ctrl_path: Option<String>,

    pub startup_cmd: Option<String>,
    pub startup_pid: Option<nix::unistd::Pid>,
    pub full_restart: bool,
    pub restart_deadline: Option<Instant>,
    pub saved_argv: Vec<OsString>,
}

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: smithay::wayland::compositor::CompositorClientState,
}

impl ClientData for ClientState {}

impl LiviState {
    pub fn new(
        event_loop: &mut EventLoop<'static, LiviState>,
        startup_cmd: Option<String>,
    ) -> Self {
        let mut display: Display<LiviState> = Display::new().expect("wayland display");
        let dh = display.handle();

        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<Self>(&dh);
        let mut seat_state = SeatState::new();
        let seat = seat_state.new_wl_seat(&dh, "seat0");
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let dmabuf_state = DmabufState::new();
        let viewporter_state = ViewporterState::new::<Self>(&dh);
        let cursor_shape_state = CursorShapeManagerState::new::<Self>(&dh);

        let source = ListeningSocketSource::new_auto().expect("wayland socket");
        let ui_socket = source.socket_name().to_string_lossy().into_owned();
        let loop_handle = event_loop.handle();
        loop_handle
            .insert_source(source, |client_stream, _, state: &mut LiviState| {
                let _ = state
                    .display_handle
                    .insert_client(client_stream, std::sync::Arc::new(ClientState::default()));
            })
            .expect("insert wayland socket");

        let poll_fd = display.backend().poll_fd().try_clone_to_owned().unwrap();
        loop_handle
            .insert_source(Generic::new(poll_fd, Interest::READ, Mode::Level), {
                let mut display = display;
                move |_, _, state: &mut LiviState| {
                    display.dispatch_clients(state).unwrap();
                    Ok(PostAction::Continue)
                }
            })
            .expect("insert display source");

        let kiosk = std::env::var("LIVI_KIOSK")
            .map(|v| v != "0")
            .unwrap_or(false);
        let roles = std::env::var("LIVI_SCREENS").unwrap_or_default();
        let roles = if roles.is_empty() {
            "main,cluster,aux"
        } else {
            &roles
        };
        let screens: Vec<Screen> = roles
            .split(',')
            .filter(|r| !r.is_empty())
            .take(8)
            .enumerate()
            .map(|(i, role)| Screen {
                role: role.to_string(),
                x: i as i32 * SCREEN_X_SLOT,
                width: 0,
                height: 0,
                req_width: 0,
                req_height: 0,
                fullscreen: kiosk,
                output: None,
                resize_pending: None,
                applied_width: 0,
                applied_height: 0,
                backdrop_color: [0.0, 0.0, 0.0, 1.0],
                has_backdrop_color: false,
                aspect: None,
            })
            .collect();

        let output_app_id =
            std::env::var("LIVI_OUTPUT_APP_ID").unwrap_or_else(|_| "dev.f-io.livi".to_string());

        Self {
            running: true,
            display_handle: dh,
            ui_socket,
            compositor_state,
            xdg_shell_state,
            _xdg_decoration_state: xdg_decoration_state,
            seat_state,
            seat,
            data_device_state,
            shm_state,
            dmabuf_state,
            render_node: None,
            syncobj_state: None,
            loop_handle: loop_handle.clone(),
            _viewporter_state: viewporter_state,
            _cursor_shape_state: cursor_shape_state,
            output_app_id,
            fill_app_ids: crate::layout::fill_app_ids(
                std::env::var("LIVI_FULLSCREEN_APP_IDS").ok().as_deref(),
            ),
            screens,
            toplevels: Vec::new(),
            video_order: Vec::new(),
            pending_video_tags: VecDeque::new(),
            video_cfgs: Vec::new(),
            cal: CalState {
                in_lut: false,
                active: false,
                gamma: 1.0,
                contrast: 1.0,
                gain: [1.0, 1.0, 1.0],
            },
            renderer: None,
            cal_program: None,
            host: HostState::new(),
            drm: None,
            fingers: Default::default(),
            ctrl_client: None,
            ctrl_buf: String::new(),
            ctrl_out: Vec::new(),
            ctrl_path: std::env::var("LIVI_COMPOSITOR_CTRL")
                .ok()
                .filter(|p| !p.is_empty()),
            startup_cmd,
            startup_pid: None,
            full_restart: false,
            restart_deadline: None,
            saved_argv: std::env::args_os().collect(),
        }
    }

    pub fn screen_idx_by_role(&self, role: &str) -> Option<usize> {
        self.screens.iter().position(|s| s.role == role)
    }

    pub fn cfg_for_tag(&mut self, tag: &str) -> &mut VideoCfg {
        if let Some(i) = self.video_cfgs.iter().position(|c| c.tag == tag) {
            return &mut self.video_cfgs[i];
        }
        self.video_cfgs.push(VideoCfg {
            tag: tag.to_string(),
            ..Default::default()
        });
        self.video_cfgs.last_mut().unwrap()
    }

    pub fn find_video_by_tag(&self, tag: &str) -> Option<usize> {
        self.toplevels
            .iter()
            .position(|t| t.kind == Kind::Video && t.tag == tag)
    }

    pub fn after_dispatch(&mut self) {
        if let Some(deadline) = self.restart_deadline
            && Instant::now() >= deadline
        {
            crate::spawn::force_restart(self);
        }
        crate::projection::flush(self);
        crate::host::apply_settled_resizes(self);
        crate::host::pump(self);
        self.display_handle.flush_clients().ok();
    }
}

pub fn role_title(role: &str) -> &str {
    match role {
        "main" => "LIVI",
        "cluster" => "Cluster",
        "aux" => "Auxiliary",
        other => other,
    }
}
