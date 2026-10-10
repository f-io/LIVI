use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use livi_host_proto::{CLUSTER_PLANE_MIN, MAIN_TAG, PLANE_MAIN, PLANE_VIDEO, SCREENS, VIDEO_TAG};
use tokio::sync::broadcast;

use crate::compositor::CompositorControl;
use crate::gst_host::{Codec, GstHost};

const TAG: &str = MAIN_TAG;
const SCREEN: &str = SCREENS[0];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crop {
    pub crop_l: f64,
    pub crop_t: f64,
    pub vis_w: f64,
    pub vis_h: f64,
    pub tier_w: f64,
    pub tier_h: f64,
}

fn content_area(frame: (u32, u32), user: (u32, u32)) -> (f64, f64) {
    let ratio = |(w, h): (u32, u32)| f64::from(w.max(1)) / f64::from(h.max(1));
    let even = |v: f64| ((v.floor() as i64) & !1).max(2) as f64;
    let (user_ar, frame_ar) = (ratio(user), ratio(frame));
    if user_ar <= frame_ar {
        (even(f64::from(frame.1) * user_ar), f64::from(frame.1))
    } else {
        (f64::from(frame.0), even(f64::from(frame.0) / user_ar))
    }
}

pub fn crop_for(tier: (u32, u32), display: (u32, u32)) -> Option<Crop> {
    let (tw, th) = tier;
    if tw == 0 || th == 0 || display.0 == 0 || display.1 == 0 {
        return None;
    }
    let (cw, ch) = content_area(tier, display);
    Some(Crop {
        crop_l: ((f64::from(tw) - cw) / 2.0).max(0.0),
        crop_t: ((f64::from(th) - ch) / 2.0).max(0.0),
        vis_w: cw,
        vis_h: ch,
        tier_w: f64::from(tw),
        tier_h: f64::from(th),
    })
}

fn same_stream(codec: Option<Codec>, data: &[u8], next: Codec, atom: &[u8]) -> bool {
    codec == Some(next) && data == atom
}

#[derive(Default)]
struct State {
    started: bool,
    claiming: bool,
    codec: Option<Codec>,
    codec_data: Vec<u8>,
    generation: u64,
}

pub struct MainPlane {
    gst: GstHost,
    comp: CompositorControl,
    state: Mutex<State>,
    created: broadcast::Sender<()>,
}

impl MainPlane {
    pub fn new(gst: GstHost, comp: CompositorControl) -> Arc<Self> {
        let (created, _) = broadcast::channel(8);
        Arc::new(Self { gst, comp, state: Mutex::new(State::default()), created })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A new player needs a keyframe from the phone.
    pub fn player_created(&self) -> broadcast::Receiver<()> {
        self.created.subscribe()
    }

    pub fn prepare(self: &Arc<Self>, codec: Codec, atom: Vec<u8>) {
        let generation = {
            let st = self.state();
            if (st.started || st.claiming) && same_stream(st.codec, &st.codec_data, codec, &atom) {
                return;
            }
            drop(st);
            self.dispose();
            let mut st = self.state();
            st.claiming = true;
            st.codec = Some(codec);
            st.codec_data = atom;
            st.generation
        };
        let plane = self.clone();
        tokio::spawn(async move {
            plane.comp.claim(TAG).await;
            let data = {
                let mut st = plane.state();
                if st.generation != generation {
                    return;
                }
                st.claiming = false;
                st.started = true;
                st.codec = Some(codec);
                st.codec_data.clone()
            };
            plane.gst.create_player(PLANE_MAIN, codec, &data);
            let _ = plane.created.send(());
        });
    }

    pub fn set_crop(&self, crop: Option<Crop>) {
        let c = crop.unwrap_or(Crop {
            crop_l: 0.0,
            crop_t: 0.0,
            vis_w: 0.0,
            vis_h: 0.0,
            tier_w: 0.0,
            tier_h: 0.0,
        });
        self.comp.videocfg(TAG, SCREEN, c.crop_l, c.crop_t, c.vis_w, c.vis_h, c.tier_w, c.tier_h);
    }

    pub fn set_visible(&self, visible: bool) {
        self.comp.videoshow(TAG, visible);
    }

    pub fn dispose(&self) {
        let started = {
            let mut st = self.state();
            st.claiming = false;
            st.generation += 1;
            st.codec = None;
            std::mem::take(&mut st.started)
        };
        self.comp.release(TAG);
        if started {
            self.gst.stop(PLANE_MAIN);
        }
    }
}

/// A plane of its own above the main one, so video LIVI fetches itself never touches the
/// projection's player and its decoder.
pub struct VideoPlane {
    gst: GstHost,
    comp: CompositorControl,
    stops: AtomicU64,
}

impl VideoPlane {
    pub fn new(gst: GstHost, comp: CompositorControl) -> Arc<Self> {
        Arc::new(Self { gst, comp, stops: AtomicU64::new(0) })
    }

