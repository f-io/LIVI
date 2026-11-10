// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use livi_core_proto::state::{DeviceStatus, DeviceView};
use livi_cp::airplay::{AirplayCommand, AirplayRemote, AirplaySender};

use super::*;

const DEVICE_PREFIX: &str = "airplay-";

#[derive(Debug)]
pub enum ApEvent {
    Started {
        id: u64,
        close: Arc<Notify>,
        peer: AirplaySender,
        remote: AirplayRemote,
    },
    Ended {
        id: u64,
    },
    NowPlaying {
        id: u64,
        patch: NowPlayingPatch,
    },
}

pub(super) struct ApPeer {
    pub(super) close: Arc<Notify>,
    peer: AirplaySender,
    remote: AirplayRemote,
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

pub(super) fn link_of(device: &str) -> Option<Link> {
    device
        .strip_prefix(DEVICE_PREFIX)
        .and_then(|id| id.parse().ok())
        .map(Link::Ap)
}

impl Projection {
    pub(super) fn on_ap(&mut self, ev: ApEvent) {
        match ev {
            ApEvent::Started {
                id,
                close,
                peer,
                remote,
            } => {
                self.ap_peers.insert(
                    id,
                    ApPeer {
                        close,
                        peer,
                        remote,
                    },
                );
                let index = self.upsert(Link::Ap(id), Transport::Wifi, &Ids::default());
                println!("[core] airplay is session #{index}");
                self.auto_activate(index);
                self.publish();
            }
            ApEvent::Ended { id } => {
                self.ap_peers.remove(&id);
                self.close_by_link(Link::Ap(id));
            }
            ApEvent::NowPlaying { id, patch } => {
                self.metadata(Link::Ap(id), HelperKind::NowPlaying(patch));
            }
        }
    }

    pub(super) fn ap_media(&self, id: u64, control: MediaControl) {
        let Some(peer) = self.ap_peers.get(&id) else {
            return;
        };
        let playing = self
            .sessions
            .iter()
            .find(|s| s.link == Link::Ap(id))
            .and_then(|s| s.now_playing.playing)
            .unwrap_or(false);
        peer.remote.send(match control {
            MediaControl::Play => AirplayCommand::Play,
            MediaControl::Pause => AirplayCommand::Pause,
            MediaControl::PlayPause if playing => AirplayCommand::Pause,
            MediaControl::PlayPause => AirplayCommand::Play,
            MediaControl::Next => AirplayCommand::Next,
            MediaControl::Prev => AirplayCommand::Prev,
        });
    }

    pub(super) fn ap_seek(&self, id: u64, ms: u32) {
        if let Some(peer) = self.ap_peers.get(&id) {
            peer.remote
                .send(AirplayCommand::Seek(f64::from(ms) / 1000.0));
        }
    }

    pub(super) fn ap_now_playing(&self, id: u64, own: &NowPlaying) -> NowPlaying {
        if own.title.is_some() {
            return own.clone();
        }
        let Some(name) = self.ap_peers.get(&id).and_then(|p| p.peer.name.as_deref()) else {
            return own.clone();
        };
        let named = |ids: &Ids| {
            self.devices
                .registry
                .entries()
                .iter()
                .filter(|e| e.ids().overlaps(ids))
                .filter_map(|e| e.name())
                .any(|n| same_name(n, name))
        };
        self.sessions
            .iter()
            .find(|s| {
                !matches!(s.link, Link::Ap(_)) && s.now_playing.title.is_some() && named(&s.device)
            })
            .map_or_else(|| own.clone(), |phone| phone.now_playing.clone())
    }

    pub(super) fn ap_views(&self) -> Vec<DeviceView> {
        self.sessions
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let Link::Ap(id) = s.link else { return None };
                let peer = self.ap_peers.get(&id).map(|p| &p.peer);
                Some(DeviceView {
                    id: format!("{DEVICE_PREFIX}{id}"),
                    name: Some(
                        peer.and_then(|p| p.name.clone())
                            .unwrap_or_else(|| "AirPlay".into()),
                    ),
                    model: peer.and_then(|p| p.model.clone()),
                    protocol: Some(Protocol::Airplay),
                    last_transport: Some("wifi".into()),
                    status: if s.active {
                        DeviceStatus::Active
                    } else {
                        DeviceStatus::Available
                    },
                    session: Some(i as u32 + 1),
                    ..Default::default()
                })
            })
            .collect()
    }

    pub(super) fn ap_video_shown(&mut self) {
        let Some(link) = self
            .sessions
            .iter()
            .map(|s| s.link)
            .find(|l| matches!(l, Link::Ap(_)))
        else {
            return;
        };
        self.video_up(link);
        if let Some(index) = self.by_link(link).map(|s| s.index) {
            self.activate(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_airplay_device_ids_name_an_airplay_link() {
        assert_eq!(link_of("airplay-7"), Some(Link::Ap(7)));
        assert_eq!(link_of("airplay-x"), None);
        assert_eq!(link_of("00:11:22:33:44:55"), None);
    }
}
