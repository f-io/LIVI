// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::time::{Duration, Instant};

use calloop_wayland_source::WaylandSource;
use smithay::backend::drm::DrmDeviceFd;
use smithay::backend::egl::context::GlAttributes;
use smithay::backend::egl::native::{EGLNativeDisplay, EGLPlatform};
use smithay::backend::egl::{EGLContext, EGLDevice, EGLDisplay, EGLSurface};
use smithay::backend::renderer::ImportDma;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::reexports::calloop::LoopHandle;
use smithay::utils::{DeviceFd, SERIAL_COUNTER};
use smithay::backend::allocator::format::FormatSet;
use smithay::reexports::wayland_protocols::wp::linux_dmabuf::zv1::server::zwp_linux_dmabuf_feedback_v1::TrancheFlags;
use smithay::wayland::dmabuf::DmabufFeedbackBuilder;
use smithay::wayland::drm_syncobj::{DrmSyncobjState, supports_syncobj_eventfd};
use smithay_client_toolkit::compositor::{
    CompositorHandler as SctkCompositorHandler, CompositorState as SctkCompositorState,
    FrameCallbackData,
};
use smithay_client_toolkit::output::{
    OutputHandler as SctkOutputHandler, OutputState as SctkOutputState,
};
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::wl_keyboard::WlKeyboard;
use smithay_client_toolkit::reexports::client::protocol::wl_output::{
    Transform as CTransform, WlOutput,
};
use smithay_client_toolkit::reexports::client::protocol::wl_pointer::WlPointer;
use smithay_client_toolkit::reexports::client::protocol::wl_seat::WlSeat as CWlSeat;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface as CWlSurface;
use smithay_client_toolkit::reexports::client::protocol::wl_touch::WlTouch;
use smithay_client_toolkit::reexports::client::{Connection, Proxy, QueueHandle};
use smithay_client_toolkit::reexports::csd_frame::WindowState;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers,
};
use smithay_client_toolkit::seat::pointer::{
    CursorIcon, PointerEvent, PointerEventKind, PointerHandler, ThemeSpec, ThemedPointer,
};
use smithay_client_toolkit::seat::touch::TouchHandler;
use smithay_client_toolkit::seat::{
    Capability, SeatHandler as SctkSeatHandler, SeatState as SctkSeatState,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::xdg::XdgShell;
use smithay_client_toolkit::shell::xdg::window::{
    DecorationMode, Window, WindowConfigure, WindowDecorations, WindowHandler,
};
use smithay_client_toolkit::shm::{Shm, ShmHandler as SctkShmHandler};

use crate::state::{LiviState, TITLEBAR_H};

mod handlers;
mod input;

const MIN_SIZE: (i32, i32) = (320, 200);

pub struct HostWindow {
    pub window: Window,
    pub egl_surface: EGLSurface,
    pub width: i32,
    pub height: i32,
    pub needs_redraw: bool,
    pub frame_pending: bool,
    pub configured: bool,
    pub host_output: Option<WlOutput>,
    pub floating: bool,
    pub fullscreen_seen: bool,
}

pub struct HostState {
    pub conn: Option<Connection>,
    pub qh: Option<QueueHandle<LiviState>>,
    pub registry: Option<RegistryState>,
    pub sctk_outputs: Option<SctkOutputState>,
    pub sctk_seats: Option<SctkSeatState>,
    pub sctk_compositor: Option<SctkCompositorState>,
    pub xdg_shell: Option<XdgShell>,
    pub egl_display: Option<EGLDisplay>,
    pub egl_context: Option<EGLContext>,
    pub windows: Vec<(usize, HostWindow)>,
    pub pointer_pos: (f64, f64),
    pub pointer_screen: Option<usize>,
    pub keyboard: Option<WlKeyboard>,
    pub pointer: Option<ThemedPointer>,
    pub shm: Option<Shm>,
    pub cursor: Option<CursorIcon>,
    pub cursor_armed: bool,
    pub pointer_on_inner: bool,
    pub pointer_on_deco: bool,
    pub touch: Option<WlTouch>,
    pub has_touch_cap: bool,
    pub last_pointer_serial: u32,
    pub host_seat: Option<CWlSeat>,
    pub touch_positions: Vec<(u32, f64, f64)>,
    pub alt_held: bool,
    pub deco: std::collections::HashMap<usize, crate::deco::DecoSet>,
}

impl HostState {
    pub fn new() -> Self {
        Self {
            conn: None,
            qh: None,
            registry: None,
            sctk_outputs: None,
            sctk_seats: None,
            sctk_compositor: None,
            xdg_shell: None,
            egl_display: None,
            egl_context: None,
            windows: Vec::new(),
            pointer_pos: (0.0, 0.0),
            pointer_screen: None,
            keyboard: None,
            pointer: None,
            shm: None,
            cursor: None,
            cursor_armed: false,
            pointer_on_inner: false,
            pointer_on_deco: false,
            touch: None,
            has_touch_cap: false,
            last_pointer_serial: 0,
            host_seat: None,
            touch_positions: Vec::new(),
            alt_held: false,
            deco: std::collections::HashMap::new(),
        }
    }

    pub fn window_for_screen(&mut self, screen_idx: usize) -> Option<&mut HostWindow> {
        self.windows
            .iter_mut()
            .find(|(i, _)| *i == screen_idx)
            .map(|(_, w)| w)
    }

    pub fn screen_for_surface(&self, surface: &CWlSurface) -> Option<usize> {
        self.windows
            .iter()
            .find(|(_, w)| w.window.wl_surface() == surface)
            .map(|(i, _)| *i)
    }
}

struct HostNativeDisplay(*mut std::ffi::c_void);
unsafe impl Send for HostNativeDisplay {}

pub struct HostEglWindow(pub wayland_egl::WlEglSurface);

unsafe impl smithay::backend::egl::native::EGLNativeSurface for HostEglWindow {
    unsafe fn create(
        &self,
        display: &std::sync::Arc<smithay::backend::egl::display::EGLDisplayHandle>,
        config_id: smithay::backend::egl::ffi::egl::types::EGLConfig,
    ) -> Result<*const std::ffi::c_void, smithay::backend::egl::EGLError> {
        const ATTRS: [std::ffi::c_int; 3] = [
            smithay::backend::egl::ffi::egl::RENDER_BUFFER as std::ffi::c_int,
            smithay::backend::egl::ffi::egl::BACK_BUFFER as std::ffi::c_int,
            smithay::backend::egl::ffi::egl::NONE as std::ffi::c_int,
        ];
        smithay::backend::egl::wrap_egl_call_ptr(|| unsafe {
            smithay::backend::egl::ffi::egl::CreatePlatformWindowSurfaceEXT(
                display.handle,
                config_id,
                self.0.ptr() as *mut _,
                ATTRS.as_ptr(),
            )
        })
    }

    fn resize(&self, width: i32, height: i32, dx: i32, dy: i32) -> bool {
        wayland_egl::WlEglSurface::resize(&self.0, width, height, dx, dy);
        true
    }

    fn identifier(&self) -> Option<String> {
        Some("livi/wayland".into())
    }
}

impl EGLNativeDisplay for HostNativeDisplay {
    fn supported_platforms(&self) -> Vec<smithay::backend::egl::native::EGLPlatform<'static>> {
        use smithay::backend::egl::ffi;
        use smithay::egl_platform;
        vec![
            egl_platform!(PLATFORM_WAYLAND_KHR, self.0, &["EGL_KHR_platform_wayland"]),
            egl_platform!(PLATFORM_WAYLAND_EXT, self.0, &["EGL_EXT_platform_wayland"]),
        ]
    }
}

