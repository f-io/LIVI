// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use livi_core_proto::config::{Config, TouchCalibration};
use livi_core_proto::input::{Input, Phase, Point};
use livi_core_proto::state::{Display, Screen};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{broadcast, mpsc, oneshot, watch};

const TITLEBAR_H: u32 = 32;
const CLAIM_TIMEOUT: Duration = Duration::from_secs(3);
const RECONNECT_EVERY: Duration = Duration::from_millis(500);
const TOUCH_BACKLOG: usize = 64;

pub fn env(cfg: &Config, ctrl: &Path) -> Vec<(String, String)> {
    let mut env = vec![
        (
            "LIVI_OUTPUT_APP_ID".to_string(),
            "dev.f-io.livi".to_string(),
        ),
        (
            "LIVI_COMPOSITOR_CTRL".to_string(),
            ctrl.display().to_string(),
        ),
        ("LIVI_SCREENS".to_string(), "main,cluster,aux".to_string()),
        ("RUST_BACKTRACE".to_string(), "1".to_string()),
    ];
    let kiosk = cfg.kiosk.main || std::env::var("LIVI_KIOSK").is_ok_and(|v| v == "1");
    if cfg.main_screen_width > 0 && cfg.main_screen_height > 0 {
        let h = cfg.main_screen_height + if kiosk { 0 } else { TITLEBAR_H };
        env.push((
            "LIVI_OUTPUT_SIZE".to_string(),
            format!("{}x{h}", cfg.main_screen_width),
        ));
    }
    let screens = [
        ("MAIN", &cfg.display_mode.main, &cfg.display_connector.main),
        (
            "CLUSTER",
            &cfg.display_mode.cluster,
            &cfg.display_connector.cluster,
        ),
        ("AUX", &cfg.display_mode.aux, &cfg.display_connector.aux),
    ];
    for (role, mode, connector) in screens {
        if !mode.is_empty() {
            let key = if role == "MAIN" {
                "LIVI_DISPLAY_MODE".to_string()
            } else {
                format!("LIVI_{role}_DISPLAY_MODE")
            };
            env.push((key, mode.clone()));
        }
        if !connector.is_empty() {
            env.push((format!("LIVI_{role}_CONNECTOR"), connector.clone()));
        }
    }
    env
}

fn screen_of(role: &str) -> Option<Screen> {
    match role {
        "main" => Some(Screen::Main),
        "cluster" => Some(Screen::Cluster),
        "aux" => Some(Screen::Aux),
        _ => None,
    }
}

fn display_of(line: &str) -> Option<Display> {
    let mut words = line.splitn(7, ' ');
    let (Some("display"), Some(connector), Some(role), Some(current), Some(modes), Some(id)) = (
        words.next(),
        words.next(),
        words.next(),
        words.next(),
        words.next(),
        words.next(),
    ) else {
        return None;
    };
    Some(Display {
        connector: connector.to_string(),
        id: id.to_string(),
        panel: words.next().unwrap_or("").trim().to_string(),
        screen: screen_of(role),
        current: (current != "-").then(|| current.to_string()),
        modes: if modes == "-" {
            Vec::new()
        } else {
            modes.split(',').map(str::to_string).collect()
        },
        touch: Vec::new(),
    })
}

fn touch_panel_of(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("touchpanel ")?;
    let (connector, name) = rest.split_once(' ')?;
    (!name.is_empty()).then_some((connector, name))
}

fn touch_calibration_lines(entries: &BTreeMap<String, TouchCalibration>) -> String {
    let mut out = format!("touchcals {}\n", entries.len());
    for (name, c) in entries {
        let display = if c.display.is_empty() {
            "-"
        } else {
            &c.display
        };
        let matrix: Vec<String> = c.matrix.iter().map(f64::to_string).collect();
        out.push_str(&format!(
            "touchcal {} {display} {} {name}\n",
            u8::from(c.enabled),
            matrix.join(" ")
        ));
    }
    out
}

