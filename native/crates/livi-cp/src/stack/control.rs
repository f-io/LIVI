// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;

impl<M: Media> Conn<M> {
    pub(super) async fn on_control_bytes(
        &mut self,
        chunk: &[u8],
        wr: &mut OwnedWriteHalf,
    ) -> io::Result<()> {
        if let Some(cipher) = self.cipher.as_mut() {
            self.sealed.extend_from_slice(chunk);
            let (data, rest) = cipher
                .decrypt(&self.sealed)
                .map_err(|_| io::Error::other("control channel decrypt failed"))?;
            self.sealed = rest;
            self.plain.extend(data);
        } else {
            self.plain.extend_from_slice(chunk);
        }
        let (requests, rest) = rtsp::parse(&self.plain);
        self.plain = rest;
        for req in requests {
            let res = self.handle(&req).await;
            let out = rtsp::build_response(&req, res);
            let sealed = match self.cipher.as_mut() {
                Some(cipher) => cipher.encrypt(&out),
                None => out,
            };
            wr.write_all(&sealed).await?;
            if self.cipher.is_none()
                && let Some(keys) = self.pair_verify.control_keys()
            {
                self.cipher = Some(ControlCipher::new(keys.read, keys.write));
                println!("[cpStack] control channel encrypted");
            }
        }
        Ok(())
    }

    pub(super) async fn mfi4_auth_setup(&mut self, req: &Request) -> Response {
        let session = self.mfi4.get_or_insert_with(|| {
            println!("[cpStack] MFi4 mutual authentication");
            mfi4::Session::new(self.ctx.pairings.dir())
        });
        if header_number(req, "x-apple-pairingtranscript").is_some_and(|v| v != 0) {
            session.binds_pairing = true;
        }
        let pair_verify = &self.pair_verify;
        let mut bound = false;
        let mut check = |shown: &[u8]| {
            let (expected, ours) = pair_verify
                .transcript_proofs()
                .ok_or_else(|| "nothing to bind, pair-verify has not run".to_string())?;
            if shown.get(..expected.len()) != Some(&expected[..]) {
                return Err("the phone's transcript proof does not match".to_string());
            }
            bound = true;
            Ok(ours.to_vec())
        };
        let result = session
            .handle(&req.body, &self.ctx.helper, &mut check)
            .await;
        if bound {
            println!("[cpStack] MFi4: this pair-verify is bound to the mutual authentication");
        }
        octet(result, "MFi4 auth-setup")
    }