pub fn serve_dmabuf(
    state: &mut LiviState,
    egl_display: &EGLDisplay,
    renderer: &GlesRenderer,
    scanout: Option<(u64, FormatSet)>,
) {
    state.render_node = render_node(egl_display);

    let formats = renderer.dmabuf_formats();
    let feedback = state.render_node.as_deref().and_then(|node| {
        let dev = std::os::unix::fs::MetadataExt::rdev(&std::fs::metadata(node).ok()?);
        let mut builder = DmabufFeedbackBuilder::new(dev, formats.clone());
        if let Some((display, planes)) = &scanout {
            let direct: Vec<_> = planes.intersection(&formats).copied().collect();
            log::info!(
                "{} buffer formats can go straight to the display",
                direct.len()
            );
            builder = builder.add_preference_tranche(*display, Some(TrancheFlags::Scanout), direct);
        }
        builder.build().ok()
    });
    let _global = match &feedback {
        Some(feedback) => state
            .dmabuf_state
            .create_global_with_default_feedback::<LiviState>(&state.display_handle, feedback),
        None => state
            .dmabuf_state
            .create_global::<LiviState>(&state.display_handle, formats),
    };

    match state.render_node.as_deref().and_then(syncobj_device) {
        Some(fd) => {
            log::info!("explicit sync over {:?}", state.render_node);
            state.syncobj_state =
                Some(DrmSyncobjState::new::<LiviState>(&state.display_handle, fd));
        }
        None => log::info!("no explicit sync, inner clients rely on implicit dmabuf fences"),
    }
}

