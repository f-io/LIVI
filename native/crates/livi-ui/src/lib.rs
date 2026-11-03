// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub mod clock;
#[cfg(not(target_os = "android"))]
mod cursor_ui;
mod dashboard_ui;
mod files_ui;
mod follow;
mod fonts;
mod fullscreen;
mod icon_ui;
mod icons;
mod keys_ui;
mod media;
mod media_ui;
mod overlay_ui;
mod secondary;
mod settings_ui;
mod touch_ui;
mod video_ui;

use std::cell::{Cell, RefCell};
use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::time::Duration;

use livi_core_client::{Client, Event};
use livi_core_proto::input::{Input, Phase, Point};
use livi_core_proto::state::{Front, Screen};
use serde_json::Value;
use slint::platform::WindowEvent;
use slint::{Color, Model, ModelRc, Timer, TimerMode, VecModel, Weak};

use crate::follow::{Follow, Update};
use livi_ui_model::shell::{Look, Tab, ui_scale};

slint::include_modules!();

type Kiosk = Box<dyn Fn(bool)>;
type Pick = Box<dyn Fn()>;

thread_local! {
    static CLIENT: RefCell<Option<Client>> = const { RefCell::new(None) };
    static MAIN: RefCell<Option<Weak<AppWindow>>> = const { RefCell::new(None) };
    static KIOSK: RefCell<Option<Kiosk>> = const { RefCell::new(None) };
    static PICK: RefCell<Option<Pick>> = const { RefCell::new(None) };
    static BASE_SCALE: Cell<Option<f32>> = const { Cell::new(None) };
    static ZOOM: Cell<f32> = const { Cell::new(1.0) };
}

const BANDS: usize = 24;

pub(crate) fn system_pick() -> bool {
    PICK.with_borrow(|p| p.as_ref().map(|pick| pick()).is_some())
}

pub fn file_picked(path: &std::path::Path) {
    let Some(weak) = MAIN.with_borrow(Clone::clone) else {
        return;
    };
    let Some(client) = CLIENT.with_borrow(Clone::clone) else {
        return;
    };
    settings_ui::file_picked(&weak, &client, path);
}

pub(crate) fn apply_zoom(ui: &AppWindow) {
    let Some(base) = BASE_SCALE.get() else { return };
    let window = ui.window();
    let physical = window.size();
    let scale = ui_scale(base, ZOOM.get(), physical.height as f32);
    if (window.scale_factor() - scale).abs() < 0.001 {
        return;
    }
    window.dispatch_event(WindowEvent::ScaleFactorChanged {
        scale_factor: scale,
    });
    window.dispatch_event(WindowEvent::Resized {
        size: physical.to_logical(scale),
    });
}

#[cfg(not(target_os = "android"))]
pub(crate) fn system_scale_changed(ui: &AppWindow, scale: f32) {
    BASE_SCALE.set(Some(scale));
    let weak = ui.as_weak();
    Timer::single_shot(Duration::ZERO, move || {
        if let Some(ui) = weak.upgrade() {
            apply_zoom(&ui);
        }
    });
}

fn rgb(c: Option<[u8; 3]>) -> Color {
    c.map_or(Color::from_argb_u8(0, 0, 0, 0), |[r, g, b]| {
        Color::from_rgb_u8(r, g, b)
    })
}

fn tab(t: Tab) -> TabData {
    let (page, name) = match t {
        Tab::Projection => (Page::Projection, "CropPortraitOutlined"),
        Tab::Telemetry => (Page::Telemetry, "SpeedOutlined"),
        Tab::Media => (Page::Media, "PlayCircleOutlined"),
        Tab::Camera => (Page::Camera, "CameraOutlined"),
        Tab::Settings => (Page::Settings, "SettingsOutlined"),
    };
    TabData {
        page,
        icon: icons::icon(name).unwrap_or_default(),
    }
}

fn apply_look(ui: &AppWindow, look: Look) {
    let theme = ui.global::<Theme>();
    theme.set_dark(look.dark);
    theme.set_primary_override(rgb(look.primary));
    theme.set_highlight_override(rgb(look.highlight));
    theme.set_background_override(rgb(look.background));
    theme.set_text_override(rgb(look.text));
    ui.set_right_hand(look.right_hand);
    ui.set_stream_width(look.stream.0);
    ui.set_stream_height(look.stream.1);
    let tabs: Vec<TabData> = look.tabs.iter().copied().map(tab).collect();
    if !tabs.iter().any(|t| t.page == ui.get_page()) {
        choose(ui, Page::Media);
    }
    ui.set_tabs(ModelRc::from(Rc::new(VecModel::from(tabs))));
    ui.set_projection_icon(tab(Tab::Projection).icon);
    secondary::theme(ui);
    apply_language(ui, &look.language);
    if ZOOM.get() != look.zoom {
        ZOOM.set(look.zoom);
        apply_zoom(ui);
    }
}

