use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use livi_core_proto::PROTOCOL;
use livi_core_proto::frame::{Decoder, FrameError, encode};
use livi_core_proto::message::{Action, FromCore, MediaControl, ToCore};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, Notify, broadcast, oneshot, watch};

use crate::config_file::{ConfigFile, apply_patch};
use crate::dongle::LinkSpeedViewer;
use crate::hub::{Frame, Hub};
use crate::projection::{UiCommand, UiSenders};
use crate::spectrum::Viewer;
use crate::status_file::Activity;
use crate::update::UpdateAsk;

pub struct Core {
    pub hub: Arc<Hub>,
    config_file: ConfigFile,
    /// One settings change at a time, each reads the result of the one before.
    config_writes: Mutex<()>,
    pub quit: Arc<Notify>,
    pub restart: AtomicBool,
    /// The user quit LIVI, not a signal or a closed window.
    pub quit_asked: AtomicBool,
    /// Only kept because state/status.json publishes it.
    pub ui_path: watch::Sender<String>,
    /// Pages the telemetry asked for, each opened once.
    pub open_page: broadcast::Sender<String>,
    pub relaunch: std::sync::Mutex<Option<PathBuf>>,
    asks: UiSenders,
    uid: u32,
}

impl Core {
    pub fn new(
        hub: Hub,
        config_file: ConfigFile,
        uid: u32,
        quit: Arc<Notify>,
        asks: UiSenders,
    ) -> Self {
        Self {
            hub: Arc::new(hub),
            config_file,
            config_writes: Mutex::new(()),
            quit,
            restart: AtomicBool::new(false),
            quit_asked: AtomicBool::new(false),
            relaunch: std::sync::Mutex::new(None),
            ui_path: watch::channel(String::new()).0,
            open_page: broadcast::channel(8).0,
            asks,
            uid,
        }
    }

    pub async fn set_config(&self, patch: &Value) -> Result<(), String> {
        let _one_at_a_time = self.config_writes.lock().await;
        let next = apply_patch(&self.hub.config(), patch)?;
        self.config_file.save(&next).map_err(|e| format!("not saved: {e}"))?;
        self.hub.update(|s| s.config = next);
        Ok(())
    }

    pub fn activity(&self) -> watch::Receiver<Activity> {
        self.asks.activity.clone()
    }

    fn ask_update(&self, ask: UpdateAsk) -> Result<(), String> {
        self.asks.update.send(ask).map_err(|_| "updates are not available".to_string())
    }

    pub fn relaunch(&self, program: PathBuf) {
        *self.relaunch.lock().unwrap_or_else(|e| e.into_inner()) = Some(program);
        self.restart.store(true, Ordering::SeqCst);
        self.quit.notify_one();
    }

    pub fn media(&self, control: MediaControl) {
        let _ = self.asks.commands.send(UiCommand::Media(control));
    }

    /// Answered once every phone had its goodbye.
    pub fn goodbye(&self) -> oneshot::Receiver<()> {
        let (done, heard) = oneshot::channel();
        let _ = self.asks.goodbye.send(done);
        heard
    }

    async fn act(&self, action: Action) -> Result<(), String> {
        match action {
            Action::SetConfig { patch } => self.set_config(&patch).await,
            Action::Quit => {
                self.quit_asked.store(true, Ordering::SeqCst);
                self.quit.notify_one();
                Ok(())
            }
            Action::Restart => {
                self.restart.store(true, Ordering::SeqCst);
                self.quit.notify_one();
                Ok(())
            }
            Action::Show { screen, front } => {
                self.asks.wants.send_modify(|w| *w.get_mut(screen) = front);
                Ok(())
            }
            Action::Media { control } => {
                let _ = self.asks.commands.send(UiCommand::Media(control));
                Ok(())
            }
            Action::Seek { position_ms } => {
                let _ = self.asks.commands.send(UiCommand::Seek(position_ms));
                Ok(())
            }
            Action::NextDevice => {
                let _ = self.asks.commands.send(UiCommand::NextDevice);
                Ok(())
            }
            Action::SelectDevice { id } => {
                let _ = self.asks.commands.send(UiCommand::SelectDevice(id));
                Ok(())
            }
            Action::ForgetDevice { id } => {
                let _ = self.asks.commands.send(UiCommand::ForgetDevice(id));
                Ok(())
            }
            Action::ConnectDevice { id } => {
                tokio::spawn(crate::devices::connect_paired(id));
                Ok(())
            }
            Action::ApplySettings => {
                let _ = self.asks.commands.send(UiCommand::ApplySettings);
                Ok(())
            }
            Action::SetDongleRadio { radio, on } => {
                let _ = self.asks.dongle.send((radio, on));
                Ok(())
            }
            Action::CheckUpdate => self.ask_update(UpdateAsk::Check),
            Action::DownloadUpdate => self.ask_update(UpdateAsk::Download),
            Action::InstallUpdate => self.ask_update(UpdateAsk::Install),
            Action::AbortUpdate => self.ask_update(UpdateAsk::Abort),
        }
    }
}

