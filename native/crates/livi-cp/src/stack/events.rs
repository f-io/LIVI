// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl<M: Media> Conn<M> {
    pub(super) fn on_event_connection(&mut self, sock: TcpStream) {
        println!("[cpStack] event channel connected");
        let Some(shared) = self.pair_verify.shared_secret() else {
            return;
        };
        let write = hkdf_sha512(&shared, b"Events-Salt", b"Events-Write-Encryption-Key");
        let read = hkdf_sha512(&shared, b"Events-Salt", b"Events-Read-Encryption-Key");
        let (rd, wr) = sock.into_split();
        self.event = Some(EventConn {
            read: rd,
            write: wr,
            cipher: ControlCipher::new(read, write),
            sealed: Vec::new(),
            plain: Vec::new(),
            cseq: 0,
        });
    }

    pub(super) async fn on_event_bytes(&mut self, chunk: &[u8]) {
        let debug = self.ctx.debug;
        let Some(ev) = self.event.as_mut() else {
            return;
        };
        ev.sealed.extend_from_slice(chunk);
        match ev.cipher.decrypt(&ev.sealed) {
            Ok((data, rest)) => {
                ev.sealed = rest;
                ev.plain.extend(data);
            }
            Err(_) => {
                println!("[cpStack] event channel decrypt failed");
                return;
            }
        }
        let (messages, rest) = rtsp::parse(&ev.plain);
        ev.plain = rest;
        for msg in messages {
            if msg.method.starts_with("RTSP/") || msg.method.starts_with("HTTP/") {
                if msg.path != "200" || (debug && !msg.body.is_empty()) {
                    let body = match bplist::decode(&msg.body) {
                        Ok(v) => format!(" {v:?}"),
                        Err(_) if msg.body.is_empty() => String::new(),
                        Err(_) => format!(" ({}B non-plist body)", msg.body.len()),
                    };
                    println!(
                        "[cpStack] event response {} {}{body}",
                        msg.path, msg.protocol
                    );
                }
                continue;
            }
            let decoded = if msg.body.is_empty() {
                String::new()
            } else {
                match bplist::decode(&msg.body) {
                    Ok(v) => format!(" {v:?}"),
                    Err(_) => format!(" ({}B non-plist body)", msg.body.len()),
                }
            };
            println!("[cpStack] event < {} {}{decoded}", msg.method, msg.path);
            let answer = rtsp::build_response(&msg, status(200));
            let sealed = ev.cipher.encrypt(&answer);
            if ev.write.write_all(&sealed).await.is_err() {
                return;
            }
        }
    }

    pub(super) async fn send_event_command(&mut self, body: &Value) {
        self.send_event_command_with(body, "").await;
    }

    pub(super) async fn send_event_command_with(&mut self, body: &Value, headers: &str) {
        let kind = body.get("type").and_then(Value::as_str).unwrap_or("");
        if self.ctx.debug && !matches!(kind, "hidSendReport" | "iAPSendMessage") {
            println!("[cpStack] event > {}{body:?}", headers.replace("\r\n", " "));
        }
        let Some(ev) = self.event.as_mut() else {
            return;
        };
        ev.cseq += 1;
        let payload = bplist::encode(body);
        let mut msg = format!(
            "POST /command RTSP/1.0\r\nContent-Type: {PLIST}\r\nContent-Length: {}\r\nCSeq: {}\r\n{headers}\r\n",
            payload.len(),
            ev.cseq
        )
        .into_bytes();
        msg.extend_from_slice(&payload);
        let sealed = ev.cipher.encrypt(&msg);
        let _ = ev.write.write_all(&sealed).await;
    }

    pub(super) async fn send_hid(&mut self, uid: u32, report: &[u8]) {
        self.send_event_command(&dict([
            ("type", Value::String("hidSendReport".into())),
            ("uuid", Value::String(format!("{uid:x}"))),
            ("hidReport", Value::Data(report.to_vec())),
        ]))
        .await;
    }

    pub(super) async fn send_night_mode(&mut self, night: bool) {
        self.send_event_command(&dict([
            ("type", Value::String("setNightMode".into())),
            ("params", dict([("nightMode", Value::Bool(night))])),
        ]))
        .await;
        self.send_appearance(self.main_uuid(), night).await;
    }

    pub(super) async fn send_cluster_appearance(&mut self, night: bool) {
        if self.ctx.config.borrow().info.cluster.is_none() {
            return;
        }
        self.send_appearance(self.alt_uuid(), night).await;
    }

    pub(super) async fn send_appearance(&mut self, uuid: String, night: bool) {
        for kind in ["uiAppearanceUpdate", "mapAppearanceUpdate"] {
            self.send_event_command(&dict([
                ("type", Value::String(kind.into())),
                (
                    "params",
                    dict([
                        ("uuid", Value::String(uuid.clone())),
                        ("appearanceMode", Value::Int(u64::from(night))),
                        ("appearanceSetting", Value::Int(APPEARANCE_AUTOMATIC)),
                    ]),
                ),
            ]))
            .await;
        }
    }

    pub(super) async fn send_limited_ui(&mut self, limited: bool) {
        self.send_event_command(&dict([
            ("type", Value::String("setLimitedUI".into())),
            ("params", dict([("limitedUI", Value::Bool(limited))])),
        ]))
        .await;
    }

    pub(super) async fn force_keyframe(&mut self, uuid: &str) {
        self.send_event_command(&dict([
            ("type", Value::String("forceKeyFrame".into())),
            ("params", dict([("uuid", Value::String(uuid.into()))])),
        ]))
        .await;
    }

    pub(super) async fn activate_cluster_stream(&mut self) {
        let url = self.wanted_cluster_url();
        self.send_event_command(&dict([
            ("type", Value::String("showUI".into())),
            (
                "params",
                dict([
                    ("uuid", Value::String(self.alt_uuid())),
                    ("url", Value::String(url.clone())),
                ]),
            ),
        ]))
        .await;
        self.cluster_url = Some(url);
        self.force_keyframe(&self.alt_uuid()).await;
    }

    pub(super) fn main_uuid(&self) -> String {
        info::display_uuids(&self.ctx.config.borrow().info).0
    }

    pub(super) fn alt_uuid(&self) -> String {
        info::display_uuids(&self.ctx.config.borrow().info).1
    }

    pub(super) fn wanted_cluster_url(&self) -> String {
        let url = &self.ctx.config.borrow().cluster_url;
        if url.is_empty() {
            CLUSTER_MAP_URL.into()
        } else {
            url.clone()
        }
    }

    pub(super) async fn follow_cluster_url(&mut self) {
        if self.cluster_url.is_none()
            || self.cluster_url == Some(self.wanted_cluster_url())
            || !self.cluster_want
            || self.event.is_none()
            || !self.main_stream_ready
        {
            return;
        }
        self.activate_cluster_stream().await;
    }
}
