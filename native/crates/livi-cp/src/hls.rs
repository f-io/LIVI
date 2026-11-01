// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use openssl::ssl::{SslConnector, SslMethod};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_openssl::SslStream;

use crate::video_proxy::ProxyOffer;

mod http;
mod mp4;
mod playlist;

pub use http::*;
pub use mp4::*;
pub use playlist::*;
pub(crate) const FETCH_TIMEOUT: Duration = Duration::from_secs(4);
pub async fn get(url: &str) -> io::Result<Vec<u8>> {
    Client::default().get(url).await
}

#[derive(Debug)]
struct StartedOver;

impl std::fmt::Display for StartedOver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the stream started over")
    }
}

impl std::error::Error for StartedOver {}

pub fn started_over(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|inner| inner.is::<StartedOver>())
}

pub struct Stream {
    url: String,
    pub track: Track,
    next: u64,
    give_up: Duration,
    client: Client,
    init: String,
    discontinuity: u64,
    block_reload: bool,
}

const POLL: Duration = Duration::from_millis(250);
const GIVE_UP: Duration = Duration::from_secs(10);
const START_BEHIND: u64 = 3;
const AHEAD: usize = 3;
const SLOW_FETCH: Duration = Duration::from_millis(500);

impl Stream {
    pub async fn open(url: &str) -> io::Result<Self> {
        let mut client = Client::default();
        let list = read_playlist(&String::from_utf8_lossy(&client.get(url).await?));
        let init = list
            .init
            .clone()
            .ok_or_else(|| io::Error::other("no init segment"))?;
        let track = read_init(&client.get(&resolve(url, &init)).await?)
            .ok_or_else(|| io::Error::other("no H.264 track with four byte NAL lengths"))?;
        let newest = list.first + list.segments.len() as u64;
        Ok(Self {
            url: url.to_string(),
            track,
            next: newest.saturating_sub(START_BEHIND).max(list.first),
            give_up: GIVE_UP,
            client,
            init,
            discontinuity: list.discontinuity,
            block_reload: list.block_reload,
        })
    }

    pub async fn run(self, frames: mpsc::Sender<Vec<u8>>) -> io::Result<()> {
        let timescale = f64::from(self.track.timescale.max(1));
        let (segments, mut fetched) = mpsc::channel::<Vec<Sample>>(AHEAD);
        let play = async move {
            let mut due = Instant::now();
            while let Some(samples) = fetched.recv().await {
                due = due.max(Instant::now());
                for sample in samples {
                    tokio::time::sleep_until(due).await;
                    if frames.send(sample.data).await.is_err() {
                        return;
                    }
                    due += Duration::from_secs_f64(f64::from(sample.duration) / timescale);
                }
            }
        };
        tokio::select! {
            fetched = self.fetch(segments) => fetched,
            () = play => Ok(()),
        }
    }

    async fn fetch(mut self, segments: mpsc::Sender<Vec<Sample>>) -> io::Result<()> {
        let mut failing: Option<Instant> = None;
        loop {
            match self.fetch_next().await {
                Ok(Some(samples)) => {
                    failing = None;
                    if segments.send(samples).await.is_err() {
                        return Ok(());
                    }
                    continue;
                }
                Ok(None) => failing = None,
                Err(e) if started_over(&e) => return Err(e),
                Err(e) => {
                    let since = *failing.get_or_insert_with(Instant::now);
                    if since.elapsed() >= self.give_up {
                        return Err(e);
                    }
                    println!("[hls] {e}, trying again");
                }
            }
            tokio::time::sleep(POLL).await;
        }
    }

    async fn fetch_next(&mut self) -> io::Result<Option<Vec<Sample>>> {
        let list = self.playlist().await?;
        let newest = list.first + list.segments.len() as u64;
        if list.init.as_ref().is_some_and(|init| *init != self.init)
            || list.discontinuity != self.discontinuity
            || self.next > newest + 1
        {
            return Err(io::Error::other(StartedOver));
        }
        if self.next < list.first {
            self.next = list.first;
        }
        let at = usize::try_from(self.next - list.first).unwrap_or(usize::MAX);
        let Some(uri) = list.segments.get(at) else {
            return Ok(None);
        };
        let asked = Instant::now();
        let segment = self.client.get(&resolve(&self.url, uri)).await?;
        let took = asked.elapsed();
        self.next += 1;
        if took >= SLOW_FETCH {
            println!(
                "[hls] {uri}: {} KB in {} ms, {} behind the newest",
                segment.len() / 1024,
                took.as_millis(),
                newest.saturating_sub(self.next)
            );
        }
        Ok(Some(read_segment(&segment, &self.track)))
    }

    async fn playlist(&mut self) -> io::Result<Playlist> {
        if self.block_reload {
            let ask = with_query(&self.url, &format!("_HLS_msn={}", self.next));
            if let Ok(body) = self.client.get(&ask).await {
                let list = read_playlist(&String::from_utf8_lossy(&body));
                self.block_reload = list.block_reload;
                return Ok(list);
            }
        }
        let list = read_playlist(&String::from_utf8_lossy(&self.client.get(&self.url).await?));
        self.block_reload = list.block_reload;
        Ok(list)
    }
}

#[cfg(test)]
mod tests;