fn render_node(display: &EGLDisplay) -> Option<std::path::PathBuf> {
    let device = EGLDevice::device_for_display(display).ok()?;
    device
        .render_device_path()
        .or_else(|_| device.drm_device_path())
        .ok()
}

fn syncobj_device(path: &std::path::Path) -> Option<DrmDeviceFd> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .ok()?;
    let fd = DrmDeviceFd::new(DeviceFd::from(std::os::fd::OwnedFd::from(file)));
    supports_syncobj_eventfd(&fd).then_some(fd)
}

pub fn init(state: &mut LiviState, handle: &LoopHandle<'static, LiviState>) {
    let conn = Connection::connect_to_env().expect("no outer wayland session");
    let (globals, event_queue) = registry_queue_init::<LiviState>(&conn).expect("host registry");
    let qh: QueueHandle<LiviState> = event_queue.handle();

    let registry = RegistryState::new(&globals);
    let sctk_outputs = SctkOutputState::new(&globals, &qh);
    let sctk_seats = SctkSeatState::new(&globals, &qh);
    let sctk_compositor = SctkCompositorState::bind(&globals, &qh).expect("host wl_compositor");
    let xdg_shell = XdgShell::bind(&globals, &qh).expect("host xdg_wm_base");
    let shm = Shm::bind(&globals, &qh).expect("host wl_shm");

    let display_ptr = conn.backend().display_ptr() as *mut std::ffi::c_void;
    let egl_display =
        unsafe { EGLDisplay::new(HostNativeDisplay(display_ptr)) }.expect("EGLDisplay");
    let egl_context = EGLContext::new_with_config(
        &egl_display,
        GlAttributes {
            version: (2, 0),
            profile: None,
            debug: false,
            vsync: false,
        },
        smithay::backend::egl::context::PixelFormatRequirements::_8_bit(),
    )
    .expect("EGLContext");
    let renderer = unsafe {
        GlesRenderer::new(
            EGLContext::new_shared_with_config(
                &egl_display,
                &egl_context,
                GlAttributes {
                    version: (2, 0),
                    profile: None,
                    debug: false,
                    vsync: false,
                },
                smithay::backend::egl::context::PixelFormatRequirements::_8_bit(),
            )
            .expect("shared EGLContext"),
        )
    }
    .expect("GlesRenderer");

    serve_dmabuf(state, &egl_display, &renderer, None);

    WaylandSource::new(conn.clone(), event_queue)
        .insert(handle.clone())
        .expect("insert host wayland source");

    state.host.conn = Some(conn);
    state.host.qh = Some(qh);
    state.host.registry = Some(registry);
    state.host.sctk_outputs = Some(sctk_outputs);
    state.host.sctk_seats = Some(sctk_seats);
    state.host.sctk_compositor = Some(sctk_compositor);
    state.host.xdg_shell = Some(xdg_shell);
    state.host.shm = Some(shm);
    state.host.egl_display = Some(egl_display);
    state.host.egl_context = Some(egl_context);
    state.renderer = Some(renderer);

    open_screen(state, 0);
}

