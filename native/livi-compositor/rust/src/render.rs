//! Per-screen rendering: backdrop, tagged video planes, the UI plane,
//! compositor decorations and dialogs, with the optional full-output
//! calibration (gamma/contrast/gain) shader pass.

use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::surface::{
    render_elements_from_surface_tree, WaylandSurfaceRenderElement,
};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind as ElementKind;
use smithay::backend::renderer::gles::{
    GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName, UniformType,
};
use smithay::backend::renderer::{Bind, Color32F, Frame, Offscreen, Renderer};
use smithay::utils::{Logical, Point, Rectangle, Transform};

use crate::state::{Kind, LiviState, BTN_GAP, BTN_W, TITLEBAR_H};

smithay::backend::renderer::element::render_elements! {
    pub LiviElement<=GlesRenderer>;
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    Deco=MemoryRenderBufferRenderElement<GlesRenderer>,
}

// The calibration fragment shader, applied over the composited frame.
// Follows smithay's custom-texture-shader contract: the //_DEFINES_ line,
// the v_coords varying and the EXTERNAL sampler variant are mandatory.
const CAL_FRAG: &str = r#"
#version 100
//_DEFINES_
#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif
precision mediump float;
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif
uniform float alpha;
varying vec2 v_coords;
uniform float u_gamma;
uniform float u_contrast;
uniform vec3 u_gain;
#if defined(DEBUG_FLAGS)
uniform float tint;
#endif
void main() {
    vec3 c = texture2D(tex, v_coords).rgb;
    c = pow(c, vec3(1.0 / u_gamma));
    c = (c - 0.5) * u_contrast + 0.5;
    c = clamp(c * u_gain, 0.0, 1.0);
    gl_FragColor = vec4(c, 1.0) * alpha;
}
"#;

pub fn cal_program(state: &mut LiviState) -> Option<GlesTexProgram> {
    if let Some(p) = state.host.cal_program.clone() {
        return Some(p);
    }
    let renderer = state.host.renderer.as_mut()?;
    match renderer.compile_custom_texture_shader(
        CAL_FRAG,
        &[
            UniformName::new("u_gamma", UniformType::_1f),
            UniformName::new("u_contrast", UniformType::_1f),
            UniformName::new("u_gain", UniformType::_3f),
        ],
    ) {
        Ok(p) => {
            state.host.cal_program = Some(p.clone());
            Some(p)
        }
        Err(e) => {
            log::error!("cal shader compile failed: {e}");
            state.cal.active = false;
            None
        }
    }
}

/// Committed size of a toplevel's main surface.
pub fn surface_size(surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface) -> (i32, i32) {
    smithay::backend::renderer::utils::with_renderer_surface_state(surface, |s| {
        s.surface_size().map(|sz| (sz.w, sz.h)).unwrap_or((0, 0))
    })
    .unwrap_or((0, 0))
}

/// Topmost sub-surface of `root` under a root-local point.
pub fn surface_under(
    root: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    local: Point<f64, Logical>,
) -> Option<(
    smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    Point<f64, Logical>,
)> {
    use smithay::wayland::compositor::{with_surface_tree_downward, TraversalAction};
    let found: std::cell::RefCell<Option<(_, Point<f64, Logical>)>> = std::cell::RefCell::new(None);
    with_surface_tree_downward(
        root,
        Point::<i32, Logical>::from((0, 0)),
        |_, states, offset| {
            let mut off = *offset;
            off += states
                .cached_state
                .get::<smithay::wayland::compositor::SubsurfaceCachedState>()
                .current()
                .location;
            TraversalAction::DoChildren(off)
        },
        |surface, states, offset| {
            let mut off = *offset;
            off += states
                .cached_state
                .get::<smithay::wayland::compositor::SubsurfaceCachedState>()
                .current()
                .location;
            // the traversal already holds the states lock and a with_states
            // re-entry here deadlocks, so read the state off the data_map
            let size = states
                .data_map
                .get::<smithay::backend::renderer::utils::RendererSurfaceStateUserData>()
                .and_then(|d| d.lock().unwrap().surface_size());
            if let Some(size) = size {
                let rect = Rectangle::<f64, Logical>::new(
                    (off.x as f64, off.y as f64).into(),
                    (size.w as f64, size.h as f64).into(),
                );
                if rect.contains(local) {
                    *found.borrow_mut() = Some((
                        surface.clone(),
                        local - Point::from((off.x as f64, off.y as f64)),
                    ));
                }
            }
        },
        |_, _, _| true,
    );
    found.into_inner()
}