    /// Taken when a play is handed to a task, a stop before it gets going cancels it.
    pub fn turn(&self) -> u64 {
        self.stops.load(Ordering::SeqCst)
    }

    /// The plane takes the whole main screen, the sink keeps the picture's shape. False when a
    /// stop came first.
    pub async fn play(&self, turn: u64, url: &str, audio_device: &str) -> bool {
        if self.turn() != turn {
            return false;
        }
        self.comp.videocfg(VIDEO_TAG, SCREEN, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        self.comp.videoshow(VIDEO_TAG, true);
        self.comp.claim(VIDEO_TAG).await;
        if self.turn() != turn {
            return false;
        }
        self.gst.play_url(PLANE_VIDEO, url, audio_device);
        true
    }

    /// 0 pauses.
    pub fn set_rate(&self, rate: f64) {
        self.gst.set_play_rate(PLANE_VIDEO, rate);
    }

    pub fn seek(&self, seconds: f64) {
        self.gst.seek(PLANE_VIDEO, seconds);
    }

    pub fn set_muted(&self, muted: bool) {
        self.gst.set_play_muted(PLANE_VIDEO, muted);
    }

    pub fn stop(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
        self.comp.release(VIDEO_TAG);
        self.comp.videoshow(VIDEO_TAG, false);
        self.gst.stop(PLANE_VIDEO);
    }
}

/// In the order of the cluster plane ids.
const CLUSTER_SCREENS: [&str; 3] = SCREENS;

fn cluster_tag(i: usize) -> String {
    livi_host_proto::cluster_tag(CLUSTER_SCREENS[i])
}

fn cluster_plane(i: usize) -> u32 {
    CLUSTER_PLANE_MIN + i as u32
}

#[derive(Default)]
struct Slot {
    shown: bool,
    started: bool,
    claiming: bool,
    generation: u64,
}

#[derive(Default)]
struct ClusterState {
    codec: Option<Codec>,
    codec_data: Vec<u8>,
    crop: Option<Crop>,
    slots: [Slot; 3],
}

/// gst-host feeds every cluster plane from the one cluster receiver.
pub struct ClusterPlanes {
    gst: GstHost,
    comp: CompositorControl,
    state: Mutex<ClusterState>,
    created: broadcast::Sender<()>,
}

impl ClusterPlanes {
    pub fn new(gst: GstHost, comp: CompositorControl) -> Arc<Self> {
        let (created, _) = broadcast::channel(8);
        Arc::new(Self { gst, comp, state: Mutex::new(ClusterState::default()), created })
    }

    fn state(&self) -> MutexGuard<'_, ClusterState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A new player needs a keyframe from the phone.
    pub fn player_created(&self) -> broadcast::Receiver<()> {
        self.created.subscribe()
    }

