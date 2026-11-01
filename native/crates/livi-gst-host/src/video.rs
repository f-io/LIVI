// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl<O: Outside> Host<O> {
    pub(super) fn create_plane(&mut self, id: u32, rest: &[u8]) {
        let Some((codec, codec_data)) = parse_plane_body(rest) else {
            return;
        };

        self.planes.borrow_mut().remove(&id);
        self.url_players.remove(&id);
        let Some(plane) = self.outside.create_plane(id, &codec, codec_data) else {
            eprintln!("livi: create player 0x{id:x} (codec {codec}) FAILED");
            return;
        };
        plane.start();

        let feeder = feeder_of(id);
        if let Some(state) = self.active_feeder(feeder) {
            for frame in state.borrow().fan.primer() {
                plane.push(frame);
            }
        }
        if let Some(Some(fan)) = self.feed_fans.borrow().get(&feeder) {
            for frame in fan.primer() {
                plane.push(frame);
            }
        }
        for feed in self.feeds_to(Route::Plane(feeder), None) {
            if let Some(Some(fan)) = self.feed_fans.borrow().get(&feed)
                && fan.is_active()
            {
                for frame in fan.primer() {
                    plane.push(frame);
                }
            }
        }
        self.planes.borrow_mut().insert(id, plane);
    }

    pub(super) fn play_url(&mut self, id: u32, rest: &[u8]) {
        let Some((url, device)) = parse_url_body(rest) else {
            return;
        };

        self.planes.borrow_mut().remove(&id);
        self.url_players.remove(&id);
        let Some(player) = self.outside.play_url(id, &url, &device) else {
            eprintln!("livi: play 0x{id:x} from {url} FAILED");
            return;
        };
        player.start();
        self.url_players.insert(id, player);
    }

    pub(super) fn open_camera(&mut self, id: u32, rest: &[u8]) {
        let Some(order) = parse_camera_body(rest) else {
            return;
        };
        self.cameras.remove(&id);
        match self.outside.open_camera(id, &order) {
            Some(camera) => {
                self.cameras.insert(id, camera);
            }
            None => eprintln!("livi: camera {} for 0x{id:x} FAILED", order.device),
        }
    }

    pub(super) fn watch_cameras(&mut self) {
        if std::mem::replace(&mut self.cameras_watched, true) {
            return;
        }
        let wire = self.wire.clone();
        self.outside.watch_cameras(Box::new(move |list| {
            wire.reply(REPLY_CAMERAS, 0, &cameras_body(&list));
        }));
    }

    pub(super) fn open_feed(&mut self, id: u32, rest: &[u8]) {
        let Ok(path) = core::str::from_utf8(rest) else {
            return;
        };
        self.helper_feed = None;
        let sink = MediaFeed::<O> {
            planes: self.planes.clone(),
            audio: self.audio.clone(),
            fans: self.feed_fans.clone(),
            wanted: self.feed_wanted.clone(),
            routes: self.feed_routes.clone(),
        };
        match self.outside.open_feed(path, Box::new(sink)) {
            Some(ears) => {
                self.helper_feed = Some(ears);
                self.wire.reply(REPLY_FEED, id, rest);
            }
            None => self.wire.reply(REPLY_FEED, id, &[]),
        }
    }

    pub(super) fn active_feeder(&self, plane_id: u32) -> Option<&Rc<RefCell<ReceiverState>>> {
        self.receivers.values().map(|r| &r.state).find(|state| {
            let st = state.borrow();
            st.fan.is_active() && st.plane_id == plane_id
        })
    }

    pub(super) fn set_gamma(&mut self, id: u32, rest: &[u8]) {
        const N: usize = 5;
        if rest.len() < N * size_of::<f64>() {
            return;
        }
        let mut v = [0f64; N];
        for (i, slot) in v.iter_mut().enumerate() {
            let bytes = &rest[i * size_of::<f64>()..][..size_of::<f64>()];
            *slot = f64::from_ne_bytes(bytes.try_into().unwrap());
        }
        if let Some(p) = self.planes.borrow().get(&id) {
            p.set_gamma(v[0], v[1], v[2], v[3], v[4]);
        }
    }

    pub(super) fn open_receiver(&mut self, id: u32, rest: &[u8]) {
        const FLAGS: usize = 5;
        const KEY_LEN: usize = 32;
        if rest.len() < FLAGS + KEY_LEN {
            return;
        }

        let state = Rc::new(RefCell::new(ReceiverState {
            plane_id: u32::from_ne_bytes(rest[..4].try_into().unwrap()),
            is_cluster: rest[4] & 1 != 0,
            config: Vec::new(),
            fan: Fanout::new(),
        }));
        let feed = Feed::<O> {
            state: state.clone(),
            planes: self.planes.clone(),
            wire: self.wire.clone(),
        };
        let key: [u8; KEY_LEN] = rest[FLAGS..FLAGS + KEY_LEN].try_into().unwrap();
        let Some((ears, port)) = self.outside.listen(key, Box::new(feed)) else {
            return;
        };

        self.receivers.insert(id, Receiver { state, _ears: ears });
        self.wire.reply(REPLY_PORT, id, &port.to_le_bytes());
    }

    pub(super) fn route_feed(&mut self, id: u32, rest: &[u8]) {
        if rest.len() < 6 {
            return;
        }
        let target = u32::from_le_bytes(rest[1..5].try_into().unwrap());
        let route = if rest[0] == 1 {
            Route::Audio(target)
        } else {
            Route::Plane(target)
        };
        self.feed_routes.borrow_mut().insert(id, route);
        self.set_active_feeder(id, &rest[5..6]);
    }

    pub(super) fn feeds_to(&self, target: Route, except: Option<u32>) -> Vec<u32> {
        self.feed_routes
            .borrow()
            .iter()
            .filter(|(feed, route)| **route == target && Some(**feed) != except)
            .map(|(feed, _)| *feed)
            .collect()
    }

    pub(super) fn hold_feeds_to(&mut self, target: Route, except: Option<u32>) {
        for feed in self.feeds_to(target, except) {
            self.set_feed_active(feed, false);
        }
    }

    pub(super) fn set_active_feeder(&mut self, id: u32, rest: &[u8]) {
        let active = rest.first().is_some_and(|b| b & 1 != 0);
        let route = self.feed_routes.borrow().get(&id).copied();
        let Some(state) = self.receivers.get(&id).map(|r| r.state.clone()) else {
            if active {
                match route {
                    Some(Route::Plane(plane)) => {
                        self.hold_receivers_of(plane, None);
                        self.set_feed_active(plane, false);
                        self.hold_feeds_to(Route::Plane(plane), Some(id));
                    }
                    Some(Route::Audio(stream)) => {
                        self.hold_feeds_to(Route::Audio(stream), Some(id))
                    }
                    None => {
                        self.hold_receivers_of(id, None);
                        self.hold_feeds_to(Route::Plane(id), None);
                    }
                }
            }
            self.set_feed_active(id, active);
            return;
        };
        let plane_id = state.borrow().plane_id;

        if !active {
            state.borrow_mut().fan.set_active(false);
            return;
        }

        self.hold_receivers_of(plane_id, Some(id));
        self.set_feed_active(plane_id, false);
        self.hold_feeds_to(Route::Plane(plane_id), None);

        let mut st = state.borrow_mut();
        st.fan.set_active(true);
        st.fan.restart();
        if !st.config.is_empty() {
            self.wire.reply(REPLY_CONFIG, plane_id, &st.config);
        }
    }

    pub(super) fn hold_receivers_of(&self, plane_id: u32, except: Option<u32>) {
        for (other, o) in &self.receivers {
            if Some(*other) != except && o.state.borrow().plane_id == plane_id {
                o.state.borrow_mut().fan.set_active(false);
            }
        }
    }

    pub(super) fn set_feed_active(&mut self, id: u32, active: bool) {
        self.feed_wanted.borrow_mut().insert(id, active);
        let mut params = Vec::new();
        if let Some(Some(fan)) = self.feed_fans.borrow_mut().get_mut(&id) {
            if active && !fan.is_active() {
                fan.restart();
                params = fan.params().to_vec();
            }
            fan.set_active(active);
        }
        if params.is_empty() {
            return;
        }
        let target = match self.feed_routes.borrow().get(&id) {
            Some(Route::Plane(plane)) => *plane,
            Some(Route::Audio(_)) => return,
            None => id,
        };
        let planes = self.planes.borrow();
        MediaFeed::<O>::for_each_target(&planes, target, |p| {
            for frame in &params {
                p.push(frame);
            }
        });
    }
}
