//! Host side: one xdg-shell window of the outer session per open screen, its
//! EGL surface, and the outer seat's pointer/touch/keyboard routed into our seat.

use std::time::{Duration, Instant};

use smithay::backend::egl::context::GlAttributes;
use smithay::backend::egl::native::{EGLNativeDisplay, EGLPlatform};
use smithay::backend::egl::{EGLContext, EGLDisplay, EGLSurface};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::ImportDma;
use smithay::reexports::calloop::LoopHandle;
use smithay::utils::SERIAL_COUNTER;
use smithay_client_toolkit::compositor::{CompositorHandler as SctkCompositorHandler, CompositorState as SctkCompositorState};
use smithay_client_toolkit::output::{OutputHandler as SctkOutputHandler, OutputState as SctkOutputState};
use calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::wl_keyboard::WlKeyboard;
use smithay_client_toolkit::reexports::client::protocol::wl_output::{Transform as CTransform, WlOutput};
use smithay_client_toolkit::reexports::client::protocol::wl_pointer::WlPointer;
use smithay_client_toolkit::reexports::client::protocol::wl_seat::WlSeat as CWlSeat;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface as CWlSurface;
use smithay_client_toolkit::reexports::client::protocol::wl_touch::WlTouch;
use smithay_client_toolkit::reexports::client::{Connection, Proxy, QueueHandle};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{KeyboardHandler, KeyEvent, Keysym, Modifiers};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::touch::TouchHandler;
use smithay_client_toolkit::seat::{Capability, SeatHandler as SctkSeatHandler, SeatState as SctkSeatState};
use smithay_client_toolkit::shell::xdg::window::{
    DecorationMode, Window, WindowConfigure, WindowDecorations, WindowHandler,
};
use smithay_client_toolkit::shell::xdg::XdgShell;
use smithay_client_toolkit::shell::WaylandSurface;

use crate::state::LiviState;

