use smithay::backend::renderer::utils::on_commit_buffer_handler;
use smithay::input::pointer::{CursorIcon, CursorImageStatus};
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecoMode;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State as XdgState;
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_seat::WlSeat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, Resource};
use smithay::utils::{Point, Serial};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    add_blocker, add_pre_commit_hook, get_parent, is_sync_subsurface, with_states, CompositorClientState,
    CompositorHandler, CompositorState,
};
use smithay::wayland::dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier};
use smithay::wayland::drm_syncobj::{DrmSyncobjCachedState, DrmSyncobjHandler, DrmSyncobjState};
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::shell::xdg::decoration::XdgDecorationHandler;
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::wayland::tablet_manager::TabletSeatHandler;

use crate::state::{Kind, LiviState, TopLevel};

impl CompositorHandler for LiviState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<crate::state::ClientState>().unwrap().compositor_state
    }

    fn new_surface(&mut self, surface: &WlSurface) {
        // A buffer with an acquire point is only read once the client's GPU work on it is done.
        add_pre_commit_hook::<Self, _>(surface, |state, _dh, surface| {
            let acquire = with_states(surface, |data| {
                data.cached_state.get::<DrmSyncobjCachedState>().pending().acquire_point.clone()
            });
            let (Some(acquire), Some(client)) = (acquire, surface.client()) else {
                return;
            };
            let Ok((blocker, source)) = acquire.generate_blocker() else {
                return;
            };
            let cleared = state.loop_handle.insert_source(source, move |_, _, state| {
                let dh = state.display_handle.clone();
                state.client_compositor_state(&client).blocker_cleared(state, &dh);
                Ok(())
            });
            if cleared.is_ok() {
                add_blocker(surface, blocker);
            }
        });
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        if is_sync_subsurface(surface) {
            return;
        }
        let mut root = surface.clone();
        while let Some(parent) = get_parent(&root) {
            root = parent;
        }
        if let Some(idx) = self.toplevels.iter().position(|t| t.toplevel.wl_surface() == &root) {
            classify_on_initial_commit(self, idx);
            self.center_dialog_by_surface(&root);
            crate::host::damage_all(self);
        }
    }
}

fn classify_on_initial_commit(state: &mut LiviState, idx: usize) {
    if state.toplevels[idx].kind != Kind::Pending {
        return;
    }
    let toplevel = state.toplevels[idx].toplevel.clone();
    if !toplevel.is_initial_configure_sent() {
        toplevel.with_pending_state(|st| {
            st.decoration_mode = Some(DecoMode::ServerSide);
        });
        toplevel.send_configure();
        return;
    }

    let (app_id, title) =
        smithay::wayland::compositor::with_states(toplevel.wl_surface(), |states| {
            let attrs = states
                .data_map
                .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                .unwrap()
                .lock()
                .unwrap();
            (attrs.app_id.clone(), attrs.title.clone())
        });

    let is_video = app_id.as_deref() == Some("livi-video");
    if is_video {
        state.toplevels[idx].kind = Kind::Video;
        if let Some(tag) = state.pending_video_tags.pop_front() {
            state.toplevels[idx].tag = tag.clone();
            log::info!("bound '{tag}' (pending left: {:?})", state.pending_video_tags);
            crate::ctrl::send(state, &format!("bound {tag}\n"));
            drop_stale_planes(state, idx, &tag);
        }
        state.toplevels[idx].screen_idx = 0;
        if state.toplevels[idx].tag.is_empty() {
            state.toplevels[idx].visible = false;
            state.toplevels[idx].awaiting_claim = true;
            log::warn!("video plane arrived before its claim, waiting for one");
        }
        stack_video(state, idx);
        let tag = state.toplevels[idx].tag.clone();
        log::info!(
            "app_id={app_id:?} title={title:?} tag='{tag}' -> video on screen '{}'",
            state.screens[0].role
        );
        crate::layout::apply_video_layout(state, idx);
        if !tag.is_empty() {
            crate::layout::apply_cfg_to_video(state, &tag, idx);
        }
        return;
    }

    let mut screen_idx = 0;
    if let Some(t) = title.as_deref()
        && let Some(role) = t.strip_prefix("livi:")
        && let Some(i) = state.screen_idx_by_role(role)
    {
        screen_idx = i;
    }
    let is_dialog = app_id.as_deref() != Some(state.output_app_id.as_str());
    state.toplevels[idx].screen_idx = screen_idx;
    state.toplevels[idx].kind = if is_dialog { Kind::Dialog } else { Kind::Ui };
    log::info!(
        "app_id={app_id:?} title={title:?} -> {} on screen '{}'",
        if is_dialog { "dialog" } else { "ui" },
        state.screens[screen_idx].role
    );
    if is_dialog {
        center_dialog(state, idx);
    } else {
        crate::layout::apply_ui_layout(state, screen_idx);
    }
    if !has_keyboard_focus(state) {
        let surface = state.toplevels[idx].toplevel.wl_surface().clone();
        crate::input::focus_surface(state, &surface);
    }
}