pub async fn serve(core: Arc<Core>, listener: UnixListener) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let core = core.clone();
                tokio::spawn(async move {
                    if let Err(e) = client(core, stream).await {
                        println!("[core] client gone: {e}");
                    }
                });
            }
            Err(e) => {
                eprintln!("[core] accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

async fn send(wr: &mut OwnedWriteHalf, msg: &FromCore) -> io::Result<()> {
    let frame = encode(msg).map_err(io::Error::other)?;
    wr.write_all(&frame).await
}

async fn welcome(core: &Core, wr: &mut OwnedWriteHalf) -> io::Result<broadcast::Receiver<Frame>> {
    let (msg, patches) = core.hub.welcome();
    send(wr, &msg).await?;
    Ok(patches)
}

async fn next_spectrum(viewer: &mut Option<Viewer>) -> Frame {
    match viewer {
        Some(v) => v.next().await,
        None => std::future::pending().await,
    }
}

async fn client(core: Arc<Core>, stream: UnixStream) -> io::Result<()> {
    let peer = stream.peer_cred()?.uid();
    if peer != core.uid {
        println!("[core] refused a client of uid {peer}");
        return Ok(());
    }
    let (mut rd, mut wr) = stream.into_split();
    let mut dec = Decoder::new();
    let mut buf = vec![0u8; 8192];

    let hello = loop {
        if let Some(msg) = dec.next_message::<ToCore>() {
            break msg.map_err(io::Error::other)?;
        }
        let n = rd.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        dec.push(&buf[..n]);
    };
    let ToCore::Hello { protocol, client } = hello else {
        let reason = "the first message has to be hello".into();
        return send(&mut wr, &FromCore::Refused { reason }).await;
    };
    if protocol != PROTOCOL {
        let reason = format!("core speaks protocol {PROTOCOL}, not {protocol}");
        return send(&mut wr, &FromCore::Refused { reason }).await;
    }
    println!("[core] {client} connected");
    let mut pages = core.open_page.subscribe();
    let mut patches = welcome(&core, &mut wr).await?;
    let mut viewer: Option<Viewer> = None;
    let mut link_speed_viewer: Option<LinkSpeedViewer> = None;

    loop {
        tokio::select! {
            read = rd.read(&mut buf) => {
                let n = read?;
                if n == 0 {
                    break;
                }
                dec.push(&buf[..n]);
                while let Some(msg) = dec.next_message::<ToCore>() {
                    match msg {
                        Ok(ToCore::Resync) => patches = welcome(&core, &mut wr).await?,
                        Ok(ToCore::Action { id, action }) => {
                            let error = core.act(action).await.err();
                            send(&mut wr, &FromCore::Reply { id, error }).await?;
                        }
                        Ok(ToCore::Input { input }) => {
                            let _ = core.asks.input.send(input);
                        }
                        Ok(ToCore::Spectrum { on: true }) => {
                            viewer.get_or_insert_with(|| core.asks.spectrum.watch());
                        }
                        Ok(ToCore::Spectrum { on: false }) => viewer = None,
                        Ok(ToCore::LinkSpeed { on: true }) => {
                            link_speed_viewer.get_or_insert_with(|| {
                                LinkSpeedViewer::new(&core.asks.link_speed_viewers)
                            });
                        }
                        Ok(ToCore::LinkSpeed { on: false }) => link_speed_viewer = None,
                        Ok(ToCore::Path { path }) => {
                            core.ui_path.send_replace(path);
                        }
                        Ok(ToCore::Camera { found }) => {
                            let _ = core.asks.commands.send(UiCommand::CameraFound(found));
                        }
                        Ok(ToCore::Hello { .. }) => {}
                        Err(FrameError::Json(e)) => println!("[core] {client} sent a broken frame: {e}"),
                        Err(e @ FrameError::TooLarge(_)) => return Err(io::Error::other(e)),
                    }
                }
            }
            patch = patches.recv() => match patch {
                Ok(frame) => wr.write_all(&frame).await?,
                Err(broadcast::error::RecvError::Lagged(_)) => patches = welcome(&core, &mut wr).await?,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            frame = next_spectrum(&mut viewer) => wr.write_all(&frame).await?,
            page = pages.recv() => match page {
                Ok(path) => send(&mut wr, &FromCore::OpenPage { path }).await?,
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
    }
    println!("[core] {client} disconnected");
    Ok(())
}

#[cfg(test)]
mod tests {
    use livi_core_proto::patch::PatchOp;
    use livi_core_proto::state::{Front, PerScreen, State};
    use serde_json::json;

    use super::*;
    use crate::config_file::defaults;
    use crate::config_file::tests::TempDir;

    struct Peer {
        stream: UnixStream,
        dec: Decoder,
    }

    impl Peer {
        async fn connect(path: &std::path::Path) -> Self {
            Self { stream: UnixStream::connect(path).await.unwrap(), dec: Decoder::new() }
        }

        async fn send(&mut self, msg: &ToCore) {
            self.stream.write_all(&encode(msg).unwrap()).await.unwrap();
        }

        async fn recv(&mut self) -> Option<FromCore> {
            let mut buf = [0u8; 4096];
            loop {
                if let Some(msg) = self.dec.next_message() {
                    return Some(msg.unwrap());
                }
                let read = self.stream.read(&mut buf);
                let n = tokio::time::timeout(Duration::from_secs(5), read).await.unwrap().unwrap();
                if n == 0 {
                    return None;
                }
                self.dec.push(&buf[..n]);
            }
        }

        async fn hello(path: &std::path::Path) -> Self {
            let mut peer = Self::connect(path).await;
            peer.send(&ToCore::Hello { protocol: PROTOCOL, client: "test".into() }).await;
            assert!(matches!(peer.recv().await, Some(FromCore::Welcome { rev: 0, .. })));
            peer
        }
    }

    fn start(dir: &TempDir) -> (Arc<Core>, std::path::PathBuf) {
        let (core, path, _, _, _) = start_with_asks(dir);
        (core, path)
    }

    fn start_with_asks(
        dir: &TempDir,
    ) -> (
        Arc<Core>,
        std::path::PathBuf,
        crate::projection::UiAsks,
        crate::projection::DongleAsks,
        crate::projection::UpdateAsks,
    ) {
        let livi = PerScreen { main: Front::Livi, cluster: Front::Livi, aux: Front::Livi };
        let hub = Hub::new(State {
            front: livi,
            sessions: Default::default(),
            now_playing: Default::default(),
            telemetry: Default::default(),
            navigation: Default::default(),
            system: Default::default(),
            devices: Default::default(),
            update: Default::default(),
            config: defaults(),
        });
        let file = ConfigFile::new(dir.0.join("config.json"), dir.0.join("backup.json"));
        // SAFETY: geteuid has no preconditions.
        let uid = unsafe { libc::geteuid() };
        let (senders, asks, dongle, update) = crate::projection::ui_channels(livi);
        let core = Arc::new(Core::new(hub, file, uid, Arc::new(Notify::new()), senders));
        let path = dir.0.join("core.sock");
        let listener = UnixListener::bind(&path).unwrap();
        tokio::spawn(serve(core.clone(), listener));
        (core, path, asks, dongle, update)
    }

    #[tokio::test]
    async fn what_the_ui_asks_reaches_the_services() {
        use livi_core_proto::input::Input;
        use livi_core_proto::message::MediaControl;
        use livi_core_proto::state::Screen;

        let dir = TempDir::new();
        let (_core, path, mut asks, mut dongle, mut update) = start_with_asks(&dir);
        let mut ui = Peer::hello(&path).await;
        let show = Action::Show { screen: Screen::Main, front: Front::Projection };
        ui.send(&ToCore::Action { id: 1, action: show }).await;
        assert_eq!(ui.recv().await, Some(FromCore::Reply { id: 1, error: None }));
        assert_eq!(asks.wants.borrow().main, Front::Projection);
        let next = Action::Media { control: MediaControl::Next };
        ui.send(&ToCore::Action { id: 2, action: next }).await;
        assert_eq!(ui.recv().await, Some(FromCore::Reply { id: 2, error: None }));
        assert_eq!(asks.commands.recv().await, Some(UiCommand::Media(MediaControl::Next)));
        ui.send(&ToCore::Action { id: 3, action: Action::NextDevice }).await;
        assert_eq!(ui.recv().await, Some(FromCore::Reply { id: 3, error: None }));
        assert_eq!(asks.commands.recv().await, Some(UiCommand::NextDevice));
        ui.send(&ToCore::Action { id: 9, action: Action::Seek { position_ms: 61000 } }).await;
        assert_eq!(ui.recv().await, Some(FromCore::Reply { id: 9, error: None }));
        assert_eq!(asks.commands.recv().await, Some(UiCommand::Seek(61000)));
        let radio = Action::SetDongleRadio { radio: livi_core_proto::message::Radio::Bt, on: true };
        ui.send(&ToCore::Action { id: 4, action: radio }).await;
        assert_eq!(ui.recv().await, Some(FromCore::Reply { id: 4, error: None }));
        assert_eq!(dongle.radios.recv().await, Some((livi_core_proto::message::Radio::Bt, true)));
        for (id, action, ask) in [
            (5, Action::CheckUpdate, UpdateAsk::Check),
            (6, Action::DownloadUpdate, UpdateAsk::Download),
            (7, Action::InstallUpdate, UpdateAsk::Install),
            (8, Action::AbortUpdate, UpdateAsk::Abort),
        ] {
            ui.send(&ToCore::Action { id, action }).await;
            assert_eq!(ui.recv().await, Some(FromCore::Reply { id, error: None }));
            assert_eq!(update.recv().await, Some(ask));
        }
        let key = Input::Key { code: "KeyA".into(), down: true };
        ui.send(&ToCore::Input { input: key.clone() }).await;
        ui.send(&ToCore::Path { path: "/media".into() }).await;
        assert_eq!(asks.input.recv().await, Some(key));
        ui.send(&ToCore::Camera { found: true }).await;
        assert_eq!(asks.commands.recv().await, Some(UiCommand::CameraFound(true)));
    }

    #[tokio::test]
    async fn a_page_the_telemetry_asked_for_reaches_every_client() {
        let dir = TempDir::new();
        let (core, path) = start(&dir);
        let mut main = Peer::hello(&path).await;
        let mut cluster = Peer::hello(&path).await;
        core.open_page.send("/media".into()).unwrap();
        let page = Some(FromCore::OpenPage { path: "/media".into() });
        assert_eq!(main.recv().await, page);
        assert_eq!(cluster.recv().await, page);
    }

    async fn wait_for(done: impl Fn() -> bool) {
        for _ in 0..1000 {
            if done() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("never happened");
    }

    #[tokio::test]
    async fn a_client_gets_spectrum_frames_while_it_draws_them() {
        let dir = TempDir::new();
        let (core, path) = start(&dir);
        let feed = core.asks.spectrum.clone();
        let mut drawing = Peer::hello(&path).await;
        let mut other = Peer::hello(&path).await;
        drawing.send(&ToCore::Spectrum { on: true }).await;
        drawing.send(&ToCore::Spectrum { on: true }).await;
        wait_for(|| feed.viewing() == 1).await;
        feed.publish(vec![0.5]);
        assert_eq!(drawing.recv().await, Some(FromCore::Spectrum { bands: vec![0.5] }));

        drawing.send(&ToCore::Spectrum { on: false }).await;
        wait_for(|| feed.viewing() == 0).await;
        other.send(&ToCore::Spectrum { on: true }).await;
        wait_for(|| feed.viewing() == 1).await;
        drop(other);
        wait_for(|| feed.viewing() == 0).await;
    }

    #[tokio::test]
    async fn the_dongle_is_asked_for_rates_while_a_client_shows_the_link_speed() {
        let dir = TempDir::new();
        let (_core, path, _, dongle, _) = start_with_asks(&dir);
        let viewers = dongle.link_speed_viewers;
        let mut settings = Peer::hello(&path).await;
        let mut other = Peer::hello(&path).await;
        settings.send(&ToCore::LinkSpeed { on: true }).await;
        settings.send(&ToCore::LinkSpeed { on: true }).await;
        wait_for(|| *viewers.borrow() == 1).await;

        settings.send(&ToCore::LinkSpeed { on: false }).await;
        wait_for(|| *viewers.borrow() == 0).await;
        other.send(&ToCore::LinkSpeed { on: true }).await;
        wait_for(|| *viewers.borrow() == 1).await;
        drop(other);
        wait_for(|| *viewers.borrow() == 0).await;
    }

    #[tokio::test]
    async fn a_config_change_reaches_every_client_and_the_file() {
        let dir = TempDir::new();
        let (_core, path) = start(&dir);
        let mut ui = Peer::hello(&path).await;
        let mut dash = Peer::hello(&path).await;

        let action = Action::SetConfig { patch: json!({ "huVolume": 0.5 }) };
        ui.send(&ToCore::Action { id: 1, action }).await;

        let set =
            PatchOp::Set { path: vec!["config".into(), "huVolume".into()], value: json!(0.5) };
        let patch = FromCore::Patch { rev: 1, ops: vec![set] };
        let first = ui.recv().await.unwrap();
        let second = ui.recv().await.unwrap();
        assert!([&first, &second].contains(&&patch));
        assert!([&first, &second].contains(&&FromCore::Reply { id: 1, error: None }));
        assert_eq!(dash.recv().await, Some(patch));

        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.0.join("config.json")).unwrap())
                .unwrap();
        assert_eq!(saved["huVolume"], 0.5);
    }

    #[tokio::test]
    async fn a_bad_action_gets_an_error_and_changes_nothing() {
        let dir = TempDir::new();
        let (core, path) = start(&dir);
        let mut ui = Peer::hello(&path).await;

        let action = Action::SetConfig { patch: json!({ "huVolume": "loud" }) };
        ui.send(&ToCore::Action { id: 2, action }).await;
        assert!(matches!(ui.recv().await, Some(FromCore::Reply { id: 2, error: Some(_) })));

        ui.send(&ToCore::Action { id: 3, action: Action::InstallUpdate }).await;
        assert_eq!(
            ui.recv().await,
            Some(FromCore::Reply { id: 3, error: Some("updates are not available".into()) })
        );
        assert_eq!(core.hub.config().hu_volume, 0.95);
    }

    #[tokio::test]
    async fn resync_sends_the_whole_state_again() {
        let dir = TempDir::new();
        let (core, path) = start(&dir);
        let mut ui = Peer::hello(&path).await;
        core.hub.update(|s| s.front.main = Front::Projection);
        assert!(matches!(ui.recv().await, Some(FromCore::Patch { rev: 1, .. })));

        ui.send(&ToCore::Resync).await;
        match ui.recv().await {
            Some(FromCore::Welcome { rev, state, .. }) => {
                assert_eq!(rev, 1);
                assert_eq!(state.front.main, Front::Projection);
            }
            other => panic!("expected a welcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_client_on_another_protocol_is_refused() {
        let dir = TempDir::new();
        let (_core, path) = start(&dir);
        let mut old = Peer::connect(&path).await;
        old.send(&ToCore::Hello { protocol: PROTOCOL + 1, client: "old".into() }).await;
        assert!(matches!(old.recv().await, Some(FromCore::Refused { .. })));
        assert_eq!(old.recv().await, None);

        let mut rude = Peer::connect(&path).await;
        rude.send(&ToCore::Resync).await;
        assert!(matches!(rude.recv().await, Some(FromCore::Refused { .. })));
    }

    #[tokio::test]
    async fn restart_is_a_quit_that_asks_to_come_back() {
        let dir = TempDir::new();
        let (core, path) = start(&dir);
        let mut ui = Peer::hello(&path).await;
        let quit = core.quit.notified();
        ui.send(&ToCore::Action { id: 4, action: Action::Restart }).await;
        assert_eq!(ui.recv().await, Some(FromCore::Reply { id: 4, error: None }));
        tokio::time::timeout(Duration::from_secs(5), quit).await.unwrap();
        assert!(core.restart.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn quit_is_answered_then_announced() {
        let dir = TempDir::new();
        let (core, path) = start(&dir);
        let mut ui = Peer::hello(&path).await;
        let quit = core.quit.notified();
        ui.send(&ToCore::Action { id: 9, action: Action::Quit }).await;
        assert_eq!(ui.recv().await, Some(FromCore::Reply { id: 9, error: None }));
        tokio::time::timeout(Duration::from_secs(5), quit).await.unwrap();
    }
}