fn apply_language(ui: &AppWindow, lang: &str) {
    settings_ui::set_language(lang);
    secondary::set_texts();
    media_ui::set_texts(ui, settings_ui::tr);
    dashboard_ui::set_texts(&ui.global::<DashState>(), settings_ui::tr);
    ui.set_no_camera(settings_ui::tr("camera.noCamera", "No camera found").into());
}

fn render_settings(ui: &AppWindow) {
    if ui.get_page() == Page::Settings {
        CLIENT.with_borrow(|c| {
            if let Some(c) = c {
                settings_ui::render(ui, c);
            }
        });
    }
}

fn apply(ui: &AppWindow, update: Update) {
    match update {
        Update::Media(m) => CLIENT.with_borrow(|c| media_ui::apply_media(ui, c.as_ref(), m)),
        Update::Dab(d) => media_ui::apply_dab(ui, d),
        Update::Dash(d) => {
            let before = dashboard_ui::map_shown(ui);
            dashboard_ui::apply(ui, *d);
            if ui.get_page() == Page::Telemetry && dashboard_ui::map_shown(ui) != before {
                report_page(ui);
            }
        }
        Update::DashOn(screen, d) => CLIENT.with_borrow(|c| {
            if let Some(c) = c {
                secondary::dash(c, screen, *d);
            }
        }),
        Update::Secondary(list) => CLIENT.with_borrow(|c| {
            if let Some(c) = c {
                secondary::follow(ui, c, list);
            }
        }),
        Update::Front(front) => match (front, ui.get_page()) {
            (Front::Projection, Page::Projection) => {}
            (Front::Projection, _) => choose(ui, Page::Projection),
            (_, Page::Projection) => choose(ui, Page::Media),
            _ => {}
        },
        Update::Phone(phone) => {
            ui.set_phone(phone);
            settle_projection_focus(ui);
        }
        Update::Look(look) => apply_look(ui, look),
        Update::Kiosk(on) => {
            fullscreen::follow_main(ui.window(), on);
            KIOSK.with_borrow(|k| {
                if let Some(k) = k {
                    k(on);
                }
            });
        }
        Update::Settings => render_settings(ui),
        Update::Spectrum(bands) => {
            let model = ui.get_bands();
            for (i, v) in bands.into_iter().enumerate().take(model.row_count()) {
                if model.row_data(i) != Some(v) {
                    model.set_row_data(i, v);
                }
            }
        }
        Update::Open(path) => open_path(ui, &path),
        Update::Volume(level) => overlay_ui::volume(ui, level),
        Update::PhoneName(name) => overlay_ui::phone(ui, &name),
        Update::Camera(found) => ui.set_camera_found(found),
        Update::MainMask(mask) => ui.set_main_mask(dashboard_ui::mask_data(mask)),
        Update::Video(video) => video_ui::show(ui, video),
        Update::Fonts(paths) => {
            if fonts::register(&paths) {
                let lang = settings_ui::language();
                if !lang.is_empty() {
                    apply_language(ui, &lang);
                    render_settings(ui);
                }
                fonts::relayout(ui.window());
                secondary::relayout();
            }
        }
    }
}

pub fn report_page(ui: &AppWindow) {
    CLIENT.with_borrow(|c| {
        if let Some(c) = c {
            reported(ui, c, ui.get_page());
        }
    });
}

fn open_path(ui: &AppWindow, path: &str) {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let page = match parts.first().copied() {
        None | Some("projection") => Page::Projection,
        Some("telemetry") => Page::Telemetry,
        Some("media") => Page::Media,
        Some("camera") => Page::Camera,
        Some("settings") => Page::Settings,
        Some(other) => {
            println!("[ui] no page for {other}");
            return;
        }
    };
    match (page, parts.get(1).copied()) {
        (Page::Media, Some("dabplus")) => ui.global::<DabState>().set_view_dab(true),
        (Page::Media, Some("phone")) => ui.global::<DabState>().set_view_dab(false),
        _ => {}
    }
    choose(ui, page);
    if page == Page::Settings {
        CLIENT.with_borrow(|c| {
            if let Some(c) = c {
                settings_ui::open_path(ui, c, &parts[1..].join("/"));
            }
        });
    }
}

fn choose(ui: &AppWindow, page: Page) {
    ui.set_page(page);
    CLIENT.with_borrow(|c| {
        if let Some(c) = c {
            reported(ui, c, page);
        }
    });
}