fn has_keyboard_focus(state: &LiviState) -> bool {
    state.seat.get_keyboard().is_some_and(|k| k.current_focus().is_some())
}

fn main_ui_surface(state: &LiviState) -> Option<WlSurface> {
    state
        .toplevels
        .iter()
        .find(|t| t.kind == Kind::Ui && t.screen_idx == 0)
        .map(|t| t.toplevel.wl_surface().clone())
}

/// Bottom to top: cluster, main, video.
fn video_rank(tag: &str) -> u8 {
    match tag {
        "video" => 2,
        "main" => 1,
        _ => 0,
    }
}

fn stack_video(state: &mut LiviState, idx: usize) {
    let rank = video_rank(&state.toplevels[idx].tag);
    let at = state
        .video_order
        .iter()
        .position(|&i| state.toplevels.get(i).is_some_and(|t| video_rank(&t.tag) > rank))
        .unwrap_or(state.video_order.len());
    state.video_order.insert(at, idx);
}

fn drop_stale_planes(state: &mut LiviState, keep: usize, tag: &str) {
    for (i, t) in state.toplevels.iter_mut().enumerate() {
        if i != keep && t.kind == Kind::Video && t.tag == tag {
            t.tag.clear();
            t.visible = false;
        }
    }
}

/// The claim and the plane's window travel over different sockets, so either can come first.
pub fn bind_waiting_plane(state: &mut LiviState, tag: &str) -> bool {
    let Some(idx) = state.toplevels.iter().rposition(|t| t.kind == Kind::Video && t.awaiting_claim)
    else {
        return false;
    };

    state.toplevels[idx].tag = tag.to_string();
    state.toplevels[idx].awaiting_claim = false;
    drop_stale_planes(state, idx, tag);

    state.video_order.retain(|&i| i != idx);
    stack_video(state, idx);

    crate::ctrl::send(state, &format!("bound {tag}\n"));
    crate::layout::apply_cfg_to_video(state, tag, idx);
    true
}

impl LiviState {
    pub fn center_dialog_by_surface(&mut self, surface: &WlSurface) {
        if let Some(idx) = self
            .toplevels
            .iter()
            .position(|t| t.toplevel.wl_surface() == surface && t.kind == Kind::Dialog)
        {
            center_dialog(self, idx);
        }
    }
}

fn center_dialog(state: &mut LiviState, idx: usize) {
    let s = &state.screens[state.toplevels[idx].screen_idx];
    let (w, h) = crate::render::surface_size(state.toplevels[idx].toplevel.wl_surface());
    if w > 0 && h > 0 {
        let x = (s.x + (s.width - w) / 2).max(s.x);
        let y = ((s.height - h) / 2).max(0);
        state.toplevels[idx].position = Point::from((x, y));
    }
}