    pub fn prepare(self: &Arc<Self>, codec: Codec, atom: Vec<u8>) {
        let restart = {
            let mut st = self.state();
            let same = same_stream(st.codec, &st.codec_data, codec, &atom);
            st.codec = Some(codec);
            st.codec_data = atom;
            !same
        };
        for i in 0..CLUSTER_SCREENS.len() {
            if restart {
                self.stop(i);
            }
            self.start(i);
        }
    }

    /// Indexed main, dash, aux.
    pub fn show_on(self: &Arc<Self>, shown: [bool; 3]) {
        for (i, on) in shown.into_iter().enumerate() {
            let was = std::mem::replace(&mut self.state().slots[i].shown, on);
            if on && !was {
                self.start(i);
            } else if !on && was {
                self.stop(i);
            }
        }
    }

    pub fn set_crop(&self, crop: Option<Crop>) {
        let started: Vec<usize> = {
            let mut st = self.state();
            st.crop = crop;
            (0..CLUSTER_SCREENS.len()).filter(|&i| st.slots[i].started).collect()
        };
        for i in started {
            self.place(i, crop);
        }
    }

    /// Which screens show the cluster stays set.
    pub fn dispose(&self) {
        self.state().codec = None;
        for i in 0..CLUSTER_SCREENS.len() {
            self.stop(i);
        }
    }

    fn start(self: &Arc<Self>, i: usize) {
        let generation = {
            let mut st = self.state();
            let known = st.codec.is_some();
            let slot = &mut st.slots[i];
            if !known || !slot.shown || slot.started || slot.claiming {
                return;
            }
            slot.claiming = true;
            slot.generation
        };
        let planes = self.clone();
        tokio::spawn(async move {
            let tag = cluster_tag(i);
            planes.comp.claim(&tag).await;
            let (codec, data, crop) = {
                let mut st = planes.state();
                let Some(codec) = st.codec else { return };
                let slot = &mut st.slots[i];
                if slot.generation != generation {
                    return;
                }
                slot.claiming = false;
                slot.started = true;
                (codec, st.codec_data.clone(), st.crop)
            };
            planes.place(i, crop);
            planes.comp.videoshow(&tag, true);
            planes.gst.create_player(cluster_plane(i), codec, &data);
            let _ = planes.created.send(());
        });
    }

    fn stop(&self, i: usize) {
        let started = {
            let mut st = self.state();
            let slot = &mut st.slots[i];
            slot.claiming = false;
            slot.generation += 1;
            std::mem::take(&mut slot.started)
        };
        let tag = cluster_tag(i);
        self.comp.release(&tag);
        if started {
            self.comp.videoshow(&tag, false);
            self.gst.stop(cluster_plane(i));
        }
    }