fn reported(ui: &AppWindow, client: &Client, page: Page) {
    let (front, path) = match page {
        Page::Projection => (Front::Projection, "/"),
        Page::Telemetry if dashboard_ui::map_shown(ui) => (Front::Cluster, "/telemetry"),
        Page::Telemetry => (Front::Livi, "/telemetry"),
        Page::Media => (Front::Livi, media_ui::path(ui)),
        Page::Camera => (Front::Camera, "/camera"),
        Page::Settings => (Front::Livi, "/settings"),
    };
    client.show(Screen::Main, front);
    client.path(path);
    client.spectrum(page == Page::Media && ui.get_show_spectrum());
    media_ui::follow(ui);
    if page != Page::Settings {
        settings_ui::leave(client);
    }
    settle_projection_focus(ui);
}

fn settle_projection_focus(ui: &AppWindow) {
    if ui.get_page() == Page::Projection && ui.get_phone() == PhoneState::Streaming {
        ui.global::<Focus>().set_zone(FocusZone::None);
    }
}

fn send_touch(client: &Client, phase: TouchPhase, x: f32, y: f32) {
    let phase = match phase {
        TouchPhase::Down => Phase::Down,
        TouchPhase::Move => Phase::Move,
        TouchPhase::Up => Phase::Up,
        TouchPhase::Cancel => Phase::Cancel,
    };
    client.input(Input::Pointer {
        screen: Screen::Main,
        points: vec![Point {
            id: 0,
            x: f64::from(x.clamp(0.0, 1.0)),
            y: f64::from(y.clamp(0.0, 1.0)),
            phase,
        }],
    });
}

pub(crate) fn attach_video(window: &slint::Window, screen: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(h) = window.window_handle().window_handle()
            && let RawWindowHandle::AppKit(h) = h.as_raw()
        {
            livi_video_view::set_window(screen, h.ns_view.as_ptr() as usize);
            return true;
        }
        false
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, screen);
        true
    }
}

pub(crate) fn detach_video(screen: &str) {
    #[cfg(target_os = "macos")]
    livi_video_view::set_window(screen, 0);
    #[cfg(not(target_os = "macos"))]
    let _ = screen;
}

fn follow_core(weak: Weak<AppWindow>) -> impl FnMut(Event, &Value) + Send + 'static {
    let mut follow = Follow::default();
    move |ev, state| {
        let updates = match ev {
            Event::Welcome { version } => {
                println!("[ui] core {version}");
                follow.changed(None, state)
            }
            Event::Changed(keys) => follow.changed(Some(&keys), state),
            Event::Spectrum(bands) => vec![Update::Spectrum(bands)],
            Event::OpenPage(path) => vec![Update::Open(path)],
            Event::Gone(why) => {
                eprintln!("[ui] core gone: {why}");
                return;
            }
        };
        if updates.is_empty() {
            return;
        }
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            updates.into_iter().for_each(|u| apply(&ui, u));
        });
    }
}

