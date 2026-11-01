// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

mod bundle;
mod clock;
mod core_process;
mod cursor_ui;
mod dashboard_ui;
mod files_ui;
mod follow;
mod fonts;
mod fullscreen;
mod games_ui;
mod icon_ui;
mod icons;
mod keys_ui;
mod log;
mod media;
mod media_ui;
mod minidsp_ui;
mod overlay_ui;
mod secondary;
mod settings_ui;
mod touch_ui;

use std::cell::{Cell, RefCell};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

use livi_core_client::{Client, Event, socket_path, wait_for_socket};
use livi_core_proto::LIFELINE_ENV;
use livi_core_proto::input::{Input, Phase, Point};
use livi_core_proto::state::{Front, Screen};
use serde_json::Value;
use slint::platform::WindowEvent;
use slint::{Color, Model, ModelRc, Timer, TimerMode, VecModel, Weak};

use crate::core_process::{CoreProcess, RELAUNCH};
use crate::follow::{Follow, Update};
use livi_ui_model::shell::{Look, Style, Tab, ui_scale};

slint::include_modules!();

thread_local! {
    static CLIENT: RefCell<Option<Client>> = const { RefCell::new(None) };
    static BASE_SCALE: Cell<Option<f32>> = const { Cell::new(None) };
    static ZOOM: Cell<f32> = const { Cell::new(1.0) };
}

const BANDS: usize = 24;

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
        Tab::Games => (Page::Games, "SportsEsportsOutlined"),
        Tab::MiniDsp => (Page::Minidsp, "GraphicEqOutlined"),
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
    theme.set_comand(look.style == Style::Comand);
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
    minidsp_ui::set_texts(ui, settings_ui::tr);
    dashboard_ui::set_texts(&ui.global::<DashState>(), settings_ui::tr);
    ui.set_no_camera(settings_ui::tr("camera.noCamera", "No camera found").into());
    games_ui::set_texts(ui, settings_ui::tr);
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
        Update::Media(m) => media_ui::apply_media(ui, m),
        Update::Dab(d) => media_ui::apply_dab(ui, d),
        Update::Games(g) => games_ui::apply(ui, g),
        Update::MiniDsp(v) => minidsp_ui::apply(ui, v),
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
        Update::Kiosk(on) => fullscreen::follow_main(ui.window(), on),
        Update::Settings => render_settings(ui),
        Update::Spectrum(bands) => {
            let model = ui.get_bands();
            for (i, v) in bands.into_iter().enumerate().take(model.row_count()) {
                if model.row_data(i) != Some(v) {
                    model.set_row_data(i, v);
                }
            }
        }
        Update::Status(text) => ui.set_status(text.into()),
        Update::Open(path) => open_path(ui, &path),
        Update::Volume(level) => overlay_ui::volume(ui, level),
        Update::PhoneName(name) => overlay_ui::phone(ui, &name),
        Update::Camera(found) => ui.set_camera_found(found),
        Update::MainMask(mask) => ui.set_main_mask(dashboard_ui::mask_data(mask)),
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
        Some("games") => Page::Games,
        Some("minidsp") => Page::Minidsp,
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

pub(crate) fn choose(ui: &AppWindow, page: Page) {
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
        Page::Games => (Front::Livi, "/games"),
        Page::Minidsp => (Front::Livi, "/minidsp"),
        Page::Settings => (Front::Livi, "/settings"),
    };
    client.show(Screen::Main, front);
    client.path(path);
    // Media always draws the spectrum: over the artwork or faint behind the page.
    client.spectrum(page == Page::Media);
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

fn connect(
    started_by_core: bool,
    core: &mut Option<CoreProcess>,
    quit_code: &Arc<AtomicI32>,
    planes: &Path,
) -> std::io::Result<UnixStream> {
    let path = socket_path();
    if started_by_core || cfg!(not(target_os = "macos")) {
        return wait_for_socket(&path, Duration::from_secs(15));
    }
    if let Ok(s) = wait_for_socket(&path, Duration::from_secs(1)) {
        return Ok(s);
    }
    let code = quit_code.clone();
    *core = Some(CoreProcess::start(planes, move |c| {
        code.store(c.unwrap_or(0), Ordering::SeqCst);
        let _ = slint::invoke_from_event_loop(|| {
            let _ = slint::quit_event_loop();
        });
    })?);
    wait_for_socket(&path, Duration::from_secs(15))
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
                let mut u = follow.changed(None, state);
                u.push(Update::Status(String::new()));
                u
            }
            Event::Changed(keys) => follow.changed(Some(&keys), state),
            Event::Spectrum(bands) => vec![Update::Spectrum(bands)],
            Event::OpenPage(path) => vec![Update::Open(path)],
            Event::Gone(why) => vec![Update::Status(format!("core: {why}"))],
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
    minidsp_ui::wire(ui, client);
    dashboard_ui::wire(&ui.global::<DashState>(), client);
    games_ui::wire(ui, client);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(b) = bundle::find() {
        b.use_bundled_gstreamer();
    }
    let started_by_core = std::env::var(LIFELINE_ENV).is_ok_and(|v| v == "1");
    let runtime = socket_path()
        .parent()
        .map(Path::to_path_buf)
        .ok_or("no runtime dir")?;
    std::fs::create_dir_all(&runtime)?;
    let planes = runtime.join("ui-planes.sock");
    let fixture = std::env::var_os("LIVI_UI_STATE");
    #[cfg(target_os = "macos")]
    if fixture.is_none() {
        livi_video_view::serve(&runtime.join("compositor.ctrl"), &planes)?;
    }

    let ui = AppWindow::new()?;
    slint::set_xdg_app_id(
        std::env::var("LIVI_OUTPUT_APP_ID").unwrap_or_else(|_| "dev.f-io.livi".into()),
    )?;
    ui.set_bands(ModelRc::from(Rc::new(VecModel::from(vec![0.0f32; BANDS]))));
    ui.set_clock(clock::now_text().into());
    ui.global::<DashState>().set_clock(clock::now_text().into());

    let quit_code = Arc::new(AtomicI32::new(0));
    let mut core = None;
    let mut _peer = None;
    let stream = if fixture.is_some() {
        let (ours, theirs) = UnixStream::pair()?;
        _peer = Some(theirs);
        ours
    } else {
        connect(started_by_core, &mut core, &quit_code, &planes)?
    };
    let weak = ui.as_weak();
    let client = Client::start(stream, "LIVI", follow_core(weak.clone()))?;
    CLIENT.with_borrow_mut(|c| *c = Some(client.clone()));
    let lang = client
        .state()
        .get("config")
        .and_then(|c| c["language"].as_str().map(str::to_string))
        .unwrap_or_else(|| "en".into());
    settings_ui::wire(&ui, &client, &lang);
    keys_ui::wire(&ui, &client);
    wire(&ui, &client);
    if let Some(path) = &fixture {
        let state: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
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
            "games" => Page::Games,
            "minidsp" => Page::Minidsp,
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

    let code = match core {
        Some(core) => core
            .stop(Duration::from_secs(8))
            .unwrap_or(quit_code.load(Ordering::SeqCst)),
        None => quit_code.load(Ordering::SeqCst),
    };
    if code == RELAUNCH {
        if let Some(b) = bundle::find() {
            b.relaunch()?;
            return Ok(());
        }
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(std::env::current_exe()?).exec();
        return Err(err.into());
    }
    Ok(())
}