fn default_size(state: &LiviState, screen_idx: usize) -> (i32, i32) {
    let s = &state.screens[screen_idx];
    let inset = s.top_inset();
    if s.req_width > 0 && s.req_height > 0 {
        return (s.req_width, s.req_height + inset);
    }
    if let Ok(v) = std::env::var("LIVI_OUTPUT_SIZE")
        && let Some((w, h)) = v.split_once('x')
        && let (Ok(w), Ok(h)) = (w.parse::<i32>(), h.parse::<i32>())
        && w > 0
        && h > 0
    {
        return (w, h + inset);
    }
    (1280, 720 + inset)
}

pub fn open_screen(state: &mut LiviState, screen_idx: usize) {
    if state.drm.is_some() {
        crate::drm::open_screen(state, screen_idx);
        return;
    }
    if state.host.window_for_screen(screen_idx).is_some() {
        return;
    }
    let (Some(compositor), Some(xdg), Some(qh), Some(egl_display), Some(egl_context)) = (
        state.host.sctk_compositor.as_ref(),
        state.host.xdg_shell.as_ref(),
        state.host.qh.as_ref(),
        state.host.egl_display.as_ref(),
        state.host.egl_context.as_ref(),
    ) else {
        return;
    };
    let (w, h) = default_size(state, screen_idx);
    let surface = compositor.create_surface(qh);
    let window = xdg.create_window(surface, WindowDecorations::RequestClient, qh);
    window.set_title(crate::state::role_title(&state.screens[screen_idx].role));
    window.set_app_id(state.output_app_id.clone());
    window.set_min_size(Some((MIN_SIZE.0 as u32, MIN_SIZE.1 as u32)));
    if state.screens[screen_idx].fullscreen {
        window.set_fullscreen(None);
    }
    window.commit();

    let wl_egl =
        wayland_egl::WlEglSurface::new(window.wl_surface().id(), w, h).expect("wl_egl_window");
    let egl_surface = unsafe {
        EGLSurface::new(
            egl_display,
            egl_context.pixel_format().unwrap(),
            egl_context.config_id(),
            HostEglWindow(wl_egl),
        )
    }
    .expect("EGLSurface");

    state.screens[screen_idx].width = w;
    state.screens[screen_idx].height = h;
    state.host.windows.push((
        screen_idx,
        HostWindow {
            window,
            egl_surface,
            width: w,
            height: h,
            needs_redraw: true,
            frame_pending: false,
            configured: false,
            host_output: None,
            floating: true,
            fullscreen_seen: false,
        },
    ));
    ensure_server_output(state, screen_idx);
    log::info!(
        "new output -> screen '{}' at x={} ({}x{})",
        state.screens[screen_idx].role,
        state.screens[screen_idx].x,
        w,
        h
    );
}

pub fn close_screen(state: &mut LiviState, screen_idx: usize) {
    if state.drm.is_some() {
        crate::drm::close_screen(state, screen_idx);
        return;
    }
    let Some(pos) = state
        .host
        .windows
        .iter()
        .position(|(i, _)| *i == screen_idx)
    else {
        return;
    };
    state.host.windows.remove(pos);
    if let Some(output) = state.screens[screen_idx].output.take() {
        drop(output);
    }
    if screen_idx == 0 {
        log::info!("main output gone -> shutting down");
        state.running = false;
    } else if let Some(ui) = state
        .toplevels
        .iter()
        .find(|t| t.kind == crate::state::Kind::Ui && t.screen_idx == screen_idx)
    {
        ui.toplevel.send_close();
    }
}