fn snapshot_later(weak: Weak<AppWindow>, path: String) {
    let wait = std::env::var("LIVI_UI_SNAPSHOT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);
    Timer::single_shot(Duration::from_millis(wait), move || {
        let Some(ui) = weak.upgrade() else { return };
        let size = ui.window().size();
        println!(
            "[ui] window {}x{} at scale {}",
            size.width,
            size.height,
            ui.window().scale_factor()
        );
        let Ok(buf) = ui.window().take_snapshot() else {
            return;
        };
        let saved = image::RgbaImage::from_raw(buf.width(), buf.height(), buf.as_bytes().to_vec())
            .map(|img| img.save(&path));
        println!("[ui] snapshot {path}: {}", matches!(saved, Some(Ok(()))));
    });
}

fn wire(ui: &AppWindow, client: &Client) {
    let weak = ui.as_weak();
    ui.on_page_chosen({
        let weak = weak.clone();
        let c = client.clone();
        move |page| {
            if let Some(ui) = weak.upgrade() {
                reported(&ui, &c, page);
                if page == Page::Settings {
                    settings_ui::reset_to_root(&ui, &c);
                }
            }
        }
    });
    ui.on_touch({
        let c = client.clone();
        move |phase, x, y| send_touch(&c, phase, x, y)
    });
    media_ui::wire(ui, client);
    ui.on_spectrum_changed({
        let c = client.clone();
        move |on| c.spectrum(on)
    });
    dashboard_ui::wire(&ui.global::<DashState>(), client);
}

pub fn key(code: &str, down: bool, repeat: bool) {
    let Some(ui) = MAIN.with_borrow(|m| m.as_ref().and_then(Weak::upgrade)) else {
        return;
    };
    CLIENT.with_borrow(|c| {
        if let Some(c) = c {
            keys_ui::key(&ui, c, code, down, repeat);
        }
    });
}

pub fn run(
    connect: impl FnOnce() -> std::io::Result<UnixStream>,
    fixture: Option<Value>,
    kiosk: impl Fn(bool) + 'static,
    pick: Option<Pick>,
) -> Result<(), Box<dyn std::error::Error>> {
    KIOSK.with_borrow_mut(|k| *k = Some(Box::new(kiosk)));
    PICK.with_borrow_mut(|p| *p = pick);
    let ui = AppWindow::new()?;
    slint::set_xdg_app_id(
        std::env::var("LIVI_OUTPUT_APP_ID").unwrap_or_else(|_| "dev.f-io.livi".into()),
    )?;
    ui.set_bands(ModelRc::from(Rc::new(VecModel::from(vec![0.0f32; BANDS]))));
    ui.set_clock(clock::now_text().into());
    ui.global::<DashState>().set_clock(clock::now_text().into());

    let mut _peer = None;
    let stream = if fixture.is_some() {
        let (ours, theirs) = UnixStream::pair()?;
        _peer = Some(theirs);
        ours
    } else {
        connect()?
    };
    let weak = ui.as_weak();
    let client = Client::start(stream, "LIVI", follow_core(weak.clone()))?;
    CLIENT.with_borrow_mut(|c| *c = Some(client.clone()));
    MAIN.with_borrow_mut(|m| *m = Some(weak.clone()));
    let lang = client
        .state()
        .get("config")
        .and_then(|c| c["language"].as_str().map(str::to_string))
        .unwrap_or_else(|| "en".into());
    settings_ui::wire(&ui, &client, &lang);
    #[cfg(not(target_os = "android"))]
    keys_ui::wire(&ui, &client);
    #[cfg(target_os = "android")]
    touch_ui::wire(&ui, &client);
    video_ui::wire(&ui, &client);
    wire(&ui, &client);
    if let Some(state) = fixture {
        *client.state() = state.clone();
        Follow::default()
            .changed(None, &state)
            .into_iter()
            .for_each(|u| apply(&ui, u));
    }

    let clock_timer = Timer::default();
    clock_timer.start(TimerMode::Repeated, Duration::from_secs(1), {
        let weak = weak.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_clock(clock::now_text().into());
                ui.global::<DashState>().set_clock(clock::now_text().into());
                secondary::tick_clock(&clock::now_text());
            }
        }
    });

    let fullscreen_timer = Timer::default();
    fullscreen_timer.start(TimerMode::Repeated, Duration::from_millis(250), {
        let weak = weak.clone();
        let client = client.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                fullscreen::poll_main(ui.window(), &client);
                secondary::poll_fullscreen(&client);
            }
        }
    });

    ui.window().set_rendering_notifier({
        let weak = weak.clone();
        move |state, _| {
            let (slint::RenderingState::RenderingSetup, Some(ui)) = (state, weak.upgrade()) else {
                return;
            };
            if !attach_video(ui.window(), "main") {
                eprintln!("[ui] the window has no native handle for the video");
            }
            #[cfg(not(target_os = "android"))]
            cursor_ui::start(&ui);
            if BASE_SCALE.get().is_none() {
                BASE_SCALE.set(Some(ui.window().scale_factor()));
                let weak = weak.clone();
                Timer::single_shot(Duration::ZERO, move || {
                    if let Some(ui) = weak.upgrade() {
                        apply_zoom(&ui);
                    }
                });
            }
        }
    })?;
    if let Some((w, h)) = std::env::var("LIVI_UI_SIZE")
        .ok()
        .and_then(|s| {
            s.split_once('x')
                .map(|(w, h)| (w.parse().ok(), h.parse().ok()))
        })
        .and_then(|(w, h)| Some((w?, h?)))
    {
        ui.window().set_size(slint::LogicalSize::new(w, h));
    }
    ui.show()?;
    reported(&ui, &client, ui.get_page());
    if let Ok(page) = std::env::var("LIVI_UI_PAGE") {
        let page = match page.as_str() {
            "projection" => Page::Projection,
            "settings" => Page::Settings,
            "telemetry" => Page::Telemetry,
            _ => Page::Media,
        };
        choose(&ui, page);
        if page == Page::Settings {
            settings_ui::open_path(
                &ui,
                &client,
                &std::env::var("LIVI_UI_SETTINGS").unwrap_or_default(),
            );
        }
    }
    if let Ok(path) = std::env::var("LIVI_UI_SNAPSHOT") {
        snapshot_later(weak.clone(), path);
    }
    slint::run_event_loop_until_quit()?;
    let _ = ui.hide();
    Ok(())
}