/// Collect the render elements for one screen, top to bottom (renderer order).
fn collect_elements(
    state: &mut LiviState,
    screen_idx: usize,
) -> Vec<LiviElement> {
    let s = &state.screens[screen_idx];
    let (sx, sw, sh) = (s.x, s.width, s.height);
    let fullscreen = s.fullscreen;
    let role = s.role.clone();
    let renderer = state.host.renderer.as_mut().unwrap();
    let mut elements: Vec<LiviElement> = Vec::new();
    let scale = smithay::utils::Scale::from(1.0);

    // Screen-local offset: the window renders layout range [sx .. sx+sw].
    let to_local = |p: Point<i32, Logical>| Point::<i32, smithay::utils::Physical>::from((p.x - sx, p.y));

    // dialogs (top)
    for t in state.toplevels.iter().filter(|t| t.kind == Kind::Dialog && t.screen_idx == screen_idx) {
        elements.extend(
            render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<GlesRenderer>>(
                renderer,
                t.toplevel.wl_surface(),
                to_local(t.position),
                scale,
                1.0,
                ElementKind::Unspecified,
            )
            .into_iter()
            .map(LiviElement::Surface),
        );
    }

    // decorations
    if !fullscreen {
        let deco_stale = state
            .host
            .deco
            .get(&screen_idx)
            .map(|d| d.titlebar_w != sw)
            .unwrap_or(true);
        if deco_stale {
            let set = crate::deco::build(&role, sw);
            state.host.deco.insert(screen_idx, set);
        }
        let renderer = state.host.renderer.as_mut().unwrap();
        if let Some(set) = state.host.deco.get(&screen_idx) {
            let slot = BTN_W + BTN_GAP;
            let items: [(&smithay::backend::renderer::element::memory::MemoryRenderBuffer, Point<i32, Logical>); 5] = [
                (&set.btn_close, Point::from((sx + sw - slot, 0))),
                (&set.btn_fs, Point::from((sx + sw - 2 * slot, 0))),
                (&set.btn_min, Point::from((sx + sw - 3 * slot, 0))),
                (&set.title, Point::from((sx + 12, 0))),
                (&set.titlebar, Point::from((sx, 0))),
            ];
            for (buf, pos) in items {
                if let Ok(el) = MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    to_local(pos).to_f64(),
                    buf,
                    None,
                    None,
                    None,
                    ElementKind::Unspecified,
                ) {
                    elements.push(LiviElement::Deco(el));
                }
            }
        }
    }

    // UI plane
    let renderer = state.host.renderer.as_mut().unwrap();
    for t in state
        .toplevels
        .iter()
        .filter(|t| t.kind == Kind::Ui && t.screen_idx == screen_idx)
    {
        elements.extend(
            render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<GlesRenderer>>(
                renderer,
                t.toplevel.wl_surface(),
                to_local(t.position),
                scale,
                1.0,
                ElementKind::Unspecified,
            )
            .into_iter()
            .map(LiviElement::Surface),
        );
    }

    // video planes, top-to-bottom = reverse of the bottom-to-top order
    for &vi in state.video_order.iter().rev() {
        let Some(t) = state.toplevels.get(vi) else { continue };
        if t.kind != Kind::Video || t.screen_idx != screen_idx || !t.visible {
            continue;
        }
        elements.extend(
            render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<GlesRenderer>>(
                renderer,
                t.toplevel.wl_surface(),
                to_local(t.position),
                scale,
                1.0,
                ElementKind::Unspecified,
            )
            .into_iter()
            .map(LiviElement::Surface),
        );
    }

    let _ = (sh, TITLEBAR_H);
    elements
}