    fn place(&self, i: usize, crop: Option<Crop>) {
        let c = crop.unwrap_or(Crop {
            crop_l: 0.0,
            crop_t: 0.0,
            vis_w: 0.0,
            vis_h: 0.0,
            tier_w: 0.0,
            tier_h: 0.0,
        });
        self.comp.videocfg(
            &cluster_tag(i),
            CLUSTER_SCREENS[i],
            c.crop_l,
            c.crop_t,
            c.vis_w,
            c.vis_h,
            c.tier_w,
            c.tier_h,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
    use tokio::net::{UnixListener, UnixStream};

    use super::*;
    use crate::test_dir::TempDir;

    #[test]
    fn carplay_streams_at_the_display_size_need_no_crop() {
        let c = crop_for((1280, 720), (1280, 720)).unwrap();
        assert_eq!((c.crop_l, c.crop_t, c.vis_w, c.vis_h), (0.0, 0.0, 1280.0, 720.0));
        assert_eq!(crop_for((0, 720), (1280, 720)), None);
    }

    #[test]
    fn a_wider_display_crops_the_frame_top_and_bottom() {
        let c = crop_for((1280, 720), (1280, 480)).unwrap();
        assert_eq!((c.vis_w, c.vis_h), (1280.0, 480.0));
        assert_eq!(c.crop_t, 120.0);
        let c = crop_for((1280, 720), (800, 600)).unwrap();
        assert_eq!((c.vis_w, c.vis_h, c.crop_l), (960.0, 720.0, 160.0));
    }

    #[test]
    fn only_the_same_stream_keeps_the_running_player() {
        assert!(same_stream(Some(Codec::H265), &[1, 2], Codec::H265, &[1, 2]));
        // CarPlay hands its parameter sets over, Android Auto sends them in the stream.
        assert!(!same_stream(Some(Codec::H265), &[1, 2], Codec::H265, &[]));
        assert!(!same_stream(Some(Codec::H265), &[1, 2], Codec::H265, &[3, 4]));
        assert!(!same_stream(Some(Codec::H264), &[], Codec::H265, &[]));
        assert!(!same_stream(None, &[], Codec::H265, &[]));
    }

    #[tokio::test]
    async fn the_player_is_created_after_the_claim_and_only_once() {
        let dir = TempDir::new();
        let comp_listener = UnixListener::bind(dir.0.join("ctrl")).unwrap();
        let gst = GstHost::new(dir.0.join("gst.sock"));
        let plane = MainPlane::new(gst.clone(), CompositorControl::connect(dir.0.join("ctrl")));
        let mut created = plane.player_created();

        plane.set_visible(true);
        plane.prepare(Codec::H264, vec![1, 2]);
        plane.prepare(Codec::H264, vec![1, 2]);
        tokio::time::timeout(Duration::from_secs(5), created.recv()).await.unwrap().unwrap();

        let (ctrl, _) = tokio::time::timeout(Duration::from_secs(5), comp_listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut lines = BufReader::new(ctrl).lines();
        let mut seen = Vec::new();
        while seen.len() < 2 {
            seen.push(
                tokio::time::timeout(Duration::from_secs(5), lines.next_line())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap(),
            );
        }
        assert!(seen.contains(&"claim main".to_string()));
        assert!(seen.contains(&"videoshow main 1".to_string()));

        let mut host = UnixStream::connect(dir.0.join("gst.sock")).await.unwrap();
        let mut buf = vec![0u8; 64];
        let n = tokio::time::timeout(Duration::from_secs(5), host.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        let mut framer = livi_host_proto::Framer::new();
        framer.push(&buf[..n]);
        let msg = framer.next_message().unwrap();
        assert_eq!((msg.op, msg.id), (1, PLANE_MAIN));
        assert_eq!(msg.rest, [&[4u8][..], b"h264", &[1, 2]].concat());
        assert!(created.try_recv().is_err());
    }

    async fn next_line(lines: &mut tokio::io::Lines<BufReader<UnixStream>>) -> String {
        tokio::time::timeout(Duration::from_secs(5), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    }

    async fn next_msg(
        host: &mut UnixStream,
        framer: &mut livi_host_proto::Framer,
    ) -> livi_host_proto::Message {
        loop {
            if let Some(msg) = framer.next_message() {
                return msg;
            }
            let mut buf = vec![0u8; 64];
            let n = tokio::time::timeout(Duration::from_secs(5), host.read(&mut buf))
                .await
                .unwrap()
                .unwrap();
            framer.push(&buf[..n]);
        }
    }

    #[tokio::test]
    async fn a_cluster_player_runs_on_each_screen_that_shows_it() {
        let dir = TempDir::new();
        let comp_listener = UnixListener::bind(dir.0.join("ctrl")).unwrap();
        let gst = GstHost::new(dir.0.join("gst.sock"));
        let planes =
            ClusterPlanes::new(gst.clone(), CompositorControl::connect(dir.0.join("ctrl")));
        let mut created = planes.player_created();

        planes.show_on([false, true, false]);
        planes.set_crop(None);
        planes.prepare(Codec::H264, vec![7]);
        tokio::time::timeout(Duration::from_secs(5), created.recv()).await.unwrap().unwrap();

        let (ctrl, _) = tokio::time::timeout(Duration::from_secs(5), comp_listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut lines = BufReader::new(ctrl).lines();
        let mut seen = Vec::new();
        for _ in 0..3 {
            seen.push(next_line(&mut lines).await);
        }
        assert!(seen.contains(&"claim cluster-dash".to_string()));
        assert!(seen.contains(&"videocfg cluster-dash dash 0 0 0 0 0 0".to_string()));
        assert!(seen.contains(&"videoshow cluster-dash 1".to_string()));

        let mut host = UnixStream::connect(dir.0.join("gst.sock")).await.unwrap();
        let mut framer = livi_host_proto::Framer::new();
        let msg = next_msg(&mut host, &mut framer).await;
        assert_eq!((msg.op, msg.id), (1, CLUSTER_PLANE_MIN + 1));

        planes.show_on([false, false, false]);
        // This compositor never confirms, so the open claim is dropped too.
        let mut after = Vec::new();
        while !after.contains(&"videoshow cluster-dash 0".to_string()) {
            after.push(next_line(&mut lines).await);
        }
        assert!(after.contains(&"unclaim cluster-dash".to_string()));
        let msg = next_msg(&mut host, &mut framer).await;
        assert_eq!(msg.id, CLUSTER_PLANE_MIN + 1);
        assert_ne!(msg.op, 1);
        assert!(created.try_recv().is_err());
    }

    #[tokio::test]
    async fn the_video_plane_lies_over_the_main_screen_until_it_stops() {
        let dir = TempDir::new();
        let comp_listener = UnixListener::bind(dir.0.join("ctrl")).unwrap();
        let gst = GstHost::new(dir.0.join("gst.sock"));
        let plane = VideoPlane::new(gst.clone(), CompositorControl::connect(dir.0.join("ctrl")));

        assert!(plane.play(plane.turn(), "http://127.0.0.1:9/i.m3u8", "car").await);
        let (ctrl, _) = tokio::time::timeout(Duration::from_secs(5), comp_listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut lines = BufReader::new(ctrl).lines();
        let mut seen = Vec::new();
        for _ in 0..3 {
            seen.push(next_line(&mut lines).await);
        }
        assert!(seen.contains(&"videocfg video main 0 0 0 0 0 0".to_string()));
        assert!(seen.contains(&"videoshow video 1".to_string()));
        assert!(seen.contains(&"claim video".to_string()));

        let mut host = UnixStream::connect(dir.0.join("gst.sock")).await.unwrap();
        let mut framer = livi_host_proto::Framer::new();
        let msg = next_msg(&mut host, &mut framer).await;
        assert_eq!((msg.op, msg.id), (22, PLANE_VIDEO));
        assert_eq!(
            livi_host_proto::parse_url_body(&msg.rest),
            Some(("http://127.0.0.1:9/i.m3u8".into(), "car".into()))
        );

        plane.set_rate(0.0);
        plane.seek(3.5);
        plane.set_muted(true);
        let rate = next_msg(&mut host, &mut framer).await;
        assert_eq!((rate.op, rate.rest), (23, 0.0f64.to_le_bytes().to_vec()));
        let seek = next_msg(&mut host, &mut framer).await;
        assert_eq!((seek.op, seek.rest), (24, 3.5f64.to_le_bytes().to_vec()));
        let mute = next_msg(&mut host, &mut framer).await;
        assert_eq!((mute.op, mute.rest), (25, vec![1]));

        plane.stop();
        let mut after = Vec::new();
        while !after.contains(&"videoshow video 0".to_string()) {
            after.push(next_line(&mut lines).await);
        }
        let msg = next_msg(&mut host, &mut framer).await;
        assert_eq!((msg.op, msg.id), (3, PLANE_VIDEO));

        let turn = plane.turn();
        plane.stop();
        assert!(!plane.play(turn, "http://127.0.0.1:9/i.m3u8", "car").await, "stopped first");
        let msg = next_msg(&mut host, &mut framer).await;
        assert_eq!((msg.op, msg.id), (3, PLANE_VIDEO), "no play behind the stop");
    }
}
