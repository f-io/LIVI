// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::Id;
use smithay::backend::renderer::element::Kind as ElementKind;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::surface::{
    WaylandSurfaceRenderElement, render_elements_from_surface_tree,
};
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::gles::element::TextureShaderElement;
use smithay::backend::renderer::gles::{
    GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName, UniformType,
};
use smithay::backend::renderer::sync::SyncPoint;
use smithay::backend::renderer::{Bind, Color32F, Frame, Offscreen, Renderer};
use smithay::utils::{Logical, Point, Rectangle, Transform};

use crate::state::{BTN_GAP, BTN_W, Kind, LiviState, TITLEBAR_H};

smithay::backend::renderer::element::render_elements! {
    pub LiviElement<=GlesRenderer>;
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    Deco=MemoryRenderBufferRenderElement<GlesRenderer>,
    Calibrated=TextureShaderElement,
}

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
    if let Some(p) = state.cal_program.clone() {
        return Some(p);
    }
    let renderer = state.renderer.as_mut()?;
    match renderer.compile_custom_texture_shader(
        CAL_FRAG,
        &[
            UniformName::new("u_gamma", UniformType::_1f),
            UniformName::new("u_contrast", UniformType::_1f),
            UniformName::new("u_gain", UniformType::_3f),
        ],
    ) {
        Ok(p) => {
            state.cal_program = Some(p.clone());
            Some(p)
        }
        Err(e) => {
            log::error!("cal shader compile failed: {e}");
            state.cal.active = false;
            None
        }
    }
}

pub fn surface_size(
    surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
) -> (i32, i32) {
    smithay::backend::renderer::utils::with_renderer_surface_state(surface, |s| {
        s.surface_size().map(|sz| (sz.w, sz.h)).unwrap_or((0, 0))
    })
    .unwrap_or((0, 0))
}