pub struct HostWindow {
    pub window: Window,
    pub egl_surface: EGLSurface,
    pub width: i32,
    pub height: i32,
    pub needs_redraw: bool,
    pub frame_pending: bool,
    /// xdg-shell: no buffer may be committed before the first configure.
    pub configured: bool,
    pub host_output: Option<WlOutput>,
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
    pub renderer: Option<GlesRenderer>,
    pub windows: Vec<(usize, HostWindow)>, // screen_idx -> window
    pub pointer_pos: (f64, f64),           // layout coords
    pub pointer_screen: Option<usize>,
    pub keyboard: Option<WlKeyboard>,
    pub pointer: Option<WlPointer>,
    pub touch: Option<WlTouch>,
    pub has_touch_cap: bool,
    pub pointer_seen: bool,
    pub last_pointer_serial: u32,
    pub host_seat: Option<CWlSeat>,
    pub touch_positions: Vec<(i32, f64, f64)>,
    pub alt_held: bool,
    pub cal_program: Option<smithay::backend::renderer::gles::GlesTexProgram>,
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
            renderer: None,
            windows: Vec::new(),
            pointer_pos: (0.0, 0.0),
            pointer_screen: None,
            keyboard: None,
            pointer: None,
            touch: None,
            has_touch_cap: false,
            pointer_seen: false,
            last_pointer_serial: 0,
            host_seat: None,
            touch_positions: Vec::new(),
            alt_held: false,
            cal_program: None,
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

// EGL over the host wayland connection (EGL_PLATFORM_WAYLAND).
struct HostNativeDisplay(*mut std::ffi::c_void);
unsafe impl Send for HostNativeDisplay {}

// EGL window surface over a wl_egl_window of the host connection.
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

/// Connect to the outer session, set up EGL + the shared GLES renderer, and
/// hook the host event queue into the loop.
pub fn init(state: &mut LiviState, handle: &LoopHandle<'static, LiviState>) {
    let conn = Connection::connect_to_env().expect("no outer wayland session");
    let (globals, event_queue) = registry_queue_init::<LiviState>(&conn).expect("host registry");
    let qh: QueueHandle<LiviState> = event_queue.handle();

    let registry = RegistryState::new(&globals);
    let sctk_outputs = SctkOutputState::new(&globals, &qh);
    let sctk_seats = SctkSeatState::new(&globals, &qh);
    let sctk_compositor = SctkCompositorState::bind(&globals, &qh).expect("host wl_compositor");
    let xdg_shell = XdgShell::bind(&globals, &qh).expect("host xdg_wm_base");

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
    let renderer = unsafe { GlesRenderer::new(
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
    ) }
    .expect("GlesRenderer");

    // Advertise dmabuf to inner clients with the formats the renderer imports.
    let formats = renderer.dmabuf_formats();
    let global = state
        .dmabuf_state
        .create_global::<LiviState>(&state.display_handle, formats);
    let _ = global;

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
    state.host.egl_display = Some(egl_display);
    state.host.egl_context = Some(egl_context);
    state.host.renderer = Some(renderer);

    // The nested main screen exists from the start, secondaries open on demand.
    open_screen(state, 0);
}

fn default_size(state: &LiviState, screen_idx: usize) -> (i32, i32) {
    let s = &state.screens[screen_idx];
    if s.req_width > 0 && s.req_height > 0 {
        return (s.req_width, s.req_height);
    }
    if let Ok(v) = std::env::var("LIVI_OUTPUT_SIZE")
        && let Some((w, h)) = v.split_once('x')
            && let (Ok(w), Ok(h)) = (w.parse(), h.parse())
                && w > 0 && h > 0 {
                    return (w, h);
                }
    (1280, 720)
}

pub fn open_screen(state: &mut LiviState, screen_idx: usize) {
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
    window.set_min_size(Some((320, 200)));
    if state.screens[screen_idx].fullscreen {
        window.set_fullscreen(None);
    }
    window.commit();

    let wl_egl = wayland_egl::WlEglSurface::new(window.wl_surface().id(), w, h)
        .expect("wl_egl_window");
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
    let Some(pos) = state.host.windows.iter().position(|(i, _)| *i == screen_idx) else {
        return;
    };
    state.host.windows.remove(pos);
    if let Some(output) = state.screens[screen_idx].output.take() {
        // Retract the wl_output global from inner clients.
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

/// Advertise a wl_output for this screen to the inner clients.
fn ensure_server_output(state: &mut LiviState, screen_idx: usize) {
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

/// The seat that drives interactive host-side grabs.
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
    log::debug!("deco move: screen={screen_idx} serial={serial} seat={}", seat.is_some());
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
    for (_, w) in state.host.windows.iter_mut() {
        w.needs_redraw = true;
    }
}

/// Physical millimetres of the host output a screen's window sits on.
pub fn panel_mm(state: &LiviState, screen_idx: usize) -> Option<(i32, i32)> {
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
    // wl_output reports the panel's unrotated mm; our screen size is post-transform.
    if matches!(
        info.transform,
        CTransform::_90 | CTransform::_270 | CTransform::Flipped90 | CTransform::Flipped270
    ) {
        Some((h, w))
    } else {
        Some((w, h))
    }
}

pub fn import_dmabuf(
    state: &mut LiviState,
    dmabuf: &smithay::backend::allocator::dmabuf::Dmabuf,
) -> bool {
    if let Some(r) = state.host.renderer.as_mut() {
        r.import_dmabuf(dmabuf, None).is_ok()
    } else {
        false
    }
}

/// Render every window that needs it and has no frame callback outstanding.
pub fn pump(state: &mut LiviState) {
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

// ── SCTK handler plumbing ────────────────────────────────────────────────────

impl SctkCompositorHandler for LiviState {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &CWlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &CWlSurface, _: CTransform) {}
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
            && let Some(w) = self.host.window_for_screen(idx) {
                w.host_output = None;
            }
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &CWlSurface, _time: u32) {
        if let Some(idx) = self.host.screen_for_surface(surface)
            && let Some(w) = self.host.window_for_screen(idx) {
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
        // A host window was closed directly -> ask its UI to close too.
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
        let cur = (self.host.windows[pos].1.width, self.host.windows[pos].1.height);
        let w = configure.new_size.0.map(|v| v.get() as i32).unwrap_or(cur.0);
        let h = configure.new_size.1.map(|v| v.get() as i32).unwrap_or(cur.1);
        let host_fullscreen = configure.is_fullscreen();
        let _ = configure.decoration_mode == DecorationMode::Server;

        {
            let hw = &mut self.host.windows[pos].1;
            if (w, h) != cur {
                hw.egl_surface.resize(w, h, 0, 0);
                hw.width = w;
                hw.height = h;
            }
            hw.configured = true;
            hw.needs_redraw = true;
        }
        self.screens[screen_idx].width = w;
        self.screens[screen_idx].height = h;
        if self.screens[screen_idx].fullscreen != host_fullscreen {
            self.screens[screen_idx].fullscreen = host_fullscreen;
        }
        // The output mode change and the video relayout each send an event the
        // waylandsink clients crash on mid-resize, so defer them until the drag settles.
        self.screens[screen_idx].resize_pending = Some(Instant::now() + RESIZE_SETTLE);
        crate::layout::apply_ui_layout(self, screen_idx);
        crate::ctrl::send_panels(self);
    }
}

/// How long the compositor waits after the last resize step before it pushes the
/// new output mode and relayouts the video planes.
const RESIZE_SETTLE: Duration = Duration::from_millis(150);

/// Applies a settled resize. Deferred from `configure` so the waylandsink clients are
/// touched once per drag, not per step. The output mode is published only at the first
/// setup, never on a later resize, since that mode change crashes the waylandsink.
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
        // new_seat only fires for seats appearing after registry init, so the
        // seat that exists at startup is captured here.
        self.host.host_seat = Some(seat.clone());
        let seats = self.host.sctk_seats.as_mut().unwrap();
        match capability {
            Capability::Pointer => {
                self.host.pointer = seats.get_pointer(qh, &seat).ok();
                self.seat.add_pointer();
            }
            Capability::Keyboard => {
                self.host.keyboard = seats.get_keyboard(qh, &seat, None).ok();
                self.seat
                    .add_keyboard(Default::default(), 600, 25)
                    .ok();
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
    }

    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _seat: CWlSeat, capability: Capability) {
        log::info!("input device gone: {capability:?}");
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
                    self.host.pointer_seen = true;
                }
                PointerEventKind::Leave { .. } => {
                    self.host.pointer_screen = None;
                    crate::input::pointer_clear_focus(self);
                }
                PointerEventKind::Motion { time } => {
                    if let Some(idx) = screen_idx.or(self.host.pointer_screen) {
                        let s = &self.screens[idx];
                        self.host.pointer_pos =
                            (s.x as f64 + ev.position.0, ev.position.1);
                        crate::input::pointer_motion(self, time);
                    }
                }
                PointerEventKind::Press { time, button, serial } => {
                    self.host.last_pointer_serial = serial;
                    crate::input::pointer_button(self, time, button, true);
                }
                PointerEventKind::Release { time, button, serial } => {
                    self.host.last_pointer_serial = serial;
                    crate::input::pointer_button(self, time, button, false);
                }
                PointerEventKind::Axis { time, vertical, horizontal, .. } => {
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
        crate::input::touch_down(self, time, id, lx, ly, screen_idx);
    }

    fn up(&mut self, _: &Connection, _: &QueueHandle<Self>, _touch: &WlTouch, _serial: u32, time: u32, id: i32) {
        crate::input::touch_up(self, time, id);
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
        // Motion comes without a surface, it stays on the down surface's screen.
        crate::input::touch_motion(self, time, id, position.0, position.1);
    }

    fn shape(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlTouch, _: i32, _: f64, _: f64) {}
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

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlKeyboard, _: &CWlSurface, _serial: u32) {}

    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlKeyboard, _serial: u32, event: KeyEvent) {
        crate::input::key(self, event.time, event.raw_code, true, event.keysym);
    }

    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlKeyboard, _serial: u32, event: KeyEvent) {
        crate::input::key(self, event.time, event.raw_code, false, event.keysym);
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _serial: u32,
        modifiers: Modifiers,
        _raw: u32,
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

smithay_client_toolkit::delegate_compositor!(LiviState);
smithay_client_toolkit::delegate_output!(LiviState);
smithay_client_toolkit::delegate_seat!(LiviState);
smithay_client_toolkit::delegate_keyboard!(LiviState);
smithay_client_toolkit::delegate_pointer!(LiviState);
smithay_client_toolkit::delegate_touch!(LiviState);
smithay_client_toolkit::delegate_xdg_shell!(LiviState);
smithay_client_toolkit::delegate_xdg_window!(LiviState);
smithay_client_toolkit::delegate_registry!(LiviState);

pub fn request_frame(state: &mut LiviState, screen_idx: usize) {
    let qh = state.host.qh.clone();
    if let (Some(w), Some(qh)) = (state.host.window_for_screen(screen_idx), qh)
        && !w.frame_pending {
            w.window
                .wl_surface()
                .frame(&qh, w.window.wl_surface().clone());
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