pub fn ensure_server_output(state: &mut LiviState, screen_idx: usize) {
    let s = &mut state.screens[screen_idx];
    if s.output.is_some() {
        return;
    }
    let output = smithay::output::Output::new(
        s.role.clone(),
        smithay::output::PhysicalProperties {
            size: (0, 0).into(),
            subpixel: smithay::output::Subpixel::Unknown,
            make: "LIVI".into(),
            model: s.role.clone(),
        },
    );
    let _global = output.create_global::<LiviState>(&state.display_handle);
    output.change_current_state(
        Some(smithay::output::Mode {
            size: (s.width, s.height).into(),
            refresh: 60_000,
        }),
        Some(smithay::utils::Transform::Normal),
        None,
        Some((s.x, 0).into()),
    );
    output.set_preferred(smithay::output::Mode {
        size: (s.width, s.height).into(),
        refresh: 60_000,
    });
    s.output = Some(output);
}

pub fn set_fullscreen(state: &mut LiviState, screen_idx: usize, fullscreen: bool) {
    if let Some(w) = state.host.window_for_screen(screen_idx) {
        if fullscreen {
            w.window.set_fullscreen(None);
        } else {
            w.window.unset_fullscreen();
        }
        w.needs_redraw = true;
    }
}

pub fn minimize(state: &mut LiviState, screen_idx: usize) {
    if let Some(w) = state.host.window_for_screen(screen_idx) {
        w.window.set_minimized();
    }
}

fn grab_seat(state: &LiviState) -> Option<CWlSeat> {
    state
        .host
        .host_seat
        .clone()
        .or_else(|| state.host.sctk_seats.as_ref()?.seats().next())
}

pub fn begin_move(state: &mut LiviState, screen_idx: usize) {
    let serial = state.host.last_pointer_serial;
    let seat = grab_seat(state);
    log::debug!(
        "deco move: screen={screen_idx} serial={serial} seat={}",
        seat.is_some()
    );
    if let (Some(w), Some(seat)) = (state.host.window_for_screen(screen_idx), seat) {
        w.window.move_(&seat, serial);
        if let Some(conn) = state.host.conn.as_ref() {
            let _ = conn.flush();
        }
    }
}

pub fn begin_resize(state: &mut LiviState, screen_idx: usize, edges: u32) {
    let serial = state.host.last_pointer_serial;
    let seat = grab_seat(state);
    if let (Some(w), Some(seat)) = (state.host.window_for_screen(screen_idx), seat) {
        let edge = match edges {
            e if e == (EDGE_BOTTOM | EDGE_LEFT) => XdgResizeEdge::BottomLeft,
            e if e == (EDGE_BOTTOM | EDGE_RIGHT) => XdgResizeEdge::BottomRight,
            EDGE_BOTTOM => XdgResizeEdge::Bottom,
            EDGE_LEFT => XdgResizeEdge::Left,
            EDGE_RIGHT => XdgResizeEdge::Right,
            _ => return,
        };
        w.window.resize(&seat, serial, edge);
        if let Some(conn) = state.host.conn.as_ref() {
            let _ = conn.flush();
        }
    }
}

pub const EDGE_LEFT: u32 = 1;
pub const EDGE_RIGHT: u32 = 2;
pub const EDGE_BOTTOM: u32 = 4;
use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_toplevel::ResizeEdge as XdgResizeEdge;

pub fn damage_all(state: &mut LiviState) {
    crate::drm::damage(state);
    for (_, w) in state.host.windows.iter_mut() {
        w.needs_redraw = true;
    }
}