pub fn surface_under(
    root: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    local: Point<f64, Logical>,
) -> Option<(
    smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    Point<f64, Logical>,
)> {
    use smithay::wayland::compositor::{TraversalAction, with_surface_tree_downward};
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

fn collect_elements(state: &mut LiviState, screen_idx: usize) -> Vec<LiviElement> {
    let s = &state.screens[screen_idx];
    let (sx, sw, sh) = (s.x, s.width, s.height);
    let fullscreen = s.fullscreen;
    let role = s.role.clone();
    let renderer = state.renderer.as_mut().unwrap();
    let mut elements: Vec<LiviElement> = Vec::new();
    let scale = smithay::utils::Scale::from(1.0);

    let to_local =
        |p: Point<i32, Logical>| Point::<i32, smithay::utils::Physical>::from((p.x - sx, p.y));

    for t in state
        .toplevels
        .iter()
        .filter(|t| t.kind == Kind::Dialog && t.screen_idx == screen_idx)
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
        let renderer = state.renderer.as_mut().unwrap();
        if let Some(set) = state.host.deco.get(&screen_idx) {
            let slot = BTN_W + BTN_GAP;
            let items: [(
                &smithay::backend::renderer::element::memory::MemoryRenderBuffer,
                Point<i32, Logical>,
            ); 5] = [
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

    let renderer = state.renderer.as_mut().unwrap();
    for t in state
        .toplevels
        .iter()
        .filter(|t| t.kind == Kind::Ui && t.screen_idx == screen_idx && !t.clear)
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

    for &vi in state.video_order.iter().rev() {
        let Some(t) = state.toplevels.get(vi) else {
            continue;
        };
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

pub struct FrameParts {
    pub elements: Vec<LiviElement>,
    pub clear: Color32F,
    pub cal: Option<GlesTexProgram>,
    pub uniforms: Vec<Uniform<'static>>,
}

pub fn frame_parts(state: &mut LiviState, screen_idx: usize) -> FrameParts {
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
    let cal = if state.cal.active && !state.cal.in_lut {
        cal_program(state)
    } else {
        None
    };
    let uniforms = vec![
        Uniform::new("u_gamma", state.cal.gamma),
        Uniform::new("u_contrast", state.cal.contrast),
        Uniform::new(
            "u_gain",
            (state.cal.gain[0], state.cal.gain[1], state.cal.gain[2]),
        ),
    ];
    FrameParts {
        elements,
        clear: Color32F::new(backdrop[0], backdrop[1], backdrop[2], backdrop[3]),
        cal,
        uniforms,
    }
}

pub type Damage = Option<Vec<Rectangle<i32, smithay::utils::Physical>>>;

pub fn draw<T>(
    renderer: &mut GlesRenderer,
    target: &mut T,
    tracker: &mut OutputDamageTracker,
    age: usize,
    parts: &FrameParts,
    size: (i32, i32),
    transform: Transform,
) -> Result<(SyncPoint, Damage), Box<dyn std::error::Error>>
where
    GlesRenderer: Bind<T>,
{
    if let Some(program) = parts.cal.as_ref() {
        let sync = render_calibrated(
            renderer,
            target,
            &parts.elements,
            parts.clear,
            size,
            program,
            &parts.uniforms,
            transform,
        )?;
        return Ok((sync, None));
    }
    let mut fb = renderer.bind(target)?;
    let result = tracker.render_output::<LiviElement, _>(
        renderer,
        &mut fb,
        age,
        &parts.elements,
        parts.clear,
    )?;
    Ok((result.sync, result.damage.cloned()))
}

pub fn render_screen(state: &mut LiviState, screen_idx: usize) {
    if state.drm.is_some() {
        crate::drm::render(state, screen_idx);
        return;
    }
    if state.renderer.is_none() {
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

    let parts = frame_parts(state, screen_idx);
    let mut tracker = OutputDamageTracker::new((width, height), 1.0, Transform::Flipped180);

    let res = {
        let renderer = state.renderer.as_mut().unwrap();
        let hw = state
            .host
            .windows
            .iter_mut()
            .find(|(i, _)| *i == screen_idx)
            .map(|(_, w)| w)
            .unwrap();
        draw(
            renderer,
            &mut hw.egl_surface,
            &mut tracker,
            0,
            &parts,
            (width, height),
            Transform::Flipped180,
        )
    };

    match res {
        Ok((sync, _)) => {
            crate::host::request_frame(state, screen_idx);
            if let Some(w) = state.host.window_for_screen(screen_idx)
                && let Err(e) = w.egl_surface.swap_buffers(None)
            {
                log::error!("swap_buffers failed: {e}");
            }
            if state.syncobj_state.is_some() {
                let _ = sync.wait();
            }
            crate::host::send_frame_callbacks(state);
        }
        Err(e) => log::error!("render failed: {e}"),
    }
}

pub fn calibrated(
    renderer: &mut GlesRenderer,
    parts: FrameParts,
    size: (i32, i32),
) -> Result<Vec<LiviElement>, Box<dyn std::error::Error>> {
    let Some(program) = parts.cal else {
        return Ok(parts.elements);
    };
    let (w, h) = size;
    let mut tex: GlesTexture = Offscreen::<GlesTexture>::create_buffer(
        renderer,
        Fourcc::Abgr8888,
        smithay::utils::Size::from((w, h)),
    )?;
    {
        let mut tracker = OutputDamageTracker::new((w, h), 1.0, Transform::Normal);
        let mut fb = Bind::<GlesTexture>::bind(renderer, &mut tex)?;
        tracker.render_output::<LiviElement, _>(
            renderer,
            &mut fb,
            0,
            &parts.elements,
            parts.clear,
        )?;
    }
    let texture = TextureRenderElement::from_static_texture(
        Id::new(),
        renderer.context_id(),
        (0.0, 0.0),
        tex,
        1,
        Transform::Normal,
        None,
        None,
        None,
        None,
        ElementKind::Unspecified,
    );
    Ok(vec![LiviElement::Calibrated(TextureShaderElement::new(
        texture,
        program,
        parts.uniforms,
    ))])
}

#[allow(clippy::too_many_arguments)]
fn render_calibrated<T>(
    renderer: &mut GlesRenderer,
    target: &mut T,
    elements: &[LiviElement],
    clear: Color32F,
    size: (i32, i32),
    program: &GlesTexProgram,
    uniforms: &[Uniform<'static>],
    transform: Transform,
) -> Result<SyncPoint, Box<dyn std::error::Error>>
where
    GlesRenderer: Bind<T>,
{
    let (w, h) = size;
    let mut tex: GlesTexture = Offscreen::<GlesTexture>::create_buffer(
        renderer,
        Fourcc::Abgr8888,
        smithay::utils::Size::from((w, h)),
    )?;
    {
        let mut offscreen_tracker = OutputDamageTracker::new((w, h), 1.0, Transform::Normal);
        let mut fb = Bind::<GlesTexture>::bind(renderer, &mut tex)?;
        offscreen_tracker.render_output::<LiviElement, _>(renderer, &mut fb, 0, elements, clear)?;
    }
    let mut fb = renderer.bind(target)?;
    let mut frame = renderer.render(&mut fb, smithay::utils::Size::from((w, h)), transform)?;
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
    Ok(frame.finish()?)
}