    pub(super) async fn handle(&mut self, req: &Request) -> Response {
        let path = req.path.to_lowercase();
        let chatty = req.method == "OPTIONS" || path.ends_with("/feedback");
        if !chatty {
            println!(
                "[cpStack] < {} {} (body {}B)",
                req.method,
                req.path,
                req.body.len()
            );
            if self.ctx.debug && !req.body.is_empty() {
                match bplist::decode(&req.body) {
                    Ok(v) => println!("[cpStack]   body: {v:?}"),
                    Err(_) => println!("[cpStack]   body: not a plist ({}B)", req.body.len()),
                }
            }
        }
        match req.method.as_str() {
            "SETUP" => return self.handle_setup(req).await,
            "RECORD" => {
                println!("[cpStack] RECORD (session started)");
                self.live = true;
                if let Some(night) = self.night_mode {
                    self.send_night_mode(night).await;
                }
                if let Some(night) = self.cluster_night_mode {
                    self.send_cluster_appearance(night).await;
                }
                if let Some(limited) = self.limited_ui {
                    self.send_limited_ui(limited).await;
                }
                if self.video_playback {
                    self.send_event_command(&video_playback::allowed_command(
                        self.video_allowed,
                        "idle",
                    ))
                    .await;
                }
                self.emit(StackEvent::Active {
                    ip: net::host_of(&self.peer),
                    controller_id: self.pair_verify.controller_id().map(str::to_string),
                });
                self.open_iap_relay().await;
                return status(200);
            }
            "TEARDOWN" => return self.handle_teardown(req),
            _ => {}
        }
        if path.ends_with("/pair-setup")
            && let Some(session) = self.mfi4.as_mut()
        {
            let (pair_setup, ctx) = (&mut self.pair_setup, &self.ctx);
            let mut through =
                |data: &[u8]| Ok(pair_setup.handle(data, &ctx.identity, &ctx.pairings));
            let result = session.handle(&req.body, &ctx.helper, &mut through).await;
            return octet(result, "MFi4 pair-setup");
        }
        if path.ends_with("/pair-setup") {
            let body = self
                .pair_setup
                .handle(&req.body, &self.ctx.identity, &self.ctx.pairings);
            return Response {
                headers: vec![("Content-Type".into(), PAIRING.into())],
                body,
                ..Default::default()
            };
        }
        if path.ends_with("/pair-verify") {
            let body = self
                .pair_verify
                .handle(&req.body, &self.ctx.identity, &self.ctx.pairings);
            return Response {
                headers: vec![("Content-Type".into(), PAIRING.into())],
                body,
                ..Default::default()
            };
        }
        if path.ends_with("/auth-setup")
            && header_number(req, "x-apple-at") == Some(mfi4::AUTH_TYPE)
        {
            return self.mfi4_auth_setup(req).await;
        }
        if path.ends_with("/auth-setup") {
            return match auth_setup::handle(&req.body, &self.ctx.helper).await {
                Ok(Some(body)) => Response {
                    headers: vec![("Content-Type".into(), OCTET.into())],
                    body,
                    ..Default::default()
                },
                Ok(None) => status(400),
                Err(e) => {
                    println!(
                        "[cpStack] handler error for {} {}: {e}",
                        req.method, req.path
                    );
                    status(500)
                }
            };
        }
        if path.ends_with("/info") {
            if !req.body.is_empty() {
                match bplist::decode(&req.body) {
                    Ok(ask) => println!("[cpStack] /info request from phone: {ask:?}"),
                    Err(_) => {
                        println!(
                            "[cpStack] /info request body not a plist ({}B)",
                            req.body.len()
                        )
                    }
                }
            }
            if let Some(refresh) = self.ctx.refresh.clone() {
                let _ = tokio::task::spawn_blocking(move || refresh()).await;
            }
            let cfg = self.ctx.config.borrow().info.clone();
            let mut info = info::build(&cfg);
            if self.video_playback {
                let features = info.get("features").and_then(Value::as_int).unwrap_or(0);
                if let Value::Dict(entries) = &mut info {
                    entries.extend(video_playback::info_entries(features, self.video_allowed));
                }
            }
            if self.ui_sync_on
                && let Value::Dict(entries) = &mut info
            {
                entries.push((ui_sync::INFO_KEY.to_string(), Value::Dict(Vec::new())));
            }
            if let Some(state) = &self.vehicle_state {
                info::add_display_panels(&mut info, cfg.theme_displays.as_ref());
                if let Value::Dict(entries) = &mut info {
                    entries.push(state.info_entry());
                }
            }
            let count = |k: &str| {
                info.get(k)
                    .and_then(Value::as_array)
                    .map_or(0, <[Value]>::len)
            };
            println!(
                "[cpStack] /info audio: disableAudioOutput={} audioFormats={} audioLatencies={}",
                cfg.disable_audio_output,
                count("audioFormats"),
                count("audioLatencies")
            );
            return plist_response(&info);
        }
        if req.method == "POST" && path.ends_with("/command") {
            return self.handle_command(req).await;
        }
        if req.method == "POST" && path.ends_with("/feedback") {
            return self.build_feedback();
        }
        println!(
            "[cpStack] unhandled request {} {} (ack 200)",
            req.method, req.path
        );
        status(200)
    }
}