pub fn panel_mm(state: &LiviState, screen_idx: usize) -> Option<(i32, i32)> {
    if state.drm.is_some() {
        return crate::drm::panel_mm(state, screen_idx);
    }
    let outputs = state.host.sctk_outputs.as_ref()?;
    let output = state
        .host
        .windows
        .iter()
        .find(|(i, _)| *i == screen_idx)?
        .1
        .host_output
        .as_ref()?;
    let info = outputs.info(output)?;
    let (w, h) = info.physical_size;
    if w <= 0 || h <= 0 {
        return None;
    }
    let turned = matches!(
        info.transform,
        CTransform::_90 | CTransform::_270 | CTransform::Flipped90 | CTransform::Flipped270
    );
    let mm = if turned { (h, w) } else { (w, h) };
    let screen = state.screens.get(screen_idx)?;
    let logical = info
        .logical_size
        .filter(|&(w, h)| w > 0 && h > 0)
        .or_else(|| {
            let (w, h) = info.modes.iter().find(|m| m.current)?.dimensions;
            let scale = info.scale_factor.max(1);
            let (w, h) = if turned { (h, w) } else { (w, h) };
            (w > 0 && h > 0).then_some((w / scale, h / scale))
        });
    match logical {
        Some(output) => Some(share_of_panel(mm, (screen.width, screen.height), output)),
        None => Some(mm),
    }
}

fn share_of_panel(mm: (i32, i32), window: (i32, i32), output: (i32, i32)) -> (i32, i32) {
    let scale =
        |mm: i32, px: i32, of: i32| ((mm as i64 * px as i64 + of as i64 / 2) / of as i64) as i32;
    (
        scale(mm.0, window.0, output.0),
        scale(mm.1, window.1, output.1),
    )
}

#[cfg(test)]
mod tests;

pub fn import_dmabuf(
    state: &mut LiviState,
    dmabuf: &smithay::backend::allocator::dmabuf::Dmabuf,
) -> bool {
    if dmabuf.node().is_none()
        && let Some(node) = state
            .render_node
            .as_deref()
            .and_then(|path| smithay::backend::drm::DrmNode::from_path(path).ok())
    {
        dmabuf.set_node(node);
    }
    let Some(r) = state.renderer.as_mut() else {
        return false;
    };
    r.import_dmabuf(dmabuf, None).is_ok()
}

pub fn pump(state: &mut LiviState) {
    crate::drm::pump(state);
    let idxs: Vec<usize> = state
        .host
        .windows
        .iter()
        .filter(|(_, w)| w.configured && w.needs_redraw && !w.frame_pending)
        .map(|(i, _)| *i)
        .collect();
    for i in idxs {
        crate::render::render_screen(state, i);
    }
}

pub fn fit_aspect(size: (i32, i32), inset: i32, aspect: (i32, i32)) -> (i32, i32) {
    let (aw, ah) = (aspect.0.max(1) as i64, aspect.1.max(1) as i64);
    let (mut cw, mut ch) = (size.0.max(1) as i64, (size.1 - inset).max(1) as i64);
    if cw * ah > ch * aw {
        cw = (ch * aw + ah / 2) / ah;
    } else {
        ch = (cw * ah + aw / 2) / aw;
    }
    let (min_w, min_h) = (MIN_SIZE.0 as i64, (MIN_SIZE.1 as i64 - inset as i64).max(1));
    if cw < min_w || ch < min_h {
        let grow = |v: i64, num: i64, den: i64| (v * num + den - 1) / den;
        cw = min_w.max(grow(min_h, aw, ah));
        ch = grow(cw, ah, aw);
    }
    (cw as i32, ch as i32 + inset)
}

pub fn refit(state: &mut LiviState, screen_idx: usize) {
    let Some(aspect) = state.screens[screen_idx].aspect else {
        return;
    };
    let Some(w) = state.host.window_for_screen(screen_idx) else {
        return;
    };
    if !w.configured || !w.floating {
        return;
    }
    let size = fit_aspect((w.width, w.height), TITLEBAR_H, aspect);
    resize_host_window(state, screen_idx, size);
}