impl BufferHandler for LiviState {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl XdgShellHandler for LiviState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        self.toplevels.push(TopLevel {
            toplevel: surface,
            kind: Kind::Pending,
            screen_idx: 0,
            tag: String::new(),
            awaiting_claim: false,
            visible: true,
            has_crop: false,
            crop_l: 0.0,
            crop_t: 0.0,
            vis_w: 0.0,
            vis_h: 0.0,
            tier_w: 0.0,
            tier_h: 0.0,
            position: Point::from((0, 0)),
        });
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        let _ = surface.send_configure();
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {}

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|st| {
            st.geometry = positioner.get_geometry();
        });
        surface.send_repositioned(token);
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let Some(idx) = self.toplevels.iter().position(|t| t.toplevel == surface) else {
            return;
        };
        let was = self.toplevels[idx].kind.clone();
        let screen_idx = self.toplevels[idx].screen_idx;
        if was == Kind::Video {
            log::info!("video plane '{}' gone", self.toplevels[idx].tag);
        }
        let had_focus = self
            .seat
            .get_keyboard()
            .is_some_and(|k| k.current_focus().as_ref() == Some(surface.wl_surface()));
        self.toplevels.remove(idx);
        self.video_order.retain(|&i| i != idx);
        for i in self.video_order.iter_mut() {
            if *i > idx {
                *i -= 1;
            }
        }
        // On "restart" main() re-execs us after this.
        if was == Kind::Ui && screen_idx == 0 && self.startup_cmd.is_some() {
            log::info!("main UI toplevel gone -> shutting down");
            self.running = false;
        }
        if had_focus && let Some(ui) = main_ui_surface(self) {
            crate::input::focus_surface(self, &ui);
        }
        crate::host::damage_all(self);
    }

    fn move_request(&mut self, _surface: ToplevelSurface, _seat: WlSeat, _serial: Serial) {
        // the host window moves, we reflow
    }

    fn resize_request(
        &mut self,
        _surface: ToplevelSurface,
        _seat: WlSeat,
        _serial: Serial,
        _edges: smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::ResizeEdge,
    ) {
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        surface.send_configure();
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        surface.send_configure();
    }

    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _output: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>,
    ) {
        let Some(idx) =
            self.toplevels.iter().position(|t| t.toplevel == surface && t.kind == Kind::Ui)
        else {
            surface.send_configure();
            return;
        };
        let screen_idx = self.toplevels[idx].screen_idx;
        self.screens[screen_idx].fullscreen = true;
        crate::host::set_fullscreen(self, screen_idx, true);
        surface.with_pending_state(|st| {
            st.states.set(XdgState::Fullscreen);
        });
        surface.send_pending_configure();
        crate::layout::apply_ui_layout(self, screen_idx);
        log::info!("request_fullscreen screen '{}'", self.screens[screen_idx].role);
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        let Some(idx) =
            self.toplevels.iter().position(|t| t.toplevel == surface && t.kind == Kind::Ui)
        else {
            surface.send_configure();
            return;
        };
        let screen_idx = self.toplevels[idx].screen_idx;
        self.screens[screen_idx].fullscreen = false;
        crate::host::set_fullscreen(self, screen_idx, false);
        surface.with_pending_state(|st| {
            st.states.unset(XdgState::Fullscreen);
        });
        surface.send_pending_configure();
        crate::layout::apply_ui_layout(self, screen_idx);
    }
}

impl XdgDecorationHandler for LiviState {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        // Electron's GTK client-side decoration path crashes.
        toplevel.with_pending_state(|st| {
            st.decoration_mode = Some(DecoMode::ServerSide);
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: DecoMode) {
        self.new_decoration(toplevel);
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        self.new_decoration(toplevel);
    }
}

impl SeatHandler for LiviState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<LiviState> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.host.cursor = match image {
            CursorImageStatus::Hidden => None,
            CursorImageStatus::Named(icon) => Some(icon),
            CursorImageStatus::Surface(_) => Some(CursorIcon::Default),
        };
        crate::host::apply_cursor(self);
    }

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&WlSurface>) {}
}

impl SelectionHandler for LiviState {
    type SelectionUserData = ();
}

impl DataDeviceHandler for LiviState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for LiviState {}
impl ServerDndGrabHandler for LiviState {}
impl TabletSeatHandler for LiviState {}

impl ShmHandler for LiviState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl DrmSyncobjHandler for LiviState {
    fn drm_syncobj_state(&mut self) -> Option<&mut DrmSyncobjState> {
        self.syncobj_state.as_mut()
    }
}

impl DmabufHandler for LiviState {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: smithay::backend::allocator::dmabuf::Dmabuf,
        notifier: ImportNotifier,
    ) {
        // Only validated here, the texture import happens at render time.
        if crate::host::import_dmabuf(self, &dmabuf) {
            let _ = notifier.successful::<LiviState>();
        } else {
            notifier.failed();
        }
    }
}

smithay::delegate_compositor!(LiviState);
smithay::delegate_xdg_shell!(LiviState);
smithay::delegate_xdg_decoration!(LiviState);
smithay::delegate_seat!(LiviState);
smithay::delegate_cursor_shape!(LiviState);
smithay::delegate_data_device!(LiviState);
smithay::delegate_shm!(LiviState);
smithay::delegate_drm_syncobj!(LiviState);
smithay::delegate_dmabuf!(LiviState);
smithay::delegate_viewporter!(LiviState);
impl smithay::wayland::output::OutputHandler for LiviState {}
smithay::delegate_output!(LiviState);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_lies_above_the_projection_and_the_clusters_below() {
        assert!(video_rank("video") > video_rank("main"));
        assert!(video_rank("main") > video_rank("cluster-main"));
        assert_eq!(video_rank(""), video_rank("cluster-dash"));
    }
}