fn phase_of(word: &str) -> Option<Phase> {
    match word {
        "down" => Some(Phase::Down),
        "move" => Some(Phase::Move),
        "up" => Some(Phase::Up),
        "cancel" => Some(Phase::Cancel),
        _ => None,
    }
}

fn touch_of(line: &str) -> Option<Input> {
    let mut words = line.split_whitespace();
    let (Some("touch"), Some(screen)) = (words.next(), words.next().and_then(screen_of)) else {
        return None;
    };
    let words: Vec<&str> = words.collect();
    let (fingers, rest) = words.as_chunks::<4>();
    if fingers.is_empty() || !rest.is_empty() {
        return None;
    }
    let points = fingers
        .iter()
        .map(|[id, phase, x, y]| {
            Some(Point {
                id: id.parse().ok()?,
                phase: phase_of(phase)?,
                x: x.parse().ok()?,
                y: y.parse().ok()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Input::Pointer { screen, points })
}

pub fn start(bin: &Path, cfg: &Config, ctrl: &Path, log: &Path) -> Option<tokio::process::Child> {
    if std::env::var("LIVI_NO_COMPOSITOR").is_ok_and(|v| v == "1") || !bin.exists() {
        return None;
    }
    let (stdout, stderr) = match log
        .parent()
        .map(std::fs::create_dir_all)
        .and_then(|_| std::fs::File::create(log).ok())
    {
        Some(f) => (
            f.try_clone()
                .map(Stdio::from)
                .unwrap_or_else(|_| Stdio::inherit()),
            Stdio::from(f),
        ),
        None => (Stdio::inherit(), Stdio::inherit()),
    };
    let mut cmd = tokio::process::Command::new(bin);
    cmd.envs(env(cfg, ctrl))
        .env(livi_core_proto::LIFELINE_ENV, "1")
        .stdin(Stdio::piped())
        .stdout(stdout)
        .stderr(stderr)
        .kill_on_drop(true);
    for var in ["APPIMAGE", "APPDIR", "ARGV0", "OWD"] {
        cmd.env_remove(var);
    }
    match cmd.spawn() {
        Ok(child) => Some(child),
        Err(e) => {
            eprintln!("[core] cannot start the compositor: {e}");
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panel {
    pub width_mm: u32,
    pub height_mm: u32,
    pub width_px: u32,
    pub height_px: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ui {
    pub wayland_display: String,
    pub render_node: Option<String>,
}

enum Cmd {
    Claim(String, oneshot::Sender<()>),
    Release(String),
    State(String, String),
}

fn round_half_up(v: f64) -> f64 {
    (v + 0.5).floor()
}

#[derive(Clone)]
pub struct CompositorControl {
    tx: mpsc::UnboundedSender<Cmd>,
    panels: watch::Receiver<HashMap<String, Panel>>,
    ui: watch::Receiver<Option<Ui>>,
    displays: watch::Receiver<Option<Vec<Display>>>,
    touches: broadcast::Sender<Input>,
    fullscreen: broadcast::Sender<(Screen, bool)>,
}

impl CompositorControl {
    pub fn connect(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let (panels_tx, panels) = watch::channel(HashMap::new());
        let (ui_tx, ui) = watch::channel(None);
        let (displays_tx, displays) = watch::channel(None);
        let (touches, _) = broadcast::channel(TOUCH_BACKLOG);
        let (fullscreen, _) = broadcast::channel(TOUCH_BACKLOG);
        tokio::spawn(
            Actor::new(
                path,
                panels_tx,
                ui_tx,
                displays_tx,
                touches.clone(),
                fullscreen.clone(),
            )
            .run(rx),
        );
        Self {
            tx,
            panels,
            ui,
            displays,
            touches,
            fullscreen,
        }
    }

    pub fn displays(&self) -> watch::Receiver<Option<Vec<Display>>> {
        self.displays.clone()
    }

    pub fn touches(&self) -> broadcast::Receiver<Input> {
        self.touches.subscribe()
    }

    pub fn fullscreen_changes(&self) -> broadcast::Receiver<(Screen, bool)> {
        self.fullscreen.subscribe()
    }

    pub async fn claim(&self, tag: &str) {
        let (done, wait) = oneshot::channel();
        let _ = self.tx.send(Cmd::Claim(tag.to_string(), done));
        let _ = wait.await;
    }

    pub fn release(&self, tag: &str) {
        let _ = self.tx.send(Cmd::Release(tag.to_string()));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn videocfg(
        &self,
        tag: &str,
        screen: &str,
        crop_l: f64,
        crop_t: f64,
        vis_w: f64,
        vis_h: f64,
        tier_w: f64,
        tier_h: f64,
    ) {
        let n = |v: f64| round_half_up(v) as i64;
        let line = format!(
            "videocfg {tag} {screen} {} {} {} {} {} {}\n",
            n(crop_l),
            n(crop_t),
            n(vis_w),
            n(vis_h),
            n(tier_w),
            n(tier_h)
        );
        let _ = self.tx.send(Cmd::State(format!("cfg:{tag}"), line));
    }

    pub fn videoshow(&self, tag: &str, visible: bool) {
        let line = format!("videoshow {tag} {}\n", u8::from(visible));
        let _ = self.tx.send(Cmd::State(format!("show:{tag}"), line));
    }

    pub fn screen(&self, role: &str, on: bool, size: Option<(u32, u32)>) {
        let size = size
            .filter(|(w, h)| *w > 0 && *h > 0)
            .map_or(String::new(), |(w, h)| format!(" {w} {h}"));
        let line = format!("screen {role} {}{size}\n", u8::from(on));
        let _ = self.tx.send(Cmd::State(format!("screen:{role}"), line));
    }

    pub fn fullscreen(&self, role: &str, on: bool) {
        let _ = self.tx.send(Cmd::State(
            format!("fullscreen:{role}"),
            fullscreen_line(role, on),
        ));
    }

    pub fn aspect(&self, role: &str, size: Option<(u32, u32)>) {
        let (w, h) = size.filter(|(w, h)| *w > 0 && *h > 0).unwrap_or((0, 0));
        let _ = self.tx.send(Cmd::State(
            format!("aspect:{role}"),
            format!("aspect {role} {w} {h}\n"),
        ));
    }

    pub fn mode(&self, role: &str, mode: &str) {
        let mode = if mode.is_empty() { "-" } else { mode };
        let _ = self.tx.send(Cmd::State(
            format!("mode:{role}"),
            format!("mode {role} {mode}\n"),
        ));
    }

    pub fn backdrop(&self, hex: &str) {
        let (r, g, b) = rgb(hex);
        let _ = self.tx.send(Cmd::State(
            "__backdrop__".into(),
            format!("backdrop {r} {g} {b}\n"),
        ));
    }

    pub fn gamma(&self, gamma: f64, contrast: f64, r: f64, g: f64, b: f64) {
        let line = format!("gamma {gamma} {contrast} {r} {g} {b}\n");
        let _ = self.tx.send(Cmd::State("__gamma__".into(), line));
    }

    pub fn touch_calibration(&self, entries: &BTreeMap<String, TouchCalibration>) {
        let lines = touch_calibration_lines(entries);
        let _ = self.tx.send(Cmd::State("__touchcal__".into(), lines));
    }

    pub fn panels(&self) -> watch::Receiver<HashMap<String, Panel>> {
        self.panels.clone()
    }

    pub fn ui(&self) -> watch::Receiver<Option<Ui>> {
        self.ui.clone()
    }
}

fn fullscreen_line(role: &str, on: bool) -> String {
    format!("fullscreen {role} {}\n", u8::from(on))
}

fn rgb(hex: &str) -> (u8, u8, u8) {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return (0, 0, 0);
    }
    let n = u32::from_str_radix(h, 16).unwrap_or(0);
    ((n >> 16) as u8, (n >> 8) as u8, n as u8)
}

pub fn backdrop_hex(cfg: &Config) -> String {
    let pick = if cfg.dark_mode {
        &cfg.background_color_dark
    } else {
        &cfg.background_color_light
    };
    match pick.as_deref().filter(|c| !c.is_empty()) {
        Some(c) => c.to_string(),
        None if cfg.dark_mode => "#000000".into(),
        None => "#f5f5f5".into(),
    }
}

struct Actor {
    path: PathBuf,
    writer: Option<tokio::net::unix::OwnedWriteHalf>,
    outbox: Vec<String>,
    state: Vec<(String, String)>,
    queue: Vec<(String, oneshot::Sender<()>)>,
    in_flight: Option<String>,
    claim_deadline: Option<tokio::time::Instant>,
    panels: watch::Sender<HashMap<String, Panel>>,
    ui: watch::Sender<Option<Ui>>,
    displays: watch::Sender<Option<Vec<Display>>>,
    incoming: Option<Incoming>,
    touches: broadcast::Sender<Input>,
    fullscreen: broadcast::Sender<(Screen, bool)>,
}

#[derive(Default)]
struct Incoming {
    count: usize,
    displays: Vec<Display>,
    touch: Vec<(String, String)>,
}

impl Actor {
    fn new(
        path: PathBuf,
        panels: watch::Sender<HashMap<String, Panel>>,
        ui: watch::Sender<Option<Ui>>,
        displays: watch::Sender<Option<Vec<Display>>>,
        touches: broadcast::Sender<Input>,
        fullscreen: broadcast::Sender<(Screen, bool)>,
    ) -> Self {
        Self {
            path,
            writer: None,
            outbox: Vec::new(),
            state: Vec::new(),
            queue: Vec::new(),
            in_flight: None,
            claim_deadline: None,
            panels,
            ui,
            displays,
            incoming: None,
            touches,
            fullscreen,
        }
    }

    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Cmd>) {
        let (lines_tx, mut lines) = mpsc::unbounded_channel::<Option<String>>();
        let mut retry = tokio::time::interval(RECONNECT_EVERY);
        loop {
            let deadline = self.claim_deadline;
            tokio::select! {
                cmd = rx.recv() => match cmd {
                    Some(cmd) => self.on_cmd(cmd).await,
                    None => return,
                },
                line = lines.recv() => match line.flatten() {
                    Some(line) => self.on_line(&line).await,
                    None => self.writer = None,
                },
                _ = async { tokio::time::sleep_until(deadline.unwrap()).await }, if deadline.is_some() => {
                    self.abort_in_flight().await;
                }
                _ = retry.tick(), if self.writer.is_none() => {
                    self.try_connect(&lines_tx).await;
                }
            }
        }
    }

    async fn try_connect(&mut self, lines_tx: &mpsc::UnboundedSender<Option<String>>) {
        let Ok(stream) = UnixStream::connect(&self.path).await else {
            return;
        };
        let (rd, wr) = stream.into_split();
        self.writer = Some(wr);
        let tx = lines_tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(rd).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = tx.send(Some(line));
            }
            let _ = tx.send(None);
        });
        self.flush().await;
    }

    async fn on_cmd(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Claim(tag, done) => {
                self.queue.push((tag, done));
                self.pump().await;
            }
            Cmd::Release(tag) => {
                self.queue.retain(|(t, _)| *t != tag);
                if self.in_flight.as_deref() == Some(&tag) {
                    self.abort_in_flight().await;
                }
            }
            Cmd::State(key, line) => {
                match self.state.iter_mut().find(|(k, _)| *k == key) {
                    Some(slot) => slot.1 = line,
                    None => self.state.push((key, line)),
                }
                self.flush().await;
            }
        }
    }

    async fn pump(&mut self) {
        if self.in_flight.is_some() || self.queue.is_empty() {
            return;
        }
        let (tag, done) = self.queue.remove(0);
        self.outbox.push(format!("claim {tag}\n"));
        self.flush().await;
        self.in_flight = Some(tag);
        self.claim_deadline = Some(tokio::time::Instant::now() + CLAIM_TIMEOUT);
        let _ = done.send(());
    }

    async fn abort_in_flight(&mut self) {
        if let Some(tag) = self.in_flight.take() {
            self.outbox.push(format!("unclaim {tag}\n"));
            self.flush().await;
        }
        self.end_claim().await;
    }

    async fn end_claim(&mut self) {
        self.in_flight = None;
        self.claim_deadline = None;
        self.pump().await;
    }

    async fn on_line(&mut self, line: &str) {
        let mut words = line.split(' ');
        match words.next() {
            Some("touch") => {
                if let Some(input) = touch_of(line) {
                    let _ = self.touches.send(input);
                }
            }
            Some("fullscreen") => {
                let (Some(role), Some(on)) = (words.next(), words.next()) else {
                    return;
                };
                let (Some(screen), Ok(on)) = (screen_of(role), on.parse::<u8>()) else {
                    return;
                };
                let on = on != 0;
                let key = format!("fullscreen:{role}");
                let line = fullscreen_line(role, on);
                match self.state.iter_mut().find(|(k, _)| *k == key) {
                    Some(slot) => slot.1 = line,
                    None => self.state.push((key, line)),
                }
                let _ = self.fullscreen.send((screen, on));
            }
            Some("bound") => {
                let tag = &line["bound ".len()..];
                if self.in_flight.as_deref() == Some(tag) {
                    self.end_claim().await;
                }
            }
            Some("panel") => {
                let parts: Vec<&str> = words.collect();
                let nums: Vec<u32> = parts
                    .iter()
                    .skip(1)
                    .filter_map(|s| s.parse().ok())
                    .collect();
                if let ([role, ..], [w_mm, h_mm, w_px, h_px]) = (parts.as_slice(), nums.as_slice())
                {
                    let panel = Panel {
                        width_mm: *w_mm,
                        height_mm: *h_mm,
                        width_px: *w_px,
                        height_px: *h_px,
                    };
                    self.panels.send_if_modified(|m| {
                        if m.get(*role) == Some(&panel) {
                            return false;
                        }
                        println!("[core] panel '{role}': {w_mm}x{h_mm} mm over {w_px}x{h_px} px");
                        m.insert(role.to_string(), panel);
                        true
                    });
                }
            }
            Some("displays") => {
                let count = words.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                self.incoming = Some(Incoming {
                    count,
                    ..Default::default()
                });
                self.take_displays();
            }
            Some("touchpanel") => {
                if let (Some(incoming), Some((connector, name))) =
                    (self.incoming.as_mut(), touch_panel_of(line))
                {
                    incoming
                        .touch
                        .push((connector.to_string(), name.to_string()));
                }
            }
            Some("display") => {
                if let (Some(incoming), Some(mut display)) =
                    (self.incoming.as_mut(), display_of(line))
                {
                    display.touch = incoming
                        .touch
                        .iter()
                        .filter(|(c, _)| *c == display.connector)
                        .map(|(_, name)| name.clone())
                        .collect();
                    incoming.displays.push(display);
                }
                self.take_displays();
            }
            Some("ui") => {
                let (Some(display), node) = (words.next(), words.next()) else {
                    return;
                };
                let render_node = node.filter(|n| *n != "-").map(str::to_string);
                self.ui.send_replace(Some(Ui {
                    wayland_display: display.to_string(),
                    render_node,
                }));
            }
            _ => {}
        }
    }

    fn take_displays(&mut self) {
        if let Some(Incoming {
            count,
            displays: list,
            ..
        }) = &self.incoming
            && list.len() >= *count
        {
            for d in list {
                println!(
                    "[core] display {} {:?} at {} '{}'",
                    d.connector,
                    d.screen,
                    d.current.as_deref().unwrap_or("-"),
                    d.panel
                );
            }
            self.displays.send_replace(Some(list.clone()));
            self.incoming = None;
        }
    }

    async fn flush(&mut self) {
        let Some(writer) = self.writer.as_mut() else {
            return;
        };
        let mut out = self.outbox.concat();
        for (_, line) in &self.state {
            out.push_str(line);
        }
        if writer.write_all(out.as_bytes()).await.is_ok() {
            self.outbox.clear();
        } else {
            self.writer = None;
        }
    }
}

pub fn panel_physical_mm(panel: &Panel, width_px: u32, height_px: u32) -> Option<(u32, u32)> {
    if panel.width_mm == 0 || panel.height_mm == 0 || panel.width_px == 0 || panel.height_px == 0 {
        return None;
    }
    let scale = |mm: u32, px: u32, native: u32| {
        round_half_up(f64::from(mm) * (f64::from(px) / f64::from(native)).powf(0.75))
    };
    let w = scale(panel.width_mm, width_px, panel.width_px);
    let h = scale(panel.height_mm, height_px, panel.height_px);
    (w > 0.0 && h > 0.0).then_some((w as u32, h as u32))
}

#[cfg(test)]
mod tests {
    use tokio::net::UnixListener;

    use super::*;
    use crate::test_dir::TempDir;
    use livi_core_proto::config::defaults;

    struct FakeCompositor {
        listener: UnixListener,
    }

    impl FakeCompositor {
        async fn accept(
            &self,
        ) -> (
            BufReader<tokio::net::unix::OwnedReadHalf>,
            tokio::net::unix::OwnedWriteHalf,
        ) {
            let (s, _) = tokio::time::timeout(Duration::from_secs(5), self.listener.accept())
                .await
                .unwrap()
                .unwrap();
            let (rd, wr) = s.into_split();
            (BufReader::new(rd), wr)
        }
    }

    async fn line(rd: &mut BufReader<tokio::net::unix::OwnedReadHalf>) -> String {
        let mut s = String::new();
        tokio::time::timeout(Duration::from_secs(5), rd.read_line(&mut s))
            .await
            .unwrap()
            .unwrap();
        s
    }

    fn setup(dir: &TempDir) -> (FakeCompositor, CompositorControl) {
        let path = dir.0.join("ctrl");
        let listener = UnixListener::bind(&path).unwrap();
        (
            FakeCompositor { listener },
            CompositorControl::connect(path),
        )
    }

    #[tokio::test]
    async fn state_reaches_the_compositor_and_is_sent_again_after_a_reconnect() {
        let dir = TempDir::new();
        let (comp, ctrl) = setup(&dir);
        ctrl.videoshow("main", false);
        ctrl.backdrop("#102030");
        let (mut rd, wr) = comp.accept().await;
        assert_eq!(line(&mut rd).await, "videoshow main 0\n");
        assert_eq!(line(&mut rd).await, "backdrop 16 32 48\n");
        drop((rd, wr));
        ctrl.videoshow("main", true);
        let (mut rd, _wr) = comp.accept().await;
        assert_eq!(line(&mut rd).await, "videoshow main 1\n");
        assert_eq!(line(&mut rd).await, "backdrop 16 32 48\n");
    }

    #[tokio::test]
    async fn claims_go_one_at_a_time_until_bound() {
        let dir = TempDir::new();
        let (comp, ctrl) = setup(&dir);
        ctrl.claim("main").await;
        let (mut rd, mut wr) = comp.accept().await;
        assert_eq!(line(&mut rd).await, "claim main\n");
        let second = tokio::spawn({
            let ctrl = ctrl.clone();
            async move { ctrl.claim("cluster-cluster").await }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!second.is_finished());
        wr.write_all(b"bound main\n").await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), second)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(line(&mut rd).await, "claim cluster-cluster\n");
        ctrl.release("cluster-cluster");
        assert_eq!(line(&mut rd).await, "unclaim cluster-cluster\n");
    }

    #[tokio::test]
    async fn the_main_shape_is_kept_and_a_missing_size_lets_it_go() {
        let dir = TempDir::new();
        let (comp, ctrl) = setup(&dir);
        ctrl.aspect("main", Some((1280, 720)));
        ctrl.aspect("main", Some((800, 480)));
        let (mut rd, _wr) = comp.accept().await;
        assert_eq!(
            line(&mut rd).await,
            "aspect main 800 480\n",
            "only the latest shape"
        );
        ctrl.aspect("main", Some((0, 480)));
        assert_eq!(line(&mut rd).await, "aspect main 0 0\n");
    }

    #[tokio::test]
    async fn the_latest_display_mode_is_kept_and_an_empty_one_is_the_panel_default() {
        let dir = TempDir::new();
        let (comp, ctrl) = setup(&dir);
        ctrl.mode("main", "800x600");
        ctrl.mode("main", "1024x768");
        let (mut rd, _wr) = comp.accept().await;
        assert_eq!(line(&mut rd).await, "mode main 1024x768\n");
        ctrl.mode("main", "");
        assert_eq!(line(&mut rd).await, "mode main -\n");
    }

    #[tokio::test]
    async fn touch_on_the_stream_arrives_as_pointer_input() {
        let dir = TempDir::new();
        let (comp, ctrl) = setup(&dir);
        let mut touches = ctrl.touches();
        ctrl.backdrop("#000000");
        let (_rd, mut wr) = comp.accept().await;
        wr.write_all(b"touch main 0 down 0.25000 0.50000 1 move 0.75000 0.10000\ntouch main 0 jump 0.1 0.1\ntouch dash 0 up 0.1 0.1\ntouch main 0 up 0.25000 0.50000\n")
            .await
            .unwrap();
        assert_eq!(
            next_touch(&mut touches).await,
            Input::Pointer {
                screen: Screen::Main,
                points: vec![
                    Point {
                        id: 0,
                        x: 0.25,
                        y: 0.5,
                        phase: Phase::Down
                    },
                    Point {
                        id: 1,
                        x: 0.75,
                        y: 0.1,
                        phase: Phase::Move
                    },
                ],
            }
        );
        assert_eq!(
            next_touch(&mut touches).await,
            Input::Pointer {
                screen: Screen::Main,
                points: vec![Point {
                    id: 0,
                    x: 0.25,
                    y: 0.5,
                    phase: Phase::Up
                }],
            },
            "an unknown phase or screen is dropped"
        );
    }

    async fn next_touch(touches: &mut broadcast::Receiver<Input>) -> Input {
        tokio::time::timeout(Duration::from_secs(5), touches.recv())
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn panels_and_the_display_are_reported() {
        let dir = TempDir::new();
        let (comp, ctrl) = setup(&dir);
        let mut ui = ctrl.ui();
        ctrl.screen("cluster", true, Some((800, 480)));
        let (mut rd, mut wr) = comp.accept().await;
        assert_eq!(line(&mut rd).await, "screen cluster 1 800 480\n");
        wr.write_all(b"ui wayland-2 /dev/dri/renderD129\npanel main 344 194 1920 1080\ndisplays 2\ntouchpanel HDMI-A-1 ILITEK ILITEK-TP\ndisplay HDMI-A-1 main 1280x720@60 1920x1080,1280x720 DEL-a0b1-00000001 DELL U2415\ndisplay DSI-1 - - 800x480 DSI-1 \n")
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), ui.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            *ui.borrow(),
            Some(Ui {
                wayland_display: "wayland-2".into(),
                render_node: Some("/dev/dri/renderD129".into())
            })
        );
        let mut panels = ctrl.panels();
        tokio::time::timeout(
            Duration::from_secs(5),
            panels.wait_for(|p| p.contains_key("main")),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(panels.borrow()["main"].width_mm, 344);
        let mut displays = ctrl.displays();
        tokio::time::timeout(Duration::from_secs(5), displays.wait_for(Option::is_some))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            *displays.borrow(),
            Some(vec![
                Display {
                    connector: "HDMI-A-1".into(),
                    id: "DEL-a0b1-00000001".into(),
                    panel: "DELL U2415".into(),
                    screen: Some(Screen::Main),
                    current: Some("1280x720@60".into()),
                    modes: vec!["1920x1080".into(), "1280x720".into()],
                    touch: vec!["ILITEK ILITEK-TP".into()],
                },
                Display {
                    connector: "DSI-1".into(),
                    id: "DSI-1".into(),
                    panel: String::new(),
                    screen: None,
                    current: None,
                    modes: vec!["800x480".into()],
                    touch: Vec::new(),
                },
            ])
        );
    }

    #[test]
    fn env_sizes_the_output_with_the_title_bar() {
        let mut cfg = defaults();
        let get = |env: &[(String, String)], k: &str| {
            env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
        };
        assert_eq!(
            get(&env(&cfg, Path::new("/run/c")), "LIVI_OUTPUT_SIZE").as_deref(),
            Some("1280x752")
        );
        cfg.kiosk.main = true;
        assert_eq!(
            get(&env(&cfg, Path::new("/run/c")), "LIVI_OUTPUT_SIZE").as_deref(),
            Some("1280x720")
        );
        assert_eq!(
            get(&env(&cfg, Path::new("/run/c")), "LIVI_COMPOSITOR_CTRL").as_deref(),
            Some("/run/c")
        );
        assert_eq!(
            get(&env(&cfg, Path::new("/run/c")), "LIVI_DISPLAY_MODE"),
            None
        );
        cfg.display_mode.main = "1024x600".into();
        cfg.display_mode.cluster = "1920x720".into();
        cfg.display_connector.cluster = "DSI-1".into();
        let set = env(&cfg, Path::new("/run/c"));
        assert_eq!(get(&set, "LIVI_DISPLAY_MODE").as_deref(), Some("1024x600"));
        assert_eq!(
            get(&set, "LIVI_CLUSTER_DISPLAY_MODE").as_deref(),
            Some("1920x720")
        );
        assert_eq!(
            get(&set, "LIVI_CLUSTER_CONNECTOR").as_deref(),
            Some("DSI-1")
        );
        assert_eq!(get(&set, "LIVI_MAIN_CONNECTOR"), None);
    }

    #[test]
    fn touch_calibration_goes_out_as_one_block() {
        let mut entries = BTreeMap::new();
        entries.insert(
            "ILITEK ILITEK-TP".to_string(),
            TouchCalibration {
                display: "DEL-a0b1-00000001".into(),
                enabled: true,
                matrix: [1.0, 0.0, 0.02, 0.0, 0.98, 0.0],
            },
        );
        entries.insert("Other".to_string(), TouchCalibration::default());
        assert_eq!(
            touch_calibration_lines(&entries),
            "touchcals 2\n\
             touchcal 1 DEL-a0b1-00000001 1 0 0.02 0 0.98 0 ILITEK ILITEK-TP\n\
             touchcal 0 - 1 0 0 0 1 0 Other\n"
        );
    }

    #[test]
    fn backdrop_and_colours() {
        let mut cfg = defaults();
        assert_eq!(backdrop_hex(&cfg), "#000000");
        cfg.dark_mode = false;
        assert_eq!(backdrop_hex(&cfg), "#f5f5f5");
        cfg.background_color_light = Some("#ffffff".into());
        assert_eq!(backdrop_hex(&cfg), "#ffffff");
        assert_eq!(rgb("#ff8000"), (255, 128, 0));
        assert_eq!(rgb("nope"), (0, 0, 0));
    }

    #[test]
    fn physical_size_follows_the_stream_resolution() {
        let panel = Panel {
            width_mm: 344,
            height_mm: 194,
            width_px: 1920,
            height_px: 1080,
        };
        assert_eq!(panel_physical_mm(&panel, 1920, 1080), Some((344, 194)));
        assert_eq!(panel_physical_mm(&panel, 1280, 720), Some((254, 143)));
        assert_eq!(
            panel_physical_mm(
                &Panel {
                    width_mm: 0,
                    ..panel
                },
                1280,
                720
            ),
            None
        );
    }
}