fn resize_host_window(state: &mut LiviState, screen_idx: usize, (w, h): (i32, i32)) {
    let Some(hw) = state.host.window_for_screen(screen_idx) else {
        return;
    };
    if (w, h) != (hw.width, hw.height) {
        hw.egl_surface.resize(w, h, 0, 0);
        hw.width = w;
        hw.height = h;
    }
    hw.needs_redraw = true;
    state.screens[screen_idx].width = w;
    state.screens[screen_idx].height = h;
    state.screens[screen_idx].resize_pending = Some(Instant::now() + RESIZE_SETTLE);
    crate::layout::apply_ui_layout(state, screen_idx);
    crate::ctrl::send_panels(state);
}

const RESIZE_SETTLE: Duration = Duration::from_millis(150);

pub fn apply_settled_resizes(state: &mut LiviState) {
    let now = Instant::now();
    for idx in 0..state.screens.len() {
        let Some(deadline) = state.screens[idx].resize_pending else {
            continue;
        };
        if now < deadline {
            continue;
        }
        state.screens[idx].resize_pending = None;
        let (w, h) = (state.screens[idx].width, state.screens[idx].height);
        let first_setup =
            state.screens[idx].applied_width == 0 && state.screens[idx].applied_height == 0;
        if first_setup && (w, h) != (0, 0) {
            state.screens[idx].applied_width = w;
            state.screens[idx].applied_height = h;
            if let Some(output) = &state.screens[idx].output {
                output.change_current_state(
                    Some(smithay::output::Mode {
                        size: (w, h).into(),
                        refresh: 60_000,
                    }),
                    None,
                    None,
                    None,
                );
            }
        }
        let videos: Vec<usize> = state
            .toplevels
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind == crate::state::Kind::Video && t.screen_idx == idx)
            .map(|(i, _)| i)
            .collect();
        for v in videos {
            crate::layout::apply_video_layout(state, v);
        }
    }
}

pub fn apply_cursor(state: &LiviState) {
    let (Some(pointer), Some(conn)) = (state.host.pointer.as_ref(), state.host.conn.as_ref())
    else {
        return;
    };
    let _ = match shown_cursor(&state.host) {
        Some(icon) => pointer.set_cursor(conn, icon),
        None => pointer.hide_cursor(),
    };
    let _ = conn.flush();
}

pub fn cursor_requested(host: &mut HostState, want: Option<CursorIcon>) {
    if !host.pointer_on_inner {
        return;
    }
    if want.is_none() {
        host.cursor_armed = true;
    }
    if host.cursor_armed {
        host.cursor = want;
    }
}

pub fn shown_cursor(host: &HostState) -> Option<CursorIcon> {
    if host.pointer_on_deco {
        Some(CursorIcon::Default)
    } else {
        host.cursor
    }
}

smithay_client_toolkit::delegate_dispatch2!(LiviState);
smithay_client_toolkit::delegate_registry!(LiviState);

pub fn request_frame(state: &mut LiviState, screen_idx: usize) {
    let qh = state.host.qh.clone();
    if let (Some(w), Some(qh)) = (state.host.window_for_screen(screen_idx), qh)
        && !w.frame_pending
    {
        w.window
            .wl_surface()
            .frame(&qh, FrameCallbackData(w.window.wl_surface().clone()));
        w.frame_pending = true;
    }
}

pub fn send_frame_callbacks(state: &mut LiviState) {
    let time_ms: u32 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u32;
    for t in &state.toplevels {
        smithay::wayland::compositor::with_surface_tree_downward(
            t.toplevel.wl_surface(),
            (),
            |_, _, _| smithay::wayland::compositor::TraversalAction::DoChildren(()),
            |_surf, states, _| {
                let mut guard = states
                    .cached_state
                    .get::<smithay::wayland::compositor::SurfaceAttributes>();
                for cb in guard.current().frame_callbacks.drain(..) {
                    cb.done(time_ms);
                }
            },
            |_, _, _| true,
        );
    }
    let _ = SERIAL_COUNTER;
    let _ = Duration::ZERO;
}