pub fn render_screen(state: &mut LiviState, screen_idx: usize) {
    if state.host.renderer.is_none() {
        return;
    }
    let Some(w) = state.host.window_for_screen(screen_idx) else {
        return;
    };
    if !w.configured {
        return;
    }
    let (width, height) = (w.width, w.height);
    if width <= 0 || height <= 0 {
        return;
    }
    w.needs_redraw = false;

    let backdrop = {
        let s = &state.screens[screen_idx];
        if std::env::var("LIVI_DEBUG_BG").is_ok() {
            [0.55, 0.0, 0.55, 1.0]
        } else if s.has_backdrop_color {
            s.backdrop_color
        } else {
            [0.0, 0.0, 0.0, 1.0]
        }
    };

    let elements = collect_elements(state, screen_idx);
    let cal = if state.cal.active { cal_program(state) } else { None };
    let uniforms = vec![
        Uniform::new("u_gamma", state.cal.gamma),
        Uniform::new("u_contrast", state.cal.contrast),
        Uniform::new(
            "u_gain",
            (state.cal.gain[0], state.cal.gain[1], state.cal.gain[2]),
        ),
    ];

    // GL window surfaces have a bottom-left origin: the on-screen pass renders
    // with Flipped180, offscreen textures stay Normal.
    let mut tracker = OutputDamageTracker::new((width, height), 1.0, Transform::Flipped180);

    let res = {
        // Split borrow: renderer and the window's EGL surface both live in host.
        let host = &mut state.host;
        let renderer = host.renderer.as_mut().unwrap();
        let hw = host
            .windows
            .iter_mut()
            .find(|(i, _)| *i == screen_idx)
            .map(|(_, w)| w)
            .unwrap();
        let clear = Color32F::new(backdrop[0], backdrop[1], backdrop[2], backdrop[3]);
        if let Some(program) = cal.as_ref() {
            // Calibration: composite into an offscreen texture, then draw it
            // through the gamma shader onto the window.
            render_calibrated(
                renderer,
                &mut hw.egl_surface,
                &elements,
                clear,
                (width, height),
                program,
                &uniforms,
            )
        } else {
            match renderer.bind(&mut hw.egl_surface) {
                Ok(mut fb) => tracker
                    .render_output::<LiviElement, _>(renderer, &mut fb, 0, &elements, clear)
                    .map(|_| ())
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error>),
                Err(e) => {
                    log::error!("bind failed: {e}");
                    return;
                }
            }
        }
    };

    match res {
        Ok(()) => {
            crate::host::request_frame(state, screen_idx);
            if let Some(w) = state.host.window_for_screen(screen_idx)
                && let Err(e) = w.egl_surface.swap_buffers(None) {
                    log::error!("swap_buffers failed: {e}");
                }
            crate::host::send_frame_callbacks(state);
        }
        Err(e) => log::error!("render failed: {e}"),
    }
}

/// Composite `elements` offscreen, then draw the result through the
/// calibration shader onto the window surface.
#[allow(clippy::too_many_arguments)]
fn render_calibrated(
    renderer: &mut GlesRenderer,
    egl_surface: &mut smithay::backend::egl::EGLSurface,
    elements: &[LiviElement],
    clear: Color32F,
    size: (i32, i32),
    program: &GlesTexProgram,
    uniforms: &[Uniform<'static>],
) -> Result<(), Box<dyn std::error::Error>> {
    let (w, h) = size;
    let mut tex: GlesTexture =
        renderer.create_buffer(Fourcc::Abgr8888, smithay::utils::Size::from((w, h)))?;
    {
        // Offscreen texture target, unflipped
        let mut offscreen_tracker =
            OutputDamageTracker::new((w, h), 1.0, Transform::Normal);
        let mut fb = renderer.bind(&mut tex)?;
        offscreen_tracker.render_output::<LiviElement, _>(renderer, &mut fb, 0, elements, clear)?;
    }
    // the flip happens on the window blit
    let mut fb = renderer.bind(egl_surface)?;
    let mut frame =
        renderer.render(&mut fb, smithay::utils::Size::from((w, h)), Transform::Flipped180)?;
    let full = Rectangle::<i32, smithay::utils::Physical>::new((0, 0).into(), (w, h).into());
    frame.render_texture_from_to(
        &tex,
        Rectangle::<f64, smithay::utils::Buffer>::new(
            (0.0, 0.0).into(),
            (w as f64, h as f64).into(),
        ),
        full,
        &[full],
        &[],
        Transform::Normal,
        1.0,
        Some(program),
        uniforms,
    )?;
    let _ = frame.finish()?;
    Ok(())
}
